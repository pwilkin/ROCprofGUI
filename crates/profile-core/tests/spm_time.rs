use profile_core::load_capture;
use serde_json::json;
use std::fs;
fn capture(timestamps: &[u64]) -> serde_json::Value {
    json!({"rocprofiler-sdk-tool":[{"counters":[{"agent_id":{"handle":42},"id":{"handle":1},"name":"SQ_CYCLES"}],"callback_records":{"spm_counter_collection":[{"dispatch_data":{"dispatch_info":{"agent_id":{"handle":42}}},"records":timestamps.iter().enumerate().map(|(i,t)|json!({"counter_id":{"handle":1},"instance_id":1,"timestamp":t,"value":i})).collect::<Vec<_>>()}]}}]})
}
#[test]
fn spm_rebases_near_u64_max_before_float_conversion_and_stably_sorts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spm.json");
    fs::write(
        &path,
        capture(&[u64::MAX, u64::MAX - 2, u64::MAX - 1]).to_string(),
    )
    .unwrap();
    let data = load_capture(&path).unwrap();
    assert_eq!(data.origin, (u64::MAX - 2).to_string());
    assert_eq!(data.counters[0].points, vec![[0., 1.], [1., 2.], [2., 0.]]);
    assert_eq!(data.end, 2.);
}
#[test]
fn spm_rejects_inexact_relative_spans_including_ambiguous_wraps() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spm.json");
    let limit = 9_007_199_254_740_991u64;
    fs::write(&path, capture(&[100, 100 + limit]).to_string()).unwrap();
    assert_eq!(
        load_capture(&path).unwrap().counters[0].points[1][0],
        limit as f64
    );
    let mut separate = capture(&[100, 101 + limit]);
    separate["rocprofiler-sdk-tool"][0]["callback_records"]["spm_counter_collection"][0]
        ["records"][1]["instance_id"] = 2.into();
    fs::write(&path, separate.to_string()).unwrap();
    assert!(load_capture(&path)
        .unwrap_err()
        .contains("exact frontend timestamp precision"));
    for timestamps in [vec![100, 101 + limit], vec![u64::MAX - 1, 0]] {
        fs::write(&path, capture(&timestamps).to_string()).unwrap();
        assert!(load_capture(&path)
            .unwrap_err()
            .contains("exact frontend timestamp precision"));
    }
}
#[test]
fn repeated_spm_timestamps_retain_values_and_report_ambiguity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spm.json");
    fs::write(&path, capture(&[101, 100, 101, 102]).to_string()).unwrap();
    let data = load_capture(&path).unwrap();
    assert_eq!(
        data.counters[0].points,
        vec![[0., 1.], [1., 0.], [1., 2.], [2., 3.]]
    );
    assert!(data
        .warnings
        .iter()
        .any(|w| w.contains("1 repeated sample timestamps")
            && w.contains("no reset or clock-wrap interpretation")));
}
