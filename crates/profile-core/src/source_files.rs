use serde::{Deserialize, Serialize};
use std::{fs::File, io::Read, path::Path};
pub const MAX_SOURCE_BYTES: usize = 2 * 1024 * 1024;
#[derive(Debug, Serialize)]
pub struct SourceFile {
    pub name: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<SourceReference>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceReference {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}
impl SourceReference {
    pub fn validate(&self) -> Result<(), String> {
        if !Path::new(&self.path).is_absolute()
            || self.path.len() > 8192
            || self.path.contains('\0')
            || self.size > MAX_SNAPSHOT_BYTES
            || self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("Invalid source file reference".into());
        }
        Ok(())
    }
}
pub fn import_replacement(
    path: &Path,
    cache: &mut crate::source_index::SourceCache,
) -> Result<SourceFile, String> {
    if std::fs::metadata(path).map_err(|e| e.to_string())?.len() <= MAX_SOURCE_BYTES as u64 {
        return read(path);
    }
    let reference = cache.reference(path)?;
    Ok(SourceFile {
        name: path
            .file_name()
            .ok_or("Missing source filename")?
            .to_string_lossy()
            .into_owned(),
        text: String::new(),
        reference: Some(reference),
    })
}
pub fn read(path: &Path) -> Result<SourceFile, String> {
    if !std::fs::metadata(path)
        .map_err(|e| e.to_string())?
        .is_file()
    {
        return Err("Select a regular source file".into());
    }
    let file = File::open(path).map_err(|e| e.to_string())?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Select a regular source file".into());
    }
    let mut bytes = Vec::new();
    file.take((MAX_SOURCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err("Source file exceeds 2 MiB".into());
    }
    let text = String::from_utf8(bytes).map_err(|_| "Source file must be UTF-8")?;
    if text.contains('\0') {
        return Err("Source file contains NUL bytes".into());
    }
    Ok(SourceFile {
        name: path
            .file_name()
            .ok_or("Missing source filename")?
            .to_string_lossy()
            .into_owned(),
        text,
        reference: None,
    })
}

pub fn read_snapshot(capture: &crate::Capture, original: &str) -> Result<SourceFile, String> {
    let local = capture
        .source_paths
        .get(original)
        .ok_or("Source path is not in this capture")?;
    let path = capture
        .root
        .join(local)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !path.starts_with(&capture.root) {
        return Err("Source snapshot escapes capture directory".into());
    }
    read(&path)
}

#[derive(Debug, Serialize)]
pub struct SourceBatch {
    pub files: std::collections::BTreeMap<String, SourceFile>,
    pub missing: Vec<String>,
}
pub fn map_directory(
    recorded: &[String],
    prefix: &str,
    directory: &Path,
) -> Result<SourceBatch, String> {
    map_directory_cached(
        recorded,
        prefix,
        directory,
        &mut crate::source_index::SourceCache::default(),
    )
}
pub fn map_directory_cached(
    recorded: &[String],
    prefix: &str,
    directory: &Path,
    cache: &mut crate::source_index::SourceCache,
) -> Result<SourceBatch, String> {
    use std::path::Component;
    if prefix.is_empty() || prefix.len() > 8192 || recorded.len() > 10000 {
        return Err("Invalid source prefix or too many recorded source files".into());
    }
    let root = directory.canonicalize().map_err(|e| e.to_string())?;
    if !root.is_dir() {
        return Err("Choose a source directory".into());
    }
    let mut batch = SourceBatch {
        files: Default::default(),
        missing: vec![],
    };
    let mut bytes = 0;
    let mut scanned = 0u64;
    for original in recorded {
        let path = Path::new(original);
        let relative = if prefix == "." && path.is_relative() {
            path
        } else {
            let Ok(relative) = path.strip_prefix(prefix) else {
                continue;
            };
            relative
        };
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(format!("Unsafe relative source path: {original}"));
        }
        let target = match root.join(relative).canonicalize() {
            Ok(path) => path,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                batch.missing.push(original.clone());
                continue;
            }
            Err(e) => return Err(format!("{original}: {e}")),
        };
        if !target.starts_with(&root) {
            return Err(format!(
                "Source mapping escapes selected directory: {original}"
            ));
        }
        scanned =
            scanned.saturating_add(std::fs::metadata(&target).map_err(|e| e.to_string())?.len());
        if scanned > MAX_SNAPSHOT_BYTES {
            return Err(
                "Source folder validation exceeds 512 MiB; map large files individually".into(),
            );
        }
        let file = import_replacement(&target, cache).map_err(|e| format!("{original}: {e}"))?;
        bytes += file.text.len();
        if bytes > 16 * 1024 * 1024 {
            return Err("Source folder mappings exceed 16 MiB".into());
        }
        batch.files.insert(original.clone(), file);
    }
    if batch.files.is_empty() && batch.missing.is_empty() {
        return Err("No recorded source paths match this prefix".into());
    }
    Ok(batch)
}

pub fn recorded_paths(capture: &crate::Capture) -> Vec<String> {
    capture
        .source_files
        .iter()
        .chain(capture.sources.keys())
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}
pub fn example_paths() -> Result<Vec<String>, String> {
    let bundle: serde_json::Value = serde_json::from_str(include_str!("../../../public/demo.json"))
        .map_err(|e| e.to_string())?;
    let capture: crate::Capture =
        serde_json::from_value(bundle["capture"].clone()).map_err(|e| e.to_string())?;
    Ok(recorded_paths(&capture))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcePage {
    pub start_line: usize,
    pub total_lines: usize,
    pub lines: Vec<String>,
}
pub const MAX_SNAPSHOT_BYTES: u64 = 512 * 1024 * 1024;
pub fn read_page(path: &Path, start_line: usize, count: usize) -> Result<SourcePage, String> {
    crate::source_index::SourceCache::default().read_page(path, start_line, count)
}

pub fn read_snapshot_page(
    capture: &crate::Capture,
    original: &str,
    start_line: usize,
    count: usize,
) -> Result<SourcePage, String> {
    crate::source_index::SourceCache::default()
        .read_snapshot_page(capture, original, start_line, count)
}

pub(crate) fn snapshot_path(
    capture: &crate::Capture,
    original: &str,
) -> Result<std::path::PathBuf, String> {
    let local = capture
        .source_paths
        .get(original)
        .ok_or("Source path is not in this capture")?;
    let path = capture
        .root
        .join(local)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !path.starts_with(&capture.root) {
        return Err("Source snapshot escapes capture directory".into());
    }
    Ok(path)
}
