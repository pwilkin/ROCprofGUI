use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
};
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttManifest {
    pub version: u32,
    pub run: u32,
    pub traces: Vec<Trace>,
    pub code_objects: Vec<Object>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Trace {
    pub path: PathBuf,
    pub shader_engine: u32,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Object {
    pub path: PathBuf,
    pub id: String,
}
impl AttManifest {
    pub fn resolve(&mut self, root: &Path) -> Result<(), String> {
        let max_traces = crate::caps::max_att_traces();
        let max_code_objects = crate::caps::max_att_code_objects();
        if self.version != 1
            || self.traces.is_empty()
            || self.traces.len() > max_traces
            || self.code_objects.len() > max_code_objects
        {
            return Err(format!(
                "ATT manifest requires version 1, 1–{max_traces} traces and at most {max_code_objects} code objects"
            ));
        }
        fn file(path: &mut PathBuf, root: &Path) -> Result<(), String> {
            if path.as_os_str().len() > 8192 {
                return Err("ATT input path exceeds 8192 bytes".into());
            }
            if path.is_relative() {
                *path = root.join(&*path);
            }
            *path = fs::canonicalize(&*path).map_err(|e| e.to_string())?;
            let meta = fs::metadata(&*path).map_err(|e| e.to_string())?;
            if !meta.is_file() || meta.len() == 0 {
                return Err("ATT inputs must be nonempty regular files".into());
            }
            Ok(())
        }
        let mut engines = BTreeSet::new();
        let mut paths = BTreeSet::new();
        for trace in &mut self.traces {
            if trace.shader_engine > 255 || !engines.insert(trace.shader_engine) {
                return Err("Shader engines must be unique integers from 0 to 255".into());
            }
            file(&mut trace.path, root)?;
            if !paths.insert(trace.path.clone()) {
                return Err("A trace file cannot map to multiple shader engines".into());
            }
        }
        let mut ids = BTreeSet::new();
        for object in &mut self.code_objects {
            if object.id.is_empty() || !object.id.bytes().all(|b| b.is_ascii_digit()) {
                return Err("Code object IDs must be unsigned decimal 64-bit integers".into());
            }
            let id = object
                .id
                .parse::<u64>()
                .map_err(|_| "Code object ID exceeds 64 bits")?;
            if !ids.insert(id) {
                return Err("Duplicate code object ID".into());
            }
            object.id = id.to_string();
            file(&mut object.path, root)?;
        }
        Ok(())
    }
}
pub fn read(path: &Path) -> Result<AttManifest, String> {
    use std::io::Read;
    let metadata = fs::metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > 1024 * 1024 {
        return Err("ATT manifest must be a regular file of at most 1 MiB".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 1024 * 1024 {
        return Err("ATT manifest exceeds 1 MiB".into());
    }
    let mut manifest: AttManifest = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    manifest.resolve(path.parent().unwrap_or(Path::new(".")))?;
    Ok(manifest)
}
pub fn write(path: &Path, mut manifest: AttManifest) -> Result<(), String> {
    manifest.resolve(path.parent().unwrap_or(Path::new(".")))?;
    if !path
        .extension()
        .is_some_and(|s| s.eq_ignore_ascii_case("rocprof-att"))
    {
        return Err("Save ATT manifests with the .rocprof-att extension".into());
    }
    if let Ok(target) = fs::canonicalize(path) {
        if manifest.traces.iter().any(|t| t.path == target)
            || manifest.code_objects.iter().any(|o| o.path == target)
        {
            return Err("Cannot overwrite an ATT input with its manifest".into());
        }
    }
    let bytes = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
    if bytes.len() > 1024 * 1024 {
        return Err("ATT manifest exceeds 1 MiB".into());
    }
    let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))
        .map_err(|e| e.to_string())?;
    file.write_all(&bytes).map_err(|e| e.to_string())?;
    file.flush().map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
