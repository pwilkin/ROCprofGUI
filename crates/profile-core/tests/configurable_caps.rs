//! Scope A: import/analysis caps are overridable via environment variables, with
//! safe defaults equal to the shipped values, and still reject explicitly — no
//! silent sampling — when exceeded. These tests mutate process-wide environment
//! variables, so they serialize against one another through `ENV_LOCK`; this is
//! the only test binary that reads the `ROCPROF_MAX_*` variables, so nothing
//! else observes the temporary overrides.

use profile_core::att_manifest::AttManifest;
use profile_core::caps;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

const VARS: [&str; 5] = [
    "ROCPROF_MAX_RECORDS",
    "ROCPROF_MAX_FILE_MIB",
    "ROCPROF_MAX_HIDDEN_TOKENS",
    "ROCPROF_MAX_ATT_TRACES",
    "ROCPROF_MAX_ATT_CODE_OBJECTS",
];
fn clear() {
    for v in VARS {
        std::env::remove_var(v);
    }
}

#[test]
fn getters_default_to_the_shipped_values() {
    let _g = ENV_LOCK.lock().unwrap();
    clear();
    assert_eq!(caps::max_records(), 2_000_000);
    assert_eq!(caps::max_records(), caps::MAX_RECORDS_DEFAULT);
    assert_eq!(caps::max_file_mib(), 256);
    assert_eq!(caps::max_file_bytes(), 256 * 1024 * 1024);
    assert_eq!(caps::max_hidden_tokens(), 4_000_000);
    assert_eq!(caps::max_att_traces(), 256);
    assert_eq!(caps::max_att_code_objects(), 256);
}

#[test]
fn getters_honor_environment_overrides() {
    let _g = ENV_LOCK.lock().unwrap();
    clear();
    std::env::set_var("ROCPROF_MAX_RECORDS", "8000000");
    std::env::set_var("ROCPROF_MAX_FILE_MIB", "512");
    std::env::set_var("ROCPROF_MAX_HIDDEN_TOKENS", "16000000");
    std::env::set_var("ROCPROF_MAX_ATT_TRACES", "1024");
    std::env::set_var("ROCPROF_MAX_ATT_CODE_OBJECTS", "2048");
    assert_eq!(caps::max_records(), 8_000_000);
    assert_eq!(caps::max_file_mib(), 512);
    assert_eq!(caps::max_file_bytes(), 512 * 1024 * 1024);
    assert_eq!(caps::max_hidden_tokens(), 16_000_000);
    assert_eq!(caps::max_att_traces(), 1024);
    assert_eq!(caps::max_att_code_objects(), 2048);
    clear();
}

#[test]
fn invalid_or_zero_overrides_fall_back_to_the_default() {
    let _g = ENV_LOCK.lock().unwrap();
    clear();
    for bad in ["0", "-5", "not-a-number", "  "] {
        std::env::set_var("ROCPROF_MAX_RECORDS", bad);
        assert_eq!(caps::max_records(), caps::MAX_RECORDS_DEFAULT, "value {bad:?}");
    }
    clear();
}

#[test]
fn record_cap_override_rejects_more_rows_without_sampling() {
    let _g = ENV_LOCK.lock().unwrap();
    clear();
    let dir = tempfile::tempdir().unwrap();
    let csv = dir.path().join("trace.csv");
    std::fs::write(
        &csv,
        "Kernel_Name,Start_Timestamp,End_Timestamp\nk,0,10\nk,20,30\n",
    )
    .unwrap();
    // Default cap: both dispatches import.
    let capture = profile_core::load_capture(&csv).unwrap();
    assert_eq!(capture.dispatches.len(), 2);
    // A one-record cap rejects the second row explicitly, rather than silently
    // importing a truncated or sampled subset.
    std::env::set_var("ROCPROF_MAX_RECORDS", "1");
    let err = profile_core::load_capture(&csv).unwrap_err();
    assert!(err.contains("1 record limit"), "{err}");
    clear();
}

/// Minimal valid IRPF v2.0 bundle carrying a two-record dispatch-events chunk, so
/// a one-record cap must reject it during the sub-record walk. Byte layout follows
/// the pinned producer schema (24-byte file header, 104-byte record headers,
/// 88-byte dispatch-event records).
fn irpf_two_dispatch_bundle() -> Vec<u8> {
    const PREFIX: &str = "application/vnd.iree.hal.profile.";
    fn dispatch_event(event_id: u64) -> Vec<u8> {
        let mut r = vec![0u8; 88];
        r[0..4].copy_from_slice(&88u32.to_le_bytes());
        r[8..16].copy_from_slice(&event_id.to_le_bytes());
        r[72..80].copy_from_slice(&1000u64.to_le_bytes());
        r[80..88].copy_from_slice(&1100u64.to_le_bytes());
        r
    }
    let mut events = dispatch_event(1);
    events.extend(dispatch_event(2));
    let mut body = Vec::new();
    let mut push = |rtype: u16, family: &str, payload: &[u8]| {
        let ct = format!("{PREFIX}{family}");
        let total = 104u64 + ct.len() as u64 + payload.len() as u64;
        let mut h = vec![0u8; 104];
        h[0..8].copy_from_slice(&total.to_le_bytes());
        h[8..16].copy_from_slice(&(payload.len() as u64).to_le_bytes());
        h[56..60].copy_from_slice(&104u32.to_le_bytes());
        h[60..64].copy_from_slice(&(ct.len() as u32).to_le_bytes());
        h[100..102].copy_from_slice(&rtype.to_le_bytes());
        body.extend_from_slice(&h);
        body.extend_from_slice(ct.as_bytes());
        body.extend_from_slice(payload);
    };
    push(1, "session", &[]);
    push(2, "dispatch-events", &events);
    push(3, "session", &[]);
    let mut out = vec![0u8; 24];
    out[0..4].copy_from_slice(&0x4650_5249u32.to_le_bytes());
    out[4..6].copy_from_slice(&2u16.to_le_bytes());
    out[8..12].copy_from_slice(&24u32.to_le_bytes());
    out.extend_from_slice(&body);
    let len = out.len() as u64;
    out[16..24].copy_from_slice(&len.to_le_bytes());
    out
}

