use std::{
    io::Write,
    path::{Path, PathBuf},
};

pub const MAX_CHUNK_BYTES: usize = 1024 * 1024;
pub const MAX_DOCUMENT_BYTES: u64 = 512 * 1024 * 1024;

pub struct TimelineFile {
    file: tempfile::NamedTempFile,
    target: PathBuf,
    bytes: u64,
    sequence: u64,
    tail: Vec<u8>,
}
impl TimelineFile {
    pub fn new(target: &Path) -> Result<Self, String> {
        let file = tempfile::NamedTempFile::new_in(
            target
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new(".")),
        )
        .map_err(|e| e.to_string())?;
        Ok(Self {
            file,
            target: target.into(),
            bytes: 0,
            sequence: 0,
            tail: Vec::new(),
        })
    }
    pub fn append(&mut self, sequence: u64, text: &str) -> Result<(), String> {
        if sequence != self.sequence {
            return Err("Out-of-order timeline export chunk".into());
        }
        if text.is_empty() || text.len() > MAX_CHUNK_BYTES {
            return Err("Timeline export chunk must contain 1 byte to 1 MiB".into());
        }
        if self.bytes + text.len() as u64 > MAX_DOCUMENT_BYTES {
            return Err("Timeline export exceeds 512 MiB".into());
        }
        if self.sequence == 0 && !text.starts_with("<!doctype html><html>") {
            return Err("Invalid timeline HTML header".into());
        }
        self.file
            .write_all(text.as_bytes())
            .map_err(|e| e.to_string())?;
        self.bytes += text.len() as u64;
        self.sequence += 1;
        self.tail.extend_from_slice(text.as_bytes());
        if self.tail.len() > 14 {
            self.tail.drain(..self.tail.len() - 14);
        }
        Ok(())
    }
    pub fn target(&self) -> &Path {
        &self.target
    }
    fn validate_and_sync(&self, chunks: u64) -> Result<(), String> {
        if chunks != self.sequence || self.bytes == 0 || !self.tail.ends_with(b"</body></html>") {
            return Err("Incomplete timeline HTML export".into());
        }
        self.file.as_file().sync_all().map_err(|e| e.to_string())
    }
    pub fn finish(self, chunks: u64) -> Result<u64, String> {
        self.validate_and_sync(chunks)?;
        self.file.persist(&self.target).map_err(|e| e.to_string())?;
        Ok(self.bytes)
    }
    pub fn finish_new(self, chunks: u64) -> Result<u64, String> {
        self.finish_new_checked(chunks, || Ok(()))
    }
    pub fn finish_new_checked(
        self,
        chunks: u64,
        before_commit: impl FnOnce() -> Result<(), String>,
    ) -> Result<u64, String> {
        self.validate_and_sync(chunks)?;
        before_commit()?;
        self.file
            .persist_noclobber(&self.target)
            .map_err(|e| e.to_string())?;
        Ok(self.bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn byte_budget_and_chunk_count_prevent_publication() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("timeline.html");
        let mut export = TimelineFile::new(&path).unwrap();
        export.append(0, "<!doctype html><html><body>").unwrap();
        export.bytes = MAX_DOCUMENT_BYTES;
        assert!(export.append(1, "x").unwrap_err().contains("512 MiB"));
        drop(export);
        assert!(!path.exists());
        let mut export = TimelineFile::new(&path).unwrap();
        export
            .append(0, "<!doctype html><html><body></body></html>")
            .unwrap();
        assert!(export.finish(2).is_err());
        assert!(!path.exists());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
    #[test]
    fn publication_is_atomic_and_abandoned_files_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("timeline.html");
        std::fs::write(&path, "existing").unwrap();
        {
            let mut export = TimelineFile::new(&path).unwrap();
            export.append(0, "<!doctype html><html><body>").unwrap();
            assert!(export.append(0, "duplicate").is_err());
            assert!(export.append(2, "skipped").is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "existing");
        }
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        let mut export = TimelineFile::new(&path).unwrap();
        export.append(0, "<!doctype html><html><body>λ").unwrap();
        export.append(1, "</body>").unwrap();
        export.append(2, "</html>").unwrap();
        let bytes = export.finish(3).unwrap();
        let actual = std::fs::read_to_string(&path).unwrap();
        assert_eq!(actual, "<!doctype html><html><body>λ</body></html>");
        assert_eq!(bytes, actual.len() as u64);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[test]
    fn invalid_or_incomplete_output_cannot_replace_destination() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("timeline.html");
        std::fs::write(&path, "existing").unwrap();
        let mut export = TimelineFile::new(&path).unwrap();
        assert!(export.append(0, "bad").is_err());
        assert!(export.append(0, &"x".repeat(MAX_CHUNK_BYTES + 1)).is_err());
        export.append(0, "<!doctype html><html><body>").unwrap();
        assert!(export.finish(1).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "existing");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}

#[derive(Default)]
pub struct CompletionReceipts(std::collections::VecDeque<(String, u64, u64)>);
impl CompletionReceipts {
    pub fn contains(&self, id: &str) -> bool {
        self.0.iter().any(|r| r.0 == id)
    }
    pub fn lookup(&self, id: &str, chunks: u64) -> Result<Option<u64>, String> {
        match self.0.iter().find(|r| r.0 == id) {
            Some((_, count, bytes)) if *count == chunks => Ok(Some(*bytes)),
            Some(_) => Err("Completed timeline export chunk count mismatch".into()),
            None => Ok(None),
        }
    }
    pub fn record(&mut self, id: String, chunks: u64, bytes: u64) {
        self.0.push_back((id, chunks, bytes));
        while self.0.len() > 32 {
            self.0.pop_front();
        }
    }
}

#[cfg(test)]
mod receipt_tests {
    use super::*;
    #[test]
    fn receipts_match_request_and_chunk_count_with_bounded_retention() {
        let mut receipts = CompletionReceipts::default();
        receipts.record("first".into(), 3, 500);
        assert_eq!(receipts.lookup("first", 3).unwrap(), Some(500));
        assert!(receipts.lookup("first", 4).is_err());
        assert_eq!(receipts.lookup("other", 3).unwrap(), None);
        for n in 0..32 {
            receipts.record(n.to_string(), 1, 10);
        }
        assert!(!receipts.contains("first"));
        assert_eq!(receipts.lookup("31", 1).unwrap(), Some(10));
    }
}

#[cfg(test)]
mod new_file_tests {
    use super::*;
    #[test]
    fn no_clobber_race_symlink_and_precommit_cancel_preserve_target() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new.html");
        let mut export = TimelineFile::new(&path).unwrap();
        export
            .append(0, "<!doctype html><html><body></body></html>")
            .unwrap();
        std::fs::write(&path, b"raced").unwrap();
        assert!(export.finish_new(1).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"raced");
        std::fs::remove_file(&path).unwrap();
        let mut export = TimelineFile::new(&path).unwrap();
        export
            .append(0, "<!doctype html><html><body></body></html>")
            .unwrap();
        assert!(export
            .finish_new_checked(1, || Err("Cancelled".into()))
            .is_err());
        assert!(!path.exists());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.path().join("missing"), &path).unwrap();
            let mut export = TimelineFile::new(&path).unwrap();
            export
                .append(0, "<!doctype html><html><body></body></html>")
                .unwrap();
            assert!(export.finish_new(1).is_err());
            assert!(std::fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink());
            std::fs::remove_file(&path).unwrap();
        }
        let mut export = TimelineFile::new(&path).unwrap();
        export
            .append(0, "<!doctype html><html><body></body></html>")
            .unwrap();
        assert!(export.finish_new(1).is_ok());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
