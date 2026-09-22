use crate::import::sidecar;
use crate::{load_wave, Capture};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Marker {
    pub id: usize,
    pub parent: Option<usize>,
    pub marker_id: u32,
    pub code_object: String,
    pub name: String,
    pub kind: String,
    pub source: String,
    pub start: f64,
    pub end: Option<f64>,
    pub point: bool,
    pub depth: usize,
    pub se: u32,
    pub cu: u32,
    pub simd: u32,
    pub slot: u32,
}
#[derive(Clone)]
struct Definition {
    name: String,
    kind: String,
    source: String,
}
fn unsigned(v: &serde_json::Value) -> Result<u32, String> {
    v.as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or("Invalid marker integer".into())
}
fn diagnostic(c: &mut Capture, message: String) {
    if c.warnings.len() < 100 {
        c.warnings.push(message);
    }
}
pub fn load(c: &mut Capture, names: &serde_json::Value) -> Result<(), String> {
    let Some(files) = names["shaderdata_filenames"].as_object() else {
        return Ok(());
    };
    if files.is_empty() {
        return Ok(());
    }
    let code = if c.root.join("code.json").exists() {
        sidecar(&c.root, "code.json")?
    } else {
        serde_json::Value::Null
    };
    let mut definitions: BTreeMap<(String, u32), Definition> = BTreeMap::new();
    let mut payloads: BTreeMap<(String, u32), u32> = BTreeMap::new();
    for row in code["sqtt_funcmap"].as_array().into_iter().flatten() {
        let co = row[0]
            .as_u64()
            .ok_or("Invalid marker code object")?
            .to_string();
        let id = unsigned(&row[1])?;
        let definition = Definition {
            name: row[3].as_str().ok_or("Invalid marker name")?.into(),
            kind: row[2].as_str().ok_or("Invalid marker kind")?.into(),
            source: row[4].as_str().unwrap_or("").into(),
        };
        if definitions.insert((co, id), definition).is_some() {
            return Err("Duplicate marker identity within code object".into());
        }
    }
    for row in code["sqtt_funcmap_payloads"]
        .as_array()
        .into_iter()
        .flatten()
    {
        payloads.insert(
            (
                row[0]
                    .as_u64()
                    .ok_or("Invalid payload code object")?
                    .to_string(),
                unsigned(&row[1])?,
            ),
            unsigned(&row[2])?,
        );
    }
    let mut buckets: BTreeMap<(u32, u32, u32, u32), Vec<(f64, u32, u32)>> = BTreeMap::new();
    let mut count = 0;
    for (se, entries) in files {
        let se = se.parse::<u32>().map_err(|_| "Invalid marker SE")?;
        for file in entries.as_array().ok_or("Invalid shaderdata file list")? {
            let data = sidecar(
                &c.root,
                file[0].as_str().ok_or("Invalid shaderdata filename")?,
            )?;
            let schema = data["records_schema"]
                .as_array()
                .ok_or("Missing shaderdata records schema")?;
            let index = |name: &str| {
                schema
                    .iter()
                    .position(|v| v.as_str() == Some(name))
                    .ok_or(format!("Missing shaderdata field {name}"))
            };
            let (time, value, cu, simd, slot, flags) = (
                index("time")?,
                index("value")?,
                index("cu")?,
                index("simd")?,
                index("wave_id")?,
                index("flags")?,
            );
            for row in data["records"]
                .as_array()
                .ok_or("Invalid shaderdata records")?
            {
                count += 1;
                if count > 2_000_000 {
                    return Err("Marker input exceeds two million records".into());
                }
                let t = row[time]
                    .as_f64()
                    .filter(|t| t.is_finite() && *t >= 0.)
                    .ok_or("Invalid marker time")?;
                buckets
                    .entry((
                        se,
                        unsigned(&row[cu])?,
                        unsigned(&row[simd])?,
                        unsigned(&row[slot])?,
                    ))
                    .or_default()
                    .push((t, unsigned(&row[value])?, unsigned(&row[flags])?));
            }
        }
    }
    c.metadata
        .insert("shaderdataRecords".into(), count.to_string());
    if definitions.is_empty() {
        diagnostic(c,format!("{count} shader-data records have no marker funcmap; raw payloads were not interpreted as scopes."));
        return Ok(());
    }
    let by_line: BTreeMap<_, _> = c
        .instructions
        .iter()
        .map(|i| (i.index, i.code_object.clone()))
        .collect();
    for ((se, cu, simd, slot), mut records) in buckets {
        records.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut waves = vec![];
        for index in c
            .waves
            .iter()
            .filter(|w| w.se == se && w.simd == simd && w.slot == slot)
        {
            let wave = load_wave(c, &index.id)?;
            if wave.cu == Some(cu) {
                waves.push(wave);
            }
        }
        let mut stack: Vec<usize> = vec![];
        let mut payload_remaining = 0;
        let mut previous_context = String::new();
        let mut unresolved = 0;
        for (time, value, flags) in records {
            if payload_remaining > 0 {
                payload_remaining -= 1;
                continue;
            }
            if flags & 3 != 0 {
                continue;
            }
            let mut context = None;
            for wave in &waves {
                if time < wave.start || time > wave.end {
                    continue;
                }
                let n = wave
                    .tokens
                    .partition_point(|t| t.start <= time)
                    .saturating_sub(1);
                if let Some(co) = wave.tokens.get(n).and_then(|t| by_line.get(&t.instruction)) {
                    context = Some((co.clone(), wave.id.clone()));
                    break;
                }
            }
            if context.is_none() {
                for interval in c.intervals.iter().filter(|i| {
                    i.se == se
                        && i.cu == cu
                        && i.simd == simd
                        && i.slot == slot
                        && i.start <= time
                        && i.end >= time
                }) {
                    let candidates: Vec<_> = definitions
                        .iter()
                        .filter(|(_, d)| {
                            matches!(d.kind.to_lowercase().as_str(), "k" | "kernel")
                                && d.name == interval.kernel
                        })
                        .collect();
                    if candidates.len() == 1 {
                        context = Some((
                            candidates[0].0 .0.clone(),
                            format!("{}:{}", interval.start, interval.end),
                        ));
                    }
                }
            }
            let Some((co, identity)) = context else {
                unresolved += 1;
                continue;
            };
            if !definitions.keys().any(|(object, _)| object == &co) {
                unresolved += 1;
                continue;
            }
            let context_id = format!("{co}:{identity}");
            if !previous_context.is_empty() && previous_context != context_id && !stack.is_empty() {
                diagnostic(c,"Open marker scopes interrupted by a new wave/code object; scopes remain incomplete.".into());
                stack.clear();
            }
            previous_context = context_id;
            let marker_id = value >> 2;
            let enter = value & 2 != 0;
            let exit = value & 1 != 0;
            if exit {
                if let Some(id) = stack.pop() {
                    c.markers[id].end = Some(time);
                } else {
                    diagnostic(
                        c,
                        format!(
                            "Orphan marker exit at SE {se} CU {cu} SIMD {simd} slot {slot}, {time}"
                        ),
                    );
                }
            }
            if exit && !enter {
                continue;
            }
            let definition = definitions.get(&(co.clone(), marker_id));
            if definition.is_none() {
                diagnostic(c, format!("Unknown marker {marker_id} in code object {co}"));
            }
            let id = c.markers.len();
            let parent = stack.last().copied();
            c.markers.push(Marker {
                id,
                parent,
                marker_id,
                code_object: co.clone(),
                name: definition
                    .map(|d| d.name.clone())
                    .unwrap_or_else(|| format!("Unknown marker {marker_id}")),
                kind: definition
                    .map(|d| d.kind.clone())
                    .unwrap_or_else(|| "unknown".into()),
                source: definition.map(|d| d.source.clone()).unwrap_or_default(),
                start: time,
                end: if enter { None } else { Some(time) },
                point: !enter,
                depth: stack.len(),
                se,
                cu,
                simd,
                slot,
            });
            if enter {
                if stack.len() >= 256 {
                    return Err("Marker nesting exceeds 256 levels".into());
                }
                stack.push(id);
            }
            payload_remaining = *payloads.get(&(co, marker_id)).unwrap_or(&0);
        }
        if unresolved > 0 {
            diagnostic(c,format!("{unresolved} shader-data records lack an unambiguous active code object at SE {se} CU {cu} SIMD {simd} slot {slot}."));
        }
        if !stack.is_empty() {
            diagnostic(c,format!("{} marker scopes remain open at trace end (SE {se} CU {cu} SIMD {simd} slot {slot}).",stack.len()));
        }
    }
    Ok(())
}
