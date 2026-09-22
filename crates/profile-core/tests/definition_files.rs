use profile_core::definition_files::{read, write, MAX_DEFINITION_BYTES};
#[test]
fn definitions_round_trip_and_invalid_writes_preserve_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("counter.def");
    let text = "# occupancy μ\r\nTotal := sum[SQ_WAVES, axis=CU]\r\n";
    write(&path, text).unwrap();
    let imported = read(&path).unwrap();
    assert_eq!(imported.name, "counter.def");
    assert_eq!(imported.text, text);
    assert!(write(&path, "invalid\0text").is_err());
    assert_eq!(read(&path).unwrap().text, text);
    assert!(write(&path, &"x".repeat(MAX_DEFINITION_BYTES + 1)).is_err());
    assert_eq!(read(&path).unwrap().text, text);
    write(&path, "Changed := 1").unwrap();
    assert_eq!(read(&path).unwrap().text, "Changed := 1");
}
#[test]
fn import_rejects_binary_and_oversized_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("counter.def");
    for bytes in [vec![0xff], vec![0], vec![b'x'; MAX_DEFINITION_BYTES + 1]] {
        std::fs::write(&path, bytes).unwrap();
        assert!(read(&path).is_err());
    }
}
