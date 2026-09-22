use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileFingerprint {
    pub size: u64,
    pub sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InputFingerprint {
    pub files: BTreeMap<PathBuf, FileFingerprint>,
}
impl InputFingerprint {
    pub fn validate(&self) -> Result<(), String> {
        if self.files.is_empty() || self.files.len() > 100000 {
            return Err("Invalid input fingerprint file count".into());
        }
        for (path, file) in &self.files {
            if !path.is_absolute()
                || path.as_os_str().len() > 8192
                || path.to_string_lossy().contains('\0')
                || file.size > 128 * 1024 * 1024 * 1024
                || file.sha256.len() != 64
                || !file
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err("Invalid input fingerprint".into());
            }
        }
        Ok(())
    }
}
fn check(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) {
        Err("Input fingerprint cancelled".into())
    } else {
        Ok(())
    }
}
fn strings(
    value: &serde_json::Value,
    root: &Path,
    files: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    match value {
        serde_json::Value::String(name) => {
            let path = root.join(name);
            if path.is_file() {
                let path = path.canonicalize().map_err(|e| e.to_string())?;
                if !path.starts_with(root) {
                    return Err("Fingerprint sidecar escapes capture root".into());
                }
                files.insert(path);
            }
        }
        serde_json::Value::Array(items) => {
            for value in items {
                strings(value, root, files)?;
            }
        }
        serde_json::Value::Object(items) => {
            for value in items.values() {
                strings(value, root, files)?;
            }
        }
        _ => {}
    }
    if files.len() > 100000 {
        return Err("Input fingerprint exceeds 100,000 files".into());
    }
    Ok(())
}
fn att_identity(path: &Path) -> Option<(String, u32)> {
    let stem = path.file_stem()?.to_str()?;
    let (prefix, identity) = crate::att_runs::split_stem(stem)?;
    let fields: Vec<_> = identity.split('_').collect();
    if fields.len() != 2 || fields[0].parse::<u32>().is_err() {
        return None;
    }
    Some((prefix.into(), fields[1].parse().ok()?))
}
fn inventory(input: &Path, cancelled: &AtomicBool) -> Result<BTreeSet<PathBuf>, String> {
    let input = input.canonicalize().map_err(|e| e.to_string())?;
    let root = if input.is_dir() {
        input.as_path()
    } else {
        input.parent().ok_or("Missing input parent")?
    };
    let extension = input
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let decoded = (input.is_dir()
        || matches!(
            input.file_name().and_then(|n| n.to_str()),
            Some("filenames.json" | "code.json" | "occupancy.json")
        ))
        && root.join("filenames.json").is_file();
    let mut files = BTreeSet::new();
    if input.is_file() {
        files.insert(input.clone());
    }
    if decoded {
        for name in [
            "filenames.json",
            "snapshots.json",
            "code.json",
            "occupancy.json",
            "realtime.json",
            "wstates2.json",
            "wstates3.json",
            "wstates4.json",
        ] {
            let path = root.join(name);
            if path.is_file() {
                files.insert(path.clone());
            }
            if matches!(name, "filenames.json" | "snapshots.json") && path.is_file() {
                if fs::metadata(&path).map_err(|e| e.to_string())?.len() > 256 * 1024 * 1024 {
                    return Err("Fingerprint manifest exceeds 256 MiB".into());
                }
                let value: serde_json::Value =
                    serde_json::from_reader(fs::File::open(&path).map_err(|e| e.to_string())?)
                        .map_err(|e| e.to_string())?;
                if name == "filenames.json" {
                    for field in [
                        "wave_filenames",
                        "other_simd_filenames",
                        "shaderdata_filenames",
                    ] {
                        strings(&value[field], root, &mut files)?;
                    }
                } else {
                    strings(&value, root, &mut files)?;
                }
            }
        }
        for item in fs::read_dir(root).map_err(|e| e.to_string())? {
            check(cancelled)?;
            let path = item.map_err(|e| e.to_string())?.path();
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name.starts_with("se") && name.ends_with("_perfcounter.json") {
                files.insert(path);
            }
        }
    } else if extension == "rocprof-att" {
        let manifest = crate::att_manifest::read(&input)?;
        files.extend(manifest.traces.into_iter().map(|t| t.path));
        files.extend(manifest.code_objects.into_iter().map(|o| o.path));
    } else if extension == "att" || input.is_dir() {
        let selected = if input.is_file() {
            Some(att_identity(&input).ok_or("ATT input has no run identity")?)
        } else {
            None
        };
        let mut objects = false;
        for item in fs::read_dir(root).map_err(|e| e.to_string())? {
            check(cancelled)?;
            let path = item.map_err(|e| e.to_string())?.path();
            let ext = path
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_ascii_lowercase();
            if ext == "att"
                && (selected.is_none() || att_identity(&path).as_ref() == selected.as_ref())
            {
                files.insert(path.clone());
            }
            if ["out", "co", "hsaco"].contains(&ext.as_str()) {
                objects = true;
                files.insert(path);
            }
        }
        if !objects && root.join("code.json").is_file() {
            files.insert(root.join("code.json"));
        }
    } else if ["rpd", "rocpd", "db", "sqlite"].contains(&extension.as_str()) {
        let wal = PathBuf::from(format!("{}-wal", input.display()));
        if wal.is_file() {
            files.insert(wal);
        }
    }
    if files.is_empty() || files.len() > 100000 {
        return Err("Input fingerprint requires 1–100,000 files".into());
    }
    for path in &files {
        if !fs::metadata(path).map_err(|e| e.to_string())?.is_file() {
            return Err("Fingerprint input is not a regular file".into());
        }
        if decoded
            && !path
                .canonicalize()
                .map_err(|e| e.to_string())?
                .starts_with(root)
        {
            return Err("Fingerprint input escapes capture root".into());
        }
    }
    Ok(files)
}
pub fn compute(input: &Path, cancelled: &AtomicBool) -> Result<InputFingerprint, String> {
    check(cancelled)?;
    let paths = inventory(input, cancelled)?;
    let mut files = BTreeMap::new();
    let mut total = 0u64;
    let mut buffer = vec![0u8; 1024 * 1024];
    for path in &paths {
        check(cancelled)?;
        let mut file = fs::File::open(path)
            .map_err(|e| format!("Cannot fingerprint {}: {e}", path.display()))?;
        let before = file.metadata().map_err(|e| e.to_string())?;
        if before.len() > 128 * 1024 * 1024 * 1024 - total {
            return Err("Input fingerprint exceeds 128 GiB".into());
        }
        let mut digest = Sha256::new();
        let mut size = 0;
        loop {
            check(cancelled)?;
            let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            total += n as u64;
            size += n as u64;
            if total > 128 * 1024 * 1024 * 1024 {
                return Err("Input fingerprint exceeds 128 GiB".into());
            }
            digest.update(&buffer[..n]);
        }
        let after = file.metadata().map_err(|e| e.to_string())?;
        if size != before.len()
            || after.len() != before.len()
            || after.modified().ok() != before.modified().ok()
        {
            return Err("Input changed while fingerprinting".into());
        }
        files.insert(
            path.clone(),
            FileFingerprint {
                size,
                sha256: format!("{:x}", digest.finalize()),
            },
        );
    }
    if paths != inventory(input, cancelled)? {
        return Err("Input file set changed while fingerprinting".into());
    }
    let fingerprint = InputFingerprint { files };
    fingerprint.validate()?;
    Ok(fingerprint)
}
