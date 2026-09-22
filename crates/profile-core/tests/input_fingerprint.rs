use profile_core::input_fingerprint::compute;
use std::{fs, sync::atomic::AtomicBool};
#[test]
fn decoded_fingerprint_covers_lazy_files_sources_and_new_optional_sidecars() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("filenames.json"),
        r#"{"wave_filenames":{"0":{"0":{"0":{"0":["wave.json",0,1]}}}}}"#,
    )
    .unwrap();
    fs::write(root.join("wave.json"), "first").unwrap();
    fs::write(
        root.join("snapshots.json"),
        r#"{"kernel.hip":"source.hip"}"#,
    )
    .unwrap();
    fs::write(root.join("source.hip"), "source").unwrap();
    let flag = AtomicBool::new(false);
    let first = compute(root, &flag).unwrap();
    assert_eq!(first.files.len(), 4);
    fs::write(root.join("wave.json"), "other").unwrap();
    assert_ne!(first, compute(root, &flag).unwrap());
    let second = compute(root, &flag).unwrap();
    fs::write(root.join("se0_perfcounter.json"), "{}").unwrap();
    assert_ne!(second, compute(root, &flag).unwrap());
    fs::write(root.join("unrelated.txt"), "ignored").unwrap();
    assert_eq!(compute(root, &flag).unwrap().files.len(), 5);
    assert!(compute(root, &AtomicBool::new(true))
        .unwrap_err()
        .contains("cancelled"));
}
#[test]
fn manifest_and_selected_att_run_use_explicit_dependencies() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for name in [
        "p_shader_engine_0_1.att",
        "p_shader_engine_1_1.att",
        "p_shader_engine_0_2.att",
        "p_code_object_id_1.out",
    ] {
        fs::write(root.join(name), name).unwrap();
    }
    let flag = AtomicBool::new(false);
    let selected = compute(&root.join("p_shader_engine_0_1.att"), &flag).unwrap();
    assert_eq!(selected.files.len(), 3);
    assert!(!selected
        .files
        .contains_key(&root.join("p_shader_engine_0_2.att")));
    fs::write(root.join("explicit.rocprof-att"),r#"{"version":1,"run":2,"traces":[{"path":"p_shader_engine_0_2.att","shaderEngine":0}],"codeObjects":[]}"#).unwrap();
    let explicit = compute(&root.join("explicit.rocprof-att"), &flag).unwrap();
    assert_eq!(explicit.files.len(), 2);
}
#[test]
fn database_fingerprint_tracks_wal_addition_and_contents() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("capture.db");
    let flag = AtomicBool::new(false);
    fs::write(&file, "abc").unwrap();
    let first = compute(&file, &flag).unwrap();
    assert_eq!(
        first.files.values().next().unwrap().sha256,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let large = dir.path().join("too-large.bin");
    fs::File::create(&large)
        .unwrap()
        .set_len(128 * 1024 * 1024 * 1024 + 1)
        .unwrap();
    assert!(compute(&large, &flag).unwrap_err().contains("128 GiB"));
    fs::write(dir.path().join("capture.db-wal"), "wal").unwrap();
    assert_ne!(first, compute(&file, &flag).unwrap());
}
