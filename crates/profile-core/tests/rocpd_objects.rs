use profile_core::{capture_archive, load_capture};
use std::{fs, path::Path, sync::atomic::AtomicBool};
#[test]
fn official_rocpd_objects_preserve_provenance_precision_and_archive_identity() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("capture.db");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rocpd/sdk-3.0.4.db"),
        &path,
    )
    .unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    let table:String=db.query_row("SELECT name FROM sqlite_schema WHERE type='table' AND name LIKE 'rocpd_info_code_object%'",[],|row|row.get(0)).unwrap();
    db.execute_batch(&format!("UPDATE {table} SET load_base=9007199254740993,load_delta=-9007199254740993; CREATE TABLE rocpd_info_code_object_second AS SELECT * FROM {table}; UPDATE rocpd_info_code_object_second SET uri=NULL,storage_type='MEMORY',pid=101; CREATE VIEW rocpd_info_code_object AS SELECT * FROM {table};")).unwrap();
    drop(db);
    let before = fs::read(&path).unwrap();
    let capture = load_capture(&path).unwrap();
    assert_eq!(before, fs::read(&path).unwrap());
    assert_eq!(capture.code_objects.len(), 2);
    assert_ne!(capture.code_objects[0].id, capture.code_objects[1].id);
    for record in &capture.code_objects {
        assert_eq!(record.object_id, "1");
        assert_eq!(record.load_base.as_deref(), Some("9007199254740993"));
        assert_eq!(record.load_delta.as_deref(), Some("-9007199254740993"));
    }
    assert!(capture
        .code_objects
        .iter()
        .any(|r| r.uri.as_deref() == Some("file:///test#offset=0")));
    assert!(capture
        .code_objects
        .iter()
        .any(|r| r.storage.as_deref() == Some("MEMORY")
            && r.uri.is_none()
            && r.scope.contains("process 101")));
    let archive = root.path().join("objects.rocprof-capture");
    let flag = AtomicBool::new(false);
    capture_archive::write(&archive, &capture, &flag).unwrap();
    let restored = capture_archive::read(&archive, &flag).unwrap();
    assert_eq!(
        serde_json::to_value(capture.code_objects).unwrap(),
        serde_json::to_value(restored.code_objects).unwrap()
    );
}
#[test]
fn unsupported_code_object_contract_is_reported_without_losing_dispatches() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("capture.db");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rocpd/sdk-3.0.4.db"),
        &path,
    )
    .unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE rocpd_info_code_object_unknown (id INTEGER,uri TEXT)")
        .unwrap();
    drop(db);
    let capture = load_capture(&path).unwrap();
    assert!(!capture.dispatches.is_empty());
    assert!(!capture.code_objects.is_empty());
    assert!(capture
        .warnings
        .iter()
        .any(|w| w.contains("unsupported code-object column contract")));
}
