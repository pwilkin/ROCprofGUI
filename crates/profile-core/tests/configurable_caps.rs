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
