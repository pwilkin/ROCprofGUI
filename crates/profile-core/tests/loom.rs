//! Loom compile-report importer tests.
//!
//! Every inline fixture here is SYNTHETIC: hand-built from the pinned
//! version-zero schema (loom/src/loom/target/reporting + the strict loader
//! loom/py/loom/reporting/compile_report.py at commit
//! 32b86d42d96a8e3098d9a56bea201e5f83d44f85), not a recorded measurement. They
//! exercise the validator and the sparse mapping without a GPU or the compiler.
//!
//! The `#[ignore]` test at the end validates against the REAL retained reports
//! in `/devel/tools/loom-reports/` (override with `ROCPROF_LOOM_REPORTS_DIR`);
//! the integration pass runs it with `cargo test -- --ignored`.

use profile_core::load_capture;
use profile_core::loom_report::{self, LoomCompileReport};
use serde_json::{json, Value};

const FLAG_MEMORY: u64 = 1 << 3;
const FLAG_PRESSURE_ROWS: u64 = 1 << 5;
const FLAG_STATIC_MIX: u64 = 1 << 9;
const FLAG_ENTRIES: u64 = 1 << 11;
const FLAG_TARGET_RESOURCES: u64 = 1 << 13;
const FLAG_RESIDENCY_CONSTRAINTS: u64 = 1 << 27;

fn target_resources() -> Value {
    json!({
        "scalar": {
            "register_class": "amdgpu.sgpr",
            "final": {"register_count": 8, "overhead_units": 2},
            "scheduled_pressure": {"peak_live_units": 6}
        },
        "vector": {
            "register_class": "amdgpu.vgpr",
            "final": {"register_count": 8, "overhead_units": 2},
            "scheduled_pressure": {"peak_live_units": 6}
        },
        "subgroup_size": 32,
        "max_subgroups_per_simd": 16,
        "resident_subgroups_per_simd": 16,
        "occupancy_percent": 100,
        "limiting_resource": "max_waves",
        "residency": {"best_tier": 16, "current_tier": 16, "limiting_resource_count": 0}
    })
}

fn residency_constraints() -> Value {
    json!({
        "count": 2,
        "rows": [
            {
                "index": 0, "function": "k", "name": "amdgpu.vgpr", "kind": "pooled_resource",
                "unit": "registers", "allocation_scope": "subgroup", "pool_scope": "SIMD",
                "pool_units": 1536, "allocation_granularity": 24, "units": 8, "rounded_units": 24,
                "independent_tier": 16, "limiting": false
            },
            {
                "index": 1, "function": "k", "name": "amdgpu.workgroup_slots", "kind": "fixed_limit",
                "independent_tier": 16, "limiting": false
            }
        ]
    })
}

/// A minimal valid summary report (single entry), mirroring the real shape.
fn summary_report() -> Value {
    let flags = FLAG_MEMORY
        | FLAG_STATIC_MIX
        | FLAG_ENTRIES
        | FLAG_TARGET_RESOURCES
        | FLAG_RESIDENCY_CONSTRAINTS;
    json!({
        "kind": "loom.compile_report",
        "schema_version": 0,
        "mode": "summary",
        "status": {"code": 0, "name": "OK"},
        "function": "k",
        "lowered": "k_roundtrip",
        "backend": "amdgpu-hal",
        "target_family": "amdgpu",
        "target_key": "gfx1151",
        "target_export": "k_roundtrip",
        "target_export_symbol": null,
        "artifact_size": 9224,
        "detail_flags": flags,
        "static_instruction_mix": {"vector_alu_count": 2, "branch_count": 1},
        "memory": {"private_bytes": 0, "local_bytes": 1024},
        "target_resources": target_resources(),
        "residency_constraints": residency_constraints(),
        "entries": {
            "count": 1,
            "rows": [{
                "index": 0,
                "function": "k",
                "source_function": "k",
                "target_export_symbol": null,
                "detail_flags": flags,
                "allocation_spill_count": 0,
                "code_byte_count": 76,
                "local_memory_bytes": 1024,
                "static_instruction_mix": {"vector_alu_count": 2, "branch_count": 1},
                "target_resources": target_resources()
            }]
        }
    })
}

fn parse_ok(value: &Value) -> LoomCompileReport {
    loom_report::parse(value, "<test>").expect("report should validate")
}

