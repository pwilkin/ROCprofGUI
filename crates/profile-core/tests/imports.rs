use profile_core::{interval_union, load_capture, load_wave};
use serde_json::json;
use std::{fs, path::PathBuf};
fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/transpose-ui")
}
#[test]
fn decoded_capture_links_wave_isa_and_source() {
    let c = load_capture(&fixture()).unwrap();
    assert_eq!(c.waves.len(), 24);
    assert_eq!(c.intervals.len(), 192);
    assert_eq!(c.instructions.len(), 12);
    assert_eq!(c.counters.len(), 8);
    assert!(c.sources.is_empty());
    assert_eq!(c.source_files.len(), 1);
    assert!(
        profile_core::source_files::read_snapshot(&c, &c.source_files[0])
            .unwrap()
            .text
            .contains("transpose")
    );
    let w = load_wave(&c, &c.waves[0].id).unwrap();
    assert_eq!(w.tokens.len(), 33);
    assert_eq!(w.cu, Some(2));
    assert_eq!(w.tokens[2].instruction, 3);
    assert_eq!(w.tokens[3].stall, 88.);
    assert!(load_wave(&c, "not-in-capture.json").is_err());
}
#[test]
fn union_counts_overlap_once() {
    assert_eq!(
        interval_union(&[(0., 10.), (5., 15.), (20., 25.), (25., 30.), (8., 9.)]),
        25.
    );
}
#[test]
fn csv_preserves_nanosecond_precision_and_dispatch_identity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("trace.csv");
    fs::write(&path,"Process_Id,Agent_Id,Queue_Id,Dispatch_Id,Kernel_Name,Start_Timestamp,End_Timestamp,VGPR_Count,Scratch_Size\n1,0,3,8,\"kernel<int, 4>\",18000000000000001,18000000000000008,32,0\n1,0,3,8,\"kernel<int, 4>\",18000000000000001,18000000000000008,32,0\n2,0,3,8,other,18000000000000002,18000000000000010,,\n").unwrap();
    let c = load_capture(&path).unwrap();
    assert_eq!(c.origin, "18000000000000001");
    assert_eq!(c.dispatches.len(), 2);
    assert_eq!(c.dispatches[0].end, 7.);
    assert_eq!(c.dispatches[1].start, 1.);
    assert_eq!(c.dispatches[0].scratch, Some(0));
    assert_eq!(c.dispatches[1].scratch, None);
    assert!(c.warnings.iter().any(|w| w.contains("1 exact duplicate")));
}
#[test]
fn csv_rejects_negative_duration_and_missing_headers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("trace.csv");
    fs::write(
        &path,
        "Kernel_Name,Start_Timestamp,End_Timestamp\nx,100,90\n",
    )
    .unwrap();
    assert!(load_capture(&path).unwrap_err().contains("before"));
    fs::write(&path, "Kernel_Name,Duration\nx,50\n").unwrap();
    assert!(load_capture(&path).unwrap_err().contains("missing"));
}
fn minimal(dir: &std::path::Path) {
    fs::write(
        dir.join("filenames.json"),
        json!({"gfxv":"gfx1151","wave_filenames":{"0":{"0":{"0":{"0":["wave.json",10,20]}}}}})
            .to_string(),
    )
    .unwrap();
}
#[test]
fn incomplete_occupancy_is_reported_not_invented() {
    let dir = tempfile::tempdir().unwrap();
    minimal(dir.path());
    fs::write(dir.path().join("occupancy.json"),json!({"dispatches":{"1":"kernel"},"0":[[5,0,0,0,0,1],[10,0,0,1,1,1],[20,0,0,1,0,1],[25,0,0,2,1,1]]}).to_string()).unwrap();
    let c = load_capture(dir.path()).unwrap();
    assert_eq!(c.intervals.len(), 1);
    assert!(c.warnings.iter().any(|w| w.contains("2 unmatched")));
}
#[test]
fn occupancy_keeps_dispatchers_separate() {
    let dir = tempfile::tempdir().unwrap();
    minimal(dir.path());
    fs::write(dir.path().join("occupancy.json"),json!({"0":[[1,0,0,0,1,1,0,0,0,0,0],[2,0,0,0,1,1,1,0,0,0,0],[3,0,0,0,0,1,0,0,0,0,0],[4,0,0,0,0,1,1,0,0,0,0,0]]}).to_string()).unwrap();
    let c = load_capture(dir.path()).unwrap();
    assert_eq!(c.intervals.len(), 2);
    assert_eq!(c.intervals[0].start, 1.);
    assert_eq!(c.intervals[1].start, 2.);
}
#[test]
fn wave_sidecar_cannot_escape_capture_root() {
    let dir = tempfile::tempdir().unwrap();
    minimal(dir.path());
    let outside = tempfile::NamedTempFile::new().unwrap();
    fs::write(
        dir.path().join("filenames.json"),
        json!({"wave_filenames":{"0":{"0":{"0":{"0":[outside.path().to_str().unwrap(),10,20]}}}}})
            .to_string(),
    )
    .unwrap();
    let c = load_capture(dir.path()).unwrap();
    assert!(load_wave(&c, &c.waves[0].id)
        .unwrap_err()
        .contains("escapes"));
}
#[test]
#[cfg(unix)]
fn symlink_sidecar_cannot_escape_capture_root() {
    let dir = tempfile::tempdir().unwrap();
    minimal(dir.path());
    let outside = tempfile::NamedTempFile::new().unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("wave.json")).unwrap();
    let c = load_capture(dir.path()).unwrap();
    assert!(load_wave(&c, "wave.json").unwrap_err().contains("escapes"));
}
#[test]
fn malformed_wave_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    minimal(dir.path());
    fs::write(
        dir.path().join("wave.json"),
        json!({"wave":{"begin":10,"end":20,"instructions":[[10,1,-2,3,1]]}}).to_string(),
    )
    .unwrap();
    let c = load_capture(dir.path()).unwrap();
    assert!(load_wave(&c, "wave.json").unwrap_err().contains("Negative"));
}
#[test]
fn spm_keeps_instances_and_absolute_clock_origin() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spm.json");
    fs::write(&path,json!({"rocprofiler-sdk-tool":[{"counters":[{"agent_id":{"handle":42},"id":{"handle":1},"name":"SQ_CYCLES"}],"callback_records":{"spm_counter_collection":[{"dispatch_data":{"dispatch_info":{"agent_id":{"handle":42}}},"records":[{"counter_id":{"handle":1},"instance_id":1,"timestamp":18000000000000001u64,"value":4},{"counter_id":{"handle":1},"instance_id":2,"timestamp":18000000000000002u64,"value":7}]}]}}]}).to_string()).unwrap();
    let c = load_capture(&path).unwrap();
    assert_eq!(c.counters.len(), 2);
    assert_eq!(c.counters[1].points[0], [1., 7.]);
    assert!(c.warnings[0].contains("no automatic clock alignment"));
}

