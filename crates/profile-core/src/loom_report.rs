//! Loom compile-report importer.
//!
//! Loom is AMD's source-first GPU kernel compiler inside ROCm/hrx-system. A
//! compile report is compiler telemetry about one compiled kernel: schedule,
//! allocation, emission, register pressure, residency and the static/dynamic
//! instruction mix the compiler modelled. It is not a trace, not a dispatch and
//! not a runtime measurement.
//!
//! These reports are version-zero, same-compiler-horizon diagnostics. The
//! stability contract from the producer is explicit: "Compile reports are
//! version-zero, same-compiler-horizon diagnostics. Regenerate them with the
//! current checkout instead of treating them as durable records or adding
//! compatibility paths to the consumer." So this importer validates strictly
//! against schema version zero, refuses anything else, and preserves the
//! present fields verbatim rather than building a durable typed schema for
//! every counter.
//!
//! The validation mirrors `loom/py/loom/reporting/compile_report.py` at the
//! pinned checkout (the strict loader): envelope shape, indexed-row invariants,
//! entry identity, the residency summary invariants and the residency-constraint
//! invariants. On top of the loader this importer also refuses a non-successful
//! compilation (`status.code != 0`), because a failed compile has no meaningful
//! static facts to surface.
//!
//! What this importer refuses to infer (enforced by never synthesising it):
//! there are no cycle or time counts anywhere in a compile report; dynamic
//! execution counts are never derived from static counts; hardware register
//! counts are never derived from `scheduled_pressure.peak_live_units`; occupancy
//! percent is static compiler evidence, not a throughput or achieved-occupancy
//! prediction; an absent `bank_service_summary` is not a claim of bank-conflict
//! freedom; and reports are not comparable across targets or compiler horizons.
//! Absent fields stay absent: a missing numeric is never defaulted to zero.
//!
//! The report carries no compiler identity, so provenance (producing tool,
//! hrx-system checkout, build date, invocation, target, config) is recorded out
//! of band: from `tool-provenance.json` / `commands.json` sidecars next to the
//! report when present, and otherwise marked unavailable rather than guessed.

use crate::model::Capture;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

/// Required envelope `kind` for a direct report.
pub const KIND: &str = "loom.compile_report";
/// The only schema version this importer accepts.
pub const SCHEMA_VERSION: i64 = 0;
/// The epistemic label attached to every imported report.
pub const STABILITY: &str = "version-zero, same-compiler-horizon";

const REPORT_STRING_IDENTITY_FIELDS: &[&str] = &[
    "artifact_kind",
    "artifact_format",
    "backend",
    "module",
    "function",
    "target_family",
    "target_key",
    "target_bundle",
    "target_snapshot",
    "target_export",
    "target_export_symbol",
    "target_config",
];

const ENTRY_IDENTITY_FIELDS: &[&str] = &[
    "function",
    "source_function",
    "target_export",
    "target_export_symbol",
];

const ENTRY_CONTEXT_FIELDS: &[&str] = &[
    "target_bundle",
    "target_snapshot",
    "target_config",
    "workload",
];

const ENVELOPE_CONTEXT_FIELDS: &[&str] = &[
    "run_id",
    "candidate_id",
    "candidate_index",
    "case",
    "benchmark",
    "sample_id",
    "sample_index",
];

/// Top-level collections stored as indexed detail rows rather than as a nested
/// analysis section. Each is `{count|row_count, rows:[...]}` in the producer
/// JSON; the strict loader does not validate their indexing, so neither do we,
/// but we count their rows toward the record cap and preserve them verbatim.
const DETAIL_ROW_KEYS: &[&str] = &[
    "pressure_rows",
    "pressure_origin_rows",
    "spill_rows",
    "allocation_high_water_rows",
    "allocation_failure_rows",
    "schedule_band_rows",
    "schedule_band_summary_rows",
    "wait_counter_rows",
    "wait_reason_summary_rows",
    "wait_action_rows",
    "target_capability_rows",
    "math_legalization",
    "target_legalization",
    "target_insertion_rows",
    "target_insertions",
];

