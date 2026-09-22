use crate::{
    wave_viewport::{TokenIdentity, ViewportWave},
    Capture, Token, Wave,
};
use std::{
    collections::BTreeMap,
    fs,
    io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::SystemTime,
};

const BLOCK_ROWS: usize = 1024;
const RECORD_BYTES: usize = 56;

#[derive(PartialEq, Eq)]
struct Stamp {
    length: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}
impl Stamp {
    fn read(path: &Path) -> Result<Self, String> {
        let m = fs::metadata(path).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Ok(Self {
            length: m.len(),
            modified: m.modified().ok(),
            created: m.created().ok(),
            #[cfg(unix)]
            identity: (m.dev(), m.ino(), m.ctime(), m.ctime_nsec()),
        })
    }
}
struct Block {
    first: usize,
    rows: usize,
    min_start: f64,
    max_end: f64,
}
pub struct TokenIndex {
    file: tempfile::NamedTempFile,
    source: PathBuf,
    input: PathBuf,
    stamp: Stamp,
    wave: Wave,
    blocks: Vec<Block>,
    rows: usize,
}
fn check(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) {
        Err("Wave query cancelled".into())
    } else {
        Ok(())
    }
}
impl TokenIndex {
    pub fn build(
        capture: &Capture,
        ordinal: usize,
        directory: &Path,
        cancelled: &AtomicBool,
    ) -> Result<Self, String> {
        Self::build_for_cache(capture, ordinal, directory, cancelled)?
            .ok_or_else(|| "Token index storage unavailable".into())
    }

    pub(crate) fn build_for_cache(
        capture: &Capture,
        ordinal: usize,
        directory: &Path,
        cancelled: &AtomicBool,
    ) -> Result<Option<Self>, String> {
        check(cancelled)?;
        let index = capture
            .waves
            .get(ordinal)
            .ok_or("Wave is not part of this capture")?;
        let input = capture.root.join(&index.id);
        let source = input.canonicalize().map_err(|e| e.to_string())?;
        if !source.starts_with(&capture.root) {
            return Err("Sidecar escapes capture directory".into());
        }
        let stamp = Stamp::read(&source)?;
        let Ok(mut file) = tempfile::NamedTempFile::new_in(directory) else {
            return Ok(None);
        };
        let mut writer = BufWriter::new(file.as_file_mut());
        let mut storage_failed = false;
        let offset = capture.se_time_offsets.get(&index.se).copied();
        let mut occurrences = BTreeMap::<i64, usize>::new();
        let mut previous = [0.; 4];
        let mut ends = [0.; 4];
        let mut blocks: Vec<Block> = Vec::new();
        let mut rows = 0;
        let wave = crate::import::load_wave_filtered(capture, ordinal, cancelled, |raw| {
            check(cancelled)?;
            let mut token = raw.clone();
            if let Some(offset) = offset {
                token.start = crate::se_clock::shift(token.start, offset)?;
            }
            let occurrence = occurrences.entry(token.instruction).or_default();
            let mut slot = 0;
            while slot < 3 && previous[slot] == token.start && ends[slot] > token.start {
                slot += 1;
            }
            previous[slot] = token.start;
            let end = token.start + token.duration.max(token.stall);
            ends[slot] = ends[slot].max(end);
            if rows % BLOCK_ROWS == 0 {
                blocks.push(Block {
                    first: rows,
                    rows: 0,
                    min_start: token.start,
                    max_end: end,
                });
            }
            let block = blocks.last_mut().unwrap();
            block.rows += 1;
            block.min_start = block.min_start.min(token.start);
            block.max_end = block.max_end.max(end);
            let mut bytes = [0; RECORD_BYTES];
            bytes[0..8].copy_from_slice(&token.start.to_le_bytes());
            bytes[8..16].copy_from_slice(&token.duration.to_le_bytes());
            bytes[16..24].copy_from_slice(&token.stall.to_le_bytes());
            bytes[24..28].copy_from_slice(&token.category.to_le_bytes());
            bytes[28..36].copy_from_slice(&token.instruction.to_le_bytes());
            bytes[36..44].copy_from_slice(&(rows as u64).to_le_bytes());
            bytes[44..52].copy_from_slice(&(*occurrence as u64).to_le_bytes());
            bytes[52..56].copy_from_slice(&(slot as u32).to_le_bytes());
            writer.write_all(&bytes).map_err(|e| {
                storage_failed = true;
                e.to_string()
            })?;
            rows += 1;
            *occurrence += 1;
            Ok(false)
        });
        check(cancelled)?;
        if storage_failed {
            return Ok(None);
        }
        let mut wave = wave?;
        if writer.flush().is_err() {
            check(cancelled)?;
            return Ok(None);
        }
        drop(writer);
        check(cancelled)?;
        if input.canonicalize().map_err(|e| e.to_string())? != source
            || Stamp::read(&source)? != stamp
        {
            return Err("Wave source changed while indexing".into());
        }
        wave.waits = serde_json::Value::Null;
        Ok(Some(Self {
            file,
            source,
            input,
            stamp,
            wave,
            blocks,
            rows,
        }))
    }