#[test]
fn window_queries_are_bounded_and_do_not_silently_sample() {
    let mut c = load_capture(&fixture()).unwrap();
    let waves = profile_core::load_wave_window(&c, 0, 0., 10000.).unwrap();
    assert_eq!(waves.len(), 12);
    let index = c.waves[0].clone();
    c.waves = vec![index; 129];
    assert!(profile_core::load_wave_window(&c, 0, 0., 10000.)
        .unwrap_err()
        .contains("128"));
}
#[test]
fn invalid_counter_value_does_not_turn_into_zero() {
    let dir = tempfile::tempdir().unwrap();
    minimal(dir.path());
    fs::write(
        dir.path().join("se0_perfcounter.json"),
        json!({"data":[[10,"bad",2,3,4,0,0]]}).to_string(),
    )
    .unwrap();
    assert!(load_capture(dir.path())
        .unwrap_err()
        .contains("counter value"));
}

#[test]
fn wait_records_preserve_isa_ids_and_reject_malformed_sources() {
    let dir = tempfile::tempdir().unwrap();
    minimal(dir.path());
    let path = dir.path().join("wave.json");
    let mut wave =
        json!({"wave":{"begin":10,"end":20,"instructions":[],"waitcnt":[[400,[[300,0],[301,1]]]]}});
    fs::write(&path, wave.to_string()).unwrap();
    let capture = load_capture(dir.path()).unwrap();
    assert_eq!(load_wave(&capture, "wave.json").unwrap().waits[0][0], 400);
    wave["wave"]["waitcnt"][0][1] = json!([["300", 0]]);
    fs::write(&path, wave.to_string()).unwrap();
    assert!(load_wave(&capture, "wave.json")
        .unwrap_err()
        .contains("wait source ISA identity"));
}

