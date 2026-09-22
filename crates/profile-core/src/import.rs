use crate::model::*;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

const MAX_FILE: u64 = 256 * 1024 * 1024;
pub(crate) const MAX_ROWS: usize = 2_000_000;
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn json(path: &Path) -> Result<Value, String> {
    let size = fs::metadata(path).map_err(err)?.len();
    if size > MAX_FILE {
        return Err(format!(
            "{} exceeds the 256 MiB per-file import limit",
            path.display()
        ));
    }
    serde_json::from_reader(fs::File::open(path).map_err(err)?)
        .map_err(|e| format!("{}: {e}", path.display()))
}
fn safe_path(root: &Path, name: &str) -> Result<PathBuf, String> {
    let path = root.join(name).canonicalize().map_err(err)?;
    if !path.starts_with(root) {
        return Err(format!("Sidecar escapes capture directory: {name}"));
    }
    Ok(path)
}
pub(crate) fn sidecar(root: &Path, name: &str) -> Result<Value, String> {
    json(&safe_path(root, name)?)
}
fn arr(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn optional_array<'a>(value: &'a Value, name: &str) -> Result<&'a [Value], String> {
    if value.is_null() {
        return Ok(&[]);
    }
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| format!("Invalid {name} array"))
}
fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.)
}
fn uint(v: &Value) -> u32 {
    v.as_u64().unwrap_or(0) as u32
}
fn text(v: &Value) -> String {
    v.as_str().map(str::to_owned).unwrap_or_else(|| {
        if v.is_null() {
            String::new()
        } else {
            v.to_string()
        }
    })
}
fn required_number(v: &Value, field: &str) -> Result<f64, String> {
    v.as_f64()
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("Missing or invalid {field}"))
}
fn check_rows(n: usize) -> Result<(), String> {
    if n > MAX_ROWS {
        Err("Import exceeds the 2 million record limit; select a smaller capture window".into())
    } else {
        Ok(())
    }
}

