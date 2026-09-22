use std::path::PathBuf;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/radv-rgp/compute-spm.rgp")
}

#[test]
fn radv_capture_imports_spm_counters_with_units_and_topology() {
    let c = profile_core::rgp::load(&fixture()).unwrap();
    assert_eq!(c.format, "RADV RGP");
    assert_eq!(c.architecture, "STRIX_HALO");
    assert_eq!(c.metadata["deviceId"], "0x1586");
    assert_eq!(c.metadata["topology"], "2 shader engines × 20 CUs");
    assert_eq!(c.metadata["spmSamples"], "329");
    assert_eq!(c.metadata["spmSampleInterval"], "4096 shader clocks");
    assert_eq!(c.counters.len(), 14);
    for s in &c.counters {
        assert_eq!(s.points.len(), 329, "{} lost samples", s.name);
        assert!(s.points.windows(2).all(|w| w[0][0] <= w[1][0]), "{} is not ordered in time", s.name);
        assert!(s.points.iter().all(|p| p[1].is_finite()), "{} has non-finite values", s.name);
    }
    assert_eq!(c.start, 0.);
    assert!(c.end > 0.);
}

#[test]
fn counter_identity_scope_and_units_come_from_the_capture() {
    let c = profile_core::rgp::load(&fixture()).unwrap();
    let by = |n: &str| c.counters.iter().find(|s| s.name == n).unwrap_or_else(|| panic!("missing {n}"));
    assert_eq!(by("L2 cache hit").unit, "%");
    assert_eq!(by("L2 cache hit").scope, "GPU · Cache");
    assert_eq!(by("Fetch size").unit, "bytes");
    assert_eq!(by("Fetch size").scope, "GPU · Memory (bytes)");
    assert_eq!(by("Ray-box tests").unit, "count");
    assert_eq!(by("Ray-box tests").scope, "GPU · Ray tracing");
    assert_eq!(by("LDS Bank Conflict").scope, "GPU · LDS");
}

/// The workload uses no LDS, issues no raytracing and runs on a unified-memory
/// APU, so these must stay zero; a non-zero reading means the value mapping drifted.
#[test]
fn counters_the_workload_cannot_exercise_stay_zero() {
    let c = profile_core::rgp::load(&fixture()).unwrap();
    for name in ["LDS Bank Conflict", "Ray-box tests", "Ray-triangle tests", "PCIe bytes"] {
        let s = c.counters.iter().find(|s| s.name == name).unwrap();
        assert!(s.points.iter().all(|p| p[1] == 0.), "{name} reported activity the workload cannot produce");
    }
    let fetch = c.counters.iter().find(|s| s.name == "Fetch size").unwrap();
    assert!(fetch.points.iter().any(|p| p[1] > 0.), "Fetch size never moved; the value run is misaligned");
}

#[test]
fn thread_trace_sections_are_reported_but_not_imported() {
    let c = profile_core::rgp::load(&fixture()).unwrap();
    assert_eq!(c.metadata["sqttSections"], "2");
    assert!(c.waves.is_empty());
    assert!(c.instructions.is_empty());
    assert!(c.warnings.iter().any(|w| w.contains("not implemented")));
    assert!(c.warnings.iter().any(|w| w.contains("device-wide")));
}

#[test]
fn both_import_dispatchers_accept_rgp() {
    let cli = profile_core::load_capture(&fixture()).unwrap();
    let desktop = profile_core::project::load_input_options(&fixture(), Default::default(), true, None).unwrap();
    assert_eq!(cli.format, "RADV RGP");
    assert_eq!(desktop.format, "RADV RGP");
    assert_eq!(cli.counters.len(), desktop.counters.len());
}

#[test]
fn malformed_captures_are_rejected_without_panicking() {
    let dir = tempfile::tempdir().unwrap();
    let good = std::fs::read(fixture()).unwrap();

    let empty = dir.path().join("empty.rgp");
    std::fs::write(&empty, b"").unwrap();
    assert!(profile_core::rgp::load(&empty).unwrap_err().contains("shorter than an RGP header"));

    let wrong = dir.path().join("wrong.rgp");
    let mut bad_magic = good.clone();
    bad_magic[..4].copy_from_slice(b"NOPE");
    std::fs::write(&wrong, &bad_magic).unwrap();
    assert!(profile_core::rgp::load(&wrong).unwrap_err().contains("B00P"));

    let truncated = dir.path().join("truncated.rgp");
    std::fs::write(&truncated, &good[..good.len() / 3]).unwrap();
    assert!(profile_core::rgp::load(&truncated).is_err());

    let header_only = dir.path().join("header.rgp");
    std::fs::write(&header_only, &good[..56]).unwrap();
    assert!(profile_core::rgp::load(&header_only).is_err());
}
