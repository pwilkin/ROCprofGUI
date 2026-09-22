use profile_core::source_files::{read, MAX_SOURCE_BYTES};
#[test]
fn source_reader_is_bounded_utf8_and_preserves_empty_files() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("kernel.hip");
    for text in ["", "// α\n__global__ void kernel() {}\n"] {
        std::fs::write(&path, text).unwrap();
        let file = read(&path).unwrap();
        assert_eq!(file.text, text);
        assert_eq!(file.name, "kernel.hip");
    }
    std::fs::write(&path, [0xff]).unwrap();
    assert!(read(&path).unwrap_err().contains("UTF-8"));
    std::fs::write(&path, [0]).unwrap();
    assert!(read(&path).unwrap_err().contains("NUL"));
    std::fs::write(&path, vec![b'x'; MAX_SOURCE_BYTES + 1]).unwrap();
    assert!(read(&path).unwrap_err().contains("2 MiB"));
    assert!(read(root.path()).unwrap_err().contains("regular"));
}

#[test]
fn snapshots_are_lazy_and_larger_sets_remain_addressable() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("filenames.json"), r#"{"gfxv":"gfx942"}"#).unwrap();
    std::fs::write(
        root.path().join("code.json"),
        r#"{"code":[["s_nop",0,1,null,1,0,0,0,0,0]]}"#,
    )
    .unwrap();
    let mut manifest = serde_json::Map::new();
    for i in 0..9 {
        let name = format!("{i}.hip");
        std::fs::write(root.path().join(&name), vec![b'x'; MAX_SOURCE_BYTES]).unwrap();
        manifest.insert(format!("build/{name}"), name.into());
    }
    std::fs::write(root.path().join("broken.hip"), [0xff]).unwrap();
    manifest.insert("build/broken.hip".into(), "broken.hip".into());
    std::fs::write(
        root.path().join("snapshots.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let capture = profile_core::load_capture(root.path()).unwrap();
    assert!(capture.sources.is_empty());
    assert_eq!(capture.source_files.len(), 10);
    assert_eq!(
        profile_core::source_files::read_snapshot(&capture, "/build/8.hip")
            .unwrap()
            .text
            .len(),
        MAX_SOURCE_BYTES
    );
    assert!(
        profile_core::source_files::read_snapshot(&capture, "/build/broken.hip")
            .unwrap_err()
            .contains("UTF-8")
    );
    assert!(
        profile_core::source_files::read_snapshot(&capture, "/etc/passwd")
            .unwrap_err()
            .contains("not in this capture")
    );
}

#[cfg(unix)]
#[test]
fn lazy_source_rechecks_symlinks_after_import() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("filenames.json"), "{}").unwrap();
    std::fs::write(
        root.path().join("code.json"),
        r#"{"code":[["s_nop",0,1,null,1,0,0,0,0,0]]}"#,
    )
    .unwrap();
    std::fs::write(
        root.path().join("snapshots.json"),
        r#"{"original.hip":"local.hip"}"#,
    )
    .unwrap();
    std::fs::write(root.path().join("local.hip"), "inside").unwrap();
    std::fs::write(outside.path().join("other.hip"), "outside").unwrap();
    let capture = profile_core::load_capture(root.path()).unwrap();
    std::fs::remove_file(root.path().join("local.hip")).unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("other.hip"),
        root.path().join("local.hip"),
    )
    .unwrap();
    assert!(
        profile_core::source_files::read_snapshot(&capture, "/original.hip")
            .unwrap_err()
            .contains("escapes")
    );
    assert!(
        profile_core::source_files::read_snapshot_page(&capture, "/original.hip", 1, 200)
            .unwrap_err()
            .contains("escapes")
    );
}

#[test]
fn source_directory_mapping_matches_prefix_boundaries_and_reports_missing_files() {
    use profile_core::source_files::map_directory;
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("sub")).unwrap();
    std::fs::write(root.path().join("a.hip"), "local a").unwrap();
    std::fs::write(root.path().join("sub/b.hip"), "local b").unwrap();
    let paths = vec![
        "/build/a.hip".into(),
        "/build/sub/b.hip".into(),
        "/build/missing.hip".into(),
        "/build-other/a.hip".into(),
    ];
    let batch = map_directory(&paths, "/build", root.path()).unwrap();
    assert_eq!(batch.files.len(), 2);
    assert_eq!(batch.files["/build/sub/b.hip"].text, "local b");
    assert_eq!(batch.missing, ["/build/missing.hip"]);
    assert!(map_directory(&paths, "/unmatched", root.path()).is_err());
    let relative = map_directory(&["a.hip".into()], ".", root.path()).unwrap();
    assert_eq!(relative.files["a.hip"].text, "local a");
    assert!(map_directory(&["/build/../a.hip".into()], "/build", root.path()).is_err());
    std::fs::write(root.path().join("sub/b.hip"), [0]).unwrap();
    assert!(map_directory(&paths, "/build", root.path())
        .unwrap_err()
        .contains("NUL"));
}
#[cfg(unix)]
#[test]
fn source_directory_mapping_rejects_symlinks_outside_selected_root() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.hip"), "outside").unwrap();
    std::os::unix::fs::symlink(outside.path().join("secret.hip"), root.path().join("a.hip"))
        .unwrap();
    assert!(profile_core::source_files::map_directory(
        &["/build/a.hip".into()],
        "/build",
        root.path()
    )
    .unwrap_err()
    .contains("escapes"));
}

