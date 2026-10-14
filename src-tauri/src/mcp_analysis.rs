use serde::{ser::{SerializeMap, SerializeSeq}, Serialize, Serializer};
use serde_json::{json, Value};
use std::{collections::BTreeMap, io::{self, Write}, sync::{atomic::{AtomicBool, Ordering}, OnceLock}};

pub fn protocol() -> &'static Value {
    static P: OnceLock<Value> = OnceLock::new();
    P.get_or_init(|| serde_json::from_str(include_str!("../../src/mcpAnalysisProtocol.json")).unwrap())
}
pub fn budget(name: &str) -> u64 { protocol()["budgets"][name].as_u64().unwrap() }
pub fn check(flag: &AtomicBool) -> Result<(), String> {
    if flag.load(Ordering::SeqCst) { Err("Cancelled".into()) } else { Ok(()) }
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
    fn flush(&mut self) -> io::Result<()> { self.inner.flush() }
}
pub fn encoded_size<T: Serialize + ?Sized>(value: &T, max: u64, flag: &AtomicBool) -> Result<u64, String> {
    check(flag)?;
    let mut writer = CappedWriter { inner: io::sink(), bytes: 0, max, flag };
    serde_json::to_writer(&mut writer, value).map_err(|e| e.to_string())?;
    Ok(writer.bytes)
}
#[derive(Debug)]
pub struct PageArgs { pub query: String, pub offset: usize, pub limit: usize }
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
fn row_size<T: Serialize>(row: &T, dataset: &str, index: usize, flag: &AtomicBool) -> Result<u64, String> {
    encoded_size(row, budget("queryRowBytes"), flag).map_err(|e| {
        if check(flag).is_err() { return "Cancelled".into(); }
        format!("MCP_QUERY_ROW_BUDGET: dataset {dataset}, row {index} exceeds {} bytes ({e}). No rows were dropped. A smaller limit cannot split this row; export the capture with capture_export for offline inspection.", budget("queryRowBytes"))
    })
}
fn page_error() -> String {
    format!("MCP_QUERY_PAGE_BUDGET: selected rows exceed {} bytes. Retry capture_query with a smaller limit and follow nextOffset; no partial page was returned.", budget("queryPageBytes"))
}
pub fn page_slice<T: Serialize>(rows: &[T], args: &PageArgs, flag: &AtomicBool, dataset: &str) -> Result<Value, String> {
    check(flag)?;
    if !args.query.is_empty() { return page_rows(rows, args, flag, dataset); }
    let selected = &rows[args.offset.min(rows.len())..rows.len().min(args.offset.saturating_add(args.limit))];
    for (i, row) in selected.iter().enumerate() { row_size(row, dataset, args.offset + i, flag)?; }
    encoded_size(selected, budget("queryPageBytes"), flag).map_err(|_| if check(flag).is_err() { "Cancelled".into() } else { page_error() })?;
    let values = selected.iter().map(|r| { check(flag)?; serde_json::to_value(r).map_err(|e| e.to_string()) }).collect::<Result<_,_>>()?;
    Ok(args.response(values, rows.len()))
}
pub fn page_rows<I, T>(rows: I, args: &PageArgs, flag: &AtomicBool, dataset: &str) -> Result<Value, String>
where I: IntoIterator<Item=T>, T: Serialize {
    page_fallible_rows(rows.into_iter().map(Ok), args, flag, dataset)
}
pub fn page_fallible_rows<I, T>(rows: I, args: &PageArgs, flag: &AtomicBool, dataset: &str) -> Result<Value, String>
where I: IntoIterator<Item=Result<T,String>>, T: Serialize {
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
            if args.query.is_empty() && size + bytes + u64::from(!matches.is_empty()) > budget("queryPageBytes") { return Err(page_error()); }
            Some(serde_json::to_value(&row).map_err(|e| e.to_string())?)
        } else { None };
        if args.query.is_empty() || value.as_ref().unwrap().to_string().to_lowercase().contains(&args.query) {
            if total >= args.offset && matches.len() < args.limit {
                let v = value.unwrap();
                bytes += encoded_size(&v, budget("queryRowBytes"), flag)? + u64::from(!matches.is_empty());
                if bytes > budget("queryPageBytes") { return Err(page_error()); }
                matches.push(v);
            }
            total = total.checked_add(1).ok_or("Page match count overflow")?;
        }
    }
    Ok(args.response(matches, total))
}
pub fn bounded_value<T: Serialize + ?Sized>(v: &T, flag: &AtomicBool) -> Result<Value,String> {
    encoded_size(v, budget("queryPageBytes"), flag).map_err(|e| format!("MCP_QUERY_PAGE_BUDGET: response exceeds {} bytes ({e}); use capture_export for offline inspection.",budget("queryPageBytes")))?;
    serde_json::to_value(v).map_err(|e| e.to_string())
}