/// The 30 documented detail-flag bits. Bit 27 (`RESIDENCY_CONSTRAINTS`) is
/// declared out of numeric order in the producer header; the mask is decoded by
/// bit value, not by declaration order.
const DETAIL_FLAG_BITS: &[(u32, &str)] = &[
    (0, "ARTIFACT_SIZE"),
    (1, "SCHEDULE"),
    (2, "ALLOCATION"),
    (3, "MEMORY"),
    (4, "EMISSION"),
    (5, "PRESSURE_ROWS"),
    (6, "SPILL_ROWS"),
    (7, "SOURCE_LOW_ROWS"),
    (8, "MOVE_CAUSES"),
    (9, "STATIC_INSTRUCTION_MIX"),
    (10, "TARGET_LEGALIZATION_ROWS"),
    (11, "ENTRIES"),
    (12, "ALLOCATION_FAILURE_ROWS"),
    (13, "TARGET_RESOURCES"),
    (14, "MATH_LEGALIZATION_ROWS"),
    (15, "PRESSURE_ORIGIN_ROWS"),
    (16, "SCHEDULE_BAND_ROWS"),
    (17, "ALLOCATION_HIGH_WATER_ROWS"),
    (18, "WAIT_PLAN"),
    (19, "TARGET_CAPABILITY_ROWS"),
    (20, "WORKLOAD"),
    (21, "DYNAMIC_INSTRUCTION_MIX"),
    (22, "SCHEDULE_BAND_SUMMARY_ROWS"),
    (23, "CONFIG_BINDING_ROWS"),
    (24, "LOW_PLANNING"),
    (25, "TARGET_INSERTION_ROWS"),
    (26, "EMISSION_BREAKDOWN"),
    (27, "RESIDENCY_CONSTRAINTS"),
    (28, "PIPELINE_PLAN"),
    (29, "PIPELINE_PLAN_ROWS"),
];

/// Decodes a detail-flag mask into the set-bit names. Bits above the documented
/// range are reported as `UNKNOWN_BIT_n` rather than silently dropped, so a
/// report from a newer horizon is visible as such instead of looking empty.
pub fn decode_flags(mask: u64) -> Vec<String> {
    let mut names = Vec::new();
    for (bit, name) in DETAIL_FLAG_BITS {
        if mask & (1u64 << bit) != 0 {
            names.push((*name).to_string());
        }
    }
    for bit in 30..64u32 {
        if mask & (1u64 << bit) != 0 {
            names.push(format!("UNKNOWN_BIT_{bit}"));
        }
    }
    names
}

/// One imported compile report: the validated envelope, report-level identity,
/// per-entry static facts, residency constraints, preserved analysis sections
/// and detail rows, plus out-of-band provenance.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoomCompileReport {
    /// Fixed epistemic label; see [`STABILITY`].
    pub stability: String,
    /// `direct` for a `loom.compile_report`, `benchmarkEnvelope` for a report
    /// embedded in an `iree-benchmark-loom` envelope.
    pub container_kind: String,
    /// `summary` or `details`.
    pub mode: String,
    /// Always `0` for an accepted report.
    pub schema_version: i64,
    /// Compiler status code; always `0` (success) for an accepted report.
    pub status_code: i64,
    /// Compiler status name, e.g. `OK`.
    pub status_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail_flags: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detail_flag_names: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub identity: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_size: Option<u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub envelope_context: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub config_bindings: Vec<LoomConfigBinding>,
    pub provenance: LoomProvenance,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<LoomCompileEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub residency_constraints: Vec<LoomResidencyConstraint>,
    /// Report-level analysis objects preserved verbatim (schedule, allocation,
    /// emission, memory, economics, instruction mixes, target_resources,
    /// wait_plan, workload, move_causes, planning, source_low, diagnostics...).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sections: BTreeMap<String, Value>,
    /// Detail-mode row collections preserved verbatim, keyed by collection name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub detail_rows: BTreeMap<String, LoomRowCollection>,
}

/// One `config_bindings` entry, sorted by key as the strict loader sorts them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoomConfigBinding {
    pub key: String,
    pub value: String,
}

/// Per-kernel static compile facts. Flat integer counters land in `counters`
/// under their producer names; nested analysis objects (target_resources,
/// static/dynamic instruction mix, economics, workload, move_causes, wait_plan,
/// planning...) are preserved verbatim in `sections`. Only present fields are
/// recorded, so an absent counter is absent rather than zero.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoomCompileEntry {
    pub index: usize,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub identity: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail_flags: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detail_flag_names: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub counters: BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sections: BTreeMap<String, Value>,
}

/// One residency constraint, validated against the loader's invariants. A
/// `fixed_limit` carries none of `units`/`rounded_units`/
/// `reduction_units_to_next_better_tier`; `units` and `rounded_units` are
/// present together or not at all.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoomResidencyConstraint {
    pub index: usize,
    pub function: String,
    pub name: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allocation_scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pool_scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pool_units: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allocation_granularity: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rounded_units: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub independent_tier: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limiting: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reduction_units_to_next_better_tier: Option<u64>,
}

