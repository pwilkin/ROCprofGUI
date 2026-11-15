//! Round-trip qualification of the IRPF v2.0 importer against real bundles.
//!
//! Two small bundles written by the hrx-system native AMD GPU driver (PM4 user
//! queues) are vendored under `fixtures/irpf/` and run by default. The retained
//! hrx-b99 campaign bundles and a full Loom hybrid (GPU + NPU) serving capture
//! are multi-megabyte external artifacts, so those tests are `#[ignore]` and
//! gated on environment variables, mirroring the decoder/archive convention in
//! the README's "Tests that do not run by default" table:
//!
//! ```bash
//! ROCPROF_IRPF_DISPATCH_BUNDLE=/path/hook-dispatch/trace.irpf \
//! ROCPROF_IRPF_QUEUE_BUNDLE=/path/hook-queue/trace.irpf \
//! ROCPROF_IRPF_LOOM_HYBRID_BUNDLE=/path/hybrid-e2e.irpf \
//! ROCPROF_IRPF_COUNTERS_BUNDLE=/path/hrx-profile.irpf \
//!   cargo test -p profile-core --test irpf -- --ignored --nocapture
//! ```
//!
//! `ROCPROF_IRPF_COUNTERS_BUNDLE` is a bundle captured with the `counters`
//! family on the native AMD GPU driver. Set `ROCPROF_IRPF_COUNTERS_EXACT=1` to
//! also require every sampled dispatch's `SQG_WAVES` to equal its geometry.

use profile_core::load_capture;
use std::path::PathBuf;

fn bundle(var: &str) -> PathBuf {
    PathBuf::from(std::env::var(var).unwrap_or_else(|_| panic!("set {var} to a trace.irpf path")))
}

#[test]
#[ignore = "requires the retained hrx-b99 dispatch IRPF bundle via ROCPROF_IRPF_DISPATCH_BUNDLE"]
fn dispatch_bundle_roundtrips_with_exact_counts() {
    let c = load_capture(&bundle("ROCPROF_IRPF_DISPATCH_BUNDLE")).unwrap();
    assert_eq!(c.format, "HRX native profile · IRPF 2.0");
    assert_eq!(c.unit, "ns");
    assert_eq!(c.metadata["hrxFunctionCount"], "46");
    assert_eq!(c.dispatches.len(), 4540);
    assert_eq!(c.code_objects.len(), 46);
    assert_eq!(c.metadata["hrxTimestampFrequencyHz"], "99810000");
    // Every dispatch resolves to a named function; nothing is left unresolved.
    assert!(c
        .dispatches
        .iter()
        .all(|d| !d.kernel.starts_with("unresolved function")));
    // The ggml kernel family is present and grids are count × size.
    assert!(c
        .dispatches
        .iter()
        .any(|d| d.kernel == "ggml_mul_mat_swiglu_q4_q8_1_x4_lowtoken_dot_q8_output"));
    // The unaligned-tick limitation is surfaced.
    assert!(c.warnings.iter().any(|w| w.contains("DEVICE_TICK_UNALIGNED")));
    // Elapsed span matches the probe's 552.986 ms (within float rounding).
    assert!((c.end - 552_986_073.5).abs() < 1.0, "end was {}", c.end);
}

#[test]
#[ignore = "requires the retained hrx-b99 queue IRPF bundle via ROCPROF_IRPF_QUEUE_BUNDLE"]
fn queue_bundle_roundtrips_from_command_operations() {
    let c = load_capture(&bundle("ROCPROF_IRPF_QUEUE_BUNDLE")).unwrap();
    assert_eq!(c.format, "HRX native profile · IRPF 2.0");
    assert_eq!(c.metadata["hrxFunctionCount"], "46");
    // No dispatch-events chunk: the static command-operation dispatch program.
    assert_eq!(c.dispatches.len(), 1708);
    assert_eq!(c.code_objects.len(), 0);
    assert!(c
        .dispatches
        .iter()
        .all(|d| !d.kernel.starts_with("unresolved function")));
    assert!(c
        .warnings
        .iter()
        .any(|w| w.contains("static command-operation program")));
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/irpf").join(name)
}