pub struct CompileReportRow<'a>(pub &'a profile_core::loom_report::LoomCompileReport);
pub struct CompileEntryRow<'a>(pub &'a profile_core::loom_report::LoomCompileEntry);
struct Entries<'a>(&'a [profile_core::loom_report::LoomCompileEntry]);
impl Serialize for Entries<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok,S::Error> {
        let mut seq = s.serialize_seq(Some(self.0.len()))?;
        for e in self.0 { seq.serialize_element(&CompileEntryRow(e))?; }
        seq.end()
    }
}
fn resources(v: Option<&Value>) -> Option<BTreeMap<&'static str, &Value>> {
    let v = v?.as_object()?;
    let mut out = BTreeMap::new();
    for (src,dst) in [("subgroup_size","subgroupSize"),("max_subgroups_per_simd","maxSubgroupsPerSimd"),("resident_subgroups_per_simd","residentSubgroupsPerSimd"),("occupancy_percent","occupancyPercent"),("limiting_resource","limitingResource"),("residency","residency")] {
        if let Some(v) = v.get(src) { out.insert(dst,v); }
    }
    Some(out)
}
struct TargetResourceFacts<'a>(&'a Value);
fn registers(v: &Value) -> BTreeMap<&'static str, &Value> {
    let mut out = BTreeMap::new();
    for (key,path) in [("registerClass",vec!["register_class"]),("finalRegisterCount",vec!["final","register_count"]),("overheadUnits",vec!["final","overhead_units"]),("scheduledPeakLiveUnits",vec!["scheduled_pressure","peak_live_units"])] {
        let mut value = Some(v);
        for p in path { value = value.and_then(Value::as_object).and_then(|o| o.get(p)); }
        if let Some(value) = value { out.insert(key,value); }
    }
    out
}
fn has_resources(v: &Value) -> bool {
    resources(Some(v)).is_some_and(|o| !o.is_empty()) || ["scalar","vector"].iter().any(|k| !registers(&v[*k]).is_empty())
}
impl Serialize for TargetResourceFacts<'_> {
    fn serialize<S: Serializer>(&self, s:S) -> Result<S::Ok,S::Error> {
        let mut map = s.serialize_map(None)?;
        for (key, src) in [("sgpr","scalar"),("vgpr","vector")] {
            let r = registers(&self.0[src]);
            if !r.is_empty() { map.serialize_entry(key, &r)?; }
        }
        for (k,v) in resources(Some(self.0)).unwrap_or_default() { map.serialize_entry(k,v)?; }
        map.end()
    }
}
struct InstructionMixFacts<'a>(&'a BTreeMap<String,Value>);
impl Serialize for InstructionMixFacts<'_> {
    fn serialize<S:Serializer>(&self,s:S)->Result<S::Ok,S::Error> {
        let mut m = s.serialize_map(None)?;
        for (src,dst) in [("static_instruction_mix","static"),("dynamic_instruction_mix","dynamic")] {
            if let Some(v) = self.0.get(src) { m.serialize_entry(dst,v)?; }
        }
        m.end()
    }
}
fn sections<M: SerializeMap>(m: &mut M, sections: &BTreeMap<String,Value>) -> Result<(),M::Error> {
    if let Some(v) = sections.get("target_resources").filter(|v| has_resources(v)) { m.serialize_entry("targetResources",&TargetResourceFacts(v))?; }
    if sections.contains_key("static_instruction_mix") || sections.contains_key("dynamic_instruction_mix") { m.serialize_entry("instructionMix",&InstructionMixFacts(sections))?; }
    for (src,dst) in [("wait_plan","waitPlan"),("workload","workload")] { if let Some(v) = sections.get(src) { m.serialize_entry(dst,v)?; } }
    Ok(())
}
impl Serialize for CompileReportRow<'_> {
    fn serialize<S: Serializer>(&self,s:S)->Result<S::Ok,S::Error> {
        let r=self.0; let mut m=s.serialize_map(None)?;
        m.serialize_entry("stability",&r.stability)?;
        m.serialize_entry("unit","static")?;
        m.serialize_entry("facts","Compile-time static facts from the Loom compiler; not measurements, dispatches or runtime counts. Absent fields are unavailable, not zero. Version-zero, same-compiler-horizon: not comparable across targets or compiler horizons.")?;
        m.serialize_entry("containerKind",&r.container_kind)?;
        m.serialize_entry("mode",&r.mode)?;
        m.serialize_entry("schemaVersion",&r.schema_version)?;
        #[derive(Serialize)] struct Status<'a>{ code:i64, name:&'a str }
        m.serialize_entry("status",&Status{code:r.status_code,name:&r.status_name})?;
        if !r.identity.is_empty() { m.serialize_entry("identity",&r.identity)?; }
        if let Some(v)=r.detail_flags { m.serialize_entry("detailFlags",&v)?; }
        if !r.detail_flag_names.is_empty() { m.serialize_entry("detailFlagNames",&r.detail_flag_names)?; }
        if !r.envelope_context.is_empty() { m.serialize_entry("envelopeContext",&r.envelope_context)?; }
        if let Some(v)=r.artifact_size { m.serialize_entry("artifactSize",&v)?; }
        if !r.config_bindings.is_empty() { m.serialize_entry("configBindings",&r.config_bindings)?; }
        m.serialize_entry("provenance",&r.provenance)?;
        sections(&mut m,&r.sections)?;
        if !r.residency_constraints.is_empty() { m.serialize_entry("residencyConstraints",&r.residency_constraints)?; }
        m.serialize_entry("entries",&Entries(&r.entries))?; m.end()
    }
}
impl Serialize for CompileEntryRow<'_> {
    fn serialize<S:Serializer>(&self,s:S)->Result<S::Ok,S::Error> {
        let e=self.0; let mut m=s.serialize_map(None)?;
        m.serialize_entry("index",&e.index)?; m.serialize_entry("displayName",&e.display_name)?;
        if !e.identity.is_empty() { m.serialize_entry("identity",&e.identity)?; }
        if let Some(v)=e.detail_flags { m.serialize_entry("detailFlags",&v)?; }
        if !e.detail_flag_names.is_empty() { m.serialize_entry("detailFlagNames",&e.detail_flag_names)?; }
        if !e.counters.is_empty() { m.serialize_entry("counters",&e.counters)?; }
        sections(&mut m,&e.sections)?; m.end()
    }
}