/// A preserved detail-row collection. `count` is the producer's declared count
/// (`count` or `row_count`) when present; `rows` is the verbatim row array.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoomRowCollection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
    pub rows: Vec<Value>,
}

/// Out-of-band provenance. The report carries no compiler identity, so this is
/// filled from sidecars next to the report when present and marked unavailable
/// otherwise. Nothing here is inferred from the report body except the
/// producing tool implied by the container kind.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoomProvenance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producing_tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkout: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub invocation: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub config: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle_policy: Option<String>,
    /// True when any provenance field was recovered from a sidecar.
    pub available: bool,
    /// Human-readable note on where provenance came from or why it is missing.
    pub note: String,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// The only producer of each container kind: a direct report comes from
/// loom-compile, a benchmark envelope from iree-benchmark-loom.
fn implied_tool(container_kind: &str) -> &'static str {
    if container_kind == "benchmarkEnvelope" {
        "iree-benchmark-loom"
    } else {
        "loom-compile"
    }
}

/// Detects a Loom compile report by the `.compile-report.json` suffix or by the
/// version-zero `loom.compile_report` discriminator near the head of the file.
/// Direct reports carry it as the first key; benchmark envelopes carry it as the
/// embedded report's `kind`, which still appears near the top. Scanning a bounded
/// head keeps a plain SPM `.json` open cheap and avoids a false positive, since
/// SPM results do not contain the literal.
pub fn is_loom_report(path: &Path) -> bool {
    if path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.to_ascii_lowercase().ends_with(".compile-report.json"))
    {
        return true;
    }
    use std::io::Read;
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = vec![0u8; 64 * 1024];
    let Ok(read) = file.read(&mut head) else {
        return false;
    };
    let needle = KIND.as_bytes();
    head[..read].windows(needle.len()).any(|w| w == needle)
}

/// Loads and validates a Loom compile report file into a [`Capture`] of static
/// compile facts.
pub fn load(path: &Path) -> Result<Capture, String> {
    let size = std::fs::metadata(path).map_err(err)?.len();
    if size > crate::caps::max_file_bytes() {
        return Err(format!(
            "Loom compile report exceeds the {} MiB per-file import limit",
            crate::caps::max_file_mib()
        ));
    }
    let bytes = std::fs::read(path).map_err(err)?;
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let source = path.display().to_string();
    let mut report = parse(&value, &source)?;
    report.provenance = load_provenance(path, &report.container_kind);
    Ok(build_capture(path, report))
}

