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
//!   cargo test -p profile-core --test irpf -- --ignored --nocapture
//! ```

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
