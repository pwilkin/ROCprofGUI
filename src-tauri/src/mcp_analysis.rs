use serde::{
    ser::{SerializeMap, SerializeSeq},
    Serialize, Serializer,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{self, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        OnceLock,
    },
};

pub fn protocol() -> &'static Value {
    static P: OnceLock<Value> = OnceLock::new();
    P.get_or_init(|| {
        serde_json::from_str(include_str!("../../src/mcpAnalysisProtocol.json")).unwrap()
    })
}
pub fn budget(name: &str) -> u64 {
    protocol()["budgets"][name].as_u64().unwrap()
}
pub fn check(flag: &AtomicBool) -> Result<(), String> {
    if flag.load(Ordering::SeqCst) {
        Err("Cancelled".into())
    } else {
        Ok(())
    }
}
pub struct CappedWriter<'a, W> {
    pub inner: W,
    pub bytes: u64,
    pub max: u64,
    pub flag: &'a AtomicBool,
}
impl<W: Write> Write for CappedWriter<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        check(self.flag).map_err(io::Error::other)?;
        if (bytes.len() as u64) > self.max.saturating_sub(self.bytes) {
            return Err(io::Error::other("encoded byte budget exceeded"));
        }
        for part in bytes.chunks(budget("readBufferBytes") as usize) {
            check(self.flag).map_err(io::Error::other)?;
            self.inner.write_all(part)?;
            self.bytes += part.len() as u64;
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
pub fn encoded_size<T: Serialize + ?Sized>(
    value: &T,
    max: u64,
    flag: &AtomicBool,
) -> Result<u64, String> {
    check(flag)?;
    let mut writer = CappedWriter {
        inner: io::sink(),
        bytes: 0,
        max,
        flag,
    };
    serde_json::to_writer(&mut writer, value).map_err(|e| e.to_string())?;
    Ok(writer.bytes)
}
#[derive(Debug)]
pub struct PageArgs {
    pub query: String,
    pub offset: usize,
    pub limit: usize,
}
impl PageArgs {
    pub fn parse(a: &Value) -> Self {
        Self {
            query: a["query"].as_str().unwrap_or("").to_lowercase(),
            offset: usize::try_from(a["offset"].as_u64().unwrap_or(0)).unwrap_or(usize::MAX),
            limit: a["limit"].as_u64().unwrap_or(100).min(1000) as usize,
        }
    }
    fn response(&self, rows: Vec<Value>, total: usize) -> Value {
        let next = self.offset.checked_add(self.limit).filter(|n| *n < total);
        json!({"rows":rows,"total":total,"offset":self.offset,"nextOffset":next,"effectiveLimit":self.limit})
    }
}
fn row_size<T: Serialize>(
    row: &T,
    dataset: &str,
    index: usize,
    flag: &AtomicBool,
) -> Result<u64, String> {
    encoded_size(row, budget("queryRowBytes"), flag).map_err(|e| {
        if check(flag).is_err() { return "Cancelled".into(); }
        format!("MCP_QUERY_ROW_BUDGET: dataset {dataset}, row {index} exceeds {} bytes ({e}). No rows were dropped. A smaller limit cannot split this row; export the capture with capture_export for offline inspection.", budget("queryRowBytes"))
    })
}
fn page_error() -> String {
    format!("MCP_QUERY_PAGE_BUDGET: selected rows exceed {} bytes. Retry capture_query with a smaller limit and follow nextOffset; no partial page was returned.", budget("queryPageBytes"))
}
pub fn page_slice<T: Serialize>(
    rows: &[T],
    args: &PageArgs,
    flag: &AtomicBool,
    dataset: &str,
) -> Result<Value, String> {
    check(flag)?;
    if !args.query.is_empty() {
        return page_rows(rows, args, flag, dataset);
    }
    let selected =
        &rows[args.offset.min(rows.len())..rows.len().min(args.offset.saturating_add(args.limit))];
    for (i, row) in selected.iter().enumerate() {
        row_size(row, dataset, args.offset + i, flag)?;
    }
    encoded_size(selected, budget("queryPageBytes"), flag).map_err(|_| {
        if check(flag).is_err() {
            "Cancelled".into()
        } else {
            page_error()
        }
    })?;
    let values = selected
        .iter()
        .map(|r| {
            check(flag)?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        })
        .collect::<Result<_, _>>()?;
    Ok(args.response(values, rows.len()))
}
pub fn page_rows<I, T>(
    rows: I,
    args: &PageArgs,
    flag: &AtomicBool,
    dataset: &str,
) -> Result<Value, String>
where
    I: IntoIterator<Item = T>,
    T: Serialize,
{
    page_fallible_rows(rows.into_iter().map(Ok), args, flag, dataset)
}
pub fn page_fallible_rows<I, T>(
    rows: I,
    args: &PageArgs,
    flag: &AtomicBool,
    dataset: &str,
) -> Result<Value, String>
where
    I: IntoIterator<Item = Result<T, String>>,
    T: Serialize,
{
    check(flag)?;
    let mut matches = Vec::new();
    let mut total = 0usize;
    let mut bytes = 2u64;
    for (index, row) in rows.into_iter().enumerate() {
        check(flag)?;
        let row = row?;
        let needed = !args.query.is_empty() || (total >= args.offset && matches.len() < args.limit);
        let value = if needed {
            let size = row_size(&row, dataset, index, flag)?;
            if args.query.is_empty()
                && size + bytes + u64::from(!matches.is_empty()) > budget("queryPageBytes")
            {
                return Err(page_error());
            }
            Some(serde_json::to_value(&row).map_err(|e| e.to_string())?)
        } else {
            None
        };
        if args.query.is_empty()
            || value
                .as_ref()
                .unwrap()
                .to_string()
                .to_lowercase()
                .contains(&args.query)
        {
            if total >= args.offset && matches.len() < args.limit {
                let v = value.unwrap();
                bytes += encoded_size(&v, budget("queryRowBytes"), flag)?
                    + u64::from(!matches.is_empty());
                if bytes > budget("queryPageBytes") {
                    return Err(page_error());
                }
                matches.push(v);
            }
            total = total.checked_add(1).ok_or("Page match count overflow")?;
        }
    }
    Ok(args.response(matches, total))
}
pub fn bounded_value<T: Serialize + ?Sized>(v: &T, flag: &AtomicBool) -> Result<Value, String> {
    encoded_size(v, budget("queryPageBytes"), flag).map_err(|e| format!("MCP_QUERY_PAGE_BUDGET: response exceeds {} bytes ({e}); use capture_export for offline inspection.",budget("queryPageBytes")))?;
    serde_json::to_value(v).map_err(|e| e.to_string())
}

pub struct CompileReportRow<'a>(pub &'a profile_core::loom_report::LoomCompileReport);
pub struct CompileEntryRow<'a>(pub &'a profile_core::loom_report::LoomCompileEntry);
struct Entries<'a>(&'a [profile_core::loom_report::LoomCompileEntry]);
impl Serialize for Entries<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(Some(self.0.len()))?;
        for e in self.0 {
            seq.serialize_element(&CompileEntryRow(e))?;
        }
        seq.end()
    }
}
fn resources(v: Option<&Value>) -> Option<BTreeMap<&'static str, &Value>> {
    let v = v?.as_object()?;
    let mut out = BTreeMap::new();
    for (src, dst) in [
        ("subgroup_size", "subgroupSize"),
        ("max_subgroups_per_simd", "maxSubgroupsPerSimd"),
        ("resident_subgroups_per_simd", "residentSubgroupsPerSimd"),
        ("occupancy_percent", "occupancyPercent"),
        ("limiting_resource", "limitingResource"),
        ("residency", "residency"),
    ] {
        if let Some(v) = v.get(src) {
            out.insert(dst, v);
        }
    }
    Some(out)
}
struct TargetResourceFacts<'a>(&'a Value);
fn registers(v: &Value) -> BTreeMap<&'static str, &Value> {
    let mut out = BTreeMap::new();
    for (key, path) in [
        ("registerClass", vec!["register_class"]),
        ("finalRegisterCount", vec!["final", "register_count"]),
        ("overheadUnits", vec!["final", "overhead_units"]),
        (
            "scheduledPeakLiveUnits",
            vec!["scheduled_pressure", "peak_live_units"],
        ),
    ] {
        let mut value = Some(v);
        for p in path {
            value = value.and_then(Value::as_object).and_then(|o| o.get(p));
        }
        if let Some(value) = value {
            out.insert(key, value);
        }
    }
    out
}
fn has_resources(v: &Value) -> bool {
    resources(Some(v)).is_some_and(|o| !o.is_empty())
        || ["scalar", "vector"]
            .iter()
            .any(|k| !registers(&v[*k]).is_empty())
}
impl Serialize for TargetResourceFacts<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(None)?;
        for (key, src) in [("sgpr", "scalar"), ("vgpr", "vector")] {
            let r = registers(&self.0[src]);
            if !r.is_empty() {
                map.serialize_entry(key, &r)?;
            }
        }
        for (k, v) in resources(Some(self.0)).unwrap_or_default() {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}