/// Validates a decoded compile-report document and extracts the static facts.
/// This is the strict loader mirrored in Rust and is the testable boundary;
/// `load` adds file IO, provenance and the [`Capture`] wrapper.
pub fn parse(value: &Value, source: &str) -> Result<LoomCompileReport, String> {
    let root = require_object(value, source)?;

    let mut container_kind = "direct";
    let mut envelope_context: BTreeMap<String, Value> = BTreeMap::new();
    let report: &Map<String, Value> = if root.get("kind").and_then(Value::as_str) == Some(KIND) {
        root
    } else if root.get("type").and_then(Value::as_str) == Some("compile_report") {
        container_kind = "benchmarkEnvelope";
        let embedded = require_object(
            root.get("compile_report").unwrap_or(&Value::Null),
            &format!("{source}.compile_report"),
        )?;
        for field in ENVELOPE_CONTEXT_FIELDS {
            if let Some(v) = root.get(*field) {
                if v.is_string() || v.is_i64() || v.is_u64() || v.is_boolean() {
                    envelope_context.insert((*field).to_string(), v.clone());
                }
            }
        }
        embedded
    } else {
        return Err(format!(
            "{source}: expected kind='{KIND}' or an explicit type='compile_report' benchmark envelope"
        ));
    };

    let kind = require_string(report.get("kind").unwrap_or(&Value::Null), &format!("{source}.kind"))?;
    if kind != KIND {
        return Err(format!("{source}.kind: expected '{KIND}', got '{kind}'"));
    }
    let schema_version = require_integer(
        report.get("schema_version").unwrap_or(&Value::Null),
        &format!("{source}.schema_version"),
    )?;
    if schema_version != SCHEMA_VERSION {
        return Err(format!(
            "{source}.schema_version: expected {SCHEMA_VERSION}, got {schema_version}; regenerate the report with this compiler checkout"
        ));
    }
    let mode = require_string(report.get("mode").unwrap_or(&Value::Null), &format!("{source}.mode"))?
        .to_string();
    if mode != "summary" && mode != "details" {
        return Err(format!(
            "{source}.mode: expected 'summary' or 'details', got '{mode}'"
        ));
    }
    for field in REPORT_STRING_IDENTITY_FIELDS {
        if let Some(v) = report.get(*field) {
            optional_string(v, &format!("{source}.{field}"))?;
        }
    }
    if let Some(v) = report.get("workload") {
        require_object(v, &format!("{source}.workload"))?;
    }

    let status = require_object(
        report.get("status").unwrap_or(&Value::Null),
        &format!("{source}.status"),
    )?;
    let status_code = require_integer(
        status.get("code").unwrap_or(&Value::Null),
        &format!("{source}.status.code"),
    )?;
    let status_name = require_string(
        status.get("name").unwrap_or(&Value::Null),
        &format!("{source}.status.name"),
    )?
    .to_string();
    if status_code != 0 {
        return Err(format!(
            "{source}.status: compile report is not a successful compilation: status '{status_name}' (code {status_code}); regenerate the report with this compiler checkout"
        ));
    }

    let mut total_rows = 0usize;

    let entries_obj = require_object(
        report.get("entries").unwrap_or(&Value::Null),
        &format!("{source}.entries"),
    )?;
    let entry_rows = validate_indexed_rows(entries_obj, &format!("{source}.entries"))?;
    total_rows += entry_rows.len();
    let mut seen_identities: HashSet<[Option<String>; 4]> = HashSet::new();
    let mut entries = Vec::with_capacity(entry_rows.len());
    for (index, row) in entry_rows.iter().enumerate() {
        let esrc = format!("{source}.entries.rows[{index}]");
        let identity = entry_identity(row, &esrc)?;
        for field in ENTRY_CONTEXT_FIELDS {
            if let Some(v) = row.get(*field) {
                if *field == "workload" {
                    require_object(v, &format!("{esrc}.{field}"))?;
                } else {
                    optional_string(v, &format!("{esrc}.{field}"))?;
                }
            }
        }
        if !seen_identities.insert(identity.clone()) {
            return Err(format!(
                "{esrc}: duplicate entry identity '{}'",
                display_name(&identity)
            ));
        }
        validate_residency_summary(row, &esrc)?;
        entries.push(build_entry(row, index, &identity));
    }

    let mut config_bindings = Vec::new();
    if let Some(cb) = report.get("config_bindings") {
        let obj = require_object(cb, &format!("{source}.config_bindings"))?;
        let rows = validate_indexed_rows(obj, &format!("{source}.config_bindings"))?;
        total_rows += rows.len();
        let mut map: BTreeMap<String, String> = BTreeMap::new();
        for (index, row) in rows.iter().enumerate() {
            let key = require_string(
                row.get("key").unwrap_or(&Value::Null),
                &format!("{source}.config_bindings.rows[{index}].key"),
            )?
            .to_string();
            let binding_value = require_string(
                row.get("value").unwrap_or(&Value::Null),
                &format!("{source}.config_bindings.rows[{index}].value"),
            )?
            .to_string();
            if map.contains_key(&key) {
                return Err(format!(
                    "{source}.config_bindings.rows[{index}]: duplicate key '{key}'"
                ));
            }
            map.insert(key, binding_value);
        }
        config_bindings = map
            .into_iter()
            .map(|(key, value)| LoomConfigBinding { key, value })
            .collect();
    }

    let mut residency_constraints = Vec::new();
    if let Some(rc) = report.get("residency_constraints") {
        let (rows, parsed) =
            index_residency_constraints(rc, &format!("{source}.residency_constraints"))?;
        total_rows += rows;
        residency_constraints = parsed;
    }

    let detail_flags = report.get("detail_flags").and_then(Value::as_u64);
    let detail_flag_names = detail_flags.map(decode_flags).unwrap_or_default();

    let mut identity: BTreeMap<String, String> = BTreeMap::new();
    for field in REPORT_STRING_IDENTITY_FIELDS.iter().chain(std::iter::once(&"lowered")) {
        if let Some(s) = report.get(*field).and_then(Value::as_str) {
            identity.insert((*field).to_string(), s.to_string());
        }
    }
    let artifact_size = report.get("artifact_size").and_then(Value::as_u64);

    let mut pulled: HashSet<&str> = HashSet::from([
        "kind",
        "schema_version",
        "mode",
        "status",
        "detail_flags",
        "entries",
        "config_bindings",
        "residency_constraints",
        "artifact_size",
        "lowered",
    ]);
    pulled.extend(REPORT_STRING_IDENTITY_FIELDS.iter().copied());

    let mut sections: BTreeMap<String, Value> = BTreeMap::new();
    let mut detail_rows: BTreeMap<String, LoomRowCollection> = BTreeMap::new();
    for (key, v) in report.iter() {
        if pulled.contains(key.as_str()) {
            continue;
        }
        if DETAIL_ROW_KEYS.contains(&key.as_str()) {
            if let Some(obj) = v.as_object() {
                if let Some(rows) = obj.get("rows").and_then(Value::as_array) {
                    let count = obj
                        .get("count")
                        .and_then(Value::as_u64)
                        .or_else(|| obj.get("row_count").and_then(Value::as_u64));
                    total_rows += rows.len();
                    detail_rows.insert(
                        key.clone(),
                        LoomRowCollection {
                            count,
                            rows: rows.clone(),
                        },
                    );
                    continue;
                }
            }
        }
        sections.insert(key.clone(), v.clone());
    }

    let cap = crate::caps::max_records();
    if total_rows > cap {
        return Err(format!(
            "Loom compile report exceeds the {cap} record limit; regenerate a smaller report"
        ));
    }

    Ok(LoomCompileReport {
        stability: STABILITY.to_string(),
        container_kind: container_kind.to_string(),
        mode,
        schema_version,
        status_code,
        status_name,
        detail_flags,
        detail_flag_names,
        identity,
        artifact_size,
        envelope_context,
        config_bindings,
        provenance: LoomProvenance {
            producing_tool: Some(implied_tool(container_kind).to_string()),
            note: "Provenance is recorded out of band; parse does not read sidecars.".to_string(),
            ..Default::default()
        },
        entries,
        residency_constraints,
        sections,
        detail_rows,
    })
}

