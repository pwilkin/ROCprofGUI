use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub const MAX_OBJECT_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, PartialEq)]
pub struct FileRange {
    pub path: PathBuf,
    pub offset: u64,
    pub size: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Extraction {
    pub recorded_uri: String,
    pub remapped: bool,
    pub path: String,
    pub source: String,
    pub offset: String,
    pub bytes: String,
    pub sha256: String,
}

fn number(value: &str) -> Result<u64, String> {
    let (digits, radix) = if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        (hex, 16)
    } else if value.starts_with('0') && value.len() > 1 {
        (&value[1..], 8)
    } else {
        (value, 10)
    };
    if digits.is_empty() || !digits.bytes().all(|c| (c as char).is_digit(radix)) {
        return Err("Invalid code-object range integer".into());
    }
    u64::from_str_radix(digits, radix).map_err(|_| "Code-object range integer overflows u64".into())
}

pub fn parse(uri: &str) -> Result<FileRange, String> {
    if uri.starts_with("memory://") {
        return Err("Memory-only code object: the recorded URI does not contain its bytes".into());
    }
    let file = uri
        .strip_prefix("file://")
        .ok_or("Only local file-backed code-object URIs can be extracted")?;
    let split = file.find(['#', '?']).unwrap_or(file.len());
    let encoded = &file[..split];
    let mut decoded = Vec::with_capacity(encoded.len());
    let mut bytes = encoded.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let a = bytes.next().and_then(|b| (b as char).to_digit(16));
            let b = bytes.next().and_then(|b| (b as char).to_digit(16));
            let (Some(a), Some(b)) = (a, b) else {
                return Err("Invalid URI percent encoding".into());
            };
            decoded.push((a * 16 + b) as u8);
        } else if byte.is_ascii_alphanumeric() || b"/._~-".contains(&byte) {
            decoded.push(byte);
        } else {
            return Err("Code-object path must use URI percent encoding".into());
        }
    }
    let path = String::from_utf8(decoded).map_err(|_| "Code-object URI path is not UTF-8")?;
    if path.contains('\0') || !path.starts_with('/') || path.starts_with("//") {
        return Err("Code-object URI must name an absolute local path without an authority".into());
    }
    let mut offset = None;
    let mut size = None;
    if split < file.len() {
        for parameter in file[split + 1..].split('&') {
            let (key, value) = parameter
                .split_once('=')
                .ok_or("Invalid code-object range parameter")?;
            let slot = match key {
                "offset" => &mut offset,
                "size" => &mut size,
                _ => return Err("Unknown code-object range parameter".into()),
            };
            if slot.is_some() {
                return Err("Duplicate code-object range parameter".into());
            }
            *slot = Some(number(value)?);
        }
    }
    Ok(FileRange {
        path: path.into(),
        offset: offset.unwrap_or(0),
        size,
    })
}

pub fn extract(
    uri: &str,
    destination: &Path,
    cancelled: &AtomicBool,
) -> Result<Extraction, String> {
    extract_from(uri, None, destination, cancelled)
}

pub fn extract_from(
    uri: &str,
    replacement: Option<&Path>,
    destination: &Path,
    cancelled: &AtomicBool,
) -> Result<Extraction, String> {
    let mut range = parse(uri)?;
    if let Some(path) = replacement {
        if !path.is_absolute() {
            return Err("Replacement code-object container must have an absolute path".into());
        }
        range.path = path.to_path_buf();
    }
    let check = || {
        if cancelled.load(Ordering::Relaxed) {
            Err("Code-object extraction cancelled".to_string())
        } else {
            Ok(())
        }
    };
    check()?;
    if !std::fs::metadata(&range.path)
        .map_err(|e| format!("Cannot open recorded code object: {e}"))?
        .is_file()
    {
        return Err("Recorded code object is not a regular file".into());
    }
    let mut source = File::open(&range.path).map_err(|e| e.to_string())?;
    let before = source.metadata().map_err(|e| e.to_string())?;
    if !before.is_file() {
        return Err("Recorded code object is not a regular file".into());
    }
    let size = range.size.unwrap_or(before.len());
    let end = range
        .offset
        .checked_add(size)
        .ok_or("Code-object range overflows u64")?;
    if size < 64 || size > MAX_OBJECT_BYTES || end > before.len() {
        return Err(
            "Code-object range must contain 64 bytes to 256 MiB and fit inside the file".into(),
        );
    }
    source
        .seek(SeekFrom::Start(range.offset))
        .map_err(|e| e.to_string())?;
    let mut header = [0; 64];
    source.read_exact(&mut header).map_err(|e| e.to_string())?;
    if &header[..7] != b"\x7fELF\x02\x01\x01" || u16::from_le_bytes([header[18], header[19]]) != 224
    {
        return Err(
            "Recorded range does not start with a little-endian ELF64 AMDGPU object".into(),
        );
    }
    let mut output =
        tempfile::NamedTempFile::new_in(destination.parent().ok_or("Missing output directory")?)
            .map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    hash.update(header);
    output.write_all(&header).map_err(|e| e.to_string())?;
    let mut remaining = size - 64;
    let mut buffer = [0; 64 * 1024];
    while remaining > 0 {
        check()?;
        let count = remaining.min(buffer.len() as u64) as usize;
        source
            .read_exact(&mut buffer[..count])
            .map_err(|e| e.to_string())?;
        output
            .write_all(&buffer[..count])
            .map_err(|e| e.to_string())?;
        hash.update(&buffer[..count]);
        remaining -= count as u64;
    }
    let after = source.metadata().map_err(|e| e.to_string())?;
    if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
        return Err("Source file changed during extraction".into());
    }
    output.as_file().sync_all().map_err(|e| e.to_string())?;
    check()?;
    output
        .persist_noclobber(destination)
        .map_err(|e| format!("Cannot save code object; choose a new file: {e}"))?;
    Ok(Extraction {
        recorded_uri: uri.into(),
        remapped: replacement.is_some(),
        path: destination.display().to_string(),
        source: range.path.display().to_string(),
        offset: range.offset.to_string(),
        bytes: size.to_string(),
        sha256: format!("{:x}", hash.finalize()),
    })
}