#[test]
fn source_pages_read_beyond_two_mib_with_exact_line_numbers() {
    use profile_core::source_files::read_page;
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("large.hip");
    let mut file = std::fs::File::create(&path).unwrap();
    for n in 1..=100_000 {
        writeln!(file, "// line {n:06} α kernel source").unwrap();
    }
    drop(file);
    assert!(std::fs::metadata(&path).unwrap().len() > MAX_SOURCE_BYTES as u64);
    let page = read_page(&path, 99_801, 200).unwrap();
    assert_eq!(page.start_line, 99_801);
    assert_eq!(page.total_lines, 100_001);
    assert_eq!(page.lines.len(), 200);
    assert!(page.lines[0].contains("099801"));
    assert!(page.lines[199].contains("100000"));
    assert_eq!(read_page(&path, 100_001, 200).unwrap().lines, [""]);
    assert!(read_page(&path, 100_002, 200).unwrap().lines.is_empty());
}

#[test]
fn source_pages_match_split_semantics_and_bound_invalid_inputs() {
    use profile_core::source_files::{read_page, MAX_SNAPSHOT_BYTES};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.hip");
    for (text, expected) in [
        ("", vec![""]),
        ("a", vec!["a"]),
        ("a\n", vec!["a", ""]),
        ("a\r\nb\r", vec!["a", "b\r"]),
        ("α\nβ\n", vec!["α", "β", ""]),
    ] {
        std::fs::write(&path, text).unwrap();
        let page = read_page(&path, 1, 200).unwrap();
        assert_eq!(page.total_lines, expected.len());
        assert_eq!(page.lines, expected);
    }
    assert!(read_page(&path, 0, 200).is_err());
    assert!(read_page(&path, 1, 201).is_err());
    std::fs::write(&path, [b'a', b'\n', 0xff]).unwrap();
    assert!(read_page(&path, 1, 1).unwrap_err().contains("UTF-8"));
    std::fs::write(&path, [b'a', b'\n', 0]).unwrap();
    assert!(read_page(&path, 1, 1).unwrap_err().contains("NUL"));
    std::fs::write(&path, vec![b'x'; MAX_SOURCE_BYTES + 1]).unwrap();
    assert!(read_page(&path, 1, 200)
        .unwrap_err()
        .contains("line exceeds"));
    let line = "x".repeat(MAX_SOURCE_BYTES / 2);
    std::fs::write(&path, format!("{line}\n{line}\n{line}\n")).unwrap();
    assert!(read_page(&path, 1, 200)
        .unwrap_err()
        .contains("page exceeds"));
    assert_eq!(
        read_page(&path, 3, 1).unwrap().lines[0].len(),
        MAX_SOURCE_BYTES / 2
    );
    std::fs::File::create(&path)
        .unwrap()
        .set_len(MAX_SNAPSHOT_BYTES + 1)
        .unwrap();
    assert!(read_page(&path, 1, 200).unwrap_err().contains("512 MiB"));
}