fn build_entry(row: &Map<String, Value>, index: usize, identity4: &[Option<String>; 4]) -> LoomCompileEntry {
    let mut identity: BTreeMap<String, String> = BTreeMap::new();
    let mut counters: BTreeMap<String, u64> = BTreeMap::new();
    let mut sections: BTreeMap<String, Value> = BTreeMap::new();
    let mut detail_flags = None;
    for (key, v) in row.iter() {
        match key.as_str() {
            "index" => {}
            "detail_flags" => detail_flags = v.as_u64(),
            _ => match v {
                Value::String(s) => {
                    identity.insert(key.clone(), s.clone());
                }
                Value::Number(_) => {
                    if let Some(u) = v.as_u64() {
                        counters.insert(key.clone(), u);
                    } else {
                        sections.insert(key.clone(), v.clone());
                    }
                }
                Value::Null => {}
                Value::Object(_) | Value::Array(_) | Value::Bool(_) => {
                    sections.insert(key.clone(), v.clone());
                }
            },
        }
    }
    LoomCompileEntry {
        index,
        display_name: display_name(identity4),
        identity,
        detail_flags,
        detail_flag_names: detail_flags.map(decode_flags).unwrap_or_default(),
        counters,
        sections,
    }
}

fn entry_identity(entry: &Map<String, Value>, source: &str) -> Result<[Option<String>; 4], String> {
    let mut values: [Option<String>; 4] = Default::default();
    for (i, field) in ENTRY_IDENTITY_FIELDS.iter().enumerate() {
        values[i] = match entry.get(*field) {
            None => None,
            Some(v) => optional_string(v, &format!("{source}.{field}"))?.map(str::to_string),
        };
    }
    if values.iter().all(Option::is_none) {
        return Err(format!("{source}: entry has no source or export identity"));
    }
    Ok(values)
}

/// The most useful available entry name, matching the strict loader's
/// `display_name`: source_function, then target_export, then function, then
/// target_export_symbol, then `<unnamed>`.
fn display_name(identity: &[Option<String>; 4]) -> String {
    identity[1]
        .clone()
        .or_else(|| identity[2].clone())
        .or_else(|| identity[0].clone())
        .or_else(|| identity[3].clone())
        .unwrap_or_else(|| "<unnamed>".to_string())
}