struct InstructionMixFacts<'a>(&'a BTreeMap<String, Value>);
impl Serialize for InstructionMixFacts<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(None)?;
        for (src, dst) in [
            ("static_instruction_mix", "static"),
            ("dynamic_instruction_mix", "dynamic"),
        ] {
            if let Some(v) = self.0.get(src) {
                m.serialize_entry(dst, v)?;
            }
        }
        m.end()
    }
}
fn sections<M: SerializeMap>(
    m: &mut M,
    sections: &BTreeMap<String, Value>,
) -> Result<(), M::Error> {
    if let Some(v) = sections
        .get("target_resources")
        .filter(|v| has_resources(v))
    {
        m.serialize_entry("targetResources", &TargetResourceFacts(v))?;
    }
    if sections.contains_key("static_instruction_mix")
        || sections.contains_key("dynamic_instruction_mix")
    {
        m.serialize_entry("instructionMix", &InstructionMixFacts(sections))?;
    }
    for (src, dst) in [("wait_plan", "waitPlan"), ("workload", "workload")] {
        if let Some(v) = sections.get(src) {
            m.serialize_entry(dst, v)?;
        }
    }
    Ok(())
}
impl Serialize for CompileReportRow<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let r = self.0;
        let mut m = s.serialize_map(None)?;
        m.serialize_entry("stability", &r.stability)?;
        m.serialize_entry("unit", "static")?;
        m.serialize_entry("facts","Compile-time static facts from the Loom compiler; not measurements, dispatches or runtime counts. Absent fields are unavailable, not zero. Version-zero, same-compiler-horizon: not comparable across targets or compiler horizons.")?;
        m.serialize_entry("containerKind", &r.container_kind)?;
        m.serialize_entry("mode", &r.mode)?;
        m.serialize_entry("schemaVersion", &r.schema_version)?;
        #[derive(Serialize)]
        struct Status<'a> {
            code: i64,
            name: &'a str,
        }
        m.serialize_entry(
            "status",
            &Status {
                code: r.status_code,
                name: &r.status_name,
            },
        )?;
        if !r.identity.is_empty() {
            m.serialize_entry("identity", &r.identity)?;
        }
        if let Some(v) = r.detail_flags {
            m.serialize_entry("detailFlags", &v)?;
        }
        if !r.detail_flag_names.is_empty() {
            m.serialize_entry("detailFlagNames", &r.detail_flag_names)?;
        }
        if !r.envelope_context.is_empty() {
            m.serialize_entry("envelopeContext", &r.envelope_context)?;
        }
        if let Some(v) = r.artifact_size {
            m.serialize_entry("artifactSize", &v)?;
        }
        if !r.config_bindings.is_empty() {
            m.serialize_entry("configBindings", &r.config_bindings)?;
        }
        m.serialize_entry("provenance", &r.provenance)?;
        sections(&mut m, &r.sections)?;
        if !r.residency_constraints.is_empty() {
            m.serialize_entry("residencyConstraints", &r.residency_constraints)?;
        }
        m.serialize_entry("entries", &Entries(&r.entries))?;
        m.end()
    }
}
impl Serialize for CompileEntryRow<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let e = self.0;
        let mut m = s.serialize_map(None)?;
        m.serialize_entry("index", &e.index)?;
        m.serialize_entry("displayName", &e.display_name)?;
        if !e.identity.is_empty() {
            m.serialize_entry("identity", &e.identity)?;
        }
        if let Some(v) = e.detail_flags {
            m.serialize_entry("detailFlags", &v)?;
        }
        if !e.detail_flag_names.is_empty() {
            m.serialize_entry("detailFlagNames", &e.detail_flag_names)?;
        }
        if !e.counters.is_empty() {
            m.serialize_entry("counters", &e.counters)?;
        }
        sections(&mut m, &e.sections)?;
        m.end()
    }
}

pub struct Summary<'a>(pub &'a profile_core::Capture);
impl Serialize for Summary<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let c = self.0;
        let mut m = s.serialize_map(None)?;
        m.serialize_entry("id", &c.id)?;
        m.serialize_entry("name", &c.name)?;
        m.serialize_entry("path", &c.input_path)?;
        m.serialize_entry("format", &c.format)?;
        m.serialize_entry("architecture", &c.architecture)?;
        m.serialize_entry("unit", &c.unit)?;
        m.serialize_entry("start", &c.start)?;
        m.serialize_entry("end", &c.end)?;
        m.serialize_entry("warnings", &c.warnings)?;
        for (k, n) in [
            ("dispatches", c.dispatches.len()),
            ("instructions", c.instructions.len()),
            ("waves", c.waves.len()),
            ("counters", c.counters.len()),
            ("compileReports", c.compile_reports.len()),
        ] {
            m.serialize_entry(k, &n)?;
        }
        m.end()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    #[test]
    fn mcp_typed_page_visits_only_selected_rows_and_cancels_empty() {
        struct Row<'a>(usize, &'a RefCell<Vec<usize>>);
        impl Serialize for Row<'_> {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                assert!([50, 51].contains(&self.0), "unselected serializer called");
                self.1.borrow_mut().push(self.0);
                self.0.serialize(s)
            }
        }
        let visits = RefCell::new(Vec::new());
        let rows: Vec<_> = (0..100001).map(|i| Row(i, &visits)).collect();
        let flag = AtomicBool::new(false);
        let v = page_slice(
            &rows,
            &PageArgs::parse(&json!({"offset":50,"limit":2})),
            &flag,
            "test",
        )
        .unwrap();
        assert_eq!(
            v,
            json!({"rows":[50,51],"total":100001,"offset":50,"nextOffset":52,"effectiveLimit":2})
        );
        assert_eq!(
            visits
                .into_inner()
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>(),
            [50, 51].into()
        );
        flag.store(true, Ordering::SeqCst);
        assert!(page_slice::<u8>(&[], &PageArgs::parse(&json!({})), &flag, "test").is_err());
    }
    #[test]
    fn mcp_row_and_page_budgets_precede_value_allocation() {
        struct Large;
        impl Serialize for Large {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                assert!(
                    !std::any::type_name::<S>().contains("value::ser"),
                    "allocated before admission"
                );
                let mut seq = s.serialize_seq(Some(5 * 1024))?;
                for _ in 0..5 * 1024 {
                    seq.serialize_element(&"x".repeat(1024))?;
                }
                seq.end()
            }
        }
        let flag = AtomicBool::new(false);
        for query in ["", "nonmatching"] {
            assert!(page_slice(
                &[Large],
                &PageArgs::parse(&json!({"query":query})),
                &flag,
                "test"
            )
            .unwrap_err()
            .starts_with("MCP_QUERY_ROW_BUDGET"));
        }
        let rows = vec!["x".repeat(3 * 1024 * 1024); 6];
        assert!(
            page_slice(&rows, &PageArgs::parse(&json!({})), &flag, "test")
                .unwrap_err()
                .starts_with("MCP_QUERY_PAGE_BUDGET")
        );
        assert!(page_slice(
            &rows,
            &PageArgs::parse(&json!({"query":"x"})),
            &flag,
            "test"
        )
        .unwrap_err()
        .starts_with("MCP_QUERY_PAGE_BUDGET"));
    }
    #[test]
    fn mcp_checked_offset_zero_and_clamp() {
        let flag = AtomicBool::new(false);
        let rows: Vec<_> = (0..1001).collect();
        let first = page_slice(
            &rows,
            &PageArgs::parse(&json!({"limit":2000})),
            &flag,
            "test",
        )
        .unwrap();
        let last = page_slice(
            &rows,
            &PageArgs::parse(&json!({"limit":2000,"offset":1000})),
            &flag,
            "test",
        )
        .unwrap();
        assert_eq!(first["effectiveLimit"], 1000);
        assert_eq!(first["nextOffset"], 1000);
        assert_eq!(last["rows"], json!([1000]));
        assert!(last["nextOffset"].is_null());
        let zero = page_slice(&rows, &PageArgs::parse(&json!({"limit":0})), &flag, "test").unwrap();
        assert_eq!(zero["rows"], json!([]));
        assert_eq!(zero["nextOffset"], 0);
        let beyond = page_slice(
            &rows,
            &PageArgs::parse(&json!({"offset":u64::MAX})),
            &flag,
            "test",
        )
        .unwrap();
        assert_eq!(beyond["total"], 1001);
        assert!(beyond["nextOffset"].is_null());
    }
}

use profile_core::{Capture, Dispatch};
use std::{
    fs,
    io::BufWriter,
    path::{Path, PathBuf},
};

