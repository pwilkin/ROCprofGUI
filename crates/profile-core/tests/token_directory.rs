use profile_core::token_directory::TokenDirectory;
use std::{
    fs,
    path::PathBuf,
    process::{Child, Command},
    time::{Duration, Instant},
};

struct Owner(Child);
impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn crash_owner_child() {
    let Some(parent) = std::env::var_os("ROCPROF_DIRECTORY_TEST_PARENT") else {
        return;
    };
    let parent = PathBuf::from(parent);
    let directory = TokenDirectory::create(&parent).unwrap();
    fs::write(directory.path().join("partial-index"), vec![42; 128 * 1024]).unwrap();
    fs::write(parent.join("ready"), directory.path().to_str().unwrap()).unwrap();
    loop {
        std::thread::park();
    }
}

#[test]
fn killed_process_cache_is_reaped_while_other_owner_survives() {
    let parent = tempfile::tempdir().unwrap();
    let live = TokenDirectory::create(parent.path()).unwrap();
    fs::write(live.path().join("index"), b"keep").unwrap();
    let mut child = Owner(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "crash_owner_child", "--nocapture"])
            .env("ROCPROF_DIRECTORY_TEST_PARENT", parent.path())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let path = loop {
        if let Ok(path) = fs::read_to_string(parent.path().join("ready")) {
            if !path.is_empty() {
                break PathBuf::from(path);
            }
        }
        assert!(
            Instant::now() < deadline,
            "Child did not initialize its cache"
        );
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "Child exited before ready"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    TokenDirectory::reap(parent.path()).unwrap();
    assert!(path.join("partial-index").exists());
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    assert!(path.join("partial-index").exists());
    TokenDirectory::reap(parent.path()).unwrap();
    assert!(!path.exists());
    assert_eq!(fs::read(live.path().join("index")).unwrap(), b"keep");
}
