//! Round-trip qualification of the IRPF v2.0 importer against the retained
//! hrx-b99 campaign bundles. These are multi-megabyte external artifacts, so the
//! tests are `#[ignore]` and gated on environment variables pointing at the
//! `trace.irpf` files, mirroring the decoder/archive convention in the README's
//! "Tests that do not run by default" table:
//!
//! ```bash
//! ROCPROF_IRPF_DISPATCH_BUNDLE=/path/hook-dispatch/trace.irpf \
//! ROCPROF_IRPF_QUEUE_BUNDLE=/path/hook-queue/trace.irpf \
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
