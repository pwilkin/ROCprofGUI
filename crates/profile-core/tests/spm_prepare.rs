use profile_core::{spm_prepare::prepare, Capture, RealtimeAnchor, RealtimeCalibration, Series};
fn counter(
    name: &str,
    id: u32,
    dispatch: u32,
    xcc: u32,
    se: u32,
    instance: u32,
    points: &[[f64; 2]],
) -> Series {
    Series {
        spm_clock: None,
        name: name.into(),
        unit: "events".into(),
        definition: None,
        att: None,
        scope: format!(
            "process 0 · agent 42 · dispatch {dispatch} · counter {id} · [{},{},{}]",
            format!(r#"{{"dimension_name":"DIMENSION_XCC","index":{xcc}}}"#),
            format!(r#"{{"dimension_name":"DIMENSION_SHADER_ENGINE","index":{se}}}"#),
            format!(r#"{{"dimension_name":"DIMENSION_INSTANCE","index":{instance}}}"#)
        ),
        points: points.to_vec(),
    }
}
fn capture() -> Capture {
    let mut c = Capture::empty("SPM".into(), "rocprofv3 SPM JSON", "ns");
    c.origin = (u64::MAX - 100).to_string();
    c
}
fn calibration() -> RealtimeCalibration {
    RealtimeCalibration {
        frequency_hz: None,
        anchors: vec![
            RealtimeAnchor {
                se: 2,
                shader_clock: "500".into(),
                realtime_clock: (u64::MAX - 100).to_string(),
            },
            RealtimeAnchor {
                se: 2,
                shader_clock: "800".into(),
                realtime_clock: (u64::MAX).to_string(),
            },
        ],
    }
}
#[test]
fn imported_coordinates_select_se_average_instances_and_preserve_originals() {
    let mut c = capture();
    c.counters = vec![
        counter(
            "SQ_CYCLES",
            1,
            10,
            0,
            2,
            0,
            &[[0., 999.], [10., 20.], [20., 40.]],
        ),
        counter(
            "SQ_CYCLES",
            1,
            10,
            0,
            2,
            1,
            &[[0., 999.], [10., 40.], [20., 60.]],
        ),
        counter(
            "SQ_CYCLES",
            1,
            10,
            0,
            3,
            0,
            &[[0., 999.], [10., 999.], [20., 999.]],
        ),
        counter("BUSY", 2, 10, 0, 2, 0, &[[0., 1.], [20., 7.]]),
    ];
    let before = serde_json::to_value(&c).unwrap();
    let groups = prepare(&c, &calibration(), 2).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].clock.shader_clocks, [500., 530., 580.]);
    assert_eq!(groups[0].clock.measured_intervals, 2);
    assert_eq!(groups[0].counters[3].points, [[500., 1.], [580., 7.]]);
    assert_eq!(
        groups[0].realtime_timestamps[1],
        (u64::MAX - 90).to_string()
    );
    assert_eq!(serde_json::to_value(&c).unwrap(), before);
}
#[test]
fn overlapping_dispatches_and_xccs_do_not_share_cycle_values() {
    let mut c = capture();
    for (dispatch, xcc, value) in [(10, 0, 20.), (11, 0, 40.), (10, 1, 60.)] {
        c.counters.push(counter(
            "SQ_CYCLES",
            1,
            dispatch,
            xcc,
            2,
            0,
            &[[0., 0.], [10., value]],
        ));
    }
    let groups = prepare(&c, &calibration(), 2).unwrap();
    let clocks: Vec<_> = groups.iter().map(|g| g.clock.shader_clocks[1]).collect();
    assert_eq!(clocks, [520., 560., 540.]);
    assert!(groups[0].domain.contains("dispatch 10"));
    assert!(groups[2].domain.contains("dispatch 11"));
}
#[test]
fn missing_selected_se_cycles_report_upstream_other_se_fallback() {
    let mut c = capture();
    c.counters.push(counter(
        "SQ_CYCLES",
        1,
        10,
        0,
        3,
        0,
        &[[0., 1.], [10., 999.]],
    ));
    let groups = prepare(&c, &calibration(), 2).unwrap();
    assert_eq!(groups[0].clock.shader_clocks, [500., 1499.]);
    assert_eq!(groups[0].clock.fallback_intervals, 0);
    assert_eq!(groups[0].other_se_intervals, 1);
}
#[test]
fn ambiguous_or_inexact_preparation_is_transactional() {
    let original = counter("SQ_CYCLES", 1, 10, 0, 2, 0, &[[0., 1.], [10., 20.]]);
    for bad in [
        original.clone(),
        counter("SQ_CYCLES", 2, 10, 0, 2, 0, &[[0., 1.], [10., 20.]]),
        counter("BUSY", 2, 10, 0, 2, 0, &[[0., 1.], [0., 2.]]),
        counter("BUSY", 2, 10, 0, 2, 0, &[[0.5, 1.]]),
        counter("BUSY", 2, 10, 0, 2, 0, &[[101., 1.]]),
    ] {
        let mut c = capture();
        c.counters = vec![original.clone(), bad];
        let before = serde_json::to_value(&c).unwrap();
        assert!(prepare(&c, &calibration(), 2).is_err());
        assert_eq!(serde_json::to_value(&c).unwrap(), before);
    }
}

#[test]
fn other_se_fallback_averages_engine_means_not_all_instances() {
    let mut c = capture();
    c.counters = vec![
        counter("SQ_CYCLES", 1, 10, 0, 0, 0, &[[0., 0.], [10., 20.]]),
        counter("SQ_CYCLES", 1, 10, 0, 0, 1, &[[0., 0.], [10., 40.]]),
        counter("SQ_CYCLES", 1, 10, 0, 1, 0, &[[0., 0.], [10., 90.]]),
        counter("SQ_CYCLES", 1, 10, 0, 2, 0, &[[0., 0.], [10., 0.]]),
    ];
    let groups = prepare(&c, &calibration(), 2).unwrap();
    assert_eq!(groups[0].clock.shader_clocks, [500., 560.]);
    assert_eq!(groups[0].other_se_intervals, 1);
}

#[test]
fn calibrated_series_realtime_metadata_survives_portable_archive() {
    let mut c = capture();
    c.counters
        .push(counter("BUSY", 2, 10, 0, 2, 0, &[[0., 1.], [10., 2.]]));
    let groups = prepare(&c, &calibration(), 2).unwrap();
    c.counters = groups[0].counters.clone();
    assert!(prepare(&c, &calibration(), 2)
        .unwrap_err()
        .contains("already calibrated"));
    let clock = c.counters[0].spm_clock.as_ref().unwrap();
    assert_eq!(clock.anchor_se, 2);
    assert_eq!(
        clock.realtime_timestamps,
        [(u64::MAX - 100).to_string(), (u64::MAX - 90).to_string()]
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("calibrated.rocprof-capture");
    profile_core::capture_archive::write(&path, &c, &std::sync::atomic::AtomicBool::new(false))
        .unwrap();
    let reopened = profile_core::project::load_input(
        &path,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&reopened.counters).unwrap(),
        serde_json::to_value(&c.counters).unwrap()
    );
}

#[test]
fn attachment_preserves_trace_and_reopens_from_combined_archive() {
    let mut trace = profile_core::load_capture(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/transpose-ui"),
    )
    .unwrap();
    trace.realtime = Some(calibration());
    let original = serde_json::to_value(&trace).unwrap();
    let mut spm = capture();
    spm.counters
        .push(counter("BUSY", 2, 10, 0, 2, 0, &[[0., 1.], [10., 2.]]));
    let combined = profile_core::spm_prepare::attach(&trace, &spm, 2).unwrap();
    assert_eq!(serde_json::to_value(&trace).unwrap(), original);
    assert_eq!(combined.counters.len(), trace.counters.len() + 1);
    assert_eq!(combined.waves.len(), trace.waves.len());
    assert!(combined.metadata["spmAttachment"].contains("anchorSe"));
    assert!(profile_core::spm_prepare::attach(&combined, &spm, 2).is_err());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("combined.rocprof-capture");
    let flag = std::sync::atomic::AtomicBool::new(false);
    profile_core::capture_archive::write_new(&path, &combined, &flag).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(profile_core::capture_archive::write_new(&path, &combined, &flag).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let reopened = profile_core::project::load_input(&path, std::sync::Arc::new(flag)).unwrap();
    assert_eq!(
        serde_json::to_value(&reopened.counters).unwrap(),
        serde_json::to_value(&combined.counters).unwrap()
    );
    assert_eq!(
        reopened.metadata["spmAttachment"],
        combined.metadata["spmAttachment"]
    );
    assert!(reopened.input_fingerprint.is_some());
    assert_eq!(
        profile_core::load_wave(&reopened, &reopened.waves[0].id)
            .unwrap()
            .tokens
            .len(),
        profile_core::load_wave(&trace, &trace.waves[0].id)
            .unwrap()
            .tokens
            .len()
    );
}

#[test]
fn combined_sampling_origin_keeps_spm_calibration_and_attachment_equivalent() {
    let mut original = capture();
    original.counters = vec![counter(
        "SQ_CYCLES",
        1,
        10,
        0,
        2,
        0,
        &[[0., 999.], [10., 20.], [20., 40.]],
    )];
    let expected = serde_json::to_value(prepare(&original, &calibration(), 2).unwrap()).unwrap();
    for method in ["host-trap", "stochastic"] {
        let mut combined = original.clone();
        combined.format = format!("rocprofv3 SPM + {method} PC sampling JSON");
        combined.origin = (u64::MAX - 110).to_string();
        combined
            .metadata
            .insert("pc_sampling_method".into(), method.into());
        for series in &mut combined.counters {
            for point in &mut series.points {
                point[0] += 10.;
            }
        }
        assert_eq!(
            serde_json::to_value(prepare(&combined, &calibration(), 2).unwrap()).unwrap(),
            expected
        );
        let mut trace = Capture::empty("Trace".into(), "ATT", "cycles");
        trace.realtime = Some(calibration());
        let attached = profile_core::spm_prepare::attach(&trace, &combined, 2).unwrap();
        assert!(attached
            .warnings
            .iter()
            .any(|w| w.contains("Only SPM counters were attached")));
        assert_eq!(attached.counters.len(), original.counters.len());
    }
}