#[test]
fn summary_report_surfaces_static_facts() {
    let report = parse_ok(&summary_report());
    assert_eq!(report.container_kind, "direct");
    assert_eq!(report.mode, "summary");
    assert_eq!(report.schema_version, 0);
    assert_eq!(report.status_code, 0);
    assert_eq!(report.status_name, "OK");
    assert_eq!(report.stability, loom_report::STABILITY);
    assert_eq!(report.identity.get("target_key").unwrap(), "gfx1151");
    assert_eq!(report.identity.get("lowered").unwrap(), "k_roundtrip");
    assert_eq!(report.artifact_size, Some(9224));

    // A nonnull identity string is kept; an explicit null is dropped, not stored.
    assert!(!report.identity.contains_key("target_export_symbol"));

    assert_eq!(report.entries.len(), 1);
    let entry = &report.entries[0];
    assert_eq!(entry.index, 0);
    assert_eq!(entry.display_name, "k");
    assert_eq!(entry.identity.get("function").unwrap(), "k");
    assert_eq!(entry.counters.get("allocation_spill_count"), Some(&0));
    assert_eq!(entry.counters.get("code_byte_count"), Some(&76));
    assert!(entry.sections.contains_key("target_resources"));
    assert!(entry.sections.contains_key("static_instruction_mix"));

    // Report-level analysis objects are preserved verbatim.
    assert!(report.sections.contains_key("memory"));
    assert!(report.sections.contains_key("target_resources"));
    assert_eq!(report.sections["memory"]["local_bytes"], json!(1024));
    assert_eq!(
        report.sections["target_resources"]["vector"]["final"]["register_count"],
        json!(8)
    );

    // Residency constraints are typed and keep the fixed-limit invariant.
    assert_eq!(report.residency_constraints.len(), 2);
    assert_eq!(report.residency_constraints[0].kind, "pooled_resource");
    assert_eq!(report.residency_constraints[0].units, Some(8));
    assert_eq!(report.residency_constraints[1].kind, "fixed_limit");
    assert_eq!(report.residency_constraints[1].units, None);

    // Decoded detail-flag names.
    assert!(report.detail_flag_names.contains(&"ENTRIES".to_string()));
    assert!(report.detail_flag_names.contains(&"TARGET_RESOURCES".to_string()));
    assert!(report
        .detail_flag_names
        .contains(&"RESIDENCY_CONSTRAINTS".to_string()));
    assert!(!report.detail_flag_names.contains(&"PRESSURE_ROWS".to_string()));
}

#[test]
fn end_to_end_import_marks_static_facts_not_measurements() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("kernel.compile-report.json");
    std::fs::write(&path, serde_json::to_vec(&summary_report()).unwrap()).unwrap();

    let capture = load_capture(&path).unwrap();
    assert!(capture.format.contains("Loom compile report"));
    assert_eq!(capture.unit, "static");
    assert_eq!(capture.architecture, "gfx1151");
    // Static facts live on their own surface, never as dispatches or counters.
    assert!(capture.dispatches.is_empty());
    assert!(capture.counters.is_empty());
    assert!(capture.waves.is_empty());
    assert_eq!(capture.compile_reports.len(), 1);
    assert_eq!(capture.compile_reports[0].entries.len(), 1);

    assert_eq!(capture.metadata["loomReportKind"], "loom.compile_report");
    assert_eq!(capture.metadata["loomReportMode"], "summary");
    assert_eq!(capture.metadata["loomSchemaVersion"], "0");
    assert_eq!(capture.metadata["loomProvenance"], "unavailable");
    assert!(capture
        .warnings
        .iter()
        .any(|w| w.contains("version-zero, same-compiler-horizon")));
    assert!(capture
        .warnings
        .iter()
        .any(|w| w.contains("no cycle or time data")));
}

#[test]
fn detail_mode_surfaces_rows_a_summary_does_not() {
    // Summary: no pressure rows, PRESSURE_ROWS bit clear.
    let summary = parse_ok(&summary_report());
    assert!(!summary.detail_rows.contains_key("pressure_rows"));
    assert!(!summary
        .detail_flag_names
        .contains(&"PRESSURE_ROWS".to_string()));

    // Details: adds a pressure-row collection and sets the PRESSURE_ROWS bit.
    let mut value = summary_report();
    value["mode"] = json!("details");
    let flags = value["detail_flags"].as_u64().unwrap() | FLAG_PRESSURE_ROWS;
    value["detail_flags"] = json!(flags);
    value["pressure_rows"] = json!({
        "count": 2,
        "rows": [
            {"index": 0, "function": "k", "register_class": "amdgpu.vgpr", "peak_live_units": 6},
            {"index": 1, "function": "k", "register_class": "amdgpu.sgpr", "peak_live_units": 6}
        ]
    });
    let details = parse_ok(&value);
    assert_eq!(details.mode, "details");
    let rows = details.detail_rows.get("pressure_rows").expect("pressure rows surfaced");
    assert_eq!(rows.count, Some(2));
    assert_eq!(rows.rows.len(), 2);
    assert!(details
        .detail_flag_names
        .contains(&"PRESSURE_ROWS".to_string()));
}

