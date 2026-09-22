use profile_core::{capture_archive, load_capture, project};
use std::{
    fs,
    sync::{atomic::AtomicBool, Arc},
};

fn fixture(data: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("filenames.json"),
        r#"{"gfxv":"gfx942","wave_filenames":{}}"#,
    )
    .unwrap();
    fs::write(dir.path().join("realtime.json"), data).unwrap();
    fs::write(
        dir.path().join("occupancy.json"),
        r#"{"dispatches":{"1":"kernel"},"0":[[10,0,0,1,1,1],[20,0,0,1,0,1]]}"#,
    )
    .unwrap();
    dir
}

#[test]
fn exact_clock_anchors_survive_import_and_portable_archive() {
    let dir = fixture(
        r#"{"metadata":{"frequency":100000000},"SE2":[[-9007199254740993,18446744073709551615],[9007199254740993,9007199254740995]],"SE0":[[0,1],[0,1]]}"#,
    );
    let capture = load_capture(dir.path()).unwrap();
    let realtime = capture.realtime.as_ref().unwrap();
    assert_eq!(realtime.frequency_hz.as_deref(), Some("100000000"));
    assert_eq!(realtime.anchors.len(), 4);
    assert_eq!(realtime.anchors[2].se, 2);
    assert_eq!(realtime.anchors[2].shader_clock, "-9007199254740993");
    assert_eq!(realtime.anchors[2].realtime_clock, "18446744073709551615");
    assert_eq!(realtime.anchors[3].shader_clock, "9007199254740993");
    let output = tempfile::tempdir().unwrap();
    let archive = output.path().join("anchors.rocprof-capture");
    capture_archive::write(&archive, &capture, &AtomicBool::new(false)).unwrap();
    let restored = project::load_input(&archive, Arc::new(AtomicBool::new(false))).unwrap();
    assert_eq!(capture.realtime, restored.realtime);
    assert_eq!(restored.unit, "cycles");
}

#[test]
fn malformed_optional_calibration_does_not_partially_import_or_break_trace() {
    for data in [
        r#"{"SE0":[[1,2],[3,4.5]]}"#,
        r#"{"SE-1":[]}"#,
        r#"{"SE0":[[1,-2]]}"#,
        r#"{"SE0":[[1,2,3]]}"#,
        r#"{"metadata":{"frequency":-1},"SE0":[[1,2]]}"#,
        "[",
    ] {
        let dir = fixture(data);
        let capture = load_capture(dir.path()).unwrap();
        assert!(capture.realtime.is_none(), "{data}");
        assert!(capture
            .warnings
            .iter()
            .any(|w| w.contains("REALTIME calibration unavailable")));
    }
    let dir = fixture(r#"{"SE0":[[1,2]]}"#);
    assert_eq!(
        load_capture(dir.path())
            .unwrap()
            .realtime
            .unwrap()
            .frequency_hz,
        None
    );
}

#[test]
fn decoder_zero_frequency_preserves_anchors_without_inventing_a_rate() {
    let dir = fixture(r#"{"metadata":{"frequency":0},"SE0":[[100,200],[300,400]]}"#);
    let calibration = load_capture(dir.path()).unwrap().realtime.unwrap();
    assert_eq!(calibration.frequency_hz, None);
    assert_eq!(calibration.anchors.len(), 2);
}