pub enum AnalysisPlan<'a> {
    Kernels(&'a Capture),
    Timeline(&'a Capture),
    Projected(&'a Capture, &'a str),
    DiagnosticsCompatibility(&'a Capture),
}
impl<'a> AnalysisPlan<'a> {
    fn select(c: &'a Capture, a: &'a Value) -> Result<Self, String> {
        match a["analysis"].as_str().ok_or("Missing analysis")? {
            "kernels" => Ok(Self::Kernels(c)),
            "timeline" => Ok(Self::Timeline(c)),
            "diagnostics" => Ok(Self::DiagnosticsCompatibility(c)),
            k if protocol()["analyses"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == k) =>
            {
                Ok(Self::Projected(c, k))
            }
            _ => Err("Unknown analysis".into()),
        }
    }
}
#[derive(Serialize)]
pub struct Identity<'a> {
    id: &'a str,
    name: &'a str,
    format: &'a str,
    architecture: &'a str,
    unit: &'a str,
    origin: &'a str,
    start: f64,
    end: f64,
}
impl<'a> Identity<'a> {
    fn new(c: &'a Capture) -> Self {
        Self {
            id: &c.id,
            name: &c.name,
            format: &c.format,
            architecture: &c.architecture,
            unit: &c.unit,
            origin: &c.origin,
            start: c.start,
            end: c.end,
        }
    }
    fn validate(&self) -> Result<(), String> {
        finite(&[self.start, self.end])?;
        for v in [
            self.id,
            self.name,
            self.format,
            self.architecture,
            self.unit,
            self.origin,
        ] {
            string(v)?;
        }
        Ok(())
    }
}
#[derive(Serialize)]
struct ResponseContext<'a> {
    capture: Identity<'a>,
    warnings: &'a [String],
}
#[derive(Debug, Clone, Serialize)]
pub struct Stream {
    pub kind: String,
    pub file: String,
    pub rows: u64,
    pub bytes: u64,
}
#[derive(Serialize)]
struct PayloadDescriptor {
    kind: &'static str,
    file: &'static str,
    bytes: u64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Manifest<'a> {
    protocol: &'static str,
    version: u64,
    analysis: &'a str,
    mode: &'a str,
    capture: Identity<'a>,
    args: &'a Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    streams: Option<&'a [Stream]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<PayloadDescriptor>,
    budget_version: u64,
}
pub struct AnalysisJob {
    pub folder: PathBuf,
    pub mode: String,
    pub analysis: String,
    pub args: Value,
    pub streams: Vec<Stream>,
    prepared: bool,
}
impl Drop for AnalysisJob {
    fn drop(&mut self) {
        if !self.prepared {
            let _ = fs::remove_file(self.folder.join("input.json"));
        }
        for name in [
            "payload.json",
            "dispatches.ndjson",
            "intervals.ndjson",
            "timeline.ndjson",
            "result.ndjson",
            "result.html",
        ] {
            let _ = fs::remove_file(self.folder.join(name));
        }
    }
}
fn finite(v: &[f64]) -> Result<(), String> {
    if v.iter().all(|n| n.is_finite()) {
        Ok(())
    } else {
        Err("MCP_ANALYSIS_RECORD_BUDGET: nonfinite analysis coordinates; use capture_export for offline inspection".into())
    }
}
fn string(v: &str) -> Result<(), String> {
    if v.len() as u64 <= budget("stringBytes") {
        Ok(())
    } else {
        Err(format!("MCP_ANALYSIS_RECORD_BUDGET: timeline input string exceeds {} UTF-8 bytes; export the capture for offline inspection",budget("stringBytes")))
    }
}
pub fn overlaps(start: f64, end: f64, range: [f64; 2]) -> bool {
    if start == end {
        range[0] <= start && start < range[1]
    } else {
        start < range[1] && end > range[0]
    }
}
pub fn validate_analysis(c: &Capture, a: &Value, flag: &AtomicBool) -> Result<Value, String> {
    check(flag)?;
    AnalysisPlan::select(c, a)?;
    if let Some(output) = a["output"].as_str() {
        let path = Path::new(output);
        if fs::symlink_metadata(path).is_ok() {
            return Err("Output already exists; choose a new path".into());
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        if !parent.is_dir() {
            return Err("Output directory does not exist".into());
        }
    }
    let mut args = serde_json::Map::new();
    if a["analysis"] == "timeline" {
        let range = if a.get("range").is_some() {
            a["range"]
                .as_array()
                .ok_or("Choose a finite positive range")?
                .iter()
                .map(|v| v.as_f64().ok_or("Choose a finite positive range"))
                .collect::<Result<Vec<_>, _>>()?
        } else {
            vec![c.start, c.end]
        };
        if range.len() != 2
            || !range.iter().all(|v| v.is_finite())
            || range[1] <= range[0]
            || !(range[1] - range[0]).is_finite()
        {
            return Err("Choose a finite positive range".into());
        }
        let count = |key: &str, default: u64, max: u64| -> Result<u64, String> {
            let n = match a.get(key) {
                None => default,
                Some(v) => v.as_u64().ok_or_else(|| format!("Invalid {key}"))?,
            };
            let n = if n == 0 { default } else { n };
            if n > max {
                Err(format!("Choose 1–{max} {key}"))
            } else {
                Ok(n)
            }
        };
        let file = a["output"].is_string();
        let max = budget(if file { "fileBytes" } else { "inlineBytes" });
        let bytes = match a.get("maxOutputBytes") {
            None => max,
            Some(v) => v
                .as_u64()
                .ok_or("maxOutputBytes must be a positive integer")?,
        };
        if bytes == 0 || bytes > max {
            return Err(format!("MCP_TIMELINE_BUDGET: maxOutputBytes must be 1..{max}; set output to a new HTML path for the file-output budget. limit does not limit timeline events."));
        }
        let timeline = a["timeline"].as_str().unwrap_or("dispatch");
        if !["dispatch", "global"].contains(&timeline) {
            return Err("Unknown timeline".into());
        }
        args.insert("timeline".into(), json!(timeline));
        args.insert("range".into(), json!(range));
        args.insert(
            "timePages".into(),
            json!(count("timePages", 1, budget("timePages"))?),
        );
        args.insert(
            "lanesPerPage".into(),
            json!(count("lanesPerPage", 12, budget("lanesPerPage"))?),
        );
        args.insert(
            "outputMode".into(),
            json!(if file { "file" } else { "inline" }),
        );
        args.insert("maxOutputBytes".into(), json!(bytes));
        args.insert(
            "maxGlyphs".into(),
            json!(budget(if file { "fileGlyphs" } else { "inlineGlyphs" })),
        );
    } else if a.get("maxOutputBytes").is_some() {
        return Err("maxOutputBytes is valid only for timeline".into());
    }
    if a["analysis"] == "kernels" {
        let page = PageArgs::parse(a);
        args.insert("query".into(), json!(page.query));
        args.insert("offset".into(), json!(page.offset));
        args.insert("limit".into(), json!(page.limit));
    }
    let context = if a["analysis"] == "kernels" {
        encoded_size(&KernelMeta(c), budget("contextBytes"), flag)
    } else if a["analysis"] == "timeline" {
        encoded_size(
            &TimelineContext {
                c,
                args: &Value::Object(args.clone()),
            },
            budget("contextBytes"),
            flag,
        )
    } else {
        encoded_size(
            &ResponseContext {
                capture: Identity::new(c),
                warnings: &c.warnings,
            },
            budget("contextBytes"),
            flag,
        )
    };
    context.map_err(|e|format!("MCP_ANALYSIS_CONTEXT_BUDGET: complete response identity/clock/warnings exceed {} bytes ({e}); use capture_export for offline inspection",budget("contextBytes")))?;
    Ok(Value::Object(args))
}
#[derive(Serialize)]
struct KernelSample<'a> {
    kernel: &'a str,
    start: f64,
    end: f64,
}
#[derive(Serialize)]
struct DispatchRecord<'a> {
    id: &'a str,
    kernel: &'a str,
    start: f64,
    end: f64,
    agent: &'a str,
    queue: &'a str,
    grid: &'a str,
    workgroup: &'a str,
}
impl<'a> From<&'a Dispatch> for DispatchRecord<'a> {
    fn from(d: &'a Dispatch) -> Self {
        Self {
            id: &d.id,
            kernel: &d.kernel,
            start: d.start,
            end: d.end,
            agent: &d.agent,
            queue: &d.queue,
            grid: &d.grid,
            workgroup: &d.workgroup,
        }
    }
}
#[derive(Serialize)]
struct TimelineRecord<T: Serialize> {
    kind: &'static str,
    index: usize,
    record: T,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MarkerRecord<'a> {
    start: f64,
    end: Option<f64>,
    point: bool,
    se: u32,
    cu: u32,
    simd: u32,
    slot: u32,
    depth: usize,
    name: &'a str,
    marker_id: u32,
    code_object: &'a str,
    source: &'a str,
}
fn record_write<T: Serialize, W: Write>(
    record: &T,
    writer: &mut CappedWriter<'_, W>,
    index: usize,
    dataset: &str,
) -> Result<u64, String> {
    check(writer.flag)?;
    let bytes=encoded_size(record,budget("recordBytes")-1,writer.flag).map_err(|e|format!("MCP_ANALYSIS_RECORD_BUDGET: {dataset} record {index} exceeds {} bytes ({e}); use positive-limit capture_query and nextOffset for offline inspection",budget("recordBytes")))?+1;
    if bytes > writer.max.saturating_sub(writer.bytes) {
        return Err(format!("MCP_ANALYSIS_SPOOL_BUDGET: {dataset} exceeds {} total bytes; use offline paged queries or import a smaller capture",budget("spoolBytes")));
    }
    serde_json::to_writer(&mut *writer, record).map_err(|e| e.to_string())?;
    writer.write_all(b"\n").map_err(|e| e.to_string())?;
    Ok(bytes)
}
fn kernel_stream<W: Write>(
    c: &Capture,
    intervals: bool,
    writer: &mut CappedWriter<'_, W>,
) -> Result<u64, String> {
    if intervals && c.intervals.len() as u64 > budget("kernelIntervals") {
        return Err(format!("MCP_KERNEL_STATE_BUDGET: conservative transport preflight counts {} residency intervals, exceeding {} even if unmatched. limit/query do not reduce aggregate input. Use positive-limit capture_query on dispatches/intervals and nextOffset for offline exact aggregation; union intervals globally, never sum per-part unions. No partial totals were returned.",c.intervals.len(),budget("kernelIntervals")));
    }
    if intervals {
        for (index, i) in c.intervals.iter().enumerate() {
            finite(&[i.start, i.end])
                .map_err(|e| format!("{e}; dataset intervals, record {index}"))?;
            record_write(
                &KernelSample {
                    kernel: &i.kernel,
                    start: i.start,
                    end: i.end,
                },
                writer,
                index,
                "intervals",
            )?;
        }
        Ok(c.intervals.len() as u64)
    } else {
        for (index, d) in c.dispatches.iter().enumerate() {
            finite(&[d.start, d.end])
                .map_err(|e| format!("{e}; dataset dispatches, record {index}"))?;
            record_write(
                &KernelSample {
                    kernel: &d.kernel,
                    start: d.start,
                    end: d.end,
                },
                writer,
                index,
                "dispatches",
            )?;
        }
        Ok(c.dispatches.len() as u64)
    }
}
fn timeline_stream<W: Write>(
    c: &Capture,
    args: &Value,
    writer: &mut CappedWriter<'_, W>,
) -> Result<u64, String> {
    let range = [
        args["range"][0].as_f64().unwrap(),
        args["range"][1].as_f64().unwrap(),
    ];
    let mut rows = 0;
    if args["timeline"] == "global" {
        for (index, i) in c.intervals.iter().enumerate() {
            check(writer.flag)?;
            finite(&[i.start, i.end])
                .map_err(|e| format!("{e}; dataset intervals, record {index}"))?;
            if overlaps(i.start, i.end, range) {
                string(&i.kernel)
                    .map_err(|e| format!("{e}; dataset intervals, record {index}, field kernel"))?;
                record_write(
                    &TimelineRecord {
                        kind: "residency",
                        index,
                        record: i,
                    },
                    writer,
                    index,
                    "intervals",
                )?;
                rows += 1;
            }
        }
        for (index, m) in c.markers.iter().enumerate() {
            check(writer.flag)?;
            let end = if m.point {
                m.start
            } else {
                m.end.unwrap_or(c.end)
            };
            finite(&[m.start, end]).map_err(|e| format!("{e}; dataset markers, record {index}"))?;
            if let Some(end) = m.end {
                finite(&[end])
                    .map_err(|e| format!("{e}; dataset markers, record {index}, field end"))?;
            }
            if overlaps(m.start, end, range) {
                for (field, v) in [
                    ("name", &m.name),
                    ("codeObject", &m.code_object),
                    ("source", &m.source),
                ] {
                    string(v).map_err(|e| {
                        format!("{e}; dataset markers, record {index}, field {field}")
                    })?;
                }
                let record = MarkerRecord {
                    start: m.start,
                    end: m.end,
                    point: m.point,
                    se: m.se,
                    cu: m.cu,
                    simd: m.simd,
                    slot: m.slot,
                    depth: m.depth,
                    name: &m.name,
                    marker_id: m.marker_id,
                    code_object: &m.code_object,
                    source: &m.source,
                };
                record_write(
                    &TimelineRecord {
                        kind: "marker",
                        index,
                        record,
                    },
                    writer,
                    index,
                    "markers",
                )?;
                rows += 1;
            }
        }
    }
    for (index, d) in c.dispatches.iter().enumerate() {
        check(writer.flag)?;
        finite(&[d.start, d.end])
            .map_err(|e| format!("{e}; dataset dispatches, record {index}"))?;
        if d.end > d.start && overlaps(d.start, d.end, range) {
            for (field, v) in [
                ("id", &d.id),
                ("kernel", &d.kernel),
                ("agent", &d.agent),
                ("queue", &d.queue),
                ("grid", &d.grid),
                ("workgroup", &d.workgroup),
            ] {
                string(v).map_err(|e| {
                    format!("{e}; dataset dispatches, record {index}, field {field}")
                })?;
            }
            record_write(
                &TimelineRecord {
                    kind: "dispatch",
                    index,
                    record: DispatchRecord::from(d),
                },
                writer,
                index,
                "dispatches",
            )?;
            rows += 1;
        }
    }
    if args["timeline"] == "global" {
        for (index, e) in c.decoder_events.iter().enumerate() {
            check(writer.flag)?;
            finite(&[e.time])
                .map_err(|e| format!("{e}; dataset events, record {index}, field time"))?;
            if overlaps(e.time, e.time, range) {
                for (field, v) in [("id", &e.id), ("kind", &e.kind)] {
                    string(v).map_err(|e| {
                        format!("{e}; dataset events, record {index}, field {field}")
                    })?;
                }
                for (k, v) in &e.fields {
                    string(k).map_err(|e| {
                        format!("{e}; dataset events, record {index}, field-map key")
                    })?;
                    string(v)
                        .map_err(|e| format!("{e}; dataset events, record {index}, field {k}"))?;
                }
                record_write(
                    &TimelineRecord {
                        kind: "event",
                        index,
                        record: e,
                    },
                    writer,
                    index,
                    "events",
                )?;
                rows += 1;
            }
        }
    }
    Ok(rows)
}
struct Projection<'a> {
    c: &'a Capture,
    kind: &'a str,
}
impl Serialize for Projection<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let c = self.c;
        let mut m = s.serialize_map(None)?;
        match self.kind {
            "occupancy" => m.serialize_entry("intervals", &c.intervals)?,
            "activity" => {
                #[derive(Serialize)]
                struct Activity {
                    idle: f64,
                    stall: f64,
                    latency: f64,
                }
                struct Instructions<'a>(&'a [profile_core::Instruction]);
                impl Serialize for Instructions<'_> {
                    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                        let mut seq = s.serialize_seq(Some(self.0.len()))?;
                        for i in self.0 {
                            seq.serialize_element(&Activity {
                                idle: i.idle,
                                stall: i.stall,
                                latency: i.latency,
                            })?;
                        }
                        seq.end()
                    }
                }
                m.serialize_entry("instructions", &Instructions(&c.instructions))?;
            }
            "hardware" => {
                m.serialize_entry("architecture", &c.architecture)?;
                m.serialize_entry("counters", &c.counters)?;
            }
            "hotspots" | "source" => m.serialize_entry("instructions", &c.instructions)?,
            "markers" => {
                m.serialize_entry("markers", &c.markers)?;
                m.serialize_entry("start", &c.start)?;
                m.serialize_entry("end", &c.end)?;
            }
            "scalar" => m.serialize_entry("counters", &c.counters)?,
            "tensor" => {}
            _ => return Err(serde::ser::Error::custom("Unknown projection")),
        }
        m.end()
    }
}
struct ApplicableArgs<'a> {
    args: &'a Value,
    kind: &'a str,
}
impl Serialize for ApplicableArgs<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(None)?;
        m.serialize_entry("analysis", self.kind)?;
        let keys: &[&str] = match self.kind {
            "occupancy" => &["se", "kernel"],
            "hotspots" | "source" => &["metric"],
            "markers" => &["range"],
            "scalar" => &["name", "formula"],
            "tensor" => &["raw", "definitions"],
            "diagnostics" => &["workspace"],
            _ => &[],
        };
        for k in keys {
            if let Some(v) = self.args.get(*k) {
                m.serialize_entry(k, v)?;
            }
        }
        m.end()
    }
}
#[derive(Serialize)]
struct Payload<C: Serialize, A: Serialize> {
    capture: C,
    args: A,
}
fn payload_write<W: Write>(c: &Capture, a: &Value, kind: &str, w: &mut W) -> Result<(), String> {
    let args = ApplicableArgs { args: a, kind };
    if kind == "diagnostics" {
        serde_json::to_writer(w, &Payload { capture: c, args })
    } else {
        serde_json::to_writer(
            w,
            &Payload {
                capture: Projection { c, kind },
                args,
            },
        )
    }
    .map_err(|e| e.to_string())
}
pub fn prepare_analysis(
    c: &Capture,
    a: &Value,
    flag: &AtomicBool,
    folder: &Path,
) -> Result<AnalysisJob, String> {
    let args = validate_analysis(c, a, flag)?;
    let plan = AnalysisPlan::select(c, a)?;
    let identity = Identity::new(c);
    identity.validate()?;
    let (mode, analysis) = match &plan {
        AnalysisPlan::Kernels(_) => ("kernel-stream", "kernels"),
        AnalysisPlan::Timeline(_) => ("timeline-stream", "timeline"),
        AnalysisPlan::Projected(_, k) => ("projected-json", *k),
        AnalysisPlan::DiagnosticsCompatibility(_) => ("diagnostics-compatibility", "diagnostics"),
    };
    let mut job = AnalysisJob {
        folder: folder.into(),
        mode: mode.into(),
        analysis: analysis.into(),
        args,
        streams: vec![],
        prepared: false,
    };
    let mut descriptors = vec![];
    match plan {
        AnalysisPlan::Kernels(c) | AnalysisPlan::Timeline(c) => {
            let files: &[(&str, &str)] = if analysis == "kernels" {
                &[
                    ("kernel-dispatch", "dispatches.ndjson"),
                    ("kernel-interval", "intervals.ndjson"),
                ]
            } else {
                &[("timeline-record", "timeline.ndjson")]
            };
            let mut total = 0;
            for (kind, file) in files {
                let mut w = CappedWriter {
                    inner: io::sink(),
                    bytes: 0,
                    max: budget("spoolBytes") - total,
                    flag,
                };
                let rows = if analysis == "kernels" {
                    kernel_stream(c, *kind == "kernel-interval", &mut w)?
                } else {
                    timeline_stream(c, &job.args, &mut w)?
                };
                total += w.bytes;
                descriptors.push(Stream {
                    kind: (*kind).into(),
                    file: (*file).into(),
                    rows,
                    bytes: w.bytes,
                });
            }
            total = 0;
            for d in &descriptors {
                let f = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(folder.join(&d.file))
                    .map_err(|e| e.to_string())?;
                let mut w = CappedWriter {
                    inner: BufWriter::new(f),
                    bytes: 0,
                    max: budget("spoolBytes") - total,
                    flag,
                };
                let rows = if analysis == "kernels" {
                    kernel_stream(c, d.kind == "kernel-interval", &mut w)?
                } else {
                    timeline_stream(c, &job.args, &mut w)?
                };
                w.flush().map_err(|e| e.to_string())?;
                if w.bytes != d.bytes || rows != d.rows {
                    return Err("Analysis spool accounting mismatch".into());
                }
                total += w.bytes;
            }
        }
        AnalysisPlan::Projected(c, _) | AnalysisPlan::DiagnosticsCompatibility(c) => {
            let mut count = CappedWriter {
                inner: io::sink(),
                bytes: 0,
                max: budget("projectedBytes"),
                flag,
            };
            payload_write(c,a,analysis,&mut count).map_err(|e|format!("MCP_ANALYSIS_INPUT_BUDGET: {analysis} projected input exceeds {} bytes ({e}); this analysis still requires its complete relevant dataset. Use positive-limit capture_query and nextOffset for offline analysis, or capture_export for diagnostics. Changing ROCPROF_MAX_FILE_MIB does not change this budget.",budget("projectedBytes")))?;
            let f = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(folder.join("payload.json"))
                .map_err(|e| e.to_string())?;
            let mut w = CappedWriter {
                inner: BufWriter::new(f),
                bytes: 0,
                max: budget("projectedBytes"),
                flag,
            };
            payload_write(c, a, analysis, &mut w)?;
            w.flush().map_err(|e| e.to_string())?;
            descriptors.push(Stream {
                kind: "payload".into(),
                file: "payload.json".into(),
                rows: 0,
                bytes: w.bytes,
            });
        }
    }
    let manifest = Manifest {
        protocol: "rocprof-studio-analysis",
        version: protocol()["version"].as_u64().unwrap(),
        analysis,
        mode,
        capture: identity,
        args: &job.args,
        streams: if analysis == "kernels" || analysis == "timeline" {
            Some(&descriptors)
        } else {
            None
        },
        payload: if analysis == "kernels" || analysis == "timeline" {
            None
        } else {
            Some(PayloadDescriptor {
                kind: "payload",
                file: "payload.json",
                bytes: descriptors[0].bytes,
            })
        },
        budget_version: protocol()["budgetVersion"].as_u64().unwrap(),
    };
    encoded_size(&manifest, budget("manifestBytes"), flag).map_err(|e|format!("MCP_ANALYSIS_RECORD_BUDGET: input.json manifest exceeds {} bytes ({e}); use a smaller query or capture_export for offline inspection",budget("manifestBytes")))?;
    let f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(folder.join("input.json"))
        .map_err(|e| e.to_string())?;
    let mut w = CappedWriter {
        inner: BufWriter::new(f),
        bytes: 0,
        max: budget("manifestBytes"),
        flag,
    };
    serde_json::to_writer(&mut w, &manifest).map_err(|e| e.to_string())?;
    w.flush().map_err(|e| e.to_string())?;
    job.streams = descriptors;
    job.prepared = true;
    Ok(job)
}