#[test]
fn indexed_source_pages_reuse_validation_and_seek_to_distant_lines() {
    use profile_core::source_index::SourceCache;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("indexed.hip");
    let text = (0..100_000)
        .map(|n| format!("line {n}\n"))
        .collect::<String>();
    std::fs::write(&path, &text).unwrap();
    let mut cache = SourceCache::default();
    cache.read_page(&path, 1, 200).unwrap();
    let first = cache.stats();
    let page = cache.read_page(&path, 99_999, 200).unwrap();
    let second = cache.stats();
    assert_eq!(page.lines, ["line 99998", "line 99999", ""]);
    assert_eq!(second.builds, 1);
    assert_eq!(second.hits, 1);
    assert_eq!(second.scanned_bytes, first.scanned_bytes);
    assert!(second.page_bytes - first.page_bytes < 4096);
    assert!(second.index_bytes < 8192);
    assert_eq!(cache.read_page(&path, 100_002, 200).unwrap().lines.len(), 0);
}
#[test]
fn source_index_refreshes_after_edits_replacements_and_invalid_changes() {
    use profile_core::source_index::SourceCache;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("indexed.hip");
    std::fs::write(&path, "old\n").unwrap();
    let mut cache = SourceCache::default();
    cache.read_page(&path, 1, 200).unwrap();
    let previous_time = std::fs::metadata(&path).unwrap().modified().unwrap();
    let replacement = dir.path().join("replacement.hip");
    std::fs::write(&replacement, "new\n").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&replacement)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(previous_time))
        .unwrap();
    std::fs::rename(&replacement, &path).unwrap();
    assert_eq!(cache.read_page(&path, 1, 200).unwrap().lines, ["new", ""]);
    assert_eq!(cache.stats().builds, 2);
    std::fs::write(&path, "longer\nsource\n").unwrap();
    assert_eq!(cache.read_page(&path, 2, 1).unwrap().lines, ["source"]);
    assert_eq!(cache.stats().builds, 3);
    std::fs::write(&path, [b'a', b'\n', 0xff]).unwrap();
    assert!(cache.read_page(&path, 1, 1).unwrap_err().contains("UTF-8"));
    assert_eq!(cache.stats().files, 0);
}
#[test]
fn source_index_file_limit_evicts_old_entries() {
    use profile_core::source_index::SourceCache;
    let dir = tempfile::tempdir().unwrap();
    let mut cache = SourceCache::default();
    for n in 0..33 {
        let path = dir.path().join(format!("{n}.hip"));
        std::fs::write(&path, "line\n").unwrap();
        cache.read_page(&path, 1, 200).unwrap();
    }
    assert_eq!(cache.stats().files, 32);
    cache.read_page(&dir.path().join("32.hip"), 1, 200).unwrap();
    assert_eq!(cache.stats().hits, 1);
    cache.read_page(&dir.path().join("0.hip"), 1, 200).unwrap();
    assert_eq!(cache.stats().builds, 34);
    assert_eq!(cache.stats().files, 32);
}

#[cfg(unix)]
#[test]
fn restored_mtime_cannot_hide_an_in_place_source_edit() {
    use profile_core::source_index::SourceCache;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("same-size.hip");
    std::fs::write(&path, "ok\nfine\n").unwrap();
    let mut cache = SourceCache::default();
    cache.read_page(&path, 1, 1).unwrap();
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::fs::write(&path, [b'o', b'k', b'\n', b'f', b'i', b'n', 0xff, b'\n']).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(modified))
        .unwrap();
    assert!(cache.read_page(&path, 1, 1).unwrap_err().contains("UTF-8"));
    assert_eq!(cache.stats().files, 0);
}

#[test]
fn large_replacements_use_fingerprinted_references_and_reject_disk_changes() {
    use profile_core::{source_files::import_replacement, source_index::SourceCache};
    use sha2::{Digest, Sha256};
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("large.hip");
    let content = "// α\r\n".repeat(400_000);
    std::fs::write(&path, &content).unwrap();
    let mut cache = SourceCache::default();
    let file = import_replacement(&path, &mut cache).unwrap();
    assert!(file.text.is_empty());
    let reference = file.reference.unwrap();
    assert_eq!(reference.size, content.len() as u64);
    assert_eq!(
        reference.sha256,
        format!("{:x}", Sha256::digest(content.as_bytes()))
    );
    let page = cache.read_reference_page(&reference, 350001, 200).unwrap();
    assert_eq!(page.lines.len(), 200);
    assert_eq!(page.lines[0], "// α");
    assert_eq!(cache.stats().builds, 1);
    std::fs::write(&path, content.replacen("α", "β", 1)).unwrap();
    assert!(cache
        .read_reference_page(&reference, 1, 200)
        .unwrap_err()
        .contains("changed since"));
    std::fs::remove_file(&path).unwrap();
    assert!(cache
        .read_reference_page(&reference, 1, 200)
        .unwrap_err()
        .contains("unavailable"));
    std::fs::write(&path, "small").unwrap();
    let small = import_replacement(&path, &mut cache).unwrap();
    assert_eq!(small.text, "small");
    assert!(small.reference.is_none());
}

#[test]
fn folder_mapping_keeps_large_references_and_small_embedded_files() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("large.hip"), "line\n".repeat(500_000)).unwrap();
    std::fs::write(root.path().join("small.hip"), "small").unwrap();
    let batch = profile_core::source_files::map_directory(
        &["/build/large.hip".into(), "/build/small.hip".into()],
        "/build",
        root.path(),
    )
    .unwrap();
    assert!(batch.files["/build/large.hip"].reference.is_some());
    assert_eq!(batch.files["/build/small.hip"].text, "small");
}