#[test]
fn occupancy_end_without_dispatcher_uses_unique_active_identity() {
    let dir = tempfile::tempdir().unwrap();
    minimal(dir.path());
    fs::write(
        dir.path().join("occupancy.json"),
        json!({"0":[[10,1,0,0,1,2,1,2],[20,1,0,0,0,2,0,0]]}).to_string(),
    )
    .unwrap();
    let capture = load_capture(dir.path()).unwrap();
    assert_eq!(capture.intervals.len(), 1);
    assert_eq!(capture.intervals[0].end, 20.);
    fs::write(
        dir.path().join("occupancy.json"),
        json!({"0":[[10,1,0,0,1,2,1,2],[11,1,0,0,1,2,2,2],[20,1,0,0,0,2,0,0]]}).to_string(),
    )
    .unwrap();
    assert!(load_capture(dir.path()).unwrap().intervals.is_empty());
}

#[test]
fn summary_preserves_missing_instruction_cost_coverage() {
    let dir = tempfile::tempdir().unwrap();
    minimal(dir.path());
    for (costs, expected) in [
        (json!(["s_nop", 0, 1, null, 1, 4096, 1, 10]), "incomplete"),
        (
            json!(["s_nop", 0, 1, null, 1, 4096, 1, 10, 2, null]),
            "incomplete",
        ),
        (
            json!(["s_nop", 0, 1, null, 1, 4096, 1, 10, 2, 0]),
            "complete",
        ),
    ] {
        fs::write(
            dir.path().join("code.json"),
            json!({"code":[costs]}).to_string(),
        )
        .unwrap();
        let capture = load_capture(dir.path()).unwrap();
        assert_eq!(capture.metadata["instruction_costs"], expected);
    }
}

#[test]
fn pc_sampling_preserves_optional_counts_and_unknown_reasons() {
    let dir = tempfile::tempdir().unwrap();
    minimal(dir.path());
    let header = json!([
        "ISA",
        "_",
        "LineNumber",
        "Source",
        "Codeobj",
        "Vaddr",
        "Hit",
        "Latency",
        "Stall",
        "Idle",
        "Stall_Reasons",
        "PC_Stalled",
        "PC_Issued"
    ]);
    let path = dir.path().join("code.json");
    fs::write(&path, json!({"header":header,"code":[["s_nop",0,1,null,1,4096,0,0,0,0,[0,3,0,0,0,0,0,0,0,0,7],3,2],["s_endpgm",0,2,null,1,4100,0,0,0,0,null,null,0]]}).to_string()).unwrap();
    let c = load_capture(dir.path()).unwrap();
    assert_eq!(c.instructions[0].pc_samples, 5);
    assert_eq!(c.instructions[0].pc_issued, Some(2));
    assert_eq!(c.instructions[0].pc_stalled, Some(3));
    assert_eq!(c.instructions[0].stall_reasons.as_ref().unwrap()[10], 7);
    assert_eq!(c.instructions[1].pc_issued, Some(0));
    assert_eq!(c.instructions[1].pc_stalled, None);
    assert_eq!(c.instructions[1].stall_reasons, None);
    for invalid in [
        json!(-1),
        json!(1.5),
        json!("2"),
        json!(9007199254740992u64),
    ] {
        fs::write(
            &path,
            json!({"header":header,"code":[["s_nop",0,1,null,1,4096,0,0,0,0,[],invalid,0]]})
                .to_string(),
        )
        .unwrap();
        assert!(load_capture(dir.path()).unwrap_err().contains("PC sample"));
    }
    fs::write(
        &path,
        json!({"header":header,"code":[["s_nop",0,1,null,1,4096,0,0,0,0,"invalid",0,0]]})
            .to_string(),
    )
    .unwrap();
    assert!(load_capture(dir.path())
        .unwrap_err()
        .contains("Stall_Reasons"));
}