#[cfg(test)]
mod transport_tests {
    use super::*;
    pub fn capture() -> Capture {
        let mut c = Capture::empty("synthetic.csv".into(), "rocprofv3 CSV", "ns");
        c.dispatches.push(Dispatch {
            id: "d".into(),
            kernel: "a".into(),
            start: 1.,
            end: 2.,
            agent: "0".into(),
            queue: "1".into(),
            grid: "64".into(),
            workgroup: "32".into(),
            shader_engine: None,
            vgpr: None,
            sgpr: None,
            lds: None,
            scratch: None,
        });
        c.end = 100.;
        c
    }
    #[test]
    fn mcp_actual_bundle_projected_handshake_and_wrong_version() {
        let dir = tempfile::tempdir().unwrap();
        let c = capture();
        let flag = AtomicBool::new(false);
        let job = prepare_analysis(&c, &json!({"analysis":"activity"}), &flag, dir.path()).unwrap();
        fs::write(
            dir.path().join("analysis.cjs"),
            include_str!(concat!(env!("OUT_DIR"), "/mcp-analysis.cjs")),
        )
        .unwrap();
        let run = || {
            std::process::Command::new("node")
                .arg(dir.path().join("analysis.cjs"))
                .arg(dir.path().join("input.json"))
                .output()
                .unwrap()
        };
        let output = run();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"null");
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(dir.path().join("input.json")).unwrap()).unwrap();
        manifest["version"] = json!(99);
        fs::write(dir.path().join("input.json"), manifest.to_string()).unwrap();
        let output = run();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr)
            .contains("rebuild the rocprof-studio binary and bundled analysis worker together"));
        drop(job);
        assert!(!dir.path().join("payload.json").exists());
    }
    #[test]
    fn mcp_irrelevant_257_mib_metadata_does_not_enter_optimized_spools() {
        let mut c = capture();
        let flag = AtomicBool::new(false);
        for analysis in ["kernels", "timeline"] {
            let dir = tempfile::tempdir().unwrap();
            let a = json!({"analysis":analysis,"range":[0,3]});
            let small = prepare_analysis(&c, &a, &flag, dir.path()).unwrap();
            let descriptors = serde_json::to_value(&small.streams).unwrap();
            let manifest = fs::read(dir.path().join("input.json")).unwrap();
            drop(small);
            fs::remove_file(dir.path().join("input.json")).unwrap();
            c.metadata.insert(
                "irrelevant".into(),
                "sentinel".repeat(257 * 1024 * 1024 / 8 + 1),
            );
            let large = prepare_analysis(&c, &a, &flag, dir.path()).unwrap();
            assert_eq!(serde_json::to_value(&large.streams).unwrap(), descriptors);
            assert_eq!(fs::read(dir.path().join("input.json")).unwrap(), manifest);
            for d in &large.streams {
                assert_eq!(
                    fs::metadata(dir.path().join(&d.file)).unwrap().len(),
                    d.bytes
                );
                assert!(!fs::read_to_string(dir.path().join(&d.file))
                    .unwrap()
                    .contains("sentinel"));
            }
            drop(large);
            c.metadata.clear();
        }
    }
    #[test]
    fn mcp_narrow_range_retains_original_ordinals_and_half_open_points() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = capture();
        let flag = AtomicBool::new(false);
        c.dispatches = (0..527878)
            .map(|index| Dispatch {
                id: index.to_string(),
                start: index as f64,
                end: index as f64 + 0.5,
                ..c.dispatches[0].clone()
            })
            .collect();
        for time in [100., 105., 110.] {
            c.decoder_events.push(profile_core::DecoderEvent {
                id: time.to_string(),
                time,
                se: 0,
                kind: "dispatch".into(),
                fields: BTreeMap::new(),
            });
        }
        let job = prepare_analysis(
            &c,
            &json!({"analysis":"timeline","timeline":"global","range":[100,110]}),
            &flag,
            dir.path(),
        )
        .unwrap();
        assert_eq!(job.streams[0].rows, 12);
        let lines = fs::read_to_string(dir.path().join("timeline.ndjson")).unwrap();
        let rows: Vec<Value> = lines
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(rows[0]["index"], 100);
        assert_eq!(rows[9]["index"], 109);
        assert_eq!(rows[10]["record"]["time"], 100.0);
        assert_eq!(rows[11]["record"]["time"], 105.0);
        assert!(overlaps(105., 105., [105., 110.]));
        assert!(!overlaps(105., 105., [100., 105.]));
        fs::write(
            dir.path().join("analysis.cjs"),
            include_str!(concat!(env!("OUT_DIR"), "/mcp-analysis.cjs")),
        )
        .unwrap();
        let output = std::process::Command::new("node")
            .arg("--max-old-space-size=256")
            .arg(dir.path().join("analysis.cjs"))
            .arg(dir.path().join("input.json"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        fs::write(dir.path().join("stdout.log"), output.stdout).unwrap();
        let result = timeline_result(&c, &json!({}), &job, &flag).unwrap();
        assert_eq!(result["export"]["glyphs"], 12);
        let html = result["html"].as_str().unwrap();
        for index in 100..110 {
            assert!(html.contains(&format!("data-key=\"dispatch:{index}\"")));
        }
        assert!(!html.contains("data-key=\"dispatch:99\""));
        assert!(!html.contains("data-key=\"event:2\""));
    }
}