fn validate_residency_summary(entry: &Map<String, Value>, source: &str) -> Result<(), String> {
    let Some(resources) = entry.get("target_resources") else {
        return Ok(());
    };
    let resources = require_object(resources, &format!("{source}.target_resources"))?;
    let Some(summary) = resources.get("residency") else {
        return Ok(());
    };
    let path = format!("{source}.target_resources.residency");
    let summary = require_object(summary, &path)?;
    for field in ["best_tier", "current_tier", "next_better_tier", "limiting_resource_count"] {
        if let Some(v) = summary.get(field) {
            if require_integer(v, &format!("{path}.{field}"))? < 0 {
                return Err(format!("{path}.{field}: expected a nonnegative integer"));
            }
        }
    }
    if let Some(reasons) = summary.get("unavailable_reasons") {
        let valid = reasons
            .as_array()
            .map(|arr| {
                !arr.is_empty()
                    && arr
                        .iter()
                        .all(|r| r.as_str().is_some_and(|s| !s.is_empty()))
            })
            .unwrap_or(false);
        if !valid {
            return Err(format!(
                "{path}.unavailable_reasons: expected nonempty reason strings"
            ));
        }
        if ["current_tier", "next_better_tier", "unique_limiting_resource"]
            .iter()
            .any(|f| summary.contains_key(*f))
        {
            return Err(format!(
                "{path}: unavailable residency cannot claim exact transitions"
            ));
        }
    }
    Ok(())
}

fn index_residency_constraints(
    value: &Value,
    source: &str,
) -> Result<(usize, Vec<LoomResidencyConstraint>), String> {
    let obj = require_object(value, source)?;
    let rows = validate_indexed_rows(obj, source)?;
    let mut identities: HashSet<(String, String)> = HashSet::new();
    let mut out = Vec::with_capacity(rows.len());
    for (index, row) in rows.iter().enumerate() {
        let rp = format!("{source}.rows[{index}]");
        let function = require_string(
            row.get("function").unwrap_or(&Value::Null),
            &format!("{rp}.function"),
        )?
        .to_string();
        let name =
            require_string(row.get("name").unwrap_or(&Value::Null), &format!("{rp}.name"))?.to_string();
        if function.is_empty()
            || name.is_empty()
            || !identities.insert((function.clone(), name.clone()))
        {
            return Err(format!(
                "{rp}: expected a unique nonempty function/resource identity"
            ));
        }
        let kind = row.get("kind").and_then(Value::as_str).unwrap_or("");
        if !matches!(kind, "pooled_resource" | "unconstrained_resource" | "fixed_limit") {
            return Err(format!("{rp}.kind: invalid residency constraint kind"));
        }
        if kind != "fixed_limit" {
            for field in ["unit", "allocation_scope", "pool_scope"] {
                let s = require_string(
                    row.get(field).unwrap_or(&Value::Null),
                    &format!("{rp}.{field}"),
                )?;
                if s.is_empty() {
                    return Err(format!("{rp}.{field}: expected a nonempty string"));
                }
            }
            if require_integer(
                row.get("pool_units").unwrap_or(&Value::Null),
                &format!("{rp}.pool_units"),
            )? < 0
            {
                return Err(format!("{rp}.pool_units: expected a nonnegative integer"));
            }
            if require_integer(
                row.get("allocation_granularity").unwrap_or(&Value::Null),
                &format!("{rp}.allocation_granularity"),
            )? <= 0
            {
                return Err(format!(
                    "{rp}.allocation_granularity: expected a positive integer"
                ));
            }
        } else if ["units", "rounded_units", "reduction_units_to_next_better_tier"]
            .iter()
            .any(|f| row.contains_key(*f))
        {
            return Err(format!("{rp}: a fixed limit has no reducible footprint"));
        }
        for field in [
            "units",
            "rounded_units",
            "independent_tier",
            "reduction_units_to_next_better_tier",
        ] {
            if let Some(v) = row.get(field) {
                if require_integer(v, &format!("{rp}.{field}"))? < 0 {
                    return Err(format!("{rp}.{field}: expected a nonnegative integer"));
                }
            }
        }
        if row.contains_key("units") != row.contains_key("rounded_units") {
            return Err(format!(
                "{rp}: footprint and rounded allocation must be available together"
            ));
        }
        if let Some(l) = row.get("limiting") {
            if !l.is_boolean() {
                return Err(format!("{rp}.limiting: expected a boolean"));
            }
        }
        if let Some(reduction_value) = row.get("reduction_units_to_next_better_tier") {
            let reduction = require_integer(
                reduction_value,
                &format!("{rp}.reduction_units_to_next_better_tier"),
            )?;
            let units = row.get("units").and_then(Value::as_i64);
            let limiting = row.get("limiting").and_then(Value::as_bool) == Some(true);
            if !limiting || units.is_none() || !(0 < reduction && reduction <= units.unwrap()) {
                return Err(format!(
                    "{rp}: reduction requires a known limiting footprint"
                ));
            }
        }
        out.push(LoomResidencyConstraint {
            index,
            function,
            name,
            kind: kind.to_string(),
            unit: row.get("unit").and_then(Value::as_str).map(str::to_string),
            allocation_scope: row
                .get("allocation_scope")
                .and_then(Value::as_str)
                .map(str::to_string),
            pool_scope: row.get("pool_scope").and_then(Value::as_str).map(str::to_string),
            pool_units: row.get("pool_units").and_then(Value::as_u64),
            allocation_granularity: row.get("allocation_granularity").and_then(Value::as_u64),
            units: row.get("units").and_then(Value::as_u64),
            rounded_units: row.get("rounded_units").and_then(Value::as_u64),
            independent_tier: row.get("independent_tier").and_then(Value::as_u64),
            limiting: row.get("limiting").and_then(Value::as_bool),
            reduction_units_to_next_better_tier: row
                .get("reduction_units_to_next_better_tier")
                .and_then(Value::as_u64),
        });
    }
    Ok((rows.len(), out))
}