    pub fn disk_bytes(&self) -> usize {
        self.rows * RECORD_BYTES
    }

    pub fn is_current(&self) -> bool {
        self.input.canonicalize().ok().as_ref() == Some(&self.source)
            && Stamp::read(&self.source).is_ok_and(|stamp| stamp == self.stamp)
    }

    pub fn estimated_resident_bytes(&self) -> usize {
        fn value_bytes(value: &serde_json::Value) -> usize {
            64 + match value {
                serde_json::Value::String(s) => s.capacity(),
                serde_json::Value::Array(a) => {
                    a.capacity() * 32 + a.iter().map(value_bytes).sum::<usize>()
                }
                serde_json::Value::Object(o) => o
                    .iter()
                    .map(|(k, v)| 128 + k.capacity() + value_bytes(v))
                    .sum(),
                _ => 0,
            }
        }
        1024 + self.wave.states.capacity() * std::mem::size_of::<[f64; 3]>()
            + self.blocks.capacity() * std::mem::size_of::<Block>()
            + value_bytes(&self.wave.info)
    }

    pub fn query(
        &self,
        start: f64,
        end: f64,
        limit: usize,
        cancelled: &AtomicBool,
    ) -> Result<(ViewportWave, usize), String> {
        check(cancelled)?;
        if !start.is_finite() || !end.is_finite() || end <= start {
            return Err("Invalid wave query range".into());
        }
        if limit > crate::wave_index::MAX_VIEWPORT_TOKENS {
            return Err("Invalid visible token budget".into());
        }
        if self.input.canonicalize().map_err(|e| e.to_string())? != self.source
            || Stamp::read(&self.source)? != self.stamp
        {
            return Err("Wave source changed; rebuild token index".into());
        }
        let file = self.file.reopen().map_err(|e| e.to_string())?;
        if file.metadata().map_err(|e| e.to_string())?.len() != self.disk_bytes() as u64 {
            return Err("Token index length changed".into());
        }
        let mut reader = BufReader::with_capacity(BLOCK_ROWS * RECORD_BYTES, file);
        let mut tokens = Vec::new();
        let mut identities = Vec::new();
        let mut bytes_read = 0;
        for block in &self.blocks {
            check(cancelled)?;
            if block.max_end < start || block.min_start > end {
                continue;
            }
            reader
                .seek(SeekFrom::Start((block.first * RECORD_BYTES) as u64))
                .map_err(|e| e.to_string())?;
            for row in 0..block.rows {
                check(cancelled)?;
                let mut bytes = [0; RECORD_BYTES];
                reader.read_exact(&mut bytes).map_err(|e| e.to_string())?;
                bytes_read += RECORD_BYTES;
                let token = Token {
                    start: f64::from_le_bytes(bytes[0..8].try_into().unwrap()),
                    duration: f64::from_le_bytes(bytes[8..16].try_into().unwrap()),
                    stall: f64::from_le_bytes(bytes[16..24].try_into().unwrap()),
                    category: u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
                    instruction: i64::from_le_bytes(bytes[28..36].try_into().unwrap()),
                };
                let index = u64::from_le_bytes(bytes[36..44].try_into().unwrap()) as usize;
                if index != block.first + row {
                    return Err("Token index record order changed".into());
                }
                if token.start <= end && token.start + token.duration.max(token.stall) >= start {
                    if tokens.len() == limit {
                        return Err("Visible wave query exceeds 500,000 tokens. Zoom in or use Target wave. No tokens were silently dropped.".into());
                    }
                    identities.push(TokenIdentity {
                        index,
                        occurrence: u64::from_le_bytes(bytes[44..52].try_into().unwrap()) as usize,
                        display_slot: u32::from_le_bytes(bytes[52..56].try_into().unwrap())
                            as usize,
                    });
                    tokens.push(token);
                }
            }
        }
        let mut wave = self.wave.clone();
        wave.tokens = tokens;
        wave.states.retain(|s| s[0] <= end && s[0] + s[1] >= start);
        wave.states.shrink_to_fit();
        check(cancelled)?;
        if self.input.canonicalize().map_err(|e| e.to_string())? != self.source
            || Stamp::read(&self.source)? != self.stamp
        {
            return Err("Wave source changed while querying".into());
        }
        Ok((
            ViewportWave {
                wave,
                token_identities: identities,
                viewport: [start, end],
            },
            bytes_read,
        ))
    }
}