#[test]
fn decoder_events_preserve_payloads_unknown_types_and_capture_bounds() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("filenames.json"), "{}").unwrap();
    let path = dir.path().join("occupancy.json");
    std::fs::write(&path, r#"{"events":{"2":[
      {"kind":"event","time":5,"type":6,"payload":18446744073709551615,"byte_offset":9007199254740993,"flags":0},
      {"kind":"dispatch","time":10,"kernel_name":"same","entry_point":{"address":18446744073709551615,"code_object_id":9007199254740993}},
      {"kind":"event","time":99,"type":999,"future_field":"retained"}
    ]}}"#).unwrap();
    let capture = profile_core::load_capture(dir.path()).unwrap();
    assert_eq!((capture.start, capture.end), (5., 99.));
    assert_eq!(capture.decoder_events.len(), 3);
    assert_eq!(capture.decoder_events[0].se, 2);
    assert_eq!(
        capture.decoder_events[0].fields["payload"],
        "18446744073709551615"
    );
    assert_eq!(
        capture.decoder_events[0].fields["byte_offset"],
        "9007199254740993"
    );
    assert!(capture.decoder_events[1].fields["entry_point"].contains("18446744073709551615"));
    assert_eq!(capture.decoder_events[2].fields["future_field"], "retained");
    assert_eq!(capture.dispatches.len(), 1);
    assert_eq!(capture.dispatches[0].start, capture.dispatches[0].end);
    std::fs::write(&path, r#"{"events":{"2":[{"kind":"event","time":"bad"}]}}"#).unwrap();
    assert!(profile_core::load_capture(dir.path())
        .unwrap_err()
        .contains("event time"));
}

#[test]
fn other_simd_records_preserve_schema_identity_and_capture_bounds() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("filenames.json"),
        json!({"other_simd_filenames":{"2":[["other.json",0,99]],"3":[["legacy.json",0,99]]}})
            .to_string(),
    )
    .unwrap();
    fs::write(dir.path().join("other.json"), json!({"wgp":7,"instructions_schema":["category","duration","time"],"instructions":[[4,12,30],[99,0,42]]}).to_string()).unwrap();
    fs::write(
        dir.path().join("legacy.json"),
        json!({"instructions":[[10,5]]}).to_string(),
    )
    .unwrap();
    let capture = load_capture(dir.path()).unwrap();
    assert_eq!((capture.start, capture.end), (10., 42.));
    assert_eq!(capture.other_simd.len(), 3);
    let record = &capture.other_simd[0];
    assert_eq!(
        (record.se, record.wgp, record.category),
        (2, Some(7), Some(4))
    );
    assert_eq!((record.start, record.duration), (30., 12.));
    assert_eq!(record.source, "other.json");
    assert_eq!(capture.other_simd[1].category, Some(99));
    assert_eq!(
        (capture.other_simd[2].wgp, capture.other_simd[2].category),
        (None, None)
    );
}

#[test]
fn other_simd_named_columns_support_reordering_extensions_and_declared_counts() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("filenames.json"),
        json!({"other_simd_filenames":{"0":[["other.json",0,99]]}}).to_string(),
    )
    .unwrap();
    for schema in [
        vec!["time", "duration", "category", "extension"],
        vec!["category", "extension", "duration", "time"],
        vec!["duration", "time", "extension", "category"],
    ] {
        let row: Vec<_> = schema
            .iter()
            .map(|name| match *name {
                "time" => json!(30),
                "duration" => json!(12),
                "category" => json!(4),
                _ => json!({"opaque":true}),
            })
            .collect();
        fs::write(dir.path().join("other.json"), json!({"instructions_schema":schema,"instructions_count":1,"instructions":[row],"wgp":7}).to_string()).unwrap();
        let capture = load_capture(dir.path()).unwrap();
        assert_eq!(capture.other_simd.len(), 1);
        let record = &capture.other_simd[0];
        assert_eq!(
            (record.start, record.duration, record.category, record.wgp),
            (30., 12., Some(4), Some(7))
        );
    }
}