fn validate_indexed_rows<'a>(
    obj: &'a Map<String, Value>,
    source: &str,
) -> Result<Vec<&'a Map<String, Value>>, String> {
    let count = require_integer(obj.get("count").unwrap_or(&Value::Null), &format!("{source}.count"))?;
    let rows = obj
        .get("rows")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{source}.rows: expected an array"))?;
    if count != rows.len() as i64 {
        return Err(format!(
            "{source}: count is {count}, but rows has {} entries",
            rows.len()
        ));
    }
    let mut out = Vec::with_capacity(rows.len());
    for (index, row_value) in rows.iter().enumerate() {
        let row = require_object(row_value, &format!("{source}.rows[{index}]"))?;
        let row_index = require_integer(
            row.get("index").unwrap_or(&Value::Null),
            &format!("{source}.rows[{index}].index"),
        )?;
        if row_index != index as i64 {
            return Err(format!(
                "{source}.rows[{index}].index: expected {index}, got {row_index}"
            ));
        }
        out.push(row);
    }
    Ok(out)
}

fn require_object<'a>(value: &'a Value, source: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{source}: expected an object"))
}

fn require_string<'a>(value: &'a Value, source: &str) -> Result<&'a str, String> {
    value
        .as_str()
        .ok_or_else(|| format!("{source}: expected a string"))
}

fn optional_string<'a>(value: &'a Value, source: &str) -> Result<Option<&'a str>, String> {
    if value.is_null() {
        return Ok(None);
    }
    Ok(Some(require_string(value, source)?))
}

fn require_integer(value: &Value, source: &str) -> Result<i64, String> {
    if value.is_boolean() {
        return Err(format!("{source}: expected an integer"));
    }
    value
        .as_i64()
        .ok_or_else(|| format!("{source}: expected an integer"))
}