#[test]
fn absent_numeric_stays_absent_never_zero() {
    let mut value = summary_report();
    // Drop local_bytes from the memory section and allocation_spill_count from
    // the entry. They must be absent downstream, not defaulted to zero.
    value["memory"] = json!({"private_bytes": 0});
    value["entries"]["rows"][0]
        .as_object_mut()
        .unwrap()
        .remove("allocation_spill_count");
    value["entries"]["rows"][0]
        .as_object_mut()
        .unwrap()
        .remove("code_byte_count");

    let report = parse_ok(&value);
    assert!(report.sections["memory"].get("local_bytes").is_none());
    let entry = &report.entries[0];
    assert!(!entry.counters.contains_key("allocation_spill_count"));
    assert!(!entry.counters.contains_key("code_byte_count"));
}

#[test]
fn benchmark_envelope_is_recognised_with_context() {
    let envelope = json!({
        "type": "compile_report",
        "run_id": "run-7",
        "candidate_id": "cand-3",
        "candidate_index": 3,
        "case": "ffn",
        "benchmark": "microbench",
        "sample_id": "sample-1",
        "sample_index": 0,
        "compile_report": summary_report()
    });
    let report = parse_ok(&envelope);
    assert_eq!(report.container_kind, "benchmarkEnvelope");
    assert_eq!(report.envelope_context["run_id"], json!("run-7"));
    assert_eq!(report.envelope_context["candidate_index"], json!(3));
    assert_eq!(report.envelope_context["sample_index"], json!(0));
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.provenance.producing_tool.as_deref(), Some("iree-benchmark-loom"));
}

// ---- malformed rejection (mirrors compile_report.py) ----

fn err_for(mutate: impl FnOnce(&mut Value)) -> String {
    let mut value = summary_report();
    mutate(&mut value);
    loom_report::parse(&value, "<test>").expect_err("should be rejected")
}

#[test]
fn rejects_wrong_kind() {
    let value = json!({"kind": "something.else", "schema_version": 0});
    let e = loom_report::parse(&value, "<test>").unwrap_err();
    assert!(e.contains("expected kind='loom.compile_report'"), "{e}");
}

#[test]
fn rejects_nonzero_schema_version() {
    let e = err_for(|v| v["schema_version"] = json!(1));
    assert!(e.contains("regenerate the report with this compiler checkout"), "{e}");
}

#[test]
fn rejects_unknown_mode() {
    let e = err_for(|v| v["mode"] = json!("bogus"));
    assert!(e.contains("expected 'summary' or 'details'"), "{e}");
}

#[test]
fn rejects_non_object_status() {
    let e = err_for(|v| v["status"] = json!("OK"));
    assert!(e.contains("status: expected an object"), "{e}");
}

#[test]
fn rejects_nonzero_status_code_surfacing_name() {
    let e = err_for(|v| v["status"] = json!({"code": 2, "name": "COMPILATION_FAILED"}));
    assert!(e.contains("not a successful compilation"), "{e}");
    assert!(e.contains("COMPILATION_FAILED"), "{e}");
}

#[test]
fn rejects_entries_count_mismatch() {
    let e = err_for(|v| v["entries"]["count"] = json!(5));
    assert!(e.contains("count is 5, but rows has 1 entries"), "{e}");
}

#[test]
fn rejects_entry_index_mismatch() {
    let e = err_for(|v| v["entries"]["rows"][0]["index"] = json!(7));
    assert!(e.contains("index: expected 0, got 7"), "{e}");
}

#[test]
fn rejects_duplicate_entry_identity() {
    let e = err_for(|v| {
        let dup = v["entries"]["rows"][0].clone();
        let rows = v["entries"]["rows"].as_array_mut().unwrap();
        let mut second = dup;
        second["index"] = json!(1);
        rows.push(second);
        v["entries"]["count"] = json!(2);
    });
    assert!(e.contains("duplicate entry identity"), "{e}");
}

#[test]
fn rejects_entry_without_identity() {
    let e = err_for(|v| {
        v["entries"]["rows"][0] = json!({"index": 0, "allocation_spill_count": 0});
    });
    assert!(e.contains("no source or export identity"), "{e}");
}

#[test]
fn rejects_duplicate_config_binding_key() {
    let e = err_for(|v| {
        v["config_bindings"] = json!({
            "count": 2,
            "rows": [
                {"index": 0, "key": "a", "value": "1"},
                {"index": 1, "key": "a", "value": "2"}
            ]
        });
    });
    assert!(e.contains("duplicate key 'a'"), "{e}");
}