#[test]
fn other_simd_rejects_invalid_intervals_identities_and_sidecar_escape() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("filenames.json"),
        json!({"other_simd_filenames":{"0":[["other.json",0,99]]}}).to_string(),
    )
    .unwrap();
    for data in [
        json!({"instructions":[[0,-1]]}),
        json!({"instructions":[[0,null]]}),
        json!({"instructions":[[1e308,1e308]]}),
        json!({"wgp":-1,"instructions":[[0,1]]}),
        json!({"instructions_schema":["time","duration","category"],"instructions":[[0,1,1.5]]}),
        json!({"instructions":{}}),
        json!({"instructions_schema":null,"instructions":[[0,1]]}),
        json!({"instructions_schema":"time,duration","instructions":[[0,1]]}),
        json!({"instructions_schema":[],"instructions":[[0,1]]}),
        json!({"instructions_schema":["category","duration"],"instructions":[[0,1]]}),
        json!({"instructions_schema":["time","category"],"instructions":[[0,1]]}),
        json!({"instructions_schema":["time","duration","time"],"instructions":[[0,1,2]]}),
        json!({"instructions_schema":["time","duration",0],"instructions":[[0,1,2]]}),
        json!({"instructions_schema":["time","duration","category"],"instructions":[[0,1]]}),
        json!({"instructions_schema":["time","duration"],"instructions":[[0,1,2]]}),
        json!({"instructions":[[0,1,2]]}),
        json!({"instructions_count":2,"instructions":[[0,1]]}),
        json!({"instructions_count":"1","instructions":[[0,1]]}),
    ] {
        fs::write(dir.path().join("other.json"), data.to_string()).unwrap();
        assert!(load_capture(dir.path()).is_err(), "Accepted {data}");
    }
    let outside = tempfile::NamedTempFile::new().unwrap();
    fs::write(
        dir.path().join("filenames.json"),
        json!({"other_simd_filenames":{"0":[[outside.path(),0,99]]}}).to_string(),
    )
    .unwrap();
    assert!(load_capture(dir.path()).is_err());
}

#[test]
fn missing_wait_sidecar_is_reconstructed_on_target_wave_load() {
    let dir = tempfile::tempdir().unwrap();
    minimal(dir.path());
    fs::write(
        dir.path().join("wave.json"),
        json!({"wave":{
            "begin":0,"end":30,"instructions":[[0,1,0,4,0],[10,1,0,4,1]]
        }})
        .to_string(),
    )
    .unwrap();
    let mut capture = load_capture(dir.path()).unwrap();
    capture.architecture = "gfx1100".into();
    capture.instructions = ["global_load_dword v0, v1, off", "s_waitcnt vmcnt(0)"].iter().enumerate().map(|(index,text)| serde_json::from_value(json!({
        "index":index,"text":text,"source":"","address":"","codeObject":"","hits":0,"latency":0,"stall":0,"idle":0,"pcSamples":0
    })).unwrap()).collect();
    let wave = load_wave(&capture, "wave.json").unwrap();
    assert_eq!(wave.waits, json!([[1, [[0, 0]]]]));
    assert_eq!(wave.info["waitAnalysis"]["occurrences"][0]["waitToken"], 1);
}

#[test]
fn decoder_gfxip_selects_wait_model_without_renaming_architecture() {
    let dir = tempfile::tempdir().unwrap();
    minimal(dir.path());
    fs::write(
        dir.path().join("wave.json"),
        json!({"wave":{
            "begin":0,"end":30,"instructions":[[0,1,0,4,0],[10,1,0,4,1]]
        }})
        .to_string(),
    )
    .unwrap();
    let mut capture = load_capture(dir.path()).unwrap();
    capture.architecture = "navi".into();
    capture.metadata.insert("gfxip".into(), "11".into());
    capture.instructions = ["global_load_dword v0, v1, off", "s_waitcnt 0"].iter().enumerate().map(|(index,text)| serde_json::from_value(json!({
        "index":index,"text":text,"source":"","address":"","codeObject":"","hits":0,"latency":0,"stall":0,"idle":0,"pcSamples":0
    })).unwrap()).collect();
    let wave = load_wave(&capture, "wave.json").unwrap();
    assert_eq!(wave.waits, json!([[1, [[0, 0]]]]));
    assert_eq!(wave.info["waitAnalysis"]["gfxip"], 11);
    assert_eq!(
        wave.info["waitAnalysis"]["architectureBasis"],
        "decoder gfxip"
    );
    assert_eq!(capture.architecture, "navi");
    capture.architecture = "gfx942".into();
    let wave = load_wave(&capture, "wave.json").unwrap();
    assert!(wave.info["waitAnalysis"]["unavailableReason"]
        .as_str()
        .unwrap()
        .contains("disagree"));
    assert!(wave.waits.is_null());
    capture.architecture = "navi".into();
    capture
        .metadata
        .insert("gfxip".into(), "not a generation".into());
    assert!(load_wave(&capture, "wave.json").unwrap().waits.is_null());
}