pub fn load_capture(path: &Path) -> Result<Capture, String> {
    load_capture_with_wave_states(path, true)
}
pub fn load_capture_with_wave_states(
    path: &Path,
    load_wave_states: bool,
) -> Result<Capture, String> {
    load_capture_options(path, load_wave_states, None)
}
pub fn sampling_methods(path: &Path) -> Result<Vec<String>, String> {
    let value = json(path)?;
    crate::pc_sampling::methods(arr(&value["rocprofiler-sdk-tool"]))
}
pub fn load_capture_options(
    path: &Path,
    load_wave_states: bool,
    sampling_method: Option<&str>,
) -> Result<Capture, String> {
    let path = path.canonicalize().map_err(err)?;
    if sampling_method.is_some() && !path.extension().is_some_and(|e| e == "json") {
        return Err("PC sampling selection requires a raw SDK JSON input".into());
    }
    if sampling_method.is_some() {
        return load_spm(&path, sampling_method);
    }
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("rocprof-capture"))
    {
        return crate::capture_archive::read(&path, &std::sync::atomic::AtomicBool::new(false));
    }
    if path.is_dir() {
        if !path.join("filenames.json").exists() {
            return crate::decoder::decode_with_wave_states(
                &path,
                &Default::default(),
                &std::sync::atomic::AtomicBool::new(false),
                load_wave_states,
            );
        }
        return load_ui(&path, load_wave_states);
    }
    if path
        .extension()
        .is_some_and(|x| x == "rpd" || x == "rocpd" || x == "db" || x == "sqlite")
    {
        return crate::rocpd::load(&path);
    }
    if path.extension().is_some_and(|x| x.eq_ignore_ascii_case("rgp")) {
        return crate::rgp::load(&path);
    }
    match path.file_name().and_then(|p|p.to_str()) {
        Some("filenames.json" | "code.json" | "occupancy.json") => load_ui(path.parent().unwrap(), load_wave_states),
        _ if path.extension().is_some_and(|x|x == "att") => crate::decoder::decode_with_wave_states(&path, &Default::default(), &std::sync::atomic::AtomicBool::new(false), load_wave_states),
        _ if path.extension().is_some_and(|x|x == "csv") => load_csv(&path),
        _ if path.extension().is_some_and(|x|x == "json") => load_spm(&path, sampling_method),
        _ => Err("Open a decoded ui_output directory, rocprofv3 kernel trace CSV, SPM results JSON, a ROCm SDK ROCPD database, or a RADV Vulkan .rgp capture. Raw ATT requires the decoder Python package and V2 library.".into()),
    }
}
fn att_selection(name: &str) -> Option<AttCounterSelection> {
    let (counter, mask, explicit) = if let Some((counter, suffix)) = name.split_once(':') {
        if suffix.is_empty() || !suffix.bytes().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        (counter, u8::from_str_radix(suffix, 16).ok()?, true)
    } else {
        (name, 15, false)
    };
    if mask > 15
        || counter.is_empty()
        || !counter
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_')
    {
        return None;
    }
    Some(AttCounterSelection {
        counter: counter.into(),
        simd_mask: mask,
        simds: (0..4).filter(|simd| mask & (1 << simd) != 0).collect(),
        explicit,
    })
}
fn load_ui(root: &Path, load_wave_states: bool) -> Result<Capture, String> {
    let names = sidecar(root, "filenames.json")?;
    let mut c = Capture::empty(
        root.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
        "rocprofv3 UI JSON",
        "cycles",
    );
    c.root = root.to_owned();
    c.metadata.insert("gfxip".into(), text(&names["gfxip"]));
    c.architecture = if names["gfxv"].is_string() {
        text(&names["gfxv"])
    } else {
        format!("gfxip {}", text(&names["gfxip"]))
    };
    c.metadata.insert(
        "coverage".into(),
        "Unknown — no independent replay coverage evidence attached".into(),
    );
    c.warnings.push("SQTT details describe the targeted CU; global occupancy can cover other CUs. Empty details do not mean zero activity.".into());
    c.warnings.push(
        "Instrumented cycles are attribution evidence, not native application throughput.".into(),
    );
    if names["demo"].as_bool() == Some(true) {
        c.metadata.insert("demo".into(), "true".into());
        c.name = "LDS transpose · example capture".into();
    }
    if let Some(ses) = names["wave_filenames"].as_object() {
        for (se, simds) in ses {
            for (simd, slots) in simds.as_object().ok_or("Invalid SIMD hierarchy")? {
                for (slot, wids) in slots.as_object().ok_or("Invalid slot hierarchy")? {
                    for (wid, entry) in wids.as_object().ok_or("Invalid wave hierarchy")? {
                        let start = required_number(&entry[1], "wave start")?;
                        let end = required_number(&entry[2], "wave end")?;
                        if end < start {
                            return Err("Wave ends before it starts".into());
                        }
                        c.waves.push(WaveIndex {
                            id: entry[0].as_str().ok_or("Invalid wave filename")?.into(),
                            se: se.parse().map_err(err)?,
                            simd: simd.parse().map_err(err)?,
                            slot: slot.parse().map_err(err)?,
                            wid: wid.parse().map_err(err)?,
                            start,
                            end,
                        });
                        check_rows(c.waves.len())?;
                    }
                }
            }
        }
    }
    c.waves
        .sort_by(|a, b| (a.se, a.simd, a.slot, a.wid).cmp(&(b.se, b.simd, b.slot, b.wid)));
    if root.join("realtime.json").exists() {
        match sidecar(root, "realtime.json").and_then(|value| crate::realtime::parse(&value)) {
            Ok(calibration) => {
                c.warnings.push(format!("{} REALTIME anchors retained but not yet applied. SQTT remains in shader cycles.", calibration.anchors.len()));
                c.realtime = Some(calibration);
            }
            Err(error) => c
                .warnings
                .push(format!("REALTIME calibration unavailable: {error}")),
        }
    }
    if root.join("code.json").exists() {
        let code = sidecar(root, "code.json")?;
        let header = arr(&code["header"]);
        let issued = header.iter().position(|s| s == "PC_Issued");
        let stalled = header.iter().position(|s| s == "PC_Stalled");
        let reasons = header.iter().position(|s| s == "Stall_Reasons");
        let sample_count = |value: &Value| -> Result<u64, String> {
            value
                .as_u64()
                .filter(|n| *n <= 9_007_199_254_740_991)
                .ok_or_else(|| "Invalid or inexact PC sample count".into())
        };
        check_rows(arr(&code["code"]).len())?;
        let complete_costs = arr(&code["code"])
            .iter()
            .all(|row| (7..=9).all(|index| row[index].as_f64().is_some_and(f64::is_finite)));
        c.metadata.insert(
            "instruction_costs".into(),
            if complete_costs {
                "complete"
            } else {
                "incomplete"
            }
            .into(),
        );
        for row in arr(&code["code"]) {
            if arr(row).len() < 8 {
                return Err("Malformed code.json instruction row".into());
            }
            let optional_count = |column: Option<usize>| -> Result<Option<u64>, String> {
                column
                    .filter(|i| !row[*i].is_null())
                    .map(|i| sample_count(&row[i]))
                    .transpose()
            };
            let pc_issued = optional_count(issued)?;
            let pc_stalled = optional_count(stalled)?;
            let stall_reasons = reasons
                .filter(|i| !row[*i].is_null())
                .map(|i| {
                    let values = row[i].as_array().ok_or("Invalid Stall_Reasons array")?;
                    if values.len() > 256 {
                        return Err("Too many PC stall reason categories".into());
                    }
                    values
                        .iter()
                        .map(sample_count)
                        .collect::<Result<Vec<_>, String>>()
                })
                .transpose()?;
            let pc_samples = pc_issued
                .unwrap_or(0)
                .checked_add(pc_stalled.unwrap_or(0))
                .filter(|n| *n <= 9_007_199_254_740_991)
                .ok_or("PC sample total exceeds exact integer range")?;
            c.instructions.push(Instruction {
                index: row[2].as_i64().ok_or("Invalid ISA line index")?,
                text: text(&row[0]),
                source: text(&row[3]),
                code_object: text(&row[4]),
                address: format!("0x{:x}", row[5].as_u64().unwrap_or(0)),
                hits: row[6].as_u64().unwrap_or(0),
                latency: num(&row[7]),
                stall: num(&row[8]),
                idle: num(&row[9]),
                pc_samples,
                pc_issued,
                pc_stalled,
                stall_reasons,
            });
        }
    }
    if root.join("occupancy.json").exists() {
        occupancy(&sidecar(root, "occupancy.json")?, &mut c)?;
    }
    let ses: BTreeSet<u32> = c
        .waves
        .iter()
        .map(|w| w.se)
        .chain(c.intervals.iter().map(|i| i.se))
        .collect();
    for se in &ses {
        let name = format!("se{se}_perfcounter.json");
        if !root.join(&name).exists() {
            continue;
        }
        let data = sidecar(root, &name)?;
        let mut series: BTreeMap<(u32, u32, usize), Vec<[f64; 2]>> = BTreeMap::new();
        let rows = match data.get("data") {
            Some(Value::Array(rows)) => rows.as_slice(),
            Some(Value::Null) => &[],
            _ => return Err("Malformed ATT counter data array".into()),
        };
        check_rows(rows.len())?;
        for row in rows {
            if row.as_array().is_none_or(|values| values.len() != 7) {
                return Err("Malformed ATT counter row".into());
            }
            let time = row[0]
                .as_i64()
                .filter(|time| time.unsigned_abs() <= 9_007_199_254_740_991)
                .ok_or(
                    "ATT counter timestamp must be an exact signed integer within the viewer range",
                )?;
            let cu = row[5]
                .as_u64()
                .filter(|cu| *cu <= u8::MAX as u64)
                .ok_or("ATT counter CU must be an unsigned 8-bit integer")?
                as u32;
            let bank = row[6]
                .as_u64()
                .filter(|bank| *bank <= 1)
                .ok_or("ATT counter bank must be 0 or 1")? as u32;
            for n in 0..4 {
                let value = row[n + 1]
                    .as_u64()
                    .filter(|value| *value <= u16::MAX as u64)
                    .ok_or("ATT counter value must be an unsigned 16-bit integer")?;
                series
                    .entry((cu, bank, n))
                    .or_default()
                    .push([time as f64, value as f64]);
            }
        }
        for ((cu, bank, n), mut points) in series {
            let index = bank as usize * 4 + n;
            let name = arr(&names["counter_names"])
                .get(index)
                .map(text)
                .unwrap_or_else(|| format!("Counter {index}"));
            points.sort_by(|a, b| a[0].total_cmp(&b[0]));
            let att = att_selection(&name);
            if name.contains(':') && att.is_none() {
                c.warnings.push(format!("ATT counter {name} has an unsupported SIMD mask; its raw identity and values are retained."));
            }
            c.counters.push(Series {
                spm_clock: None,
                definition: None,
                att,
                name,
                unit: "raw count".into(),
                scope: format!("SE {se} · CU {cu} · bank {bank}"),
                points,
            });
        }
    }
    if !c.counters.is_empty() {
        c.warnings.push("ATT counter sidecars do not record counter types or physical units; values are retained as raw counts without unit conversion.".into());
    }
    c.metadata
        .insert("loadWaveStates".into(), load_wave_states.to_string());
    let wave_ses: BTreeSet<u32> = c.waves.iter().map(|wave| wave.se).collect();
    for (state, label) in [(2, "EXEC"), (3, "WAIT"), (4, "STALL")] {
        let name = format!("wstates{state}.json");
        if !load_wave_states {
            continue;
        }
        if !root.join(&name).exists() {
            continue;
        }
        if wave_ses.len() != 1 {
            c.warnings.push(format!("{name} was not loaded: precomputed wave states require a single detailed-wave shader engine"));
            continue;
        }
        let se = wave_ses.first().unwrap();
        let data = sidecar(root, &name)?;
        let times = data["time"]
            .as_array()
            .ok_or_else(|| format!("Invalid time array in {name}"))?;
        let values = data["state"]
            .as_array()
            .ok_or_else(|| format!("Invalid state array in {name}"))?;
        if times.len() != values.len() {
            return Err(format!("Mismatched arrays in {name}"));
        }
        check_rows(times.len())?;
        let mut points = Vec::with_capacity(times.len());
        for (t, v) in times.iter().zip(values) {
            let time = required_number(t, "wave-state timestamp")?;
            let value = required_number(v, "wave-state population")?;
            if value < 0. {
                return Err("Negative wave-state population".into());
            }
            if points.last().is_some_and(|p: &[f64; 2]| p[0] > time) {
                return Err("Unordered precomputed wave-state timestamps".into());
            }
            points.push([time, value]);
        }
        c.counters.push(Series {
            spm_clock: None,
            definition: None,
            att: None,
            name: format!("Wave state · {label}"),
            unit: "waves".into(),
            scope: format!("SE {se} · precomputed wave states"),
            points,
        });
    }
    if root.join("snapshots.json").exists() {
        fn walk(v: &Value, prefix: &str, out: &mut Vec<(String, String)>) {
            if let Some(s) = v.as_str() {
                out.push((prefix.into(), s.into()));
            }
            if let Some(o) = v.as_object() {
                for (k, v) in o {
                    walk(
                        v,
                        &format!(
                            "{}/{}",
                            prefix.trim_end_matches('/'),
                            k.trim_start_matches('/')
                        ),
                        out,
                    );
                }
            }
        }
        let mut files = vec![];
        walk(&sidecar(root, "snapshots.json")?, "", &mut files);
        if files.len() > 100_000 {
            return Err("Source manifest exceeds 100,000 entries".into());
        }
        for (original, local) in files {
            match safe_path(root, &local).and_then(|path| {
                if !path.is_file() {
                    return Err("Source snapshot is not a regular file".into());
                }
                Ok(())
            }) {
                Ok(()) => {
                    if c.source_paths.contains_key(&original) {
                        return Err(format!("Ambiguous source snapshot path: {original}"));
                    }
                    c.source_files.push(original.clone());
                    c.source_paths.insert(original, PathBuf::from(local));
                }
                Err(e) => c
                    .warnings
                    .push(format!("Source snapshot unavailable: {local}: {e}")),
            }
        }
    }
    if let Some(engines) = names["other_simd_filenames"].as_object() {
        for (se, _) in engines {
            let se = se
                .parse::<u32>()
                .map_err(|_| "Invalid other-SIMD shader engine")?;
            let records = read_other_simd(&c, &names, se)?;
            check_rows(c.other_simd.len().saturating_add(records.len()))?;
            c.other_simd.extend(records);
        }
    }
    if c.waves.is_empty()
        && c.instructions.is_empty()
        && c.intervals.is_empty()
        && c.counters.is_empty()
        && c.decoder_events.is_empty()
        && c.other_simd.is_empty()
    {
        return Err("No supported trace records found in this directory".into());
    }
    let ranges: Vec<(f64, f64)> = c
        .waves
        .iter()
        .map(|w| (w.start, w.end))
        .chain(c.intervals.iter().map(|w| (w.start, w.end)))
        .chain(c.decoder_events.iter().map(|e| (e.time, e.time)))
        .chain(c.other_simd.iter().map(|e| (e.start, e.start + e.duration)))
        .chain(
            c.counters
                .iter()
                .flat_map(|s| s.points.iter().map(|p| (p[0], p[0]))),
        )
        .collect();
    c.start = ranges.iter().map(|x| x.0).reduce(f64::min).unwrap_or(0.);
    c.end = ranges
        .iter()
        .map(|x| x.1)
        .reduce(f64::max)
        .unwrap_or(1.)
        .max(c.start + 1.);
    crate::markers::load(&mut c, &names)?;
    if c.realtime.as_ref().is_some_and(|r| {
        r.anchors
            .iter()
            .map(|a| a.se)
            .collect::<BTreeSet<_>>()
            .len()
            > 1
    }) {
        match crate::se_clock::normalize(&c) {
            Ok(aligned) => c = aligned,
            Err(error) => c
                .warnings
                .push(format!("Cross-SE alignment not applied: {error}")),
        }
    }
    Ok(c)
}
fn occupancy(v: &Value, c: &mut Capture) -> Result<(), String> {
    let mut unmatched = 0;
    for (se, rows) in v.as_object().ok_or("Invalid occupancy JSON")? {
        let Ok(se) = se.parse::<u32>() else {
            continue;
        };
        check_rows(arr(rows).len())?;
        let mut sorted: Vec<_> = arr(rows).iter().collect();
        sorted.sort_by(|a, b| num(&a[0]).total_cmp(&num(&b[0])));
        let mut active: BTreeMap<(u32, u32, u32, String, u32, u32), f64> = BTreeMap::new();
        for row in sorted {
            if arr(row).len() < 6 {
                return Err("Malformed occupancy row".into());
            }
            let key = (
                uint(&row[1]),
                uint(&row[2]),
                uint(&row[3]),
                text(&row[5]),
                uint(&row[6]),
                uint(&row[7]),
            );
            let time = required_number(&row[0], "occupancy time")?;
            if uint(&row[4]) != 0 {
                if active.insert(key, time).is_some() {
                    unmatched += 1;
                }
            } else if let Some(start) = active.remove(&key).or_else(|| {
                if key.4 != 0 || key.5 != 0 {
                    return None;
                }
                let candidates: Vec<_> = active
                    .keys()
                    .filter(|k| k.0 == key.0 && k.1 == key.1 && k.2 == key.2 && k.3 == key.3)
                    .cloned()
                    .collect();
                if candidates.len() == 1 {
                    active.remove(&candidates[0])
                } else {
                    None
                }
            }) {
                c.intervals.push(Interval {
                    start,
                    end: time,
                    se,
                    cu: key.0,
                    simd: key.1,
                    slot: key.2,
                    kernel: v["dispatches"][&key.3]
                        .as_str()
                        .unwrap_or("Unknown kernel")
                        .into(),
                });
            } else {
                unmatched += 1;
            }
        }
        unmatched += active.len();
    }
    if unmatched > 0 {
        c.warnings.push(format!(
            "{unmatched} unmatched occupancy boundaries excluded; capture may be truncated."
        ));
    }
    if let Some(ses) = v["events"].as_object() {
        for (se, events) in ses {
            let shader_engine = se
                .parse::<u32>()
                .map_err(|_| "Invalid decoder event shader engine")?;
            let records = events.as_array().ok_or("Invalid decoder event records")?;
            check_rows(c.decoder_events.len().saturating_add(records.len()))?;
            for (index, e) in records.iter().enumerate() {
                let object = e.as_object().ok_or("Invalid decoder event record")?;
                let kind = e["kind"].as_str().ok_or("Missing decoder event kind")?;
                let time = required_number(&e["time"], "decoder event time")?;
                c.decoder_events.push(DecoderEvent {
                    id: format!("{se}:{index}"),
                    se: shader_engine,
                    time,
                    kind: kind.into(),
                    fields: object
                        .iter()
                        .filter(|(key, _)| key.as_str() != "kind")
                        .map(|(key, value)| (key.clone(), text(value)))
                        .collect(),
                });
                if e["kind"] != "dispatch" {
                    continue;
                }
                c.dispatches.push(Dispatch {
                    shader_engine: Some(shader_engine),
                    id: format!("{se}:{index}"),
                    kernel: text(&e["kernel_name"]),
                    start: num(&e["time"]),
                    end: num(&e["time"]),
                    agent: format!("SE {se}"),
                    queue: format!("ME {} / pipe {}", text(&e["me_id"]), text(&e["pipe_id"])),
                    vgpr: e["vgprs"].as_u64(),
                    sgpr: e["sgprs"].as_u64(),
                    lds: e["lds_size"].as_u64(),
                    scratch: None,
                    grid: "Not recorded".into(),
                    workgroup: format!(
                        "{} × {} × {}",
                        text(&e["thread_dim_x"]),
                        text(&e["thread_dim_y"]),
                        text(&e["thread_dim_z"])
                    ),
                });
            }
        }
    }
    Ok(())
}

