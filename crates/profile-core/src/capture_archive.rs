use crate::Capture;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Component, Path},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
const MAGIC: &[u8; 8] = b"RPCAP002";
const OBSERVATION_MAGIC: &[u8; 8] = b"RPCAP003";
const LEGACY_MAGIC: &[u8; 8] = b"RPCAP001";
const MAX_INSTRUCTION_CHUNK: usize = 16 * 1024 * 1024;
const MAX_INSTRUCTIONS: usize = 2_000_000;
const MAX_MANIFEST: usize = 128 * 1024 * 1024;
const MAX_TOTAL: u64 = 128 * 1024 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
struct Entry {
    path: String,
    size: u64,
    sha256: String,
}
#[derive(Serialize, Deserialize)]
struct Manifest {
    capture: Capture,
    sources: BTreeMap<String, String>,
    files: Vec<Entry>,
    #[serde(default)]
    instruction_chunks: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    observation_chunks: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    observation_count: Option<usize>,
}
struct BoundedWriter<'a, W> {
    inner: W,
    remaining: usize,
    flag: &'a AtomicBool,
}
impl<W: Write> Write for BoundedWriter<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.flag.load(Ordering::Relaxed) {
            return Err(std::io::Error::other("Capture archive cancelled"));
        }
        if bytes.len() > self.remaining {
            return Err(std::io::Error::other(
                "Capture archive JSON size limit exceeded",
            ));
        }
        let n = self.inner.write(bytes)?;
        self.remaining -= n;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