/// Reads provenance from `tool-provenance.json` / `commands.json` sidecars in
/// the report's directory, matching the command that produced this report by
/// its `--compile-report-output` path. Absent sidecars leave provenance
/// unavailable rather than inventing it.
fn load_provenance(path: &Path, container_kind: &str) -> LoomProvenance {
    let mut provenance = LoomProvenance {
        producing_tool: Some(implied_tool(container_kind).to_string()),
        ..Default::default()
    };
    let Some(dir) = path.parent() else {
        provenance.note = "Report has no parent directory; provenance not supplied.".to_string();
        return provenance;
    };
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

    if let Some(tp) = find_sidecar(dir, "tool-provenance.json") {
        provenance.source_commit = tp
            .get("source_commit")
            .and_then(Value::as_str)
            .map(str::to_string);
        provenance.checkout = tp.get("checkout").and_then(Value::as_str).map(str::to_string);
        let want_tool = provenance.producing_tool.clone().unwrap_or_default();
        if let Some(tool) = tp
            .get("tools")
            .and_then(Value::as_array)
            .and_then(|tools| {
                tools
                    .iter()
                    .find(|t| t.get("tool").and_then(Value::as_str) == Some(want_tool.as_str()))
            })
        {
            provenance.tool_sha256 = tool.get("sha256").and_then(Value::as_str).map(str::to_string);
            provenance.build_date = tool
                .get("build_completed_at")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
    }

    if let Some(commands) = find_sidecar(dir, "commands.json") {
        if let Some(command) = commands.as_array().and_then(|commands| {
            commands.iter().find(|c| {
                c.get("argv")
                    .and_then(Value::as_array)
                    .is_some_and(|argv| argv.iter().any(|a| command_names_report(a, file_name)))
            })
        }) {
            if let Some(argv) = command.get("argv").and_then(Value::as_array) {
                provenance.invocation = argv
                    .iter()
                    .filter_map(|a| a.as_str().map(str::to_string))
                    .collect();
                if let Some(tool) = provenance
                    .invocation
                    .first()
                    .and_then(|a| Path::new(a).file_name())
                    .and_then(|n| n.to_str())
                {
                    provenance.producing_tool = Some(tool.to_string());
                }
            }
            provenance.target = command.get("target").and_then(Value::as_str).map(str::to_string);
            if let Some(config) = command.get("config").and_then(Value::as_object) {
                provenance.config = config.clone().into_iter().collect();
                provenance.bundle_policy = config
                    .get("artifact_bundle_policy")
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
        }
    }

    provenance.available = provenance.source_commit.is_some()
        || provenance.tool_sha256.is_some()
        || !provenance.invocation.is_empty();
    provenance.note = if provenance.available {
        "Recovered from tool-provenance.json / commands.json sidecars; a compile report carries no compiler identity of its own.".to_string()
    } else {
        "No tool-provenance.json / commands.json sidecar alongside the report. A compile report carries no compiler identity, so the producing tool, hrx-system checkout, build date and invocation must be supplied out of band.".to_string()
    };
    provenance
}

fn command_names_report(arg: &Value, file_name: &str) -> bool {
    arg.as_str().is_some_and(|a| {
        (a.starts_with("--compile-report-output") || a.starts_with("--output"))
            && Path::new(a.rsplit('=').next().unwrap_or(a))
                .file_name()
                .and_then(|n| n.to_str())
                == Some(file_name)
    })
}

/// Looks for a provenance sidecar beside the report, then one directory up (the
/// bundle root, where the retained reports keep tool-provenance.json and
/// commands.json above a reports/ subdirectory).
fn find_sidecar(report_dir: &Path, name: &str) -> Option<Value> {
    if let Some(value) = read_sidecar(&report_dir.join(name)) {
        return Some(value);
    }
    report_dir
        .parent()
        .and_then(|parent| read_sidecar(&parent.join(name)))
}

fn read_sidecar(path: &Path) -> Option<Value> {
    let size = std::fs::metadata(path).ok()?.len();
    if size > crate::caps::max_file_bytes() {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn build_capture(path: &Path, report: LoomCompileReport) -> Capture {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let mut capture = Capture::empty(
        name,
        "Loom compile report (version-zero static facts)",
        "static",
    );
    capture.architecture = report
        .identity
        .get("target_key")
        .or_else(|| report.identity.get("target_family"))
        .cloned()
        .unwrap_or_else(|| "Unknown".to_string());

    capture.warnings.push(format!(
        "Loom compile report: {STABILITY} static compile facts. These are compiler telemetry about a compiled kernel, not GPU measurements, not dispatches and not runtime counts."
    ));
    capture.warnings.push(
        "A compile report contains no cycle or time data. Occupancy percent and scheduled register pressure are static compiler evidence, not observed GPU occupancy or hardware register counts. Absent fields are unavailable, not zero."
            .to_string(),
    );
    capture.warnings.push(
        "Compile reports are not comparable across targets or compiler horizons (an AMDGPU report and a SPIR-V report are not comparable even at schema version zero). Regenerate with the current compiler checkout rather than treating a report as a durable record."
            .to_string(),
    );
    if !report.provenance.available {
        capture.warnings.push(report.provenance.note.clone());
    }

    let meta = &mut capture.metadata;
    meta.insert("loomReportKind".into(), KIND.into());
    meta.insert("loomReportMode".into(), report.mode.clone());
    meta.insert("loomSchemaVersion".into(), report.schema_version.to_string());
    meta.insert("loomStability".into(), STABILITY.into());
    meta.insert("loomContainerKind".into(), report.container_kind.clone());
    meta.insert("loomStatus".into(), format!("{} ({})", report.status_name, report.status_code));
    meta.insert("loomEntryCount".into(), report.entries.len().to_string());
    if let Some(target) = report.identity.get("target_key") {
        meta.insert("loomTargetKey".into(), target.clone());
    }
    if let Some(family) = report.identity.get("target_family") {
        meta.insert("loomTargetFamily".into(), family.clone());
    }
    if let Some(function) = report.identity.get("function") {
        meta.insert("loomFunction".into(), function.clone());
    }
    if let Some(tool) = &report.provenance.producing_tool {
        meta.insert("loomProducingTool".into(), tool.clone());
    }
    if let Some(commit) = &report.provenance.source_commit {
        meta.insert("loomSourceCommit".into(), commit.clone());
    }
    meta.insert(
        "loomProvenance".into(),
        if report.provenance.available { "available" } else { "unavailable" }.into(),
    );
    meta.insert(
        "coverage".into(),
        "Static compile-time facts; not a runtime measurement.".into(),
    );

    capture.compile_reports.push(report);
    capture
}