#[test]
fn rejects_residency_units_without_rounded_units() {
    let e = err_for(|v| {
        v["residency_constraints"]["rows"][0]
            .as_object_mut()
            .unwrap()
            .remove("rounded_units");
    });
    assert!(e.contains("footprint and rounded allocation"), "{e}");
}

#[test]
fn rejects_fixed_limit_carrying_units() {
    let e = err_for(|v| {
        v["residency_constraints"]["rows"][1]["units"] = json!(4);
        v["residency_constraints"]["rows"][1]["rounded_units"] = json!(4);
    });
    assert!(e.contains("a fixed limit has no reducible footprint"), "{e}");
}

#[test]
fn rejects_unavailable_residency_claiming_a_tier() {
    let e = err_for(|v| {
        v["entries"]["rows"][0]["target_resources"]["residency"] = json!({
            "best_tier": 16,
            "current_tier": 16,
            "unavailable_reasons": ["unknown_workgroup_size"]
        });
    });
    assert!(e.contains("unavailable residency cannot claim exact transitions"), "{e}");
}

#[test]
fn rejects_pre_schema_gist_shape() {
    // The pre-schema public gist is a benchmark result with an embedded
    // compile_report but no kind and no type=compile_report envelope. It must be
    // rejected as not a current compile report, not quietly adapted.
    let pre_schema = json!({
        "benchmark": "shared_memory_tile",
        "case": "roundtrip",
        "status": {"code": 0, "name": "ok"},
        "measured_duration_ns": 1234,
        "compile_report": {"some": "older shape"}
    });
    let e = loom_report::parse(&pre_schema, "<gist>").unwrap_err();
    assert!(
        e.contains("expected kind='loom.compile_report'")
            && e.contains("type='compile_report'"),
        "{e}"
    );

    // Wrapping the same pre-schema body in an explicit envelope still fails,
    // because the embedded report has no string kind.
    let wrapped = json!({"type": "compile_report", "compile_report": {"some": "older shape"}});
    let e = loom_report::parse(&wrapped, "<gist>").unwrap_err();
    assert!(e.contains("kind: expected a string"), "{e}");
}

// ---- real-report integration hook (env-gated) ----

/// Validates the importer against the REAL retained Loom reports. Point
/// `ROCPROF_LOOM_REPORTS_DIR` at the directory, or rely on the default
/// `/devel/tools/loom-reports`. Run with `cargo test -- --ignored`.
#[test]
#[ignore = "requires real Loom reports via ROCPROF_LOOM_REPORTS_DIR (default /devel/tools/loom-reports)"]
fn real_reports_validate_and_map() {
    let base = std::env::var("ROCPROF_LOOM_REPORTS_DIR")
        .unwrap_or_else(|_| "/devel/tools/loom-reports".to_string());
    let base = std::path::PathBuf::from(base);
    let reports = base.join("reports");
    if !reports.is_dir() {
        eprintln!("skipping: {} is not present", reports.display());
        return;
    }

    let mut summary_rows = None;
    let mut details_rows = None;
    for entry in std::fs::read_dir(&reports).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if !name.ends_with(".json") {
            continue;
        }
        assert!(loom_report::is_loom_report(&path), "{name} not detected as a loom report");
        let capture = load_capture(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(capture.compile_reports.len(), 1, "{name}");
        let report = &capture.compile_reports[0];
        assert_eq!(report.schema_version, 0, "{name}");
        assert_eq!(report.status_code, 0, "{name}");
        assert_eq!(report.entries.len(), 1, "{name}");
        assert!(capture.dispatches.is_empty(), "{name} must not create dispatches");
        // Provenance is recovered from the sidecars shipped beside the reports.
        assert!(report.provenance.source_commit.is_some(), "{name} provenance");

        if name.contains("shared_memory_vector_tile.gfx1151.summary") {
            summary_rows = Some(report.detail_rows.len());
            assert!(!report.detail_rows.contains_key("pressure_rows"), "{name}");
        }
        if name.contains("shared_memory_vector_tile.gfx1151.details") {
            details_rows = Some(report.detail_rows.len());
            assert!(report.detail_rows.contains_key("pressure_rows"), "{name}");
        }
    }
    if let (Some(s), Some(d)) = (summary_rows, details_rows) {
        assert!(d > s, "details should surface more detail-row collections than summary");
    }

    // The pre-schema public gist shipped as a fixture must be rejected.
    let pre_schema = base.join("fixtures/shared_memory_tile_benchmark_result.pre-schema.json");
    if pre_schema.is_file() {
        let bytes = std::fs::read(&pre_schema).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(
            loom_report::parse(&value, "<gist>").is_err(),
            "pre-schema gist must be rejected"
        );
    }
}
