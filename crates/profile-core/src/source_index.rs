use crate::source_files::{SourcePage, MAX_SNAPSHOT_BYTES, MAX_SOURCE_BYTES};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{File, Metadata},
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::SystemTime,
};

const STRIDE: usize = 200;
const MAX_INDEX_BYTES: usize = 64 * 1024 * 1024;
const MAX_INDEX_FILES: usize = 32;
#[derive(Debug, PartialEq, Eq)]
struct Stamp {
    length: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}
impl Stamp {
    fn read(metadata: &Metadata) -> Self {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Self {
            length: metadata.len(),
            modified: metadata.modified().ok(),
            created: metadata.created().ok(),
            #[cfg(unix)]
            identity: (
                metadata.dev(),
                metadata.ino(),
                metadata.ctime(),
                metadata.ctime_nsec(),
            ),
        }
    }
}
#[derive(Debug)]
struct Index {
    sha256: String,
    stamp: Stamp,
    offsets: Vec<u64>,
    total: usize,
    used: u64,
}
impl Index {
    fn bytes(&self) -> usize {
        self.offsets.capacity() * std::mem::size_of::<u64>()
    }
}
#[derive(Debug, Default, Clone, Copy)]
pub struct SourceCacheStats {
    pub files: usize,
    pub index_bytes: usize,
    pub builds: u64,
    pub hits: u64,
    pub scanned_bytes: u64,
    pub page_bytes: u64,
}
#[derive(Debug, Default)]
pub struct SourceCache {
    entries: BTreeMap<PathBuf, Index>,
    clock: u64,
    stats: SourceCacheStats,
}
impl SourceCache {
    pub fn reference(
        &mut self,
        path: &Path,
    ) -> Result<crate::source_files::SourceReference, String> {
        self.read_page(path, 1, 1)?;
        let path = path.canonicalize().map_err(|e| e.to_string())?;
        let index = self
            .entries
            .get(&path)
            .ok_or("Source changed while creating reference")?;
        Ok(crate::source_files::SourceReference {
            path: path.to_str().ok_or("Source path must be UTF-8")?.into(),
            size: index.stamp.length,
            sha256: index.sha256.clone(),
        })
    }
    pub fn read_reference_page(
        &mut self,
        reference: &crate::source_files::SourceReference,
        start: usize,
        count: usize,
    ) -> Result<SourcePage, String> {
        reference.validate()?;
        let path = Path::new(&reference.path);
        let page = self
            .read_page(path, start, count)
            .map_err(|e| format!("Referenced source unavailable: {e}"))?;
        let path = path.canonicalize().map_err(|e| e.to_string())?;
        let index = self
            .entries
            .get(&path)
            .ok_or("Referenced source changed while reading")?;
        if index.sha256 != reference.sha256 || index.stamp.length != reference.size {
            return Err("Referenced source changed since it was mapped. Map the file again to accept its current contents.".into());
        }
        Ok(page)
    }
    pub fn stats(&self) -> SourceCacheStats {
        SourceCacheStats {
            files: self.entries.len(),
            index_bytes: self.entries.values().map(Index::bytes).sum(),
            ..self.stats
        }
    }
    pub fn read_snapshot_page(
        &mut self,
        capture: &crate::Capture,
        original: &str,
        start: usize,
        count: usize,
    ) -> Result<SourcePage, String> {
        let path = crate::source_files::snapshot_path(capture, original)?;
        self.read_page(&path, start, count)
    }
    pub fn read_page(
        &mut self,
        path: &Path,
        start: usize,
        count: usize,
    ) -> Result<SourcePage, String> {
        if start == 0 || count == 0 || count > STRIDE {
            return Err("Source page must start at line 1 or later and contain 1–200 lines".into());
        }
        let path = path.canonicalize().map_err(|e| e.to_string())?;
        if !std::fs::metadata(&path)
            .map_err(|e| e.to_string())?
            .is_file()
        {
            return Err("Select a regular source file".into());
        }
        let mut file = File::open(&path).map_err(|e| e.to_string())?;
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        if !metadata.is_file() {
            return Err("Select a regular source file".into());
        }
        if metadata.len() > MAX_SNAPSHOT_BYTES {
            return Err("Source snapshot exceeds 512 MiB".into());
        }
        let stamp = Stamp::read(&metadata);
        self.clock = self.clock.saturating_add(1);
        if self
            .entries
            .get(&path)
            .is_some_and(|index| index.stamp == stamp)
        {
            self.stats.hits += 1;
        } else {
            self.entries.remove(&path);
            let (mut index, scanned) = build(&mut file, stamp)?;
            if index.bytes() > MAX_INDEX_BYTES {
                return Err("Source line index exceeds the 64 MiB cache budget".into());
            }
            index.used = self.clock;
            while !self.entries.is_empty()
                && (self.entries.len() >= MAX_INDEX_FILES
                    || self.stats().index_bytes + index.bytes() > MAX_INDEX_BYTES)
            {
                let oldest = self
                    .entries
                    .iter()
                    .min_by_key(|(_, i)| i.used)
                    .unwrap()
                    .0
                    .clone();
                self.entries.remove(&oldest);
            }
            self.stats.builds += 1;
            self.stats.scanned_bytes += scanned;
            self.entries.insert(path.clone(), index);
        }
        let index = self.entries.get_mut(&path).unwrap();
        index.used = self.clock;
        let result = indexed_page(&mut file, index, start, count);
        let unchanged = file
            .metadata()
            .map(|m| Stamp::read(&m) == index.stamp)
            .unwrap_or(false)
            && std::fs::metadata(&path)
                .map(|m| Stamp::read(&m) == index.stamp)
                .unwrap_or(false);
        if !unchanged {
            self.entries.remove(&path);
            return Err("Source changed while reading; retry the source page".into());
        }
        match result {
            Ok((page, bytes)) => {
                self.stats.page_bytes += bytes;
                Ok(page)
            }
            Err(error) => {
                self.entries.remove(&path);
                Err(error)
            }
        }
    }
}
fn line_text(buffer: &mut Vec<u8>) -> Result<&str, String> {
    if buffer.len() > MAX_SOURCE_BYTES {
        return Err("A source line exceeds 2 MiB".into());
    }
    if buffer.last() == Some(&b'\n') {
        buffer.pop();
        if buffer.last() == Some(&b'\r') {
            buffer.pop();
        }
    }
    let text = std::str::from_utf8(buffer).map_err(|_| "Source file must be UTF-8")?;
    if text.contains('\0') {
        return Err("Source file contains NUL bytes".into());
    }
    Ok(text)
}
fn read_line(reader: &mut impl BufRead, buffer: &mut Vec<u8>) -> Result<usize, String> {
    buffer.clear();
    reader
        .take((MAX_SOURCE_BYTES + 1) as u64)
        .read_until(b'\n', buffer)
        .map_err(|e| e.to_string())
}
fn build(file: &mut File, stamp: Stamp) -> Result<(Index, u64), String> {
    let mut digest = Sha256::new();
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(&mut *file);
    let mut offsets = vec![0];
    let mut buffer = Vec::new();
    let mut bytes = 0u64;
    let mut total = 0usize;
    let mut newline = true;
    loop {
        let n = read_line(&mut reader, &mut buffer)?;
        if n == 0 {
            if newline {
                total += 1;
            }
            break;
        }
        bytes += n as u64;
        digest.update(&buffer);
        if bytes > MAX_SNAPSHOT_BYTES {
            return Err("Source snapshot exceeds 512 MiB".into());
        }
        newline = buffer.last() == Some(&b'\n');
        line_text(&mut buffer)?;
        total += 1;
        if newline && total % STRIDE == 0 {
            offsets.push(bytes);
        }
    }
    if Stamp::read(&reader.get_ref().metadata().map_err(|e| e.to_string())?) != stamp {
        return Err("Source changed while indexing; retry the source page".into());
    }
    Ok((
        Index {
            sha256: format!("{:x}", digest.finalize()),
            stamp,
            offsets,
            total,
            used: 0,
        },
        bytes,
    ))
}
fn indexed_page(
    file: &mut File,
    index: &Index,
    start: usize,
    count: usize,
) -> Result<(SourcePage, u64), String> {
    let mut page = SourcePage {
        start_line: start,
        total_lines: index.total,
        lines: vec![],
    };
    if start > index.total {
        return Ok((page, 0));
    }
    let bucket = (start - 1) / STRIDE;
    file.seek(SeekFrom::Start(index.offsets[bucket]))
        .map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(file);
    let mut buffer = Vec::new();
    let mut bytes = 0u64;
    let mut page_bytes = 0;
    for number in bucket * STRIDE + 1..=start.saturating_add(count - 1).min(index.total) {
        let n = read_line(&mut reader, &mut buffer)?;
        bytes += n as u64;
        if n == 0 && number < index.total {
            return Err("Source changed while reading; retry the source page".into());
        }
        let text = line_text(&mut buffer)?;
        if number >= start {
            page_bytes += text.len();
            if page_bytes > MAX_SOURCE_BYTES {
                return Err("Source page exceeds 2 MiB; individual lines are too large".into());
            }
            page.lines.push(text.to_owned());
        }
    }
    Ok((page, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn byte_budget_evicts_the_oldest_index_before_insertion() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new.hip");
        std::fs::write(&path, "line\n").unwrap();
        let mut cache = SourceCache::default();
        for n in 0..2 {
            cache.entries.insert(
                dir.path().join(format!("old{n}.hip")),
                Index {
                    sha256: String::new(),
                    stamp: Stamp::read(&std::fs::metadata(&path).unwrap()),
                    offsets: Vec::with_capacity(MAX_INDEX_BYTES / 2 / std::mem::size_of::<u64>()),
                    total: 1,
                    used: n,
                },
            );
        }
        assert_eq!(cache.stats().index_bytes, MAX_INDEX_BYTES);
        cache.read_page(&path, 1, 200).unwrap();
        assert!(!cache.entries.contains_key(&dir.path().join("old0.hip")));
        assert!(cache.entries.contains_key(&dir.path().join("old1.hip")));
        assert!(cache.stats().index_bytes <= MAX_INDEX_BYTES);
    }
}
