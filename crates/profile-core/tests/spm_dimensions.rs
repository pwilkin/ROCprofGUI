use profile_core::load_capture;
use serde_json::{json, Value};
use std::fs;
fn input(instance: u64) -> Value {
    json!({"rocprofiler-sdk-tool":[{"counters":[{"agent_id":{"handle":42},"id":{"handle":1},"name":"A"}],"callback_records":{"spm_counter_collection":[{"dispatch_data":{"dispatch_info":{"agent_id":{"handle":42}}},"records":[{"counter_id":{"handle":1},"instance_id":instance,"timestamp":100,"value":7}]}]}}]})
}
fn coordinates(value: &Value) -> (Value, Vec<String>) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("spm.json");
    fs::write(&path, value.to_string()).unwrap();
    let capture = load_capture(&path).unwrap();
    let scope = &capture.counters[0].scope;
    (
        serde_json::from_str(scope.split_once(" · counter 1 · ").unwrap().1).unwrap(),
        capture.warnings,
    )
}
#[test]
fn packed_spm_coordinates_preserve_all_sdk_fields_and_ignore_counter_bits() {
    let id = (0xabcd_u64 << 48)
        | (1023 << 36)
        | (6 << 30)
        | (5 << 24)
        | (4 << 18)
        | (3 << 12)
        | (2 << 6)
        | 1;
    let (dimensions, warnings) = coordinates(&input(id));
    let expected = [
        ("DIMENSION_XCC", 1),
        ("DIMENSION_AID", 2),
        ("DIMENSION_SHADER_ENGINE", 3),
        ("DIMENSION_AGENT", 4),
        ("DIMENSION_SHADER_ARRAY", 5),
        ("DIMENSION_WGP", 6),
        ("DIMENSION_INSTANCE", 1023),
    ];
    assert_eq!(dimensions.as_array().unwrap().len(), 7);
    for (name, index) in expected {
        assert!(dimensions
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["dimension_name"] == name && d["index"] == index));
    }
    assert!(warnings
        .iter()
        .any(|w| w.contains("SDK packed instance ID")));
    let (zero, _) = coordinates(&input(0));
    assert_eq!(zero.as_array().unwrap().len(), 3);
    assert!(zero.as_array().unwrap().iter().all(|d| d["index"] == 0));
}
#[test]
fn explicit_spm_dimensions_override_packed_layout_including_reserved_bits() {
    let id = 1u64 << 46;
    let mut value = input(id);
    value["rocprofiler-sdk-tool"][0]["counters"][0]["instances"] = json!([{"instance_id":id,"dimensions":[{"dimension_name":"DIMENSION_XCC","index":7},{"dimension_name":"CUSTOM_PARTITION","index":9}]}]);
    let (dimensions, warnings) = coordinates(&value);
    assert!(dimensions
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["dimension_name"] == "DIMENSION_XCC" && d["index"] == 7));
    assert!(!warnings
        .iter()
        .any(|w| w.contains("SDK packed instance ID")));
}
#[test]
fn ambiguous_and_malformed_spm_coordinates_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spm.json");
    fs::write(&path, input(1 << 46).to_string()).unwrap();
    assert!(load_capture(&path)
        .unwrap_err()
        .contains("reserved coordinate bits"));
    for dims in [
        json!([{"dimension_name":"X","index":0},{"dimension_name":"X","index":1}]),
        json!([{"dimension_name":"X","index":9007199254740992u64}]),
        json!([{"dimension_name":"X","index":-1}]),
    ] {
        let mut value = input(0);
        value["rocprofiler-sdk-tool"][0]["counters"][0]["instances"] =
            json!([{"instance_id":0,"dimensions":dims}]);
        fs::write(&path, value.to_string()).unwrap();
        assert!(load_capture(&path).unwrap_err().contains("SPM dimension"));
    }
    let mut value = input(0);
    value["rocprofiler-sdk-tool"][0]["counters"][0]["instances"] = json!([{"instance_id":0,"dimensions":[]},{"instance_id":0,"dimensions":[{"dimension_name":"X","index":1}]}]);
    fs::write(&path, value.to_string()).unwrap();
    assert!(load_capture(&path)
        .unwrap_err()
        .contains("Conflicting SPM instance dimensions"));
}
