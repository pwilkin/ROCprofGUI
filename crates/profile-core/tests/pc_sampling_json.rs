use profile_core::{capture_archive, load_capture, Capture};
use serde_json::{json, Value};
use std::{fs, sync::atomic::AtomicBool, sync::Arc};

fn sample(object: u64, offset: u64, issued: u64, reason: &str, index: i64) -> Value {
    json!({"inst_index": index, "record": {
        "timestamp": 18_000_000_000_000_000_000u64,
        "pc": {"code_object_id": object, "code_object_offset": offset},
        "wave_issued": issued,
        "snapshot": {"lck_err":0, "stall_reason":format!("ROCPROFILER_PC_SAMPLING_INSTRUCTION_NOT_ISSUED_REASON_{reason}")}
    }})
}
fn input() -> Value {
    json!({"rocprofiler-sdk-tool":[{
        "strings":{"pc_sample_instructions":["s_waitcnt vmcnt(0)","v_add_f32 v0, v1, v2"],"pc_sample_comments":["kernel.cpp:12",""]},
        "buffer_records":{"pc_sample_stochastic":[sample(4, 32, 0, "WAITCNT", 0), sample(4, 32, 1, "NONE", 0), sample(4, 40, 1, "NONE", 1)]}
    }]})
}
fn load(value: &Value) -> Result<Capture, String> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("samples.json");
    fs::write(&path, value.to_string()).unwrap();
    load_capture(&path)
}
#[test]
fn stochastic_aggregates_by_pc_with_exact_large_timestamps_and_no_timing_costs() {
    let mut value = input();
    value["rocprofiler-sdk-tool"][0]["buffer_records"]["pc_sample_stochastic"][2]["record"]
        ["timestamp"] = json!(18_000_000_000_000_000_003u64);
    let capture = load(&value).unwrap();
    assert_eq!(capture.origin, "18000000000000000000");
    assert_eq!(capture.end, 3.);
    assert_eq!(capture.instructions.len(), 2);
    let first = &capture.instructions[0];
    assert_eq!(first.pc_samples, 2);
    assert_eq!((first.pc_issued, first.pc_stalled), (Some(1), Some(1)));
    assert_eq!(first.stall_reasons.as_ref().unwrap()[3], 1);
    assert_eq!(first.stall_reasons.as_ref().unwrap().iter().sum::<u64>(), 1);
    assert_eq!(first.address, "0x20");
    assert_eq!(first.hits, 0);
    assert_eq!(first.latency, 0.);
    assert_eq!(capture.metadata["instruction_costs"], "incomplete");
    assert!(capture.waves.is_empty());
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("samples.rocprof-capture");
    let cancel = AtomicBool::new(false);
    capture_archive::write(&archive, &capture, &cancel).unwrap();
    let restored = capture_archive::read(&archive, &cancel).unwrap();
    assert_eq!(
        serde_json::to_value(&capture.instructions).unwrap(),
        serde_json::to_value(&restored.instructions).unwrap()
    );
    for (key, value) in &capture.metadata {
        assert_eq!(restored.metadata.get(key), Some(value));
    }
}
#[test]
fn same_assembly_at_distinct_addresses_objects_and_processes_stays_separate() {
    let mut value = input();
    let tool = &mut value["rocprofiler-sdk-tool"][0];
    tool["buffer_records"]["pc_sample_stochastic"] = json!([
        sample(4, 32, 0, "WAITCNT", 0),
        sample(4, 36, 0, "WAITCNT", 0),
        sample(5, 32, 0, "WAITCNT", 0)
    ]);
    let duplicate = tool.clone();
    value["rocprofiler-sdk-tool"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    let capture = load(&value).unwrap();
    assert_eq!(capture.instructions.len(), 6);
    assert!(capture.instructions.iter().all(|i| i.pc_samples == 1));
    assert_ne!(
        capture.instructions[0].code_object,
        capture.instructions[3].code_object
    );
}
#[test]
fn unresolved_samples_are_counted_and_lock_contention_does_not_discard_valid_samples() {
    let mut value = input();
    let samples = value["rocprofiler-sdk-tool"][0]["buffer_records"]["pc_sample_stochastic"]
        .as_array_mut()
        .unwrap();
    samples[0]["record"]["snapshot"]["lck_err"] = 1.into();
    samples.push(sample(0, u64::MAX, 0, "WAITCNT", -1));
    let capture = load(&value).unwrap();
    assert_eq!(capture.metadata["pc_sampling_records"], "4");
    assert_eq!(capture.metadata["pc_sampling_unresolved"], "1");
    assert_eq!(capture.metadata["pc_sampling_lock_errors"], "1");
    assert_eq!(
        capture
            .instructions
            .iter()
            .map(|i| i.pc_samples)
            .sum::<u64>(),
        3
    );
    assert!(capture
        .warnings
        .iter()
        .any(|w| w.contains("retained; other waves")));
}
#[test]
fn invalid_or_conflicting_records_fail_without_partial_import() {
    for (pointer, invalid) in [
        ("/record/wave_issued", json!(2)),
        ("/record/snapshot/lck_err", json!(true)),
        ("/record/snapshot/stall_reason", json!("FUTURE_REASON")),
        ("/record/pc/code_object_offset", json!(-1)),
        ("/record/timestamp", json!(1.5)),
        ("/inst_index", json!(99)),
        ("/record/pc/code_object_id", json!(0)),
    ] {
        let mut value = input();
        let sample =
            &mut value["rocprofiler-sdk-tool"][0]["buffer_records"]["pc_sample_stochastic"][0];
        *sample.pointer_mut(pointer).unwrap() = invalid;
        assert!(load(&value).is_err(), "{pointer}");
    }
    let mut value = input();
    value["rocprofiler-sdk-tool"][0]["buffer_records"]["pc_sample_stochastic"][1]["inst_index"] =
        1.into();
    assert!(load(&value).unwrap_err().contains("Conflicting"));
}
#[test]
fn unsupported_collections_are_not_silently_dropped() {
    let mut value = input();
    value["rocprofiler-sdk-tool"][0]["buffer_records"]["pc_sample_host_trap"] = json!([{}]);
    assert!(load(&value).unwrap_err().contains("host-trap"));
    let mut value = input();
    value["rocprofiler-sdk-tool"][0]["callback_records"] = json!({"spm_counter_collection":[{}]});
    assert!(load(&value).unwrap_err().contains("SPM agent ID"));
    let mut value = input();
    value["rocprofiler-sdk-tool"][0]["buffer_records"]["pc_sample_stochastic"] =
        json!({"bad":"schema"});
    assert!(load(&value).unwrap_err().contains("stochastic array"));
}

#[test]
fn sdk_reason_names_map_to_the_existing_viewer_categories() {
    let reasons = [
        "NONE",
        "NO_INSTRUCTION_AVAILABLE",
        "ALU_DEPENDENCY",
        "WAITCNT",
        "INTERNAL_INSTRUCTION",
        "BARRIER_WAIT",
        "ARBITER_NOT_WIN",
        "ARBITER_WIN_EX_STALL",
        "OTHER_WAIT",
        "SLEEP_WAIT",
    ];
    let mut value = input();
    value["rocprofiler-sdk-tool"][0]["buffer_records"]["pc_sample_stochastic"] = Value::Array(
        reasons
            .iter()
            .map(|reason| sample(4, 32, 0, reason, 0))
            .collect(),
    );
    let capture = load(&value).unwrap();
    assert_eq!(capture.instructions[0].stall_reasons, Some(vec![1; 10]));
    assert_eq!(capture.instructions[0].pc_stalled, Some(10));
}

fn host_input() -> Value {
    let mut value = input();
    let tool = &mut value["rocprofiler-sdk-tool"][0];
    let mut samples = tool["buffer_records"]["pc_sample_stochastic"].take();
    for sample in samples.as_array_mut().unwrap() {
        sample["record"]
            .as_object_mut()
            .unwrap()
            .remove("wave_issued");
        sample["record"].as_object_mut().unwrap().remove("snapshot");
    }
    tool["buffer_records"]["pc_sample_host_trap"] = samples;
    value
}

#[test]
fn host_trap_preserves_samples_without_fabricating_stochastic_measurements() {
    let capture = load(&host_input()).unwrap();
    assert_eq!(capture.metadata["pc_sampling_method"], "host-trap");
    assert_eq!(capture.instructions.len(), 2);
    assert_eq!(capture.instructions[0].pc_samples, 2);
    assert_eq!(capture.instructions[1].pc_samples, 1);
    assert!(!capture.metadata.contains_key("pc_sampling_lock_errors"));
    for instruction in &capture.instructions {
        assert_eq!(instruction.pc_issued, None);
        assert_eq!(instruction.pc_stalled, None);
        assert_eq!(instruction.stall_reasons, None);
        assert_eq!(instruction.hits, 0);
    }
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("host.rocprof-capture");
    let cancel = AtomicBool::new(false);
    capture_archive::write(&archive, &capture, &cancel).unwrap();
    let restored = capture_archive::read(&archive, &cancel).unwrap();
    assert_eq!(
        serde_json::to_value(&capture.instructions).unwrap(),
        serde_json::to_value(&restored.instructions).unwrap()
    );
    assert_eq!(restored.metadata["pc_sampling_method"], "host-trap");
}

#[test]
fn host_trap_identity_and_unresolved_samples_use_the_same_strict_pc_contract() {
    let mut value = host_input();
    let samples = value["rocprofiler-sdk-tool"][0]["buffer_records"]["pc_sample_host_trap"]
        .as_array_mut()
        .unwrap();
    let mut unknown = samples[0].clone();
    unknown["inst_index"] = (-1).into();
    unknown["record"]["pc"]["code_object_id"] = 0.into();
    samples.push(unknown);
    samples[1]["record"]["pc"]["code_object_offset"] = json!(u64::MAX);
    let capture = load(&value).unwrap();
    assert_eq!(capture.metadata["pc_sampling_unresolved"], "1");
    assert_eq!(capture.instructions.len(), 3);
    assert_eq!(capture.instructions[1].address, "0xffffffffffffffff");
    value["rocprofiler-sdk-tool"][0]["buffer_records"]["pc_sample_host_trap"][0]["record"]["pc"]
        ["code_object_offset"] = json!(-1);
    assert!(load(&value).unwrap_err().contains("offset"));
}

fn add_spm(value: &mut Value, timestamp: u64) {
    let tool = &mut value["rocprofiler-sdk-tool"][0];
    tool["counters"] = json!([{"agent_id":{"handle":42},"id":{"handle":1},"name":"SQ_WAVES","block":"SQ","units":"events"}]);
    tool["callback_records"] = json!({"spm_counter_collection":[{
        "dispatch_data":{"dispatch_info":{"agent_id":{"handle":42},"dispatch_id":17}},
        "records":[{"counter_id":{"handle":1},"instance_id":7,"timestamp":timestamp,"value":4},
                   {"counter_id":{"handle":1},"instance_id":7,"timestamp":timestamp+5,"value":9}]
    }]});
}

#[test]
fn combined_spm_sampling_retains_both_collections_on_an_exact_common_origin() {
    for host in [false, true] {
        for (timestamp, origin, points, end) in [
            (
                17_999_999_999_999_999_990u64,
                "17999999999999999990",
                vec![[0., 4.], [5., 9.]],
                11.,
            ),
            (
                18_000_000_000_000_000_010u64,
                "18000000000000000000",
                vec![[10., 4.], [15., 9.]],
                15.,
            ),
        ] {
            let mut value = if host { host_input() } else { input() };
            let original = load(&value).unwrap();
            add_spm(&mut value, timestamp);
            let capture = load(&value).unwrap();
            assert_eq!(capture.origin, origin);
            assert_eq!(capture.end, end);
            assert_eq!(capture.counters[0].points, points);
            assert!(capture.counters[0].scope.contains("dispatch 17"));
            assert_eq!(
                serde_json::to_value(&capture.instructions).unwrap(),
                serde_json::to_value(&original.instructions).unwrap()
            );
            assert_eq!(
                capture.metadata["pc_sampling_original_origin"],
                original.origin
            );
            assert!(capture.metadata["coverage"].contains("SPM and PC"));
            let root = tempfile::tempdir().unwrap();
            let archive = root.path().join("combined.rocprof-capture");
            let cancel = AtomicBool::new(false);
            capture_archive::write(&archive, &capture, &cancel).unwrap();
            let restored = capture_archive::read(&archive, &cancel).unwrap();
            assert_eq!(restored.origin, capture.origin);
            assert_eq!(
                serde_json::to_value(restored.counters).unwrap(),
                serde_json::to_value(capture.counters).unwrap()
            );
            assert_eq!(
                serde_json::to_value(restored.instructions).unwrap(),
                serde_json::to_value(capture.instructions).unwrap()
            );
        }
    }
}

#[test]
fn combined_json_rejects_inexact_spans_and_malformed_collections() {
    let mut value = input();
    add_spm(&mut value, 1);
    assert!(load(&value).unwrap_err().contains("span exceeds exact"));
    for (pointer, invalid) in [
        ("/counters", json!({})),
        ("/callback_records/spm_counter_collection", json!({})),
        (
            "/callback_records/spm_counter_collection/0/records",
            json!(null),
        ),
        (
            "/callback_records/spm_counter_collection/0/records/0/timestamp",
            json!(-1),
        ),
    ] {
        let mut value = input();
        add_spm(&mut value, 18_000_000_000_000_000_000);
        *value["rocprofiler-sdk-tool"][0]
            .pointer_mut(pointer)
            .unwrap() = invalid;
        assert!(load(&value).is_err(), "{pointer}");
    }
}

#[test]
fn issued_samples_do_not_require_or_attribute_an_inapplicable_stall_reason() {
    for reason in [
        json!(null),
        json!("not meaningful for issued samples"),
        json!(27),
    ] {
        let mut value = input();
        value["rocprofiler-sdk-tool"][0]["buffer_records"]["pc_sample_stochastic"][1]["record"]
            ["snapshot"]["stall_reason"] = reason;
        let capture = load(&value).unwrap();
        assert_eq!(capture.instructions[0].pc_issued, Some(1));
        assert_eq!(capture.instructions[0].pc_stalled, Some(1));
        assert_eq!(
            capture.instructions[0]
                .stall_reasons
                .as_ref()
                .unwrap()
                .iter()
                .sum::<u64>(),
            1
        );
        assert_eq!(
            capture.instructions[0].stall_reasons.as_ref().unwrap()[3],
            1
        );
    }
}

#[test]
fn explicit_method_selection_keeps_distributions_separate_and_retains_spm() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("mixed.json");
    let mut value = input();
    let host =
        host_input()["rocprofiler-sdk-tool"][0]["buffer_records"]["pc_sample_host_trap"][0].clone();
    value["rocprofiler-sdk-tool"][0]["buffer_records"]["pc_sample_host_trap"] = json!([host]);
    add_spm(&mut value, 18_000_000_000_000_000_010);
    fs::write(&path, value.to_string()).unwrap();
    assert_eq!(
        profile_core::sampling_methods(&path).unwrap(),
        ["host-trap", "stochastic"]
    );
    assert!(load_capture(&path)
        .unwrap_err()
        .contains("explicit method selection"));
    let host = profile_core::load_capture_options(&path, &Arc::new(AtomicBool::new(false)), true, Some("host-trap")).unwrap();
    let stochastic = profile_core::load_capture_options(&path, &Arc::new(AtomicBool::new(false)), true, Some("stochastic")).unwrap();
    assert_eq!(
        host.instructions.iter().map(|i| i.pc_samples).sum::<u64>(),
        1
    );
    assert_eq!(
        stochastic
            .instructions
            .iter()
            .map(|i| i.pc_samples)
            .sum::<u64>(),
        3
    );
    assert_eq!(host.metadata["pc_sampling_other_method_records"], "3");
    assert_eq!(stochastic.metadata["pc_sampling_other_method_records"], "1");
    assert_eq!(host.metadata["pc_sampling_selection"], "host-trap");
    assert_eq!(host.instructions[0].pc_issued, None);
    assert_eq!(stochastic.instructions[0].pc_issued, Some(1));
    assert_eq!(
        serde_json::to_value(&host.counters).unwrap(),
        serde_json::to_value(&stochastic.counters).unwrap()
    );
    assert!(profile_core::load_capture_options(&path, &Arc::new(AtomicBool::new(false)), true, Some("unknown")).is_err());
    value["rocprofiler-sdk-tool"][0]["buffer_records"]["pc_sample_host_trap"] = json!([]);
    fs::write(&path, value.to_string()).unwrap();
    assert!(
        profile_core::load_capture_options(&path, &Arc::new(AtomicBool::new(false)), true, Some("host-trap"))
            .unwrap_err()
            .contains("unavailable")
    );
}
