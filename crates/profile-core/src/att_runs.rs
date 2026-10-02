use serde::Serialize;
use std::{collections::BTreeMap, fs, path::Path};

pub const SE_MARKER: &str = "_shader_engine_";

pub fn split_stem(stem: &str) -> Option<(&str, &str)> {
    let at = stem.to_ascii_lowercase().rfind(SE_MARKER)?;
    Some((&stem[..at], &stem[at + SE_MARKER.len()..]))
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttRun {
    pub prefix: String,
    pub run: u32,
    pub shader_engines: Vec<u32>,
    pub bytes: u64,
    pub path: String,
}
pub fn discover(path: &Path) -> Result<Vec<AttRun>, String> {
    if !path.is_dir() || path.join("filenames.json").is_file() {
        return Ok(vec![]);
    }
    let root = fs::canonicalize(path).map_err(|e| e.to_string())?;
    let mut groups: BTreeMap<(String, u32), BTreeMap<u32, (String, u64)>> = BTreeMap::new();
    let mut count = 0;
    for item in fs::read_dir(&root).map_err(|e| e.to_string())? {
        let item = item.map_err(|e| e.to_string())?;
        let file = item.path();
        if !file
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("att"))
        {
            continue;
        }
        count += 1;
        if count > 10000 {
            return Err("ATT discovery exceeds 10,000 files".into());
        }
        let name = item
            .file_name()
            .into_string()
            .map_err(|_| "ATT filename is not UTF-8")?;
        let stem = &name[..name.len() - 4];
        let (prefix, identity) = split_stem(stem).ok_or_else(|| {
            format!("Cannot infer SE/run from {name}; expected *_shader_engine_SE_RUN.att")
        })?;
        let fields: Vec<_> = identity.split('_').collect();
        if fields.len() != 2
            || fields
                .iter()
                .any(|f| f.is_empty() || !f.bytes().all(|b| b.is_ascii_digit()))
        {
            return Err(format!("Invalid ATT SE/run identity in {name}"));
        }
        let se = fields[0]
            .parse::<u32>()
            .map_err(|_| format!("ATT shader engine is out of range: {name}"))?;
        let run = fields[1]
            .parse::<u32>()
            .map_err(|_| format!("ATT run is out of range: {name}"))?;
        let canonical = fs::canonicalize(&file).map_err(|e| e.to_string())?;
        if !canonical.starts_with(&root) {
            return Err(format!("ATT file escapes selected directory: {name}"));
        }
        let metadata = fs::metadata(&canonical).map_err(|e| e.to_string())?;
        if !metadata.is_file() {
            return Err(format!("ATT input is not a regular file: {name}"));
        }
        let group = groups.entry((prefix.to_owned(), run)).or_default();
        if group
            .insert(
                se,
                (
                    file.to_str().ok_or("ATT path is not UTF-8")?.to_owned(),
                    metadata.len(),
                ),
            )
            .is_some()
        {
            return Err(format!("Duplicate shader engine {se} for ATT run {run}"));
        }
    }
    groups
        .into_iter()
        .map(|((prefix, run), files)| {
            let bytes = files.values().try_fold(0u64, |sum, (_, n)| {
                sum.checked_add(*n).ok_or("ATT size overflow")
            })?;
            let path = files
                .values()
                .next()
                .ok_or("ATT run has no files")?
                .0
                .clone();
            Ok(AttRun {
                prefix,
                run,
                shader_engines: files.keys().copied().collect(),
                bytes,
                path,
            })
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn groups_runs_by_prefix_and_numeric_identity() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "p_shader_engine_2_03.att",
            "p_shader_engine_0_3.att",
            "p_shader_engine_0_4.att",
            "q_shader_engine_0_3.ATT",
        ] {
            fs::write(dir.path().join(name), b"123").unwrap();
        }
        let runs = discover(dir.path()).unwrap();
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0].shader_engines, vec![0, 2]);
        assert_eq!(runs[0].bytes, 6);
        assert_eq!(runs[0].run, 3);
        fs::write(dir.path().join("p_shader_engine_00_03.att"), b"").unwrap();
        assert!(discover(dir.path()).unwrap_err().contains("Duplicate"));
    }
    #[test]
    fn decoded_inputs_and_malformed_names_are_distinguished() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("unknown.att"), b"").unwrap();
        assert!(discover(dir.path()).unwrap_err().contains("Cannot infer"));
        fs::write(dir.path().join("filenames.json"), b"{}").unwrap();
        assert!(discover(dir.path()).unwrap().is_empty());
    }
}