use std::io::{BufRead, BufReader, Read};
pub struct NdjsonRows<'a> {
    reader: BufReader<fs::File>,
    flag: &'a AtomicBool,
    rows: u64,
    bytes: u64,
    expected_rows: u64,
    expected_bytes: u64,
    done: bool,
}
impl<'a> NdjsonRows<'a> {
    pub fn new(path: &Path, rows: u64, bytes: u64, flag: &'a AtomicBool) -> Result<Self, String> {
        let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        if !meta.is_file() || meta.len() != bytes || bytes > budget("kernelResultBytes") {
            return Err("Invalid kernel result size/type".into());
        }
        Ok(Self {
            reader: BufReader::with_capacity(
                budget("readBufferBytes") as usize,
                fs::File::open(path).map_err(|e| e.to_string())?,
            ),
            flag,
            rows: 0,
            bytes: 0,
            expected_rows: rows,
            expected_bytes: bytes,
            done: false,
        })
    }
    fn read_row(&mut self) -> Result<Option<Value>, String> {
        check(self.flag)?;
        let mut line = Vec::new();
        loop {
            check(self.flag)?;
            let buffer = self.reader.fill_buf().map_err(|e| e.to_string())?;
            if buffer.is_empty() {
                if !line.is_empty() {
                    return Err("Truncated kernel result record".into());
                }
                if self.rows != self.expected_rows || self.bytes != self.expected_bytes {
                    return Err("Kernel result count/byte mismatch".into());
                }
                return Ok(None);
            }
            let len = buffer
                .iter()
                .position(|b| *b == b'\n')
                .map_or(buffer.len(), |i| i + 1);
            let newline = buffer[len - 1] == b'\n';
            if line.len() + len > budget("recordBytes") as usize {
                return Err("MCP_ANALYSIS_RECORD_BUDGET: kernel result row too large".into());
            }
            line.extend_from_slice(&buffer[..len]);
            self.bytes += len as u64;
            self.reader.consume(len);
            if newline {
                break;
            }
        }
        self.rows += 1;
        if self.rows > self.expected_rows {
            return Err("Kernel result row count mismatch".into());
        }
        let value: Value = serde_json::from_slice(&line).map_err(|e| e.to_string())?;
        let object = value.as_object().ok_or("Invalid kernel result row")?;
        let keys = [
            "name",
            "count",
            "total",
            "min",
            "max",
            "waves",
            "waveTime",
            "activeTime",
        ];
        if object.len() != keys.len()
            || keys.iter().any(|k| !object.contains_key(*k))
            || !value["name"].is_string()
            || !value["count"].is_number()
            || !value["waves"].is_number()
            || keys[2..]
                .iter()
                .any(|k| !value[*k].is_number() && !value[*k].is_null())
        {
            return Err("Invalid kernel result fields".into());
        }
        Ok(Some(value))
    }
}
impl Iterator for NdjsonRows<'_> {
    type Item = Result<Value, String>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        match self.read_row() {
            Ok(Some(row)) => Some(Ok(row)),
            Ok(None) => {
                self.done = true;
                None
            }
            Err(e) => {
                self.done = true;
                Some(Err(e))
            }
        }
    }
}
pub fn read_receipt(job: &AnalysisJob, flag: &AtomicBool) -> Result<Value, String> {
    check(flag)?;
    let log = job.folder.join("stdout.log");
    if fs::metadata(&log).map_err(|e| e.to_string())?.len() > budget("receiptBytes") {
        return Err("Analysis receipt exceeds 64 KiB".into());
    }
    let v: Value = serde_json::from_slice(&fs::read(log).map_err(|e| e.to_string())?)
        .map_err(|e| format!("Invalid analysis receipt: {e}"))?;
    if v["protocol"] != protocol()["protocol"] || v["version"] != protocol()["version"] {
        return Err("Analysis worker protocol mismatch; rebuild the rocprof-studio binary and bundled analysis worker together.".into());
    }
    if job.mode == "kernel-stream" {
        let fields = ["protocol", "version", "kind", "file", "rows", "bytes"];
        let o = v.as_object().ok_or("Invalid kernel receipt")?;
        if o.len() != fields.len()
            || fields.iter().any(|k| !o.contains_key(*k))
            || v["kind"] != "kernel-rows"
            || v["file"] != "result.ndjson"
            || v["rows"]
                .as_u64()
                .is_none_or(|n| n > budget("kernelGroups"))
            || v["bytes"]
                .as_u64()
                .is_none_or(|n| n > budget("kernelResultBytes"))
        {
            return Err("Invalid kernel receipt fields".into());
        }
        let _ = NdjsonRows::new(
            &job.folder.join("result.ndjson"),
            v["rows"].as_u64().unwrap(),
            v["bytes"].as_u64().unwrap(),
            flag,
        )?;
    }
    Ok(v)
}
pub fn kernel_metadata(c: &Capture) -> Value {
    serde_json::to_value(KernelMeta(c)).expect("borrowed kernel metadata")
}
pub fn kernel_result(
    c: &Capture,
    a: &Value,
    job: &AnalysisJob,
    flag: &AtomicBool,
) -> Result<Value, String> {
    let receipt = read_receipt(job, flag)?;
    let rows = NdjsonRows::new(
        &job.folder.join("result.ndjson"),
        receipt["rows"].as_u64().unwrap(),
        receipt["bytes"].as_u64().unwrap(),
        flag,
    )?;
    let meta = kernel_metadata(c);
    if let Some(output) = a["output"].as_str() {
        let path = Path::new(output);
        let mut temp = tempfile::NamedTempFile::new_in(
            path.parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new(".")),
        )
        .map_err(|e| e.to_string())?;
        temp.write_all(b"[").map_err(|e| e.to_string())?;
        for (i, row) in rows.enumerate() {
            check(flag)?;
            let row = row?;
            if i > 0 {
                temp.write_all(b",").map_err(|e| e.to_string())?;
            }
            serde_json::to_writer(&mut temp, &row).map_err(|e| e.to_string())?;
        }
        temp.write_all(b"]").map_err(|e| e.to_string())?;
        temp.as_file().sync_all().map_err(|e| e.to_string())?;
        check(flag)?;
        temp.persist_noclobber(path).map_err(|e| e.to_string())?;
        return Ok(json!({"path":output,"analysisMeta":meta}));
    }
    let mut page = page_fallible_rows(rows, &PageArgs::parse(a), flag, "kernel aggregates")?;
    page["analysisMeta"] = meta;
    Ok(page)
}
pub fn monitor(job: &AnalysisJob) -> Result<(), String> {
    let stdout = if job.mode == "kernel-stream" || job.mode == "timeline-stream" {
        budget("receiptBytes")
    } else {
        budget("stdoutBytes")
    };
    let html = job.args["maxOutputBytes"]
        .as_u64()
        .unwrap_or(budget("fileBytes"));
    for (name, max) in [
        ("stdout.log", stdout),
        ("result.ndjson", budget("kernelResultBytes")),
        ("result.html", html),
    ] {
        if let Ok(m) = fs::symlink_metadata(job.folder.join(name)) {
            if !m.is_file() || m.len() > max {
                return Err(format!("MCP_ANALYSIS_SPOOL_BUDGET: {name} exceeds {max} bytes or is not a regular file; no output was published"));
            }
        }
    }
    for stream in &job.streams {
        if fs::metadata(job.folder.join(&stream.file))
            .map_err(|e| e.to_string())?
            .len()
            > stream.bytes
        {
            return Err("MCP_ANALYSIS_SPOOL_BUDGET: input spool grew beyond preflight size".into());
        }
    }
    Ok(())
}
#[cfg(test)]
mod kernel_tests {
    use super::*;
    #[test]
    fn mcp_rust_prepared_bundle_kernel_page_and_all_row_output() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = super::transport_tests::capture();
        let flag = AtomicBool::new(false);
        c.dispatches = (0..1001)
            .map(|i| Dispatch {
                kernel: format!("kernel-{i:04}"),
                ..c.dispatches[0].clone()
            })
            .collect();
        let args = json!({"analysis":"kernels","limit":2000,"range":[50,60],"kernel":"ignored"});
        let job = prepare_analysis(&c, &args, &flag, dir.path()).unwrap();
        fs::write(
            dir.path().join("analysis.cjs"),
            include_str!(concat!(env!("OUT_DIR"), "/mcp-analysis.cjs")),
        )
        .unwrap();
        let output = std::process::Command::new("node")
            .arg("--max-old-space-size=256")
            .arg(dir.path().join("analysis.cjs"))
            .arg(dir.path().join("input.json"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        fs::write(dir.path().join("stdout.log"), output.stdout).unwrap();
        let first = kernel_result(&c, &args, &job, &flag).unwrap();
        assert_eq!(first["effectiveLimit"], 1000);
        assert_eq!(first["total"], 1001);
        assert_eq!(first["nextOffset"], 1000);
        let last = kernel_result(&c, &json!({"offset":1000,"limit":2000}), &job, &flag).unwrap();
        assert_eq!(last["rows"].as_array().unwrap().len(), 1);
        assert!(last["nextOffset"].is_null());
        assert_eq!(
            first["rows"][0],
            json!({"name":"kernel-0000","count":1,"total":1,"min":1,"max":1,"waves":0,"waveTime":0,"activeTime":0})
        );
        assert_eq!(
            first["analysisMeta"]["availability"]["residency"],
            "not-recorded"
        );
        let target = dir.path().join("all.json");
        kernel_result(
            &c,
            &json!({"output":target,"limit":1,"query":"nonmatch"}),
            &job,
            &flag,
        )
        .unwrap();
        let all: Value = serde_json::from_slice(&fs::read(&target).unwrap()).unwrap();
        assert_eq!(all.as_array().unwrap().len(), 1001);
        assert!(kernel_result(&c, &json!({"output":target}), &job, &flag).is_err());
    }
}

fn same_range(a: &Value, b: &Value) -> bool {
    a.as_array().zip(b.as_array()).is_some_and(|(a, b)| {
        a.len() == 2
            && b.len() == 2
            && a.iter()
                .zip(b)
                .all(|(a, b)| a.as_f64().is_some() && a.as_f64() == b.as_f64())
    })
}
pub fn timeline_result(
    c: &Capture,
    a: &Value,
    job: &AnalysisJob,
    flag: &AtomicBool,
) -> Result<Value, String> {
    let receipt = read_receipt(job, flag)?;
    let fields = [
        "protocol", "version", "kind", "file", "bytes", "pages", "segments", "glyphs", "budgets",
        "range",
    ];
    let o = receipt.as_object().ok_or("Invalid timeline receipt")?;
    if o.len() != fields.len()
        || fields.iter().any(|k| !o.contains_key(*k))
        || receipt["kind"] != "timeline-html"
        || receipt["file"] != "result.html"
        || !same_range(&receipt["range"], &job.args["range"])
    {
        return Err("Invalid timeline receipt fields/range".into());
    }
    for (field, max) in [
        ("bytes", job.args["maxOutputBytes"].as_u64().unwrap()),
        ("pages", budget("pages")),
        ("segments", budget("segments")),
        ("glyphs", job.args["maxGlyphs"].as_u64().unwrap()),
    ] {
        if receipt[field].as_u64().is_none_or(|n| n > max) {
            return Err(format!("Invalid timeline receipt {field}"));
        }
    }
    if receipt["pages"].as_u64().unwrap() == 0
        || receipt["segments"].as_u64().unwrap() < job.args["timePages"].as_u64().unwrap()
    {
        return Err("Incomplete timeline page/segment receipt".into());
    }
    let expected = json!({"maxBytes":job.args["maxOutputBytes"],"maxGlyphs":job.args["maxGlyphs"],"chunkBytes":budget("chunkBytes"),"pages":budget("pages"),"segments":budget("segments"),"windowMarks":budget("windowMarks"),"windowBytes":budget("windowBytes"),"timePages":job.args["timePages"],"lanesPerPage":job.args["lanesPerPage"]});
    if receipt["budgets"] != expected {
        return Err("Timeline receipt budget mismatch".into());
    }
    let source = job.folder.join("result.html");
    let bytes = receipt["bytes"].as_u64().unwrap();
    let meta = fs::symlink_metadata(&source).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() != bytes {
        return Err("Timeline result byte count/type mismatch".into());
    }
    let export = json!({"range":receipt["range"],"clock":{"unit":c.unit,"origin":c.origin,"start":c.start,"end":c.end},"coverage":"recorded-data-only; replay completeness unknown","warnings":c.warnings,"bytes":bytes,"pages":receipt["pages"],"segments":receipt["segments"],"glyphs":receipt["glyphs"],"budgets":receipt["budgets"]});
    if let Some(output) = a["output"].as_str() {
        let mut destination = profile_core::timeline_file::TimelineFile::new(Path::new(output))?;
        let chunks = copy_timeline(&source, &mut destination, bytes, flag)?;
        destination.finish_new_checked(chunks, || check(flag))?;
        return Ok(json!({"path":output,"export":export}));
    }
    if bytes > budget("inlineBytes") || bytes > job.args["maxOutputBytes"].as_u64().unwrap() {
        return Err(
            "MCP_TIMELINE_BUDGET: inline HTML exceeds its budget; set output to a new HTML path"
                .into(),
        );
    }
    check(flag)?;
    let mut html = String::with_capacity(bytes as usize);
    let mut reader = BufReader::with_capacity(
        budget("readBufferBytes") as usize,
        fs::File::open(source).map_err(|e| e.to_string())?,
    );
    let mut buffer = vec![0; budget("readBufferBytes") as usize];
    let mut encoded = Vec::new();
    loop {
        check(flag)?;
        let n = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        encoded.extend_from_slice(&buffer[..n]);
        let valid = match std::str::from_utf8(&encoded) {
            Ok(s) => s.len(),
            Err(e) if e.error_len().is_none() => e.valid_up_to(),
            Err(_) => return Err("Invalid timeline UTF-8".into()),
        };
        html.push_str(std::str::from_utf8(&encoded[..valid]).unwrap());
        encoded.drain(..valid);
    }
    if !encoded.is_empty()
        || html.len() as u64 != bytes
        || !html.starts_with("<!doctype html><html>")
        || !html.ends_with("</body></html>")
    {
        return Err("Incomplete timeline HTML/byte receipt".into());
    }
    check(flag)?;
    Ok(json!({"html":html,"export":export}))
}
pub fn copy_timeline(
    source: &Path,
    destination: &mut profile_core::timeline_file::TimelineFile,
    expected: u64,
    flag: &AtomicBool,
) -> Result<u64, String> {
    copy_timeline_reader(
        fs::File::open(source).map_err(|e| e.to_string())?,
        destination,
        expected,
        flag,
    )
}
fn copy_timeline_reader(
    mut reader: impl Read,
    destination: &mut profile_core::timeline_file::TimelineFile,
    expected: u64,
    flag: &AtomicBool,
) -> Result<u64, String> {
    let max = budget("chunkBytes") as usize;
    let mut buffer = vec![0u8; max];
    let mut carry = 0;
    let mut chunks = 0;
    let mut bytes = 0u64;
    loop {
        check(flag)?;
        let n = reader
            .read(&mut buffer[carry..])
            .map_err(|e| e.to_string())?;
        if n == 0 {
            if carry > 0 {
                return Err("Truncated timeline UTF-8".into());
            }
            break;
        }
        let len = carry + n;
        let valid = match std::str::from_utf8(&buffer[..len]) {
            Ok(s) => s.len(),
            Err(e) if e.error_len().is_none() => e.valid_up_to(),
            Err(_) => return Err("Invalid timeline UTF-8".into()),
        };
        if valid > 0 {
            destination.append(chunks, std::str::from_utf8(&buffer[..valid]).unwrap())?;
            chunks += 1;
            bytes += valid as u64;
        }
        carry = len - valid;
        if carry > 3 {
            return Err("Invalid timeline UTF-8 carry".into());
        }
        buffer.copy_within(valid..len, 0);
    }
    if bytes != expected {
        return Err("Timeline file/receipt byte count mismatch".into());
    }
    check(flag)?;
    Ok(chunks)
}
#[cfg(test)]
mod publication_tests {
    use super::*;
    fn run(job: &AnalysisJob, heap: u32) {
        fs::write(
            job.folder.join("analysis.cjs"),
            include_str!(concat!(env!("OUT_DIR"), "/mcp-analysis.cjs")),
        )
        .unwrap();
        let output = std::process::Command::new("node")
            .arg(format!("--max-old-space-size={heap}"))
            .arg(job.folder.join("analysis.cjs"))
            .arg(job.folder.join("input.json"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        fs::write(job.folder.join("stdout.log"), output.stdout).unwrap();
    }
    #[test]
    fn mcp_bundle_timeline_receipt_inline_and_file_publication() {
        let dir = tempfile::tempdir().unwrap();
        let c = super::transport_tests::capture();
        let flag = AtomicBool::new(false);
        let args = json!({"analysis":"timeline","range":[0,3],"timePages":0,"lanesPerPage":0});
        let job = prepare_analysis(&c, &args, &flag, dir.path()).unwrap();
        assert_eq!(job.args["timePages"], 1);
        assert_eq!(job.args["lanesPerPage"], 12);
        run(&job, 256);
        let inline = timeline_result(&c, &args, &job, &flag).unwrap();
        assert!(inline["html"].as_str().unwrap().contains("dispatch:0"));
        assert_eq!(inline["export"]["glyphs"], 1);
        assert_eq!(
            inline["export"]["bytes"],
            inline["html"].as_str().unwrap().len()
        );
        let path = dir.path().join("timeline.html");
        let response = timeline_result(&c, &json!({"output":path}), &job, &flag).unwrap();
        assert_eq!(response["path"], path.to_str().unwrap());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            inline["html"].as_str().unwrap()
        );
        assert!(timeline_result(&c, &json!({"output":path}), &job, &flag).is_err());
        let mut receipt: Value =
            serde_json::from_slice(&fs::read(dir.path().join("stdout.log")).unwrap()).unwrap();
        receipt["bytes"] = json!(1);
        fs::write(dir.path().join("stdout.log"), receipt.to_string()).unwrap();
        assert!(timeline_result(&c, &args, &job, &flag).is_err());
        drop(job);
        assert!(!dir.path().join("result.html").exists());
        assert!(!dir.path().join("timeline.ndjson").exists());
    }
    #[test]
    fn mcp_utf8_copy_cancellation_and_footer_failure_leave_no_output() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("target.html");
        let source = dir.path().join("source.html");
        let flag = AtomicBool::new(false);
        let text = format!(
            "<!doctype html><html><body>{}λ</body></html>",
            "x".repeat(1024 * 1024 - 27)
        );
        fs::write(&source, &text).unwrap();
        let mut target = profile_core::timeline_file::TimelineFile::new(&path).unwrap();
        let chunks = copy_timeline(&source, &mut target, text.len() as u64, &flag).unwrap();
        target.finish_new_checked(chunks, || check(&flag)).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
        fs::remove_file(&path).unwrap();
        flag.store(true, Ordering::SeqCst);
        let mut target = profile_core::timeline_file::TimelineFile::new(&path).unwrap();
        assert!(copy_timeline(&source, &mut target, text.len() as u64, &flag).is_err());
        drop(target);
        assert!(!path.exists());
        flag.store(false, Ordering::SeqCst);
        fs::write(&source, "<!doctype html><html><body>").unwrap();
        let mut target = profile_core::timeline_file::TimelineFile::new(&path).unwrap();
        let n = fs::metadata(&source).unwrap().len();
        let chunks = copy_timeline(&source, &mut target, n, &flag).unwrap();
        assert!(target.finish_new(chunks).is_err());
        assert!(!path.exists());
    }
    #[test]
    fn mcp_prepare_cancel_and_oversize_string_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = super::transport_tests::capture();
        let flag = AtomicBool::new(true);
        assert!(prepare_analysis(&c, &json!({"analysis":"timeline"}), &flag, dir.path()).is_err());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
        flag.store(false, Ordering::SeqCst);
        c.dispatches[0].kernel = "x".repeat(budget("stringBytes") as usize + 1);
        assert!(
            prepare_analysis(&c, &json!({"analysis":"timeline"}), &flag, dir.path())
                .err()
                .unwrap()
                .contains("RECORD_BUDGET")
        );
        assert!(!dir.path().join("timeline.ndjson").exists());
    }
}

