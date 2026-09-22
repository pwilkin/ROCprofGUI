use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::Path,
};

const PREFIX: &str = "session-v1-";
const OWNER: &str = "owner.lock";
const MAGIC: &[u8] = b"rocprof-studio token index directory v1\n";

pub struct TokenDirectory {
    directory: tempfile::TempDir,
    _owner: File,
}

impl TokenDirectory {
    pub fn create(parent: &Path) -> io::Result<Self> {
        fs::create_dir_all(parent)?;
        Self::reap(parent)?;
        let directory = tempfile::Builder::new().prefix(PREFIX).tempdir_in(parent)?;
        let mut owner = File::create_new(directory.path().join(OWNER))?;
        owner.try_lock().map_err(io::Error::from)?;
        owner.write_all(MAGIC)?;
        Ok(Self {
            directory,
            _owner: owner,
        })
    }

    pub fn path(&self) -> &Path {
        self.directory.path()
    }

    pub fn reap(parent: &Path) -> io::Result<()> {
        for entry in fs::read_dir(parent)? {
            let Ok(entry) = entry else { continue };
            if !entry.file_name().to_string_lossy().starts_with(PREFIX)
                || !entry.file_type().is_ok_and(|kind| kind.is_dir())
            {
                continue;
            }
            let marker = entry.path().join(OWNER);
            if !fs::symlink_metadata(&marker).is_ok_and(|meta| meta.is_file()) {
                continue;
            }
            let Ok(mut owner) = File::options().read(true).write(true).open(marker) else {
                continue;
            };
            if owner.try_lock().is_err() {
                continue;
            }
            let mut signature = Vec::new();
            if (&mut owner)
                .take(MAGIC.len() as u64 + 1)
                .read_to_end(&mut signature)
                .is_err()
                || signature != MAGIC
            {
                continue;
            }
            let _ = fs::remove_dir_all(entry.path());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_directories_are_preserved_and_drop_removes_owned_data() {
        let parent = tempfile::tempdir().unwrap();
        let first = TokenDirectory::create(parent.path()).unwrap();
        let path = first.path().to_owned();
        fs::write(path.join("index"), b"live data").unwrap();
        let second = TokenDirectory::create(parent.path()).unwrap();
        assert_eq!(fs::read(path.join("index")).unwrap(), b"live data");
        drop(first);
        assert!(!path.exists());
        assert!(second.path().exists());
    }

    #[test]
    fn only_unlocked_versioned_owned_directories_are_reaped() {
        let parent = tempfile::tempdir().unwrap();
        for (name, marker) in [
            ("session-v1-stale", MAGIC),
            ("session-v1-unknown", b"unknown"),
            (".tmp-old", MAGIC),
        ] {
            let path = parent.path().join(name);
            fs::create_dir(&path).unwrap();
            fs::write(path.join(OWNER), marker).unwrap();
            fs::write(path.join("index"), b"data").unwrap();
        }
        let _current = TokenDirectory::create(parent.path()).unwrap();
        assert!(!parent.path().join("session-v1-stale").exists());
        assert!(parent.path().join("session-v1-unknown/index").exists());
        assert!(parent.path().join(".tmp-old/index").exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_do_not_authorize_cleanup() {
        use std::os::unix::fs::symlink;
        let parent = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        fs::write(external.path().join(OWNER), MAGIC).unwrap();
        symlink(external.path(), parent.path().join("session-v1-link")).unwrap();
        let path = parent.path().join("session-v1-marker-link");
        fs::create_dir(&path).unwrap();
        symlink(external.path().join(OWNER), path.join(OWNER)).unwrap();
        TokenDirectory::reap(parent.path()).unwrap();
        assert!(external.path().join(OWNER).exists());
        assert!(path.exists());
    }
}