fn check(flag: &AtomicBool) -> Result<(), String> {
    if flag.load(Ordering::Relaxed) {
        Err("Capture archive cancelled".into())
    } else {
        Ok(())
    }
}
fn safe(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 8192
        && !name.contains(['\\', '\0', ':'])
        && Path::new(name)
            .components()
            .all(|p| matches!(p, Component::Normal(_)))
}
fn digest_file(path: &Path, flag: &AtomicBool) -> Result<(u64, String), String> {
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() > MAX_TOTAL {
        return Err("Invalid or oversized archive source".into());
    }
    let mut digest = Sha256::new();
    let mut size = 0;
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        check(flag)?;
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        size += n as u64;
        if size > MAX_TOTAL {
            return Err("Archive source exceeds 128 GiB".into());
        }
        digest.update(&buffer[..n]);
    }
    if size != meta.len()
        || file.metadata().map_err(|e| e.to_string())?.modified().ok() != meta.modified().ok()
    {
        return Err("Archive source changed while hashing".into());
    }
    Ok((size, format!("{:x}", digest.finalize())))
}
pub fn write(path: &Path, capture: &Capture, flag: &AtomicBool) -> Result<(), String> {
    write_impl(path, capture, flag, false)
}
pub fn write_new(path: &Path, capture: &Capture, flag: &AtomicBool) -> Result<(), String> {
    write_impl(path, capture, flag, true)
}
fn write_impl(
    path: &Path,
    capture: &Capture,
    flag: &AtomicBool,
    new_file: bool,
) -> Result<(), String> {
    check(flag)?;
    let destination = path.canonicalize().ok();
    if destination.as_ref().is_some_and(|p| {
        Some(p) == capture.input_path.as_ref()
            || capture
                .input_fingerprint
                .as_ref()
                .is_some_and(|f| f.files.contains_key(p))
    }) {
        return Err("Cannot overwrite capture input with archive".into());
    }
    if let (Some(input), Some(expected)) = (&capture.input_path, &capture.input_fingerprint) {
        if &crate::input_fingerprint::compute(input, flag)? != expected {
            return Err("Capture input changed since import; reopen it before archiving".into());
        }
    }
    let mut manifest = Manifest {
        capture: capture.clone_without_instructions(),
        sources: BTreeMap::new(),
        files: vec![],
        instruction_chunks: vec![],
        observation_chunks: vec![],
        observation_count: None,
    };
    manifest.capture.id.clear();
    let mut inputs = BTreeMap::new();
    let staging = tempfile::tempdir().map_err(|e| e.to_string())?;
    let instructions = &capture.instructions;
    if instructions.len() > MAX_INSTRUCTIONS {
        return Err("Capture archive exceeds two million instructions".into());
    }
    for (i, chunk) in instructions.chunks(2048).enumerate() {
        check(flag)?;
        let name = format!("instructions/{i}.json");
        let source = staging.path().join(format!("{i}.json"));
        let file = fs::File::create(&source).map_err(|e| e.to_string())?;
        let mut writer = BoundedWriter {
            inner: std::io::BufWriter::new(file),
            remaining: MAX_INSTRUCTION_CHUNK,
            flag,
        };
        serde_json::to_writer(&mut writer, chunk).map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())?;
        inputs.insert(name.clone(), source);
        manifest.instruction_chunks.push(name);
    }
    if let Some(observations) = &capture.pc_sampling_observations {
        if observations.rows.len() > MAX_INSTRUCTIONS
            || observations
                .rows
                .iter()
                .any(|row| row.len() != observations.columns.len())
        {
            return Err("Invalid capture archive observation count or row width".into());
        }
        manifest.observation_count = Some(observations.rows.len());
        manifest.capture.pc_sampling_observations = Some(Arc::new(crate::PcSamplingObservations {
            columns: observations.columns.clone(),
            rows: Vec::new(),
        }));
        let mut offset = 0;
        while offset < observations.rows.len() {
            check(flag)?;
            let mut size = 2;
            let mut end = offset;
            for row in observations.rows[offset..].iter().take(2048) {
                let comma = usize::from(end > offset);
                let available = MAX_INSTRUCTION_CHUNK.saturating_sub(size + comma);
                let Some(bytes) = crate::catalog_transfer::encoded_size(row, available, flag)?
                else {
                    if end == offset {
                        return Err(
                            "Capture archive observation exceeds the 16 MiB chunk limit".into()
                        );
                    }
                    break;
                };
                size += comma + bytes;
                end += 1;
            }
            let index = manifest.observation_chunks.len();
            let name = format!("observations/{index}.json");
            let source = staging.path().join(format!("observations-{index}.json"));
            let file = fs::File::create(&source).map_err(|e| e.to_string())?;
            let mut writer = BoundedWriter {
                inner: std::io::BufWriter::new(file),
                remaining: MAX_INSTRUCTION_CHUNK,
                flag,
            };
            serde_json::to_writer(&mut writer, &observations.rows[offset..end])
                .map_err(|e| e.to_string())?;
            writer.flush().map_err(|e| e.to_string())?;
            inputs.insert(name.clone(), source);
            manifest.observation_chunks.push(name);
            offset = end;
        }
    }
    let root = if capture.waves.is_empty() && capture.source_paths.is_empty() {
        capture.root.clone()
    } else {
        capture.root.canonicalize().map_err(|e| e.to_string())?
    };
    for (i, wave) in manifest.capture.waves.iter_mut().enumerate() {
        let source = root
            .join(&wave.id)
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if !source.starts_with(&root) {
            return Err("Wave archive source escapes capture root".into());
        }
        let name = format!("waves/{i}.json");
        inputs.insert(name.clone(), source);
        wave.id = name;
    }
    for (i, (name, path)) in capture.source_paths.iter().enumerate() {
        let source = root.join(path).canonicalize().map_err(|e| e.to_string())?;
        if !source.starts_with(&root) {
            return Err("Source archive entry escapes capture root".into());
        }
        let target = format!("sources/{i}.txt");
        manifest.sources.insert(name.clone(), target.clone());
        inputs.insert(target, source);
    }
    if inputs.len() > 100000 {
        return Err("Capture archive exceeds 100,000 files".into());
    }
    let mut total = 0u64;
    for (name, source) in &inputs {
        if destination.as_ref() == Some(source) {
            return Err("Cannot overwrite a capture sidecar with archive".into());
        }
        let (size, sha256) = digest_file(source, flag)?;
        if capture
            .input_fingerprint
            .as_ref()
            .and_then(|f| f.files.get(source))
            .is_some_and(|expected| expected.size != size || expected.sha256 != sha256)
        {
            return Err("Capture sidecar changed since import".into());
        }
        total = total.checked_add(size).ok_or("Archive size overflow")?;
        if total > MAX_TOTAL {
            return Err("Capture archive exceeds 128 GiB".into());
        }
        manifest.files.push(Entry {
            path: name.clone(),
            size,
            sha256,
        });
    }
    let mut writer = BoundedWriter {
        inner: Vec::new(),
        remaining: MAX_MANIFEST,
        flag,
    };
    serde_json::to_writer(&mut writer, &manifest).map_err(|e| e.to_string())?;
    let bytes = writer.inner;
    if bytes.len() > MAX_MANIFEST || total + 48 + bytes.len() as u64 > MAX_TOTAL {
        return Err("Capture archive exceeds manifest or total size limit".into());
    }
    let mut output =
        tempfile::NamedTempFile::new_in(path.parent().ok_or("Missing archive parent")?)
            .map_err(|e| e.to_string())?;
    output
        .write_all(if manifest.observation_count.is_some() {
            OBSERVATION_MAGIC
        } else {
            MAGIC
        })
        .and_then(|_| output.write_all(&(bytes.len() as u64).to_le_bytes()))
        .and_then(|_| output.write_all(&Sha256::digest(&bytes)))
        .and_then(|_| output.write_all(&bytes))
        .map_err(|e| e.to_string())?;
    let mut buffer = vec![0u8; 1024 * 1024];
    for entry in &manifest.files {
        let mut input = fs::File::open(&inputs[&entry.path]).map_err(|e| e.to_string())?;
        let mut digest = Sha256::new();
        let mut size = 0u64;
        loop {
            check(flag)?;
            let n = input.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            size += n as u64;
            if size > entry.size {
                return Err("Archive source changed while copying".into());
            }
            digest.update(&buffer[..n]);
            output.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
        }
        if size != entry.size || format!("{:x}", digest.finalize()) != entry.sha256 {
            return Err("Archive source changed while copying".into());
        }
    }
    check(flag)?;
    output.as_file().sync_all().map_err(|e| e.to_string())?;
    if new_file {
        output
            .persist_noclobber(path)
            .map_err(|e| format!("Choose a new capture archive path: {e}"))?;
    } else {
        output.persist(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}
pub fn read(path: &Path, flag: &AtomicBool) -> Result<Capture, String> {
    check(flag)?;
    let mut input = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut header = [0u8; 48];
    input.read_exact(&mut header).map_err(|e| e.to_string())?;
    let legacy = &header[..8] == LEGACY_MAGIC;
    let observation_version = &header[..8] == OBSERVATION_MAGIC;
    if &header[..8] != MAGIC && !legacy && !observation_version {
        return Err("Unsupported capture archive version".into());
    }
    let length = u64::from_le_bytes(header[8..16].try_into().unwrap());
    if length > MAX_MANIFEST as u64 {
        return Err("Capture archive manifest exceeds 128 MiB".into());
    }
    let archive_size = input.metadata().map_err(|e| e.to_string())?.len();
    if archive_size > MAX_TOTAL || archive_size < 48 + length {
        return Err("Capture archive length mismatch".into());
    }
    let mut bytes = vec![0u8; length as usize];
    input.read_exact(&mut bytes).map_err(|e| e.to_string())?;
    if Sha256::digest(&bytes)[..] != header[16..48] {
        return Err("Capture archive manifest checksum mismatch".into());
    }
    let mut manifest: Manifest = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let mut names = BTreeSet::new();
    let mut total = 48 + length;
    if manifest.files.len() > 100000 {
        return Err("Capture archive exceeds 100,000 files".into());
    }
    for entry in &manifest.files {
        if !safe(&entry.path)
            || !names.insert(entry.path.as_str())
            || entry.sha256.len() != 64
            || !entry
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("Invalid capture archive entry".into());
        }
        total = total
            .checked_add(entry.size)
            .ok_or("Archive size overflow")?;
        if total > MAX_TOTAL {
            return Err("Capture archive exceeds 128 GiB".into());
        }
    }
    if total != input.metadata().map_err(|e| e.to_string())?.len() {
        return Err("Capture archive length mismatch".into());
    }
    let mut wave_ids = BTreeSet::new();
    if !manifest.capture.start.is_finite()
        || !manifest.capture.end.is_finite()
        || manifest.capture.end < manifest.capture.start
        || manifest.capture.waves.iter().any(|w| {
            !names.contains(w.id.as_str())
                || !wave_ids.insert(&w.id)
                || !w.start.is_finite()
                || !w.end.is_finite()
                || w.end < w.start
        })
        || manifest
            .sources
            .values()
            .any(|p| !names.contains(p.as_str()))
    {
        return Err("Invalid capture archive references or bounds".into());
    }
    let mut chunk_names = BTreeSet::new();
    if (legacy && !manifest.instruction_chunks.is_empty())
        || (!legacy && !manifest.capture.instructions.is_empty())
        || manifest.instruction_chunks.len() > MAX_INSTRUCTIONS.div_ceil(2048)
        || manifest.instruction_chunks.iter().any(|name| {
            !name.starts_with("instructions/")
                || !chunk_names.insert(name)
                || !manifest
                    .files
                    .iter()
                    .any(|entry| entry.path == *name && entry.size <= MAX_INSTRUCTION_CHUNK as u64)
        })
    {
        return Err("Invalid capture archive instruction chunks".into());
    }
    let mut observation_names = BTreeSet::new();
    if (!observation_version
        && (manifest.observation_count.is_some() || !manifest.observation_chunks.is_empty()))
        || (observation_version
            && (manifest.observation_count.is_none()
                || manifest
                    .capture
                    .pc_sampling_observations
                    .as_ref()
                    .is_none_or(|o| !o.rows.is_empty())))
        || manifest
            .observation_count
            .is_some_and(|count| count > MAX_INSTRUCTIONS)
        || manifest.observation_chunks.len() > MAX_INSTRUCTIONS
        || manifest.observation_chunks.iter().any(|name| {
            !name.starts_with("observations/")
                || !observation_names.insert(name)
                || !manifest
                    .files
                    .iter()
                    .any(|entry| entry.path == *name && entry.size <= MAX_INSTRUCTION_CHUNK as u64)
        })
    {
        return Err("Invalid capture archive observation chunks".into());
    }
    let workspace = Arc::new(tempfile::tempdir().map_err(|e| e.to_string())?);
    let mut buffer = vec![0u8; 1024 * 1024];
    for entry in &manifest.files {
        check(flag)?;
        let target = workspace.path().join(&entry.path);
        fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)
            .map_err(|e| e.to_string())?;
        let mut remaining = entry.size;
        let mut digest = Sha256::new();
        while remaining > 0 {
            check(flag)?;
            let n = remaining.min(buffer.len() as u64) as usize;
            input
                .read_exact(&mut buffer[..n])
                .map_err(|e| e.to_string())?;
            digest.update(&buffer[..n]);
            output.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
            remaining -= n as u64;
        }
        if format!("{:x}", digest.finalize()) != entry.sha256 {
            return Err(format!("Capture archive checksum mismatch: {}", entry.path));
        }
    }
    for name in &manifest.instruction_chunks {
        check(flag)?;
        let bytes = fs::read(workspace.path().join(name)).map_err(|e| e.to_string())?;
        let chunk: Vec<crate::Instruction> =
            serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if chunk.len() > 2048
            || manifest.capture.instructions.len() + chunk.len() > MAX_INSTRUCTIONS
        {
            return Err("Capture archive instruction count limit exceeded".into());
        }
        manifest.capture.instructions.extend(chunk);
    }
    if observation_version {
        let observations =
            Arc::make_mut(manifest.capture.pc_sampling_observations.as_mut().unwrap());
        for name in &manifest.observation_chunks {
            check(flag)?;
            let bytes = fs::read(workspace.path().join(name)).map_err(|e| e.to_string())?;
            let chunk: Vec<Vec<String>> =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if chunk.is_empty()
                || chunk.len() > 2048
                || observations.rows.len() + chunk.len() > manifest.observation_count.unwrap()
                || chunk
                    .iter()
                    .any(|row| row.len() != observations.columns.len())
            {
                return Err("Invalid capture archive observation count or row width".into());
            }
            observations.rows.extend(chunk);
        }
        if observations.rows.len() != manifest.observation_count.unwrap() {
            return Err("Capture archive observation count mismatch".into());
        }
    }
    check(flag)?;
    manifest.capture.root = workspace.path().to_path_buf();
    manifest.capture.source_paths = manifest
        .sources
        .into_iter()
        .map(|(name, path)| (name, workspace.path().join(path)))
        .collect();
    manifest.capture.workspace = Some(workspace);
    manifest.capture.metadata.insert("archive".into(),format!("Portable normalized capture v{}; archive contents verified at extraction; original raw inputs and workspace preferences are not included", if legacy { 1 } else if observation_version { 3 } else { 2 }));
    Ok(manifest.capture)
}