pub fn load_wave(c: &Capture, id: &str) -> Result<Wave, String> {
    load_wave_cancellable(c, id, &std::sync::atomic::AtomicBool::new(false))
}
pub fn load_wave_cancellable(
    c: &Capture,
    id: &str,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<Wave, String> {
    check_wave_cancelled(cancelled)?;
    let index = c
        .waves
        .iter()
        .position(|w| w.id == id)
        .ok_or("Wave is not part of this capture")?;
    let mut wave = load_wave_at_cancellable(c, index, cancelled)?;
    crate::waits::reconstruct_capture_cancellable(&mut wave, c, cancelled)?;
    Ok(wave)
}
pub(crate) fn load_wave_at(c: &Capture, index: usize) -> Result<Wave, String> {
    load_wave_at_cancellable(c, index, &std::sync::atomic::AtomicBool::new(false))
}
fn check_wave_cancelled(cancelled: &std::sync::atomic::AtomicBool) -> Result<(), String> {
    if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
        Err("Wave query cancelled".into())
    } else {
        Ok(())
    }
}
#[derive(Debug, serde::Deserialize)]
struct WaveDocument {
    wave: WaveDocumentBody,
}
#[derive(Debug, serde::Deserialize)]
struct WaveDocumentBody {
    #[serde(default, deserialize_with = "deserialize_wave_tokens")]
    instructions: Vec<Token>,
    #[serde(flatten)]
    fields: serde_json::Map<String, Value>,
}
fn deserialize_wave_tokens<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<Token>, D::Error> {
    struct Tokens;
    impl<'de> serde::de::Visitor<'de> for Tokens {
        type Value = Vec<Token>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("an instruction token array or null")
        }
        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(Vec::new())
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> Result<Self::Value, A::Error> {
            use serde::de::Error;
            struct ExcessToken;
            impl<'de> serde::Deserialize<'de> for ExcessToken {
                fn deserialize<D: serde::Deserializer<'de>>(_: D) -> Result<Self, D::Error> {
                    Err(serde::de::Error::custom("Input exceeds two million rows"))
                }
            }
            let mut tokens = Vec::new();
            while tokens.len() < MAX_ROWS {
                let Some(row) = seq.next_element::<Value>()? else {
                    return Ok(tokens);
                };
                tokens.push(parse_wave_token(&row).map_err(A::Error::custom)?);
            }
            seq.next_element::<ExcessToken>()?;
            Ok(tokens)
        }
    }
    deserializer.deserialize_any(Tokens)
}
pub(crate) fn parse_wave_token(row: &Value) -> Result<Token, String> {
    if arr(row).len() < 5 {
        return Err("Malformed instruction token".into());
    }
    let stall = required_number(&row[2], "token stall")?;
    let duration = stall.max(required_number(&row[3], "token duration")?);
    if stall < 0. || duration < 0. {
        return Err("Negative token duration".into());
    }
    Ok(Token {
        start: required_number(&row[0], "token clock")?,
        duration,
        stall,
        category: uint(&row[1]),
        instruction: row[4].as_i64().ok_or("Invalid instruction index")?,
    })
}
fn read_wave_json<T: serde::de::DeserializeOwned>(
    source: impl std::io::Read,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<T, String> {
    check_wave_cancelled(cancelled)?;
    let reader = std::io::BufReader::with_capacity(
        64 * 1024,
        crate::cancellable_read::CancellableRead::with_message(
            source,
            cancelled,
            "Wave query cancelled",
        ),
    );
    let result = serde_json::from_reader(reader).map_err(err);
    check_wave_cancelled(cancelled)?;
    result
}
pub(crate) fn load_wave_at_cancellable(
    c: &Capture,
    index: usize,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<Wave, String> {
    load_wave_using(c, index, cancelled, |file| read_wave_json(file, cancelled))
}
pub(crate) fn load_wave_filtered(
    c: &Capture,
    index: usize,
    cancelled: &std::sync::atomic::AtomicBool,
    mut accept: impl FnMut(&Token) -> Result<bool, String>,
) -> Result<Wave, String> {
    load_wave_using(c, index, cancelled, |file| {
        let mut instructions = Vec::new();
        let fields = crate::wave_stream::read(file, cancelled, |token| {
            check_wave_cancelled(cancelled)?;
            if accept(&token)? {
                instructions.push(token);
            }
            Ok(())
        })?;
        Ok(WaveDocument {
            wave: WaveDocumentBody {
                instructions,
                fields,
            },
        })
    })
}
fn load_wave_using(
    c: &Capture,
    index: usize,
    cancelled: &std::sync::atomic::AtomicBool,
    read: impl FnOnce(fs::File) -> Result<WaveDocument, String>,
) -> Result<Wave, String> {
    check_wave_cancelled(cancelled)?;
    let id = &c
        .waves
        .get(index)
        .ok_or("Wave is not part of this capture")?
        .id;
    let path = safe_path(&c.root, id)?;
    if fs::metadata(&path).map_err(err)?.len() > MAX_FILE {
        return Err(format!(
            "{} exceeds the 256 MiB per-file import limit",
            path.display()
        ));
    }
    let data = read(fs::File::open(&path).map_err(err)?)?;
    let tokens = data.wave.instructions;
    let w = Value::Object(data.wave.fields);
    let start = required_number(&w["begin"], "wave begin")?;
    let end = required_number(&w["end"], "wave end")?;
    if end < start {
        return Err("Invalid wave duration".into());
    }
    let mut states = vec![];
    let mut time = start;
    for row in arr(&w["timeline"]) {
        check_wave_cancelled(cancelled)?;
        let duration = required_number(&row[1], "state duration")?;
        if duration < 0. {
            return Err("Negative state duration".into());
        }
        states.push([time, duration, num(&row[0])]);
        time += duration;
    }
    if !w["waitcnt"].is_null() {
        let waits = w["waitcnt"]
            .as_array()
            .ok_or("Invalid wait dependency list")?;
        check_rows(waits.len())?;
        for wait in waits {
            check_wave_cancelled(cancelled)?;
            if wait.as_array().map(|a| a.len()) != Some(2) || wait[0].as_i64().is_none() {
                return Err("Invalid wait ISA identity".into());
            }
            let sources = wait[1].as_array().ok_or("Invalid wait sources")?;
            check_rows(sources.len())?;
            for source in sources {
                check_wave_cancelled(cancelled)?;
                if source.as_array().map(|a| a.len()) != Some(2)
                    || source[0].as_i64().is_none()
                    || source[1].as_i64().is_none()
                {
                    return Err("Invalid wait source ISA identity".into());
                }
            }
        }
    }
    let mut wave = Wave {
        id: id.into(),
        start,
        end,
        cu: w["cu"].as_u64().map(|x| x as u32),
        tokens,
        states,
        waits: w["waitcnt"].clone(),
        info: w["info"].clone(),
    };
    if let Some(offset) = c.se_time_offsets.get(&c.waves[index].se) {
        wave.start = crate::se_clock::shift(wave.start, *offset)?;
        wave.end = crate::se_clock::shift(wave.end, *offset)?;
        for token in &mut wave.tokens {
            check_wave_cancelled(cancelled)?;
            token.start = crate::se_clock::shift(token.start, *offset)?;
        }
        for state in &mut wave.states {
            check_wave_cancelled(cancelled)?;
            state[0] = crate::se_clock::shift(state[0], *offset)?;
        }
    }
    check_wave_cancelled(cancelled)?;
    Ok(wave)
}

fn load_csv(path: &Path) -> Result<Capture, String> {
    load_csv_cancellable(path, &std::sync::atomic::AtomicBool::new(false))
}
pub(crate) fn load_csv_cancellable(
    path: &Path,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<Capture, String> {
    crate::cancellable_read::check(cancelled)?;
    if fs::metadata(path).map_err(err)?.len() > MAX_FILE {
        return Err("CSV exceeds 256 MiB import limit".into());
    }
    let file = fs::File::open(path).map_err(err)?;
    load_csv_reader(path, file, cancelled)
}
fn load_csv_reader(
    path: &Path,
    source: impl std::io::Read,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<Capture, String> {
    let mut reader = csv::Reader::from_reader(crate::cancellable_read::CancellableRead::new(
        source, cancelled,
    ));
    let header = reader.headers().map_err(err)?.clone();
    let col = |name: &str| {
        header
            .iter()
            .position(|s| s.trim_matches('\u{feff}').trim() == name)
    };
    if col("Sample_Timestamp").is_some() {
        return crate::pc_sampling_csv::load(path, reader, cancelled);
    }
    for name in ["Kernel_Name", "Start_Timestamp", "End_Timestamp"] {
        if col(name).is_none() {
            return Err(format!("Not a rocprofv3 kernel trace CSV: missing {name}"));
        }
    }
    let mut c = Capture::empty(
        path.file_name().unwrap().to_string_lossy().into(),
        "rocprofv3 dispatch CSV",
        "ns",
    );
    let mut rows = vec![];
    let mut origin = u64::MAX;
    for row in reader.records() {
        crate::cancellable_read::check(cancelled)?;
        let row = row.map_err(err)?;
        let get = |name: &str| col(name).and_then(|i| row.get(i)).unwrap_or("").trim();
        let start = get("Start_Timestamp")
            .parse::<u64>()
            .map_err(|_| "Invalid start timestamp")?;
        let end = get("End_Timestamp")
            .parse::<u64>()
            .map_err(|_| "Invalid end timestamp")?;
        if end < start {
            return Err("Dispatch ends before it starts".into());
        }
        origin = origin.min(start);
        rows.push((row, start, end));
        check_rows(rows.len())?;
    }
    if rows.is_empty() {
        return Err("Kernel trace CSV contains no dispatches".into());
    }
    c.origin = origin.to_string();
    let mut seen = BTreeSet::new();
    let mut duplicates = 0;
    for (row, start, end) in rows {
        crate::cancellable_read::check(cancelled)?;
        let get = |name: &str| col(name).and_then(|i| row.get(i)).unwrap_or("").trim();
        let key = (
            get("Process_Id").to_owned(),
            get("Agent_Id").to_owned(),
            get("Queue_Id").to_owned(),
            get("Dispatch_Id").to_owned(),
            start,
            end,
            get("Kernel_Name").to_owned(),
        );
        if !seen.insert(key) {
            duplicates += 1;
            continue;
        }
        let parse = |name: &str| get(name).parse::<u64>().ok();
        let d = Dispatch {
            shader_engine: None,
            id: format!(
                "{}:{}:{}:{}",
                get("Process_Id"),
                get("Agent_Id"),
                get("Queue_Id"),
                get("Dispatch_Id")
            ),
            kernel: get("Kernel_Name").into(),
            start: (start - origin) as f64,
            end: (end - origin) as f64,
            agent: get("Agent_Id").into(),
            queue: get("Queue_Id").into(),
            vgpr: parse("VGPR_Count"),
            sgpr: parse("SGPR_Count"),
            lds: parse("LDS_Block_Size"),
            scratch: parse("Scratch_Size"),
            grid: format!(
                "{} × {} × {}",
                get("Grid_Size_X"),
                get("Grid_Size_Y"),
                get("Grid_Size_Z")
            ),
            workgroup: format!(
                "{} × {} × {}",
                get("Workgroup_Size_X"),
                get("Workgroup_Size_Y"),
                get("Workgroup_Size_Z")
            ),
        };
        c.end = c.end.max(d.end);
        c.dispatches.push(d);
    }
    c.warnings.push("Replay coverage is unknown. Retained PM4 can omit graph-replay dispatches; correlate against HIP/API records before claiming complete kernel shares.".into());
    c.warnings.push("Summed kernel duration can exceed elapsed time when queues overlap. Dispatch CSV cannot provide wave occupancy or ISA hotspots.".into());
    if duplicates > 0 {
        c.warnings.push(format!("Removed {duplicates} exact duplicate dispatch records. Counter passes with different timestamps are not merged."));
    }
    c.metadata
        .insert("coverage".into(), "Unknown — dispatch-only import".into());
    crate::cancellable_read::check(cancelled)?;
    Ok(c)
}
fn spm_id(value: &Value, field: &str, handle: bool) -> Result<Option<u64>, String> {
    if value.is_null() {
        return Ok(None);
    }
    let value = if handle {
        value
            .get("handle")
            .ok_or_else(|| format!("Invalid SPM {field}"))?
    } else {
        value
    };
    value
        .as_u64()
        .map(Some)
        .ok_or_else(|| format!("Invalid SPM {field}"))
}
fn spm_partition(group: &Value, ordinal: usize) -> Result<(String, bool), String> {
    let info = &group["dispatch_data"]["dispatch_info"];
    let dispatch = spm_id(&info["dispatch_id"], "dispatch ID", false)?;
    let mut scope = dispatch
        .map(|id| format!("dispatch {id}"))
        .unwrap_or_else(|| format!("collection {ordinal}"));
    if let Some(queue) = spm_id(&info["queue_id"], "queue ID", true)? {
        scope.push_str(&format!(" · queue {queue}"));
    }
    if let Some(stream) = spm_id(&group["stream_id"], "stream ID", true)? {
        scope.push_str(&format!(" · stream {stream}"));
    }
    Ok((scope, dispatch.is_none()))
}
fn load_spm(path: &Path, sampling_method: Option<&str>) -> Result<Capture, String> {
    let value = json(path)?;
    let tools = arr(&value["rocprofiler-sdk-tool"]);
    let sampling = crate::pc_sampling::present(tools);
    if sampling_method.is_some() && !sampling {
        return Err("Selected PC sampling method is unavailable in this input".into());
    }
    if tools.is_empty() {
        return Err("Unrecognized JSON. Select filenames.json for a decoded thread trace or a rocprofv3 SPM results file.".into());
    }
    let mut c = if sampling {
        crate::pc_sampling::load(path, tools, sampling_method)?
    } else {
        Capture::empty(
            path.file_name().unwrap().to_string_lossy().into(),
            "rocprofv3 SPM JSON",
            "ns",
        )
    };
    let sampling_origin = sampling
        .then(|| c.origin.parse::<u64>())
        .transpose()
        .map_err(err)?;
    let mut raw: BTreeMap<(String, String, String), Vec<(u64, f64)>> = BTreeMap::new();
    let mut origin = sampling_origin.unwrap_or(u64::MAX);
    let mut definitions = BTreeMap::new();
    let mut packed_coordinates = false;
    let mut unassociated_collections = false;
    let mut count = 0;
    for (tool_index, tool) in tools.iter().enumerate() {
        let mut defs = BTreeMap::new();
        for def in optional_array(&tool["counters"], "SPM counter definitions")? {
            let key = (text(&def["agent_id"]["handle"]), text(&def["id"]["handle"]));
            if let Some(previous) = defs.insert(key, def) {
                if previous != def {
                    return Err("Conflicting SPM counter definitions".into());
                }
            }
        }
        let mut dimension_tables = defs
            .iter()
            .map(|(key, definition)| {
                crate::spm_dimensions::instances(definition).map(|table| (key.clone(), table))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        for (ordinal, group) in optional_array(
            &tool["callback_records"]["spm_counter_collection"],
            "SPM collections",
        )?
        .iter()
        .enumerate()
        {
            let agent = spm_id(
                &group["dispatch_data"]["dispatch_info"]["agent_id"],
                "agent ID",
                true,
            )?
            .ok_or("Missing SPM agent ID")?
            .to_string();
            let (partition, unknown_dispatch) = spm_partition(group, ordinal)?;
            unassociated_collections |= unknown_dispatch;
            for record in group["records"]
                .as_array()
                .ok_or("Missing or invalid SPM records array")?
            {
                let id = text(&record["counter_id"]["handle"]);
                let instance_id = record["instance_id"]
                    .as_u64()
                    .ok_or("Invalid SPM sample instance ID")?;
                let instance = instance_id.to_string();
                let def = defs
                    .get(&(agent.clone(), id.clone()))
                    .ok_or("SPM record references unknown counter/agent")?;
                let timestamp = record["timestamp"]
                    .as_u64()
                    .ok_or("Invalid SPM timestamp")?;
                let val = required_number(&record["value"], "SPM counter value")?;
                let table = dimension_tables
                    .get_mut(&(agent.clone(), id.clone()))
                    .ok_or("Missing SPM dimension table")?;
                let dims = if let Some(dimensions) = table.get(&instance_id) {
                    dimensions.clone()
                } else {
                    packed_coordinates = true;
                    let dimensions = crate::spm_dimensions::packed(instance_id)?;
                    table.insert(instance_id, dimensions.clone());
                    dimensions
                };
                let scope = format!(
                    "process {tool_index} · agent {agent} · {partition} · counter {id} · {dims}"
                );
                let key = (text(&def["name"]), scope, instance);
                let flag = |field: &str| -> Result<Option<bool>, String> {
                    match &def[field] {
                        Value::Null => Ok(None),
                        Value::Bool(value) => Ok(Some(*value)),
                        value if value.as_u64() == Some(0) => Ok(Some(false)),
                        value if value.as_u64() == Some(1) => Ok(Some(true)),
                        _ => Err(format!("Invalid SPM counter {field}")),
                    }
                };
                definitions.entry(key.clone()).or_insert((
                    CounterDefinition {
                        id,
                        description: text(&def["description"]),
                        block: text(&def["block"]),
                        expression: text(&def["expression"]),
                        is_derived: flag("is_derived")?,
                        is_constant: flag("is_constant")?,
                    },
                    def["units"]
                        .as_str()
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or("unspecified")
                        .to_owned(),
                ));
                raw.entry(key).or_default().push((timestamp, val));
                origin = origin.min(timestamp);
                count += 1;
                check_rows(count)?;
            }
        }
    }
    if raw.is_empty() {
        if sampling {
            return Ok(c);
        }
        return Err("No SPM records found".into());
    }
    if let Some(previous) = sampling_origin {
        let span = u128::from(previous - origin) + c.end as u128;
        if span > 9_007_199_254_740_991 {
            return Err(
                "Combined SPM/PC sampling span exceeds exact frontend timestamp precision".into(),
            );
        }
        c.end = span as f64;
        c.format = format!(
            "rocprofv3 SPM + {} PC sampling JSON",
            c.metadata["pc_sampling_method"]
        );
        c.metadata
            .insert("pc_sampling_original_origin".into(), previous.to_string());
    }
    c.origin = origin.to_string();
    for (key, mut points) in raw {
        let (definition, unit) = definitions.remove(&key).ok_or("Missing SPM definition")?;
        let (name, scope, _) = key;
        points.sort_by_key(|x| x.0);
        if points.last().unwrap().0 - origin > 9_007_199_254_740_991 {
            return Err("SPM counter span exceeds exact frontend timestamp precision; select a shorter capture window".into());
        }
        let repeated = points
            .windows(2)
            .filter(|pair| pair[0].0 == pair[1].0)
            .count();
        if repeated > 0 {
            c.warnings.push(format!("SPM counter {name} ({scope}) contains {repeated} repeated sample timestamps. Values are retained in recorded order within each timestamp; no reset or clock-wrap interpretation is applied."));
        }
        let points: Vec<_> = points
            .into_iter()
            .map(|(t, v)| [(t - origin) as f64, v])
            .collect();
        c.end = c.end.max(points.last().unwrap()[0]);
        c.counters.push(Series {
            spm_clock: None,
            definition: Some(definition),
            att: None,
            name,
            scope,
            unit,
            points,
        });
    }
    c.warnings.push("SPM is shown on its own timestamp axis. SQ_CYCLES/realtime calibration is required before alignment with SQTT; no automatic clock alignment is applied.".into());
    c.metadata.insert(
        "coverage".into(),
        if sampling { "SPM and PC samples; collection gaps and cross-collection dispatch attribution remain unqualified" } else { "SPM samples only; collection gaps remain unqualified" }.into(),
    );
    if unassociated_collections {
        c.warnings.push("SPM collections without dispatch IDs remain separate collection domains; matching agent, queue or stream does not establish a shared dispatch.".into());
    }
    if packed_coordinates {
        c.warnings.push("Some SPM coordinates were decoded from the SDK packed instance ID because explicit instance dimensions were absent. Nonzero extra dimensions remain separate domains; the SDK layout is an interpretation, not measured topology.".into());
        c.metadata.insert(
            "spmCoordinates".into(),
            "Explicit metadata preferred; SDK packed-ID fallback used".into(),
        );
    }
    Ok(c)
}

fn read_other_simd(capture: &Capture, names: &Value, se: u32) -> Result<Vec<OtherSimd>, String> {
    let mut result = vec![];
    let entries = names["other_simd_filenames"][se.to_string()]
        .as_array()
        .ok_or("Invalid other-SIMD file list")?;
    for entry in entries {
        let name = entry[0].as_str().ok_or("Invalid other-SIMD filename")?;
        let data = sidecar(&capture.root, name)?;
        let (time, duration, category, width) =
            if let Some(schema) = data.get("instructions_schema") {
                let schema = schema
                    .as_array()
                    .ok_or("Invalid other-SIMD instruction schema")?;
                let mut columns = BTreeSet::new();
                if schema.len() > 64
                    || schema.iter().any(|value| {
                        value.as_str().is_none_or(|name| {
                            name.is_empty() || name.len() > 128 || !columns.insert(name)
                        })
                    })
                {
                    return Err("Invalid or duplicate other-SIMD schema columns".into());
                }
                (
                    schema
                        .iter()
                        .position(|s| s == "time")
                        .ok_or("Other-SIMD schema is missing time")?,
                    schema
                        .iter()
                        .position(|s| s == "duration")
                        .ok_or("Other-SIMD schema is missing duration")?,
                    schema.iter().position(|s| s == "category"),
                    schema.len(),
                )
            } else {
                (0, 1, None, 2)
            };
        let optional_u32 = |value: &Value| -> Result<Option<u32>, String> {
            if value.is_null() {
                return Ok(None);
            }
            value
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .map(Some)
                .ok_or_else(|| "Invalid other-SIMD category/WGP identity".into())
        };
        let wgp = optional_u32(&data["wgp"])?;
        let rows = data["instructions"]
            .as_array()
            .ok_or("Invalid other-SIMD instructions")?;
        if let Some(count) = data.get("instructions_count") {
            if count.as_u64() != Some(rows.len() as u64) {
                return Err("Other-SIMD instruction count does not match recorded rows".into());
            }
        }
        check_rows(result.len().saturating_add(rows.len()))?;
        for row in rows {
            if row.as_array().is_none_or(|row| row.len() != width) {
                return Err("Other-SIMD row width does not match its schema".into());
            }
            let start = required_number(&row[time], "other-SIMD time")?;
            let duration = required_number(&row[duration], "other-SIMD duration")?;
            if duration < 0. || !(start + duration).is_finite() {
                return Err("Invalid other-SIMD interval".into());
            }
            result.push(OtherSimd {
                se,
                wgp,
                source: name.into(),
                start,
                duration,
                category: category
                    .map(|index| optional_u32(&row[index]))
                    .transpose()?
                    .flatten(),
            });
        }
    }
    Ok(result)
}

#[cfg(test)]
mod csv_cancellation_tests {
    use super::*;
    use std::{
        io::Read,
        sync::atomic::{AtomicBool, Ordering},
    };

    struct CancelAfterPrefix<'a> {
        prefix: &'a [u8],
        cancelled: &'a AtomicBool,
        reads: usize,
    }
    impl Read for CancelAfterPrefix<'_> {
        fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
            self.reads += 1;
            if self.reads == 2 {
                self.cancelled.store(true, Ordering::Relaxed);
                output[0] = b'x';
                return Ok(1);
            }
            if self.reads > 2 {
                panic!("Parser continued reading after cancellation");
            }
            let n = self.prefix.len().min(output.len());
            output[..n].copy_from_slice(&self.prefix[..n]);
            self.prefix = &self.prefix[n..];
            Ok(n)
        }
    }
    #[test]
    fn csv_parsers_stop_after_partial_input_and_during_quoted_records() {
        for prefix in [
            "Sample_Timestamp,Exec_Mask,Dispatch_Id,Instruction,Instruction_Comment,Correlation_Id\n1,1,1,s_endpgm,,1\n2,1,1,\"partial",
            "Kernel_Name,Start_Timestamp,End_Timestamp\nkernel,1,2\n\"partial",
        ] {
            let flag = AtomicBool::new(false);
            let mut reader = CancelAfterPrefix { prefix: prefix.as_bytes(), cancelled: &flag, reads: 0 };
            let error = load_csv_reader(Path::new("input.csv"), &mut reader, &flag).unwrap_err();
            assert!(error.contains("cancelled"), "{error}");
            assert_eq!(reader.reads, 2);
        }
    }
    #[test]
    fn pre_cancelled_csv_import_never_reads_input() {
        let flag = AtomicBool::new(true);
        let mut reader = CancelAfterPrefix {
            prefix: b"",
            cancelled: &flag,
            reads: 0,
        };
        assert!(load_csv_reader(Path::new("input.csv"), &mut reader, &flag)
            .unwrap_err()
            .contains("cancelled"));
        assert_eq!(reader.reads, 0);
    }
}

#[cfg(test)]
mod wave_read_cancellation_tests {
    use super::read_wave_json;
    use std::{
        cell::Cell,
        io::Read,
        sync::atomic::{AtomicBool, Ordering},
    };
    #[test]
    fn streamed_tokens_keep_order_identity_and_normalization() {
        let source = br#"{"wave":{"begin":0,"end":20,"cu":2,"instructions":[[9,6,4,2,-1,"extension"],[1,3,0,5,9007199254740993]],"timeline":[],"info":{"name":"retained"}},"unknown":[1,2,3]}"#;
        let result: super::WaveDocument =
            read_wave_json(&source[..], &AtomicBool::new(false)).unwrap();
        let tokens = result.wave.instructions;
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].start, 9.0);
        assert_eq!(tokens[0].duration, 4.0);
        assert_eq!(tokens[0].stall, 4.0);
        assert_eq!(tokens[0].instruction, -1);
        assert_eq!(tokens[1].start, 1.0);
        assert_eq!(tokens[1].instruction, 9007199254740993);
        assert_eq!(result.wave.fields["info"]["name"], "retained");
        for instructions in ["[]", "null"] {
            let json = format!(r#"{{"wave":{{"instructions":{instructions}}}}}"#);
            let parsed: super::WaveDocument =
                read_wave_json(json.as_bytes(), &AtomicBool::new(false)).unwrap();
            assert!(parsed.wave.instructions.is_empty());
        }
    }
    #[test]
    fn streamed_tokens_reject_malformed_rows_and_container() {
        for instructions in ["{}", "1", "[[1,2,3]]", "[[0,6,-1,2,7]]", "[[0,6,0,1,1.5]]"] {
            let json = format!(r#"{{"wave":{{"instructions":{instructions}}}}}"#);
            assert!(read_wave_json::<super::WaveDocument>(
                json.as_bytes(),
                &AtomicBool::new(false)
            )
            .is_err());
        }
    }
    #[test]
    #[ignore = "explicit two-million-token allocation boundary qualification"]
    fn streamed_token_limit_accepts_boundary_and_rejects_extra_before_parsing() {
        let prefix = r#"{"wave":{"begin":0,"end":1,"instructions":["#;
        let row = "[0,6,0,1,7]";
        let mut source = String::from(prefix);
        for index in 0..super::MAX_ROWS {
            if index > 0 {
                source.push(',');
            }
            source.push_str(row);
        }
        let rows_end = source.len();
        source.push_str("]}}");
        let parsed: super::WaveDocument =
            read_wave_json(source.as_bytes(), &AtomicBool::new(false)).unwrap();
        assert_eq!(parsed.wave.instructions.len(), super::MAX_ROWS);
        assert!(parsed
            .wave
            .instructions
            .iter()
            .all(|t| t.instruction == 7 && t.duration == 1.0));
        drop(parsed);
        source.truncate(rows_end);
        source.push_str(",[");
        let error =
            read_wave_json::<super::WaveDocument>(source.as_bytes(), &AtomicBool::new(false))
                .unwrap_err();
        assert!(error.contains("Input exceeds two million rows"), "{error}");
        source.truncate(rows_end - row.len());
        source.push_str("[0,6,-1,1,7]]}}");
        let error =
            read_wave_json::<super::WaveDocument>(source.as_bytes(), &AtomicBool::new(false))
                .unwrap_err();
        assert!(error.contains("Negative token duration"), "{error}");
    }
    struct PartialJson<'a> {
        calls: &'a Cell<usize>,
        flag: &'a AtomicBool,
    }
    impl Read for PartialJson<'_> {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            assert!(bytes.len() <= 64 * 1024);
            let count = self.calls.get() + 1;
            self.calls.set(count);
            match count {
                1 => {
                    let prefix = b"{\"wave\":{\"instructions\":[[0,6,0,1,7]]},\"padding\":\"";
                    bytes[..prefix.len()].copy_from_slice(prefix);
                    Ok(prefix.len())
                }
                2 => {
                    bytes.fill(b'x');
                    self.flag.store(true, Ordering::Relaxed);
                    Ok(bytes.len())
                }
                _ => panic!("reader continued after cancellation"),
            }
        }
    }
    #[test]
    fn quoted_json_cancels_during_a_bounded_read_without_continuing() {
        let calls = Cell::new(0);
        let flag = AtomicBool::new(false);
        assert_eq!(
            read_wave_json::<super::WaveDocument>(
                PartialJson {
                    calls: &calls,
                    flag: &flag
                },
                &flag
            )
            .unwrap_err(),
            "Wave query cancelled"
        );
        assert_eq!(calls.get(), 2);
    }
    #[test]
    fn pre_cancelled_json_performs_no_read() {
        let calls = Cell::new(0);
        let flag = AtomicBool::new(true);
        assert_eq!(
            read_wave_json::<super::WaveDocument>(
                PartialJson {
                    calls: &calls,
                    flag: &flag
                },
                &flag
            )
            .unwrap_err(),
            "Wave query cancelled"
        );
        assert_eq!(calls.get(), 0);
    }
}