pub struct Summary<'a>(pub &'a profile_core::Capture);
impl Serialize for Summary<'_> {
    fn serialize<S:Serializer>(&self,s:S)->Result<S::Ok,S::Error> {
        let c=self.0; let mut m=s.serialize_map(None)?;
        m.serialize_entry("id",&c.id)?; m.serialize_entry("name",&c.name)?; m.serialize_entry("path",&c.input_path)?;
        m.serialize_entry("format",&c.format)?; m.serialize_entry("architecture",&c.architecture)?; m.serialize_entry("unit",&c.unit)?;
        m.serialize_entry("start",&c.start)?; m.serialize_entry("end",&c.end)?; m.serialize_entry("warnings",&c.warnings)?;
        for (k,n) in [("dispatches",c.dispatches.len()),("instructions",c.instructions.len()),("waves",c.waves.len()),("counters",c.counters.len()),("compileReports",c.compile_reports.len())] { m.serialize_entry(k,&n)?; }
        m.end()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    #[test]
    fn mcp_typed_page_visits_only_selected_rows_and_cancels_empty() {
        struct Row<'a>(usize,&'a RefCell<Vec<usize>>);
        impl Serialize for Row<'_> {
            fn serialize<S:Serializer>(&self,s:S)->Result<S::Ok,S::Error> {
                assert!([50,51].contains(&self.0),"unselected serializer called");
                self.1.borrow_mut().push(self.0); self.0.serialize(s)
            }
        }
        let visits=RefCell::new(Vec::new());
        let rows:Vec<_>=(0..100001).map(|i|Row(i,&visits)).collect();
        let flag=AtomicBool::new(false);
        let v=page_slice(&rows,&PageArgs::parse(&json!({"offset":50,"limit":2})),&flag,"test").unwrap();
        assert_eq!(v,json!({"rows":[50,51],"total":100001,"offset":50,"nextOffset":52,"effectiveLimit":2}));
        assert_eq!(visits.into_inner().into_iter().collect::<std::collections::BTreeSet<_>>(),[50,51].into());
        flag.store(true,Ordering::SeqCst);
        assert!(page_slice::<u8>(&[],&PageArgs::parse(&json!({})),&flag,"test").is_err());
    }
    #[test]
    fn mcp_row_and_page_budgets_precede_value_allocation() {
        struct Large;
        impl Serialize for Large {
            fn serialize<S:Serializer>(&self,s:S)->Result<S::Ok,S::Error> {
                assert!(!std::any::type_name::<S>().contains("value::ser"),"allocated before admission");
                let mut seq=s.serialize_seq(Some(5*1024))?;
                for _ in 0..5*1024 { seq.serialize_element(&"x".repeat(1024))?; }
                seq.end()
            }
        }
        let flag=AtomicBool::new(false);
        for query in ["","nonmatching"] {
            assert!(page_slice(&[Large],&PageArgs::parse(&json!({"query":query})),&flag,"test").unwrap_err().starts_with("MCP_QUERY_ROW_BUDGET"));
        }
        let rows=vec!["x".repeat(3*1024*1024);6];
        assert!(page_slice(&rows,&PageArgs::parse(&json!({})),&flag,"test").unwrap_err().starts_with("MCP_QUERY_PAGE_BUDGET"));
        assert!(page_slice(&rows,&PageArgs::parse(&json!({"query":"x"})),&flag,"test").unwrap_err().starts_with("MCP_QUERY_PAGE_BUDGET"));
    }
    #[test]
    fn mcp_checked_offset_zero_and_clamp() {
        let flag=AtomicBool::new(false); let rows:Vec<_>=(0..1001).collect();
        let first=page_slice(&rows,&PageArgs::parse(&json!({"limit":2000})),&flag,"test").unwrap();
        let last=page_slice(&rows,&PageArgs::parse(&json!({"limit":2000,"offset":1000})),&flag,"test").unwrap();
        assert_eq!(first["effectiveLimit"],1000); assert_eq!(first["nextOffset"],1000);
        assert_eq!(last["rows"],json!([1000])); assert!(last["nextOffset"].is_null());
        let zero=page_slice(&rows,&PageArgs::parse(&json!({"limit":0})),&flag,"test").unwrap();
        assert_eq!(zero["rows"],json!([])); assert_eq!(zero["nextOffset"],0);
        let beyond=page_slice(&rows,&PageArgs::parse(&json!({"offset":u64::MAX})),&flag,"test").unwrap();
        assert_eq!(beyond["total"],1001); assert!(beyond["nextOffset"].is_null());
    }
}
