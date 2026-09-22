use profile_core::load_capture;
use serde_json::json;
use std::fs;
fn fixture(records: serde_json::Value) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (name, value) in [
        (
            "filenames.json",
            json!({"wave_filenames":{"0":{"0":{"0":{"0":["wave.json",0,100]}}}},"shaderdata_filenames":{"0":[["markers.json",0,100]]}}),
        ),
        (
            "wave.json",
            json!({"wave":{"cu":2,"begin":0,"end":100,"instructions":[[0,6,0,1,1]]}}),
        ),
        (
            "code.json",
            json!({"code":[["v_add",6,1,"test.hip:4",7,4096,1,1,0,0]],"sqtt_funcmap":[[7,1,"F","outer","test.hip:4"],[7,2,"U","inner"],[7,3,"P","point"],[8,1,"F","different code object"]]}),
        ),
        (
            "markers.json",
            json!({"records_schema":["time","value","cu","simd","wave_id","flags"],"records":records}),
        ),
    ] {
        fs::write(dir.path().join(name), value.to_string()).unwrap();
    }
    dir
}
#[test]
fn nested_scopes_transition_and_points_preserve_code_object_identity() {
    let d = fixture(json!([
        [10, 6, 2, 0, 0, 0],
        [20, 10, 2, 0, 0, 0],
        [25, 12, 2, 0, 0, 0],
        [30, 7, 2, 0, 0, 0],
        [40, 1, 2, 0, 0, 0],
        [50, 1, 2, 0, 0, 0]
    ]));
    let c = load_capture(d.path()).unwrap();
    let m = &c.markers;
    assert_eq!(m.len(), 4);
    assert_eq!(m[0].name, "outer");
    assert_eq!(m[0].end, Some(50.));
    assert_eq!(m[1].end, Some(30.));
    assert_eq!(m[1].parent, Some(0));
    assert!(m[2].point);
    assert_eq!(m[2].parent, Some(1));
    assert_eq!(m[3].start, 30.);
    assert_eq!(m[3].end, Some(40.));
    assert_eq!(m[3].depth, 1);
}
#[test]
fn incomplete_unknown_and_orphan_markers_remain_diagnostic() {
    let d = fixture(json!([
        [5, 1, 2, 0, 0, 0],
        [10, 6, 2, 0, 0, 0],
        [20, 398, 2, 0, 0, 0]
    ]));
    let c = load_capture(d.path()).unwrap();
    assert_eq!(c.markers.len(), 2);
    assert!(c.markers.iter().all(|m| m.end.is_none()));
    assert!(c.warnings.iter().any(|w| w.contains("Orphan")));
    assert!(c.warnings.iter().any(|w| w.contains("Unknown marker 99")));
    assert!(c.warnings.iter().any(|w| w.contains("remain open")));
}
#[test]
fn payloads_and_flagged_shader_records_are_not_misread_as_markers() {
    let d = fixture(json!([
        [1, 6, 2, 0, 0, 1],
        [10, 6, 2, 0, 0, 0],
        [11, 1, 2, 0, 0, 0],
        [12, 398, 2, 0, 0, 0],
        [20, 1, 2, 0, 0, 0]
    ]));
    let p = d.path().join("code.json");
    let mut code: serde_json::Value = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
    code["sqtt_funcmap_payloads"] = json!([[7, 1, 2]]);
    fs::write(p, code.to_string()).unwrap();
    let c = load_capture(d.path()).unwrap();
    assert_eq!(c.markers.len(), 1);
    assert_eq!(c.markers[0].end, Some(20.));
}
#[test]
fn missing_funcmap_or_context_does_not_turn_arbitrary_shaderdata_into_scopes() {
    let d = fixture(json!([[10, 6, 99, 0, 0, 0]]));
    let c = load_capture(d.path()).unwrap();
    assert!(c.markers.is_empty());
    assert!(c.warnings.iter().any(|w| w.contains("unambiguous")));
    let p = d.path().join("code.json");
    fs::write(p, json!({"code":[]}).to_string()).unwrap();
    let c = load_capture(d.path()).unwrap();
    assert!(c.markers.is_empty());
    assert!(c.warnings.iter().any(|w| w.contains("no marker funcmap")));
}
