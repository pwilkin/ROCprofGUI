use profile_core::{load_capture, load_wave, se_clock::normalize};
use std::{
    fs,
    sync::{atomic::AtomicBool, Arc},
};
fn fixture(realtime: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("filenames.json"),r#"{"gfxv":"gfx1100","wave_filenames":{"0":{"0":{"0":{"0":["w0.json",10,20]}}},"1":{"0":{"0":{"0":["w1.json",10,20]}}}}}"#).unwrap();
    for se in 0..2 {
        fs::write(dir.path().join(format!("w{se}.json")),r#"{"wave":{"begin":10,"end":20,"instructions":[[11,6,2,3,1]],"timeline":[[0,4],[1,6]],"waitcnt":[],"info":{}}}"#).unwrap();
    }
    if realtime {
        fs::write(
            dir.path().join("realtime.json"),
            r#"{"SE0":[[25,1000],[50,1001]],"SE1":[[25,1002],[50,1003]]}"#,
        )
        .unwrap();
    }
    dir
}
fn calibration() -> profile_core::RealtimeCalibration {
    serde_json::from_value(serde_json::json!({"frequencyHz":null,"anchors":[
        {"se":0,"shaderClock":"25","realtimeClock":"1000"},{"se":0,"shaderClock":"50","realtimeClock":"1001"},
        {"se":1,"shaderClock":"25","realtimeClock":"1002"},{"se":1,"shaderClock":"50","realtimeClock":"1003"}
    ]})).unwrap()
}
#[test]
fn all_attributed_records_shift_together_and_input_remains_unchanged() {
    let dir = fixture(false);
    let mut c = load_capture(dir.path()).unwrap();
    c.realtime = Some(calibration());
    c.intervals.push(
        serde_json::from_value(
            serde_json::json!({"start":10,"end":20,"se":1,"cu":0,"simd":0,"slot":0,"kernel":"k"}),
        )
        .unwrap(),
    );
    c.decoder_events.push(
        serde_json::from_value(
            serde_json::json!({"id":"e","se":1,"time":12,"kind":"test","fields":{}}),
        )
        .unwrap(),
    );
    c.other_simd.push(serde_json::from_value(serde_json::json!({"se":1,"wgp":0,"source":"test","start":13,"duration":4,"category":1})).unwrap());
    c.counters.push(serde_json::from_value(serde_json::json!({"name":"A","unit":"count","scope":"SE 1 · CU 0 · bank 0","points":[[14,9]]})).unwrap());
    c.markers.push(serde_json::from_value(serde_json::json!({"id":0,"parent":null,"markerId":1,"codeObject":"1","name":"m","kind":"range","source":"","start":15,"end":19,"point":false,"depth":0,"se":1,"cu":0,"simd":0,"slot":0})).unwrap());
    let original = serde_json::to_value(&c).unwrap();
    let aligned = normalize(&c).unwrap();
    assert_eq!(serde_json::to_value(&c).unwrap(), original);
    assert_eq!(aligned.waves[1].start, 60.);
    assert_eq!(aligned.intervals[0].end, 70.);
    assert_eq!(aligned.decoder_events[0].time, 62.);
    assert_eq!(aligned.other_simd[0].start, 63.);
    assert_eq!(aligned.other_simd[0].duration, 4.);
    assert_eq!(aligned.counters[0].points, [[64., 9.]]);
    assert_eq!(aligned.markers[0].start, 65.);
    assert_eq!(aligned.markers[0].end, Some(69.));
    assert_eq!(
        aligned.realtime.as_ref().unwrap().anchors[2].shader_clock,
        "75"
    );
    let wave = load_wave(&aligned, "w1.json").unwrap();
    assert_eq!((wave.start, wave.end), (60., 70.));
    assert_eq!(wave.tokens[0].start, 61.);
    assert_eq!(wave.tokens[0].duration, 3.);
    assert_eq!(wave.states[0][0], 60.);
    assert_eq!(wave.states[1][0], 64.);
    assert_eq!(aligned.start, 10.);
    assert_eq!(aligned.end, 70.);
    assert!(normalize(&aligned).is_err());
}
#[test]
fn importer_and_archive_lazy_load_apply_offsets_exactly_once() {
    let dir = fixture(true);
    let c = load_capture(dir.path()).unwrap();
    assert_eq!(c.se_time_offsets[&1], 50);
    let wave = load_wave(&c, "w1.json").unwrap();
    let output = tempfile::tempdir().unwrap();
    let path = output.path().join("aligned.rocprof-capture");
    profile_core::capture_archive::write(&path, &c, &AtomicBool::new(false)).unwrap();
    let restored =
        profile_core::project::load_input(&path, Arc::new(AtomicBool::new(false))).unwrap();
    let restored_wave = load_wave(&restored, &restored.waves[1].id).unwrap();
    assert_eq!(restored_wave.start, wave.start);
    assert_eq!(restored_wave.tokens[0].start, wave.tokens[0].start);
    assert_eq!(restored.waves[1].start, wave.start);
    assert_eq!(restored.se_time_offsets, c.se_time_offsets);
}
#[test]
fn ambiguous_series_and_seed_only_calibration_leave_capture_unshifted() {
    let dir = fixture(false);
    let mut c = load_capture(dir.path()).unwrap();
    c.realtime = Some(calibration());
    c.counters.push(serde_json::from_value(serde_json::json!({"name":"State","unit":"waves","scope":"precomputed · single SE","points":[[10,1]]})).unwrap());
    assert!(normalize(&c).unwrap_err().contains("attribution"));
    assert_eq!(c.waves[1].start, 10.);
    fs::write(
        dir.path().join("realtime.json"),
        r#"{"SE0":[[25,1000]],"SE1":[[25,1002]]}"#,
    )
    .unwrap();
    let c = load_capture(dir.path()).unwrap();
    assert!(c.se_time_offsets.is_empty());
    assert!(c.warnings.iter().any(|w| w.contains("seed alone")));
}

#[test]
fn spm_attachment_uses_the_shifted_realtime_anchor_clock() {
    let dir = fixture(true);
    let trace = load_capture(dir.path()).unwrap();
    let mut spm = profile_core::Capture::empty("SPM".into(), "rocprofv3 SPM JSON", "ns");
    spm.origin = "1000".into();
    spm.counters.push(serde_json::from_value(serde_json::json!({"name":"BUSY","unit":"events","scope":"process 0 · agent 1 · counter 1 · []","points":[[0,1],[1,2]]})).unwrap());
    let combined = profile_core::spm_prepare::attach(&trace, &spm, 1).unwrap();
    assert_eq!(
        combined.counters.last().unwrap().points,
        [[25., 1.], [50., 2.]]
    );
    assert_eq!(load_wave(&combined, "w1.json").unwrap().start, 60.);
    assert_eq!(combined.se_time_offsets[&1], 50);
}

#[test]
fn decoded_dispatch_table_and_event_keep_the_same_shader_clock() {
    let dir = fixture(true);
    std::fs::write(
        dir.path().join("occupancy.json"),
        r#"{"events":{"1":[{"kind":"dispatch","time":12,"kernel_name":"kernel","vgprs":32}]}}"#,
    )
    .unwrap();
    let capture = load_capture(dir.path()).unwrap();
    assert_eq!(capture.dispatches[0].shader_engine, Some(1));
    assert_eq!(capture.dispatches[0].start, 62.);
    assert_eq!(capture.dispatches[0].end, 62.);
    assert_eq!(capture.decoder_events[0].time, 62.);
}
