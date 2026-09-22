use std::{io::Write, path::Path};
pub fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("Diagnostic manifest exceeds 16 MiB".into());
    }
    let report: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if report["schemaVersion"] != 1 || report["kind"] != "rocprof-studio-diagnostics" {
        return Err("Invalid diagnostic manifest schema".into());
    }
    let mut file =
        tempfile::NamedTempFile::new_in(path.parent().ok_or("Missing report directory")?)
            .map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn invalid_reports_do_not_replace_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("diagnostics.json");
        let bytes = br#"{"schemaVersion":1,"kind":"rocprof-studio-diagnostics","capture":{"name":"example"}}"#;
        super::write(&path, bytes).unwrap();
        for invalid in [
            b"{}".as_slice(),
            b"not json",
            br#"{"schemaVersion":2,"kind":"rocprof-studio-diagnostics"}"#,
        ] {
            assert!(super::write(&path, invalid).is_err());
        }
        assert!(super::write(&path, &vec![b' '; 16 * 1024 * 1024 + 1]).is_err());
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
}
