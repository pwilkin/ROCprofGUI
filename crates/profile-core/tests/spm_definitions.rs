use profile_core::{capture_archive, load_capture};
use serde_json::json;
use std::{fs, sync::atomic::AtomicBool};
fn sample() -> serde_json::Value {
    json!({"rocprofiler-sdk-tool":[{
        "counters":[
            {"agent_id":{"handle":42},"id":{"handle":1},"name":"SAME_NAME","description":"Recorded hardware events","block":"SQ","expression":"","is_derived":0,"is_constant":false},
            {"agent_id":{"handle":42},"id":{"handle":2},"name":"SAME_NAME","description":"Derived rate","block":"SQ","expression":"A / B","is_derived":true,"is_constant":1,"units":"percent"}
        ],
        "callback_records":{"spm_counter_collection":[{"dispatch_data":{"dispatch_info":{"agent_id":{"handle":42}}},"records":[
            {"counter_id":{"handle":1},"instance_id":7,"timestamp":1000,"value":4},
            {"counter_id":{"handle":2},"instance_id":7,"timestamp":1000,"value":50}
        ]}]}
    }]})
}
#[test]
fn spm_definition_identity_units_and_flags_survive_archive_roundtrip() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("spm.json");
    fs::write(&input, sample().to_string()).unwrap();
    let capture = load_capture(&input).unwrap();
    assert_eq!(capture.counters.len(), 2);
    let a = &capture.counters[0];
    let b = &capture.counters[1];
    assert_ne!(a.scope, b.scope);
    assert_eq!(a.name, b.name);
    assert_eq!(a.points, vec![[0., 4.]]);
    assert_eq!(b.points, vec![[0., 50.]]);
    assert_eq!(a.unit, "unspecified");
    assert_eq!(b.unit, "percent");
    let d = a.definition.as_ref().unwrap();
    assert_eq!(d.id, "1");
    assert_eq!(d.block, "SQ");
    assert_eq!(d.description, "Recorded hardware events");
    assert_eq!(d.is_derived, Some(false));
    assert_eq!(d.is_constant, Some(false));
    let d = b.definition.as_ref().unwrap();
    assert_eq!(d.expression, "A / B");
    assert_eq!(d.is_derived, Some(true));
    assert_eq!(d.is_constant, Some(true));
    let archive = root.path().join("capture.rocprof-capture");
    let flag = AtomicBool::new(false);
    capture_archive::write(&archive, &capture, &flag).unwrap();
    let restored = capture_archive::read(&archive, &flag).unwrap();
    assert_eq!(
        serde_json::to_value(capture.counters).unwrap(),
        serde_json::to_value(restored.counters).unwrap()
    );
}
#[test]
fn spm_conflicting_definitions_and_invalid_flags_fail_explicitly() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("spm.json");
    let mut value = sample();
    value["rocprofiler-sdk-tool"][0]["counters"][1]["id"]["handle"] = 1.into();
    fs::write(&path, value.to_string()).unwrap();
    assert!(load_capture(&path)
        .unwrap_err()
        .contains("Conflicting SPM counter definitions"));
    let mut value = sample();
    value["rocprofiler-sdk-tool"][0]["counters"][0]["is_derived"] = 2.into();
    fs::write(&path, value.to_string()).unwrap();
    assert!(load_capture(&path)
        .unwrap_err()
        .contains("Invalid SPM counter is_derived"));
    let mut value = sample();
    value["rocprofiler-sdk-tool"][0]["counters"][0]
        .as_object_mut()
        .unwrap()
        .remove("is_derived");
    fs::write(&path, value.to_string()).unwrap();
    assert_eq!(
        load_capture(&path).unwrap().counters[0]
            .definition
            .as_ref()
            .unwrap()
            .is_derived,
        None
    );
}
