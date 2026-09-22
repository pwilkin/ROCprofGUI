use std::{io::Write, path::Path};
pub const MAX_PLOT_BYTES: usize = 16 * 1024 * 1024;
pub fn validate(format: &str, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > MAX_PLOT_BYTES {
        return Err("Plot exceeds 16 MiB".into());
    }
    match format {
        "png" if bytes.starts_with(b"\x89PNG\r\n\x1a\n") => Ok(()),
        "svg"
            if std::str::from_utf8(bytes)
                .is_ok_and(|s| s.starts_with("<svg ") && s.ends_with("</svg>")) =>
        {
            Ok(())
        }
        "html"
            if std::str::from_utf8(bytes).is_ok_and(|s| {
                s.starts_with("<!doctype html><html>") && s.ends_with("</html>")
            }) =>
        {
            Ok(())
        }
        _ => Err("Invalid plot format or content".into()),
    }
}
pub fn write(path: &Path, format: &str, bytes: &[u8]) -> Result<(), String> {
    validate(format, bytes)?;
    let mut file =
        tempfile::NamedTempFile::new_in(path.parent().ok_or("Missing parent directory")?)
            .map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_preserves_bytes_and_rejects_invalid_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plot.svg");
        let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><text>sample</text></svg>";
        write(&path, "svg", svg).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), svg);
        assert!(write(&path, "png", svg).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), svg);
        assert!(validate("svg", &vec![0; MAX_PLOT_BYTES + 1]).is_err());
        assert!(validate("exe", b"123").is_err());
        let html = b"<!doctype html><html><body>Timeline pages</body></html>";
        let document = dir.path().join("timeline.html");
        write(&document, "html", html).unwrap();
        assert_eq!(std::fs::read(document).unwrap(), html);
        assert!(validate("html", b"<script>bad()</script>").is_err());
    }
}
