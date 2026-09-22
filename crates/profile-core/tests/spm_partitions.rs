use profile_core::{capture_archive, load_capture};
use serde_json::{json, Value};
use std::{fs, sync::atomic::AtomicBool};
fn input() -> Value {
    let group = |dispatch: Value, queue: u64, stream: u64, time: u64, value: u64| json!({"dispatch_data":{"dispatch_info":{"agent_id":{"handle":42},"dispatch_id":dispatch,"queue_id":{"handle":queue}}},"stream_id":{"handle":stream},"records":[{"counter_id":{"handle":1},"instance_id":0,"timestamp":time,"value":value}]});
    json!({"rocprofiler-sdk-tool":[{"counters":[{"agent_id":{"handle":42},"id":{"handle":1},"name":"A"}],"callback_records":{"spm_counter_collection":[group(json!(0),7,0,100,1),group(json!(0),7,0,200,2),group(json!(1),7,0,100,3),group(json!(0),8,0,100,4),group(json!(0),7,1,100,5),group(Value::Null,7,0,100,6),group(Value::Null,7,0,100,7)]}}]})
}
#[test]
fn spm_dispatch_queue_stream_and_unassociated_collections_stay_separate() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spm.json");
    fs::write(&path, input().to_string()).unwrap();
    let capture = load_capture(&path).unwrap();
    assert_eq!(capture.counters.len(), 6);
    let base = capture
        .counters
        .iter()
        .find(|s| s.scope.contains("dispatch 0 · queue 7 · stream 0"))
        .unwrap();
    assert_eq!(base.points, vec![[0., 1.], [100., 2.]]);
    for (identity, value) in [
        ("dispatch 1 · queue 7 · stream 0", 3.),
        ("dispatch 0 · queue 8 · stream 0", 4.),
        ("dispatch 0 · queue 7 · stream 1", 5.),
        ("collection 5 · queue 7 · stream 0", 6.),
        ("collection 6 · queue 7 · stream 0", 7.),
    ] {
        let series = capture
            .counters
            .iter()
            .find(|s| s.scope.contains(identity))
            .unwrap();
        assert_eq!(series.points, vec![[0., value]]);
    }
    assert!(capture
        .warnings
        .iter()
        .any(|w| w.contains("without dispatch IDs remain separate")));
    let archive = dir.path().join("capture.rocprof-capture");
    let flag = AtomicBool::new(false);
    capture_archive::write(&archive, &capture, &flag).unwrap();
    let restored = capture_archive::read(&archive, &flag).unwrap();
    assert_eq!(
        serde_json::to_value(capture.counters).unwrap(),
        serde_json::to_value(restored.counters).unwrap()
    );
}
#[test]
fn malformed_spm_partition_ids_do_not_become_unassociated_collections() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spm.json");
    for field in ["dispatch_id", "queue_id", "agent_id"] {
        let mut value = input();
        value["rocprofiler-sdk-tool"][0]["callback_records"]["spm_counter_collection"][0]
            ["dispatch_data"]["dispatch_info"][field] = json!(-1);
        fs::write(&path, value.to_string()).unwrap();
        assert!(load_capture(&path).unwrap_err().contains("Invalid SPM"));
    }
    let mut value = input();
    value["rocprofiler-sdk-tool"][0]["callback_records"]["spm_counter_collection"][0]
        ["stream_id"] = json!("broken");
    fs::write(&path, value.to_string()).unwrap();
    assert!(load_capture(&path)
        .unwrap_err()
        .contains("Invalid SPM stream ID"));
}