#[test]
fn record_cap_override_rejects_irpf_dispatches_without_sampling() {
    let _g = ENV_LOCK.lock().unwrap();
    clear();
    let dir = tempfile::tempdir().unwrap();
    let bundle = dir.path().join("trace.irpf");
    std::fs::write(&bundle, irpf_two_dispatch_bundle()).unwrap();
    // Default cap: both IRPF dispatch events import.
    let capture = profile_core::load_capture(&bundle).unwrap();
    assert_eq!(capture.dispatches.len(), 2);
    // A one-record cap rejects the second dispatch explicitly, not by sampling.
    std::env::set_var("ROCPROF_MAX_RECORDS", "1");
    let err = profile_core::load_capture(&bundle).unwrap_err();
    assert!(err.contains("record limit"), "{err}");
    clear();
}

#[test]
fn record_cap_override_rejects_loom_rows_without_sampling() {
    let _g = ENV_LOCK.lock().unwrap();
    clear();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("k.compile-report.json");
    let report = serde_json::json!({
        "kind": "loom.compile_report",
        "schema_version": 0,
        "mode": "summary",
        "status": {"code": 0, "name": "OK"},
        "entries": {"count": 2, "rows": [
            {"index": 0, "function": "k0"},
            {"index": 1, "function": "k1"}
        ]}
    });
    std::fs::write(&path, serde_json::to_vec(&report).unwrap()).unwrap();
    // Default cap: both entries import.
    let capture = profile_core::load_capture(&path).unwrap();
    assert_eq!(capture.compile_reports[0].entries.len(), 2);
    // A one-record cap rejects the collection BEFORE Vec::with_capacity / row
    // clones (the former only-after-full-materialization check could OOM first).
    std::env::set_var("ROCPROF_MAX_RECORDS", "1");
    let err = profile_core::load_capture(&path).unwrap_err();
    assert!(err.contains("record limit"), "{err}");
    clear();
}

#[test]
fn file_cap_override_rejects_a_large_csv_on_size() {
    let _g = ENV_LOCK.lock().unwrap();
    clear();
    let dir = tempfile::tempdir().unwrap();
    let csv = dir.path().join("big.csv");
    let mut text = String::from("Kernel_Name,Start_Timestamp,End_Timestamp\n");
    while text.len() < 1_200_000 {
        text.push_str("kernel,0,10\n");
    }
    std::fs::write(&csv, &text).unwrap();
    // A 1.1 MiB file imports under the default 256 MiB cap.
    assert!(profile_core::load_capture(&csv).is_ok());
    // A 1 MiB cap rejects it on size, before any row is parsed.
    std::env::set_var("ROCPROF_MAX_FILE_MIB", "1");
    let err = profile_core::load_capture(&csv).unwrap_err();
    assert!(err.contains("1 MiB import limit"), "{err}");
    clear();
}

#[test]
fn file_cap_override_applies_to_rgp() {
    // rgp.rs used to hard-code a 256 MiB cap, so ROCPROF_MAX_FILE_MIB was silently
    // ineffective for .rgp. The size check (first thing in the importer) now routes
    // through caps::max_file_bytes() and interpolates the effective value.
    let _g = ENV_LOCK.lock().unwrap();
    clear();
    let dir = tempfile::tempdir().unwrap();
    let rgp = dir.path().join("capture.rgp");
    std::fs::write(&rgp, vec![0u8; 1_200_000]).unwrap();
    std::env::set_var("ROCPROF_MAX_FILE_MIB", "1");
    let err = profile_core::load_capture(&rgp).unwrap_err();
    assert!(err.contains("1 MiB per-file import limit"), "{err}");
    clear();
}

#[test]
fn att_manifest_trace_cap_is_configurable() {
    let _g = ENV_LOCK.lock().unwrap();
    clear();
    let manifest = r#"{"version":1,"run":0,"traces":[
        {"path":"a.att","shaderEngine":0},
        {"path":"b.att","shaderEngine":1}],"codeObjects":[]}"#;
    // A one-trace cap rejects the two-trace manifest on the count, before any
    // file is touched, with a message that states the effective bound.
    std::env::set_var("ROCPROF_MAX_ATT_TRACES", "1");
    let mut parsed: AttManifest = serde_json::from_str(manifest).unwrap();
    let err = parsed.resolve(dir_root()).unwrap_err();
    assert!(err.contains("1–1 traces"), "{err}");
    clear();
}

fn dir_root() -> &'static std::path::Path {
    std::path::Path::new("/")
}