#[derive(Serialize)]
struct Clock<'a> {
    unit: &'a str,
    origin: &'a str,
    start: f64,
    end: f64,
}
impl<'a> Clock<'a> {
    fn new(c: &'a Capture) -> Self {
        Self {
            unit: &c.unit,
            origin: &c.origin,
            start: c.start,
            end: c.end,
        }
    }
}
struct KernelMeta<'a>(&'a Capture);
impl Serialize for KernelMeta<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let c = self.0;
        let mut m = s.serialize_map(None)?;
        m.serialize_entry("analysis", "kernels")?;
        m.serialize_entry("scope", "full-capture")?;
        m.serialize_entry("clock", &Clock::new(c))?;
        m.serialize_entry(
            "coverage",
            "recorded-data-only; replay completeness unknown",
        )?;
        let denominators = BTreeMap::from([
            (
                "total",
                "summed recorded dispatch durations; overlaps counted",
            ),
            (
                "waveTime",
                "summed recorded matching residency lifetimes; overlaps counted",
            ),
            (
                "activeTime",
                "union of recorded matching residency lifetimes per kernel",
            ),
        ]);
        m.serialize_entry("denominators", &denominators)?;
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Availability<'a> {
            recorded_dispatches: usize,
            recorded_intervals: usize,
            residency: &'a str,
            zero_qualification: &'a str,
            dispatch_duration: &'a str,
        }
        m.serialize_entry("availability",&Availability{recorded_dispatches:c.dispatches.len(),recorded_intervals:c.intervals.len(),residency:if c.intervals.is_empty(){"not-recorded"}else{"recorded"},zero_qualification:"Zero residency fields mean no matching recorded evidence, not measured absence of GPU work",dispatch_duration:if c.unit=="cycles"{"unavailable: SQTT dispatch events have no dispatch end timestamp"}else{"recorded dispatch intervals; not native application wall time"}})?;
        m.serialize_entry("warnings", &c.warnings)?;
        m.serialize_entry(
            "budgets",
            &BTreeMap::from([
                ("kernelGroups", budget("kernelGroups")),
                ("kernelIntervals", budget("kernelIntervals")),
            ]),
        )?;
        m.end()
    }
}
struct TimelineContext<'a> {
    c: &'a Capture,
    args: &'a Value,
}
impl Serialize for TimelineContext<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(None)?;
        m.serialize_entry("range", &self.args["range"])?;
        m.serialize_entry("clock", &Clock::new(self.c))?;
        m.serialize_entry(
            "coverage",
            "recorded-data-only; replay completeness unknown",
        )?;
        m.serialize_entry("warnings", &self.c.warnings)?;
        for (key, max) in [
            ("bytes", budget("fileBytes")),
            ("pages", budget("pages")),
            ("segments", budget("segments")),
            ("glyphs", budget("fileGlyphs")),
        ] {
            m.serialize_entry(key, &max)?;
        }
        m.serialize_entry(
            "budgets",
            &BTreeMap::from([
                ("maxBytes", budget("fileBytes")),
                ("maxGlyphs", budget("fileGlyphs")),
                ("chunkBytes", budget("chunkBytes")),
                ("pages", budget("pages")),
                ("segments", budget("segments")),
                ("windowMarks", budget("windowMarks")),
                ("windowBytes", budget("windowBytes")),
                ("timePages", budget("timePages")),
                ("lanesPerPage", budget("lanesPerPage")),
            ]),
        )?;
        m.end()
    }
}
#[cfg(test)]
mod admission_tests {
    use super::*;
    #[test]
    fn mcp_context_and_nonstreaming_preflight_refuse_before_worker_or_payload() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = super::transport_tests::capture();
        let flag = AtomicBool::new(false);
        c.warnings.push("x".repeat(budget("contextBytes") as usize));
        for analysis in ["kernels", "timeline"] {
            assert!(
                prepare_analysis(&c, &json!({"analysis":analysis}), &flag, dir.path())
                    .err()
                    .unwrap()
                    .contains("CONTEXT_BUDGET")
            );
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
        }
        c.warnings.clear();
        c.metadata
            .insert("irrelevant".into(), "x".repeat(257 * 1024 * 1024));
        assert!(
            prepare_analysis(&c, &json!({"analysis":"diagnostics"}), &flag, dir.path())
                .err()
                .unwrap()
                .contains("INPUT_BUDGET")
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
        c.metadata.clear();
        c.intervals = vec![
            profile_core::Interval {
                kernel: "unmatched".into(),
                start: 0.,
                end: 1.,
                se: 0,
                cu: 0,
                simd: 0,
                slot: 0
            };
            budget("kernelIntervals") as usize + 1
        ];
        assert!(
            prepare_analysis(&c, &json!({"analysis":"kernels"}), &flag, dir.path())
                .err()
                .unwrap()
                .contains("conservative transport")
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }
    #[test]
    fn mcp_cancel_during_spool_and_result_copy_drops_staging() {
        let dir = tempfile::tempdir().unwrap();
        let flag = AtomicBool::new(false);
        struct CancelWriter<'a>(&'a AtomicBool);
        impl Write for CancelWriter<'_> {
            fn write(&mut self, b: &[u8]) -> io::Result<usize> {
                self.0.store(true, Ordering::SeqCst);
                Ok(b.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut writer = CappedWriter {
            inner: CancelWriter(&flag),
            bytes: 0,
            max: budget("spoolBytes"),
            flag: &flag,
        };
        assert!(record_write(
            &KernelSample {
                kernel: "a",
                start: 0.,
                end: 1.
            },
            &mut writer,
            0,
            "dispatches"
        )
        .is_err());
        flag.store(false, Ordering::SeqCst);
        struct CancelReader<'a> {
            flag: &'a AtomicBool,
            read: bool,
        }
        impl Read for CancelReader<'_> {
            fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
                if self.read {
                    return Ok(0);
                }
                self.read = true;
                let text = b"<!doctype html><html><body>";
                b[..text.len()].copy_from_slice(text);
                self.flag.store(true, Ordering::SeqCst);
                Ok(text.len())
            }
        }
        let path = dir.path().join("target.html");
        let mut destination = profile_core::timeline_file::TimelineFile::new(&path).unwrap();
        assert!(copy_timeline_reader(
            CancelReader {
                flag: &flag,
                read: false
            },
            &mut destination,
            100,
            &flag
        )
        .is_err());
        drop(destination);
        assert!(!path.exists());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
#[cfg(test)]
mod projection_tests {
    use super::*;
    #[test]
    fn mcp_all_projected_modes_equal_legacy_bundle_and_activity_stays_null() {
        let root = tempfile::tempdir().unwrap();
        let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let legacy = root.path().join("legacy.cjs");
        let code="require('esbuild').buildSync({entryPoints:['tests/fixtures/a1-legacy-worker.ts'],bundle:true,platform:'node',format:'cjs',outfile:process.argv[1]})";
        let output = std::process::Command::new("node")
            .current_dir(project)
            .args(["-e", code])
            .arg(&legacy)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let demo: Value = serde_json::from_str(include_str!("../../public/demo.json")).unwrap();
        let mut base = serde_json::to_value(super::transport_tests::capture()).unwrap();
        for (k, v) in demo["capture"].as_object().unwrap() {
            base[k] = v.clone();
        }
        let c: Capture = serde_json::from_value(base).unwrap();
        let flag = AtomicBool::new(false);
        for analysis in [
            "occupancy",
            "activity",
            "hardware",
            "hotspots",
            "source",
            "markers",
            "scalar",
            "tensor",
            "diagnostics",
        ] {
            let folder = root.path().join(analysis);
            fs::create_dir(&folder).unwrap();
            let args = match analysis {
                "occupancy" => json!({"analysis":analysis,"se":0}),
                "hotspots" | "source" => json!({"analysis":analysis,"metric":"latency"}),
                "markers" => json!({"analysis":analysis,"range":[500,7240]}),
                "scalar" => json!({"analysis":analysis,"formula":"C0+C1"}),
                "tensor" => {
                    json!({"analysis":analysis,"definitions":"A := RAW*2","raw":{"RAW":{"shape":[1,1,1,2],"values":[2,4]}}})
                }
                _ => json!({"analysis":analysis}),
            };
            let job = prepare_analysis(&c, &args, &flag, &folder).unwrap();
            fs::write(
                folder.join("analysis.cjs"),
                include_str!(concat!(env!("OUT_DIR"), "/mcp-analysis.cjs")),
            )
            .unwrap();
            let input = folder.join("legacy.json");
            serde_json::to_writer(
                fs::File::create(&input).unwrap(),
                &json!({"capture":c,"args":args}),
            )
            .unwrap();
            let run = |bundle: &Path, input: &Path| {
                let output = std::process::Command::new("node")
                    .arg(bundle)
                    .arg(input)
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{analysis}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                serde_json::from_slice::<Value>(&output.stdout).unwrap()
            };
            let mut old = run(&legacy, &input);
            let mut new = run(&folder.join("analysis.cjs"), &folder.join("input.json"));
            if analysis == "diagnostics" {
                old.as_object_mut().unwrap().remove("generatedAt");
                new.as_object_mut().unwrap().remove("generatedAt");
            }
            assert_eq!(new, old, "{analysis}");
            let payload: Value =
                serde_json::from_slice(&fs::read(folder.join("payload.json")).unwrap()).unwrap();
            if analysis == "tensor" {
                assert_eq!(payload["capture"], json!({}));
            }
            drop(job);
        }
        let mut invalid = c;
        invalid.instructions.clear();
        for instruction in [
            None,
            Some(profile_core::Instruction {
                index: 0,
                text: "invalid".into(),
                source: String::new(),
                address: String::new(),
                code_object: String::new(),
                hits: 1,
                latency: -1.,
                stall: 0.,
                idle: 0.,
                pc_samples: 0,
                pc_issued: None,
                pc_stalled: None,
                stall_reasons: None,
            }),
        ] {
            if let Some(i) = instruction {
                invalid.instructions.push(i);
            }
            let folder = tempfile::tempdir().unwrap();
            let job = prepare_analysis(
                &invalid,
                &json!({"analysis":"activity"}),
                &flag,
                folder.path(),
            )
            .unwrap();
            fs::write(
                folder.path().join("analysis.cjs"),
                include_str!(concat!(env!("OUT_DIR"), "/mcp-analysis.cjs")),
            )
            .unwrap();
            let output = std::process::Command::new("node")
                .arg(folder.path().join("analysis.cjs"))
                .arg(folder.path().join("input.json"))
                .output()
                .unwrap();
            assert!(output.status.success());
            assert_eq!(output.stdout, b"null");
            drop(job);
        }
    }
}

pub fn publish_json(
    data: &Value,
    path: &Path,
    diagnostics: bool,
    flag: &AtomicBool,
) -> Result<(), String> {
    check(flag)?;
    let max = if diagnostics {
        if data["schemaVersion"] != 1 || data["kind"] != "rocprof-studio-diagnostics" {
            return Err("Invalid diagnostic manifest schema".into());
        }
        16 * 1024 * 1024
    } else {
        u64::MAX
    };
    let mut count = CappedWriter {
        inner: io::sink(),
        bytes: 0,
        max,
        flag,
    };
    serde_json::to_writer_pretty(&mut count, data).map_err(|e| {
        if diagnostics {
            format!("Diagnostic manifest exceeds 16 MiB ({e})")
        } else {
            e.to_string()
        }
    })?;
    let mut temp = tempfile::NamedTempFile::new_in(
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )
    .map_err(|e| e.to_string())?;
    let mut writer = CappedWriter {
        inner: BufWriter::new(temp.as_file_mut()),
        bytes: 0,
        max,
        flag,
    };
    serde_json::to_writer_pretty(&mut writer, data).map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())?;
    drop(writer);
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    check(flag)?;
    temp.persist_noclobber(path).map_err(|e| e.to_string())?;
    Ok(())
}
#[cfg(test)]
mod json_output_tests {
    use super::*;
    #[test]
    fn mcp_diagnostics_no_clobber_schema_and_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("diagnostic.json");
        let flag = AtomicBool::new(false);
        let data =
            json!({"schemaVersion":1,"kind":"rocprof-studio-diagnostics","capture":{"name":"c"}});
        fs::write(&path, b"raced").unwrap();
        assert!(publish_json(&data, &path, true, &flag).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"raced");
        fs::remove_file(&path).unwrap();
        assert!(publish_json(&json!({}), &path, true, &flag).is_err());
        assert!(!path.exists());
        flag.store(true, Ordering::SeqCst);
        assert!(publish_json(&data, &path, true, &flag).is_err());
        assert!(!path.exists());
        flag.store(false, Ordering::SeqCst);
        publish_json(&data, &path, true, &flag).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(path).unwrap()).unwrap(),
            data
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}

