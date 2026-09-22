use profile_core::att_manifest::{read, write, AttManifest};
use std::fs;
#[test]
fn explicit_manifest_resolves_files_and_preserves_large_code_object_ids() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("arbitrary.bin"), b"trace").unwrap();
    fs::write(dir.path().join("kernel.bin"), b"object").unwrap();
    let path = dir.path().join("capture.rocprof-att");
    fs::write(&path, r#"{"version":1,"run":7,"traces":[{"path":"arbitrary.bin","shaderEngine":2}],"codeObjects":[{"path":"kernel.bin","id":"18446744073709551615"}]}"#).unwrap();
    let manifest = read(&path).unwrap();
    assert!(manifest.traces[0].path.is_absolute());
    assert_eq!(manifest.code_objects[0].id, u64::MAX.to_string());
    write(&path, manifest).unwrap();
    assert_eq!(read(&path).unwrap().run, 7);
    assert_eq!(
        fs::read(dir.path().join("arbitrary.bin")).unwrap(),
        b"trace"
    );
    assert_eq!(fs::read(dir.path().join("kernel.bin")).unwrap(), b"object");
}
#[test]
fn manifest_rejects_ambiguous_missing_and_invalid_inputs_without_overwriting_them() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.rocprof-att");
    fs::write(&input, b"trace").unwrap();
    let make = || {
        serde_json::from_value::<AttManifest>(serde_json::json!({"version":1,"run":1,"traces":[{"path":input,"shaderEngine":0}],"codeObjects":[]})).unwrap()
    };
    assert!(write(&input, make()).unwrap_err().contains("overwrite"));
    assert_eq!(fs::read(&input).unwrap(), b"trace");
    let mut duplicate = make();
    duplicate.traces.push(profile_core::att_manifest::Trace {
        path: input.clone(),
        shader_engine: 1,
    });
    assert!(duplicate
        .resolve(dir.path())
        .unwrap_err()
        .contains("multiple shader"));
    let mut duplicate = make();
    duplicate.traces.push(profile_core::att_manifest::Trace {
        path: input.clone(),
        shader_engine: 0,
    });
    assert!(duplicate
        .resolve(dir.path())
        .unwrap_err()
        .contains("unique"));
    let mut invalid = make();
    invalid
        .code_objects
        .push(profile_core::att_manifest::Object {
            path: input.clone(),
            id: "18446744073709551616".into(),
        });
    assert!(invalid.resolve(dir.path()).unwrap_err().contains("64 bits"));
    let mut missing = make();
    missing.traces[0].path = dir.path().join("missing");
    assert!(missing.resolve(dir.path()).is_err());
    let mut empty = make();
    empty.traces.clear();
    assert!(empty.resolve(dir.path()).is_err());
}