/// A real bundle from the native AMD GPU driver (PM4 on KFD user queues), which
/// rocprofv3 cannot observe at all: 64 timed dispatches of one Loom kernel.
#[test]
fn native_pm4_driver_bundle_imports_named_timed_dispatches() {
    let c = load_capture(&fixture("loom-native-pm4-increment.irpf")).unwrap();
    assert_eq!(c.format, "HRX native profile · IRPF 2.0");
    assert_eq!(c.unit, "ns");
    assert_eq!(c.dispatches.len(), 64);
    assert_eq!(c.code_objects.len(), 1);
    assert_eq!(c.metadata["hrxSessionCount"], "1");
    assert_eq!(c.metadata["hrxDevices"], "HRX device 0 (amd-gpu)");
    assert_eq!(c.metadata["hrxTimestampFrequencyHz"], "99810000");
    assert!(c.dispatches.iter().all(|d| d.kernel == "increment"
        && d.grid == "1 × 1 × 1"
        && d.workgroup == "1 × 1 × 1"
        && d.agent == "HRX device 0 (amd-gpu)"
        // The device has four compute queues; this stream is queue 0's.
        && d.queue == "HRX queue 0"
        && d.end > d.start));
    // Serialized on one queue: no dispatch starts before the previous one ended.
    assert!(c.dispatches.windows(2).all(|w| w[1].start >= w[0].end));
    // Waves and ISA are not part of a native dispatch stream.
    assert!(c.waves.is_empty() && c.instructions.is_empty());
}

/// What a GPU + NPU device group leaves when the NPU rejects `dispatch-events`:
/// a finalized bundle with no dispatch record. It must be rejected with the
/// reason, not opened as a silent empty capture.
#[test]
fn aborted_device_group_bundle_is_rejected_with_its_cause() {
    let e = load_capture(&fixture("loom-hybrid-aborted-session.irpf")).unwrap_err();
    assert!(e.contains("holds no dispatch records"), "{e}");
    assert!(e.contains("HRX device 0 (amd-gpu)"), "{e}");
    assert!(e.contains("0 executable function(s) and 0 queue event(s)"), "{e}");
}

/// The Qwen3.8-27B hybrid (GPU + XDNA) served by a loom-serve built with the
/// per-device family fallback: one short and one 5,556-token request.
#[test]
#[ignore = "requires the Loom hybrid serving bundle via ROCPROF_IRPF_LOOM_HYBRID_BUNDLE"]
fn loom_hybrid_bundle_roundtrips_gpu_dispatches_and_names_the_silent_npu() {
    let c = load_capture(&bundle("ROCPROF_IRPF_LOOM_HYBRID_BUNDLE")).unwrap();
    assert_eq!(c.unit, "ns");
    assert_eq!(c.dispatches.len(), 47875);
    assert_eq!(c.metadata["hrxSessionCount"], "2");
    assert_eq!(c.metadata["hrxDevices"], "HRX device 0 (amd-gpu); HRX device 1 (xdna)");
    assert!(c
        .dispatches
        .iter()
        .all(|d| !d.kernel.starts_with("unresolved function") && d.agent == "HRX device 0 (amd-gpu)"));
    assert!(c.dispatches.iter().any(|d| d.kernel == "g_gemm_q4_k_store_m256"));
    // Nothing was dropped: the periodic flush kept the driver's ring from filling.
    assert!(!c.warnings.iter().any(|w| w.contains("TRUNCATED")));
    assert!(c
        .warnings
        .iter()
        .any(|w| w.contains("recorded no dispatch records: HRX device 1 (xdna)")));
}

/// The synthetic counter fixture (see fixtures/irpf/PROVENANCE.md), through the
/// same router the app uses: counter rows joined to dispatches, one series per
/// counter, and the driver's statement about the sample window.
#[test]
fn synthetic_counter_bundle_imports_rows_joined_to_dispatches() {
    let c = load_capture(&fixture("synthetic-hrx-counters.irpf")).unwrap();
    assert_eq!(c.dispatches.len(), 3);
    // Two samples of five counters; the third dispatch was not sampled.
    assert_eq!(c.event_counters.len(), 10);
    assert_eq!(c.counters.len(), 5);
    let waves: Vec<(&str, Option<f64>)> = c
        .event_counters
        .iter()
        .filter(|r| r.name == "SQG_WAVES")
        .map(|r| (r.dispatches[0].as_str(), r.value))
        .collect();
    assert_eq!(waves, [("dispatch-events:1", Some(4.0)), ("dispatch-events:2", Some(8.0))]);
    assert_eq!(c.metadata["hrxCounterSampledDispatches"], "2 of 3");
    assert!(c.metadata["hrxCounterWindowExclusive"].contains("true (among the producing device's queues"));
    assert!(c.warnings.iter().any(|w| w.contains("are window sums")));
    assert!(c.warnings.iter().any(|w| w.contains("The counter sample bound of 2 dispatch(es) was reached")));
    assert!(c.warnings.iter().any(|w| w.contains("Cycle counters (SQG_BUSY_CYCLES) are never a per-dispatch count")));
    assert!(!c.warnings.iter().any(|w| w.contains("hardware counters are not part of")));
}