#[cfg(test)]
mod large_publication_tests {
    use super::*;
    #[test]
    fn mcp_file_publication_above_16_mib_never_uses_inline_or_plot_budget() {
        let dir = tempfile::tempdir().unwrap();
        let c = Capture::empty("synthetic.csv".into(), "CSV", "ns");
        let flag = AtomicBool::new(false);
        let target = dir.path().join("large.html");
        let args = json!({"analysis":"timeline","output":target});
        let job = prepare_analysis(&c, &args, &flag, dir.path()).unwrap();
        let source = dir.path().join("result.html");
        let mut file = BufWriter::new(fs::File::create(&source).unwrap());
        file.write_all(b"<!doctype html><html><body><p>").unwrap();
        let chunk = vec![b'x'; 1024 * 1024];
        for _ in 0..17 {
            file.write_all(&chunk).unwrap();
        }
        file.write_all(b"</p></body></html>").unwrap();
        file.flush().unwrap();
        drop(file);
        let bytes = fs::metadata(&source).unwrap().len();
        let receipt = json!({"protocol":protocol()["protocol"],"version":protocol()["version"],"kind":"timeline-html","file":"result.html","bytes":bytes,"pages":1,"segments":1,"glyphs":0,"range":job.args["range"],"budgets":{"maxBytes":job.args["maxOutputBytes"],"maxGlyphs":job.args["maxGlyphs"],"chunkBytes":budget("chunkBytes"),"pages":budget("pages"),"segments":budget("segments"),"windowMarks":budget("windowMarks"),"windowBytes":budget("windowBytes"),"timePages":job.args["timePages"],"lanesPerPage":job.args["lanesPerPage"]}});
        fs::write(dir.path().join("stdout.log"), receipt.to_string()).unwrap();
        let response = timeline_result(&c, &args, &job, &flag).unwrap();
        assert_eq!(response["export"]["bytes"], bytes);
        assert_eq!(fs::metadata(&target).unwrap().len(), bytes);
        assert!(bytes > budget("inlineBytes"));
        assert!(timeline_result(&c, &json!({}), &job, &flag)
            .unwrap_err()
            .contains("set output"));
        drop(job);
        assert!(!source.exists());
        assert!(target.exists());
    }
}
