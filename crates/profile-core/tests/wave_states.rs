use profile_core::load_capture;
use std::fs;
#[test]
fn disabled_wave_states_are_not_parsed_and_do_not_remove_wave_indexes() {
    let dir = fixture();
    fs::write(
        dir.path().join("wstates2.json"),
        "malformed optional sidecar",
    )
    .unwrap();
    assert!(load_capture(dir.path()).is_err());
    fs::write(dir.path().join("se1_perfcounter.json"), r#"{"data":[[10,1,2,3,4,0,0]]}"#).unwrap();
    let capture = profile_core::load_capture_with_wave_states(dir.path(), false).unwrap();
    assert_eq!(capture.counters.len(), 4);
    assert!(capture.counters.iter().all(|s| s.unit == "raw count"));
    assert_eq!(capture.waves.len(), 1);
    assert_eq!(capture.metadata["loadWaveStates"], "false");
}
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("filenames.json"),
        r#"{"gfxv":"gfx1100","wave_filenames":{"1":{"0":{"0":{"0":["wave.json",10,20]}}}}}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("occupancy.json"),
        r#"{"dispatches":{"1":"kernel"},"0":[[10,0,0,1,1,1],[20,0,0,1,0,1]]}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("wstates2.json"),
        r#"{"time":[10,20],"state":[3,0]}"#,
    )
    .unwrap();
    dir
}
#[test]
fn detailed_wave_scope_survives_global_occupancy_and_clock_alignment() {
    let dir = fixture();
    let raw = load_capture(dir.path()).unwrap();
    assert_eq!(raw.counters.len(), 1);
    assert_eq!(raw.counters[0].scope, "SE 1 · precomputed wave states");
    assert_eq!(raw.counters[0].points, [[10., 3.], [20., 0.]]);
    fs::write(
        dir.path().join("realtime.json"),
        r#"{"SE0":[[25,1000],[50,1001]],"SE1":[[25,1002],[50,1003]]}"#,
    )
    .unwrap();
    let aligned = load_capture(dir.path()).unwrap();
    assert_eq!(aligned.counters[0].points, [[60., 3.], [70., 0.]]);
    assert_eq!(aligned.waves[0].start, 60.);
    assert_eq!(aligned.intervals[0].start, 10.);
}
#[test]
fn ambiguous_wave_hierarchy_reports_skipped_precomputed_series() {
    let dir = fixture();
    let path = dir.path().join("filenames.json");
    let mut names: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    names["wave_filenames"]["0"] = names["wave_filenames"]["1"].clone();
    fs::write(path, serde_json::to_vec(&names).unwrap()).unwrap();
    let capture = load_capture(dir.path()).unwrap();
    assert!(capture.counters.is_empty());
    assert!(capture
        .warnings
        .iter()
        .any(|w| w.contains("single detailed-wave shader engine")));
}
#[test]
fn malformed_precomputed_values_are_not_silently_replaced_by_zero() {
    for data in [
        r#"{"time":[10],"state":[null]}"#,
        r#"{"time":[10],"state":[-1]}"#,
        r#"{"time":[20,10],"state":[1,2]}"#,
        r#"{"time":null,"state":[]}"#,
    ] {
        let dir = fixture();
        fs::write(dir.path().join("wstates2.json"), data).unwrap();
        assert!(load_capture(dir.path()).is_err(), "{data}");
    }
}