/// `tests/fixtures/hrx-counters-capture.json` is this importer's output for the
/// synthetic counter fixture; the frontend's tests read it, so it must not drift.
/// Rewrite it after an intended change with
/// `ROCPROF_UPDATE_FIXTURES=1 cargo test -p profile-core --test irpf frontend`.
#[test]
fn frontend_counter_capture_fixture_is_the_importer_output() {
    let capture = load_capture(&fixture("synthetic-hrx-counters.irpf")).unwrap();
    let current = serde_json::to_value(&capture).unwrap();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/hrx-counters-capture.json");
    if std::env::var("ROCPROF_UPDATE_FIXTURES").is_ok_and(|v| v == "1") {
        std::fs::write(&path, serde_json::to_string_pretty(&current).unwrap() + "\n").unwrap();
    }
    let stored: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    for (key, value) in current.as_object().unwrap() {
        assert_eq!(Some(value), stored.get(key), "capture field {key}");
    }
    assert_eq!(current, stored);
}

/// Workgroups and threads per workgroup of a dispatch, from the model's grid
/// (count × size per dimension) and workgroup strings.
fn geometry(grid: &str, workgroup: &str) -> Option<(u64, u64)> {
    let dims = |text: &str| -> Option<Vec<u64>> {
        text.split(" × ").map(|part| part.trim().parse::<u64>().ok()).collect()
    };
    let (grid, size) = (dims(grid)?, dims(workgroup)?);
    if grid.len() != 3 || size.len() != 3 || size.iter().any(|s| *s == 0) {
        return None;
    }
    let groups = (0..3).map(|i| grid[i] / size[i]).product();
    Some((groups, size.iter().product()))
}

/// A real capture with the `counters` family from the native AMD GPU driver.
/// Reports, per sampled dispatch with recorded geometry, whether SQG_WAVES is
/// workgroups × ceil(threads per workgroup / wave size), the wave size being the
/// one whose counter (SQG_WAVES_32 or SQG_WAVES_64) is nonzero.
#[test]
#[ignore = "requires a counters bundle of the native AMD GPU driver via ROCPROF_IRPF_COUNTERS_BUNDLE"]
fn real_counters_bundle_joins_wave_counts_to_dispatch_geometry() {
    let c = load_capture(&bundle("ROCPROF_IRPF_COUNTERS_BUNDLE")).unwrap();
    assert_eq!(c.metadata["hrxCounterImport"], "imported");
    assert!(!c.event_counters.is_empty() && !c.counters.is_empty());
    println!("counters: {}", c.metadata["hrxCounters"]);
    println!("capture: {}", c.metadata.get("hrxCounterCapture").map_or("(no record)", String::as_str));
    println!("sampled dispatches: {}", c.metadata["hrxCounterSampledDispatches"]);
    println!("totals: {}", c.metadata["hrxCounterTotals"]);
    let dispatches: std::collections::BTreeMap<&str, &profile_core::Dispatch> =
        c.dispatches.iter().map(|d| (d.id.as_str(), d)).collect();
    // Counter values of each sampled dispatch, by counter name.
    let mut sampled: std::collections::BTreeMap<&str, std::collections::BTreeMap<&str, f64>> = Default::default();
    for row in &c.event_counters {
        if let ([id], Some(value)) = (row.dispatches.as_slice(), row.value) {
            sampled.entry(id.as_str()).or_default().insert(row.name.as_str(), value);
        }
    }
    assert!(!sampled.is_empty(), "no counter row is joined to a dispatch");
    let (mut exact, mut checked, mut unknown) = (0u64, 0u64, 0u64);
    let mut mismatches = Vec::new();
    for (id, values) in &sampled {
        let dispatch = dispatches[id];
        let (Some(waves), Some((groups, threads))) =
            (values.get("SQG_WAVES"), geometry(&dispatch.grid, &dispatch.workgroup))
        else {
            unknown += 1;
            continue;
        };
        let wave_size = match (values.get("SQG_WAVES_32"), values.get("SQG_WAVES_64")) {
            (Some(w32), Some(w64)) if *w32 > 0. && *w64 == 0. => 32,
            (Some(w32), Some(w64)) if *w64 > 0. && *w32 == 0. => 64,
            _ => {
                unknown += 1;
                continue;
            }
        };
        let expected = groups * threads.div_ceil(wave_size);
        checked += 1;
        if *waves == expected as f64 {
            exact += 1;
        } else if mismatches.len() < 20 {
            mismatches.push(format!(
                "{} {} grid {} workgroup {}: SQG_WAVES {} expected {} (wave{wave_size})",
                id, dispatch.kernel, dispatch.grid, dispatch.workgroup, waves, expected
            ));
        }
    }
    println!(
        "SQG_WAVES against geometry: {exact} of {checked} sampled dispatches exact; {unknown} without recorded geometry or a single wave size"
    );
    for line in &mismatches {
        println!("  mismatch: {line}");
    }
    assert!(checked > 0, "no sampled dispatch has recorded geometry and a single wave size");
    if std::env::var("ROCPROF_IRPF_COUNTERS_EXACT").is_ok_and(|v| v == "1") {
        assert_eq!(exact, checked, "{mismatches:?}");
    }
}
