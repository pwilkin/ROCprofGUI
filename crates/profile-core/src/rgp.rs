use crate::model::*;
use std::{fs, path::Path};

const MAX_FILE: u64 = 256 * 1024 * 1024;
const MAGIC: u32 = 0x5030_3042; // "B00P"
const HEADER_BYTES: usize = 56;
const CHUNK_HEADER_BYTES: usize = 16;

// RGP chunk identifiers. RADV emits DERIVED_SPM (128) rather than raw SPM_DB (8)
// on every GFX10+ ASIC, so only the derived form is read here.
const CHUNK_ASIC_INFO: u8 = 0;
const CHUNK_SQTT_DESC: u8 = 1;
const CHUNK_SQTT_DATA: u8 = 2;
const CHUNK_CODE_OBJECT_DATABASE: u8 = 9;
const CHUNK_DERIVED_SPM: u8 = 128;

const MAX_SPM_SAMPLES: usize = 2_000_000;
const MAX_SPM_SERIES: usize = 4096;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn u32_at(d: &[u8], at: usize) -> Result<u32, String> {
    d.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| "RGP chunk is truncated".into())
}

fn u64_at(d: &[u8], at: usize) -> Result<u64, String> {
    d.get(at..at + 8)
        .map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
        .ok_or_else(|| "RGP chunk is truncated".into())
}

fn f64_at(d: &[u8], at: usize) -> Result<f64, String> {
    u64_at(d, at).map(f64::from_bits)
}

/// A fixed-width name field; RGP pads these with NULs and does not guarantee UTF-8.
fn name_at(d: &[u8], at: usize, len: usize) -> String {
    let raw = d.get(at..at + len).unwrap_or_default();
    let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).trim().to_owned()
}

pub struct Chunk {
    pub kind: u8,
    pub offset: usize,
    pub size: usize,
}

pub fn chunks(d: &[u8]) -> Result<Vec<Chunk>, String> {
    if d.len() < HEADER_BYTES {
        return Err("Not an RGP capture: the file is shorter than an RGP header".into());
    }
    if u32_at(d, 0)? != MAGIC {
        return Err("Not an RGP capture: missing B00P signature".into());
    }
    let start = u32_at(d, 16)? as usize;
    if start < HEADER_BYTES || start > d.len() {
        return Err("RGP chunk table starts outside the file".into());
    }
    let mut out = Vec::new();
    let mut at = start;
    while at + CHUNK_HEADER_BYTES <= d.len() {
        let kind = (u32_at(d, at)? & 0xff) as u8;
        let size = u32_at(d, at + 8)? as usize;
        if size < CHUNK_HEADER_BYTES || at + size > d.len() {
            break;
        }
        out.push(Chunk { kind, offset: at, size });
        if out.len() > 4096 {
            return Err("RGP capture declares more than 4,096 chunks".into());
        }
        at += size;
    }
    if out.is_empty() {
        return Err("RGP capture contains no readable chunks".into());
    }
    Ok(out)
}

pub fn version(d: &[u8]) -> Result<(u32, u32), String> {
    Ok((u32_at(d, 4)?, u32_at(d, 8)?))
}

struct Asic {
    name: String,
    device_id: u32,
    shader_clock_hz: u64,
    shader_engines: u32,
    cus_per_engine: u32,
}

/// Field offsets are those observed in RADV-written RGP v1.6 ASIC_INFO chunks;
/// every value is range-checked rather than trusted.
fn asic(d: &[u8], c: &Chunk) -> Result<Asic, String> {
    let a = &d[c.offset..c.offset + c.size];
    if a.len() < 200 {
        return Err("RGP ASIC_INFO chunk is too small".into());
    }
    let shader_engines = u32_at(a, 56)?;
    let cus_per_engine = u32_at(a, 60)?;
    Ok(Asic {
        name: name_at(a, 152, 32),
        device_id: u32_at(a, 40)?,
        shader_clock_hz: u64_at(a, 24)?,
        shader_engines: if (1..=32).contains(&shader_engines) { shader_engines } else { 0 },
        cus_per_engine: if cus_per_engine <= 256 { cus_per_engine } else { 0 },
    })
}

/// A self-describing RGP record: a leading size, a header size, then a name.
struct Record<'a> {
    size: usize,
    name: String,
    fields: [u32; 3],
    body: &'a [u8],
}

fn record<'a>(d: &'a [u8], at: usize) -> Result<Record<'a>, String> {
    let size = u32_at(d, at)? as usize;
    let header = u32_at(d, at + 4)? as usize;
    let name_len = u32_at(d, at + 8)? as usize;
    if size < header || header < 12 || at + size > d.len() || header + name_len > size {
        return Err("RGP record is malformed".into());
    }
    Ok(Record {
        size,
        name: name_at(d, at + header, name_len),
        fields: [
            u32_at(d, at + 12)?,
            u32_at(d, at + 16).unwrap_or(0),
            u32_at(d, at + 20).unwrap_or(0),
        ],
        body: &d[at + header + name_len..at + size],
    })
}

fn unit_name(code: u32) -> &'static str {
    match code {
        1 => "%",
        4 => "bytes",
        5 => "count",
        _ => "unspecified",
    }
}

/// Nanoseconds per tick of the GPU timestamp that SPM samples and SQTT realtime
/// tokens both carry. Measured at 100.003 MHz against CLOCK_CALIBRATION CPU
/// timestamps across captures taken 0.1 s to 80 minutes apart.
const GPU_TIMESTAMP_NS: f64 = 10.0;

/// One derived-SPM series: name, unit, group scope and one value per sample.
struct SpmSeries {
    name: String,
    /// RADV describes each of its own derived counters. A RADV_SPM_COUNTERS_CONFIG
    /// counter is a pass-through of one raw hardware event and has no description.
    description: String,
    unit: &'static str,
    scope: String,
    values: Vec<f64>,
}

impl SpmSeries {
    fn raw(&self) -> bool {
        self.description.is_empty()
    }
}

struct SpmData {
    stamps: Vec<u64>,
    interval: u64,
    series: Vec<SpmSeries>,
}

/// Reads the derived SPM counters RADV writes on GFX10 and later. Sample timestamps
/// are GPU timestamp ticks, not shader clocks.
fn spm(d: &[u8], c: &Chunk) -> Result<SpmData, String> {
    let b = &d[c.offset..c.offset + c.size];
    let samples = u32_at(b, 24)? as usize;
    let groups = u32_at(b, 28)? as usize;
    let counters = u32_at(b, 32)? as usize;
    let components = u32_at(b, 36)? as usize;
    let interval = u32_at(b, 40)? as u64;
    if samples > MAX_SPM_SAMPLES {
        return Err(format!("SPM section exceeds the {MAX_SPM_SAMPLES} sample limit"));
    }
    if counters + components > MAX_SPM_SERIES {
        return Err(format!("SPM section exceeds the {MAX_SPM_SERIES} series limit"));
    }
    let mut at = 44;
    let stamps: Vec<u64> = (0..samples)
        .map(|i| u64_at(b, at + i * 8))
        .collect::<Result<_, _>>()?;
    at += samples * 8;
    let mut group_of = vec![String::new(); counters];
    for _ in 0..groups {
        let r = record(b, at)?;
        for index in r.body.chunks_exact(4) {
            let i = u32::from_le_bytes([index[0], index[1], index[2], index[3]]) as usize;
            if let Some(slot) = group_of.get_mut(i) {
                slot.clone_from(&r.name);
            }
        }
        at += r.size;
    }
    let mut named = Vec::with_capacity(counters);
    for _ in 0..counters {
        let r = record(b, at)?;
        named.push((r.name.clone(), name_at(r.body, 0, r.fields[0] as usize), r.fields[2]));
        at += r.size;
    }
    let values = c
        .size
        .checked_sub((counters + components) * samples * 8)
        .ok_or("SPM value table exceeds its chunk")?;
    let mut series = Vec::with_capacity(counters);
    for (i, (name, description, unit)) in named.into_iter().enumerate() {
        let base = values + i * samples * 8;
        series.push(SpmSeries {
            name,
            description,
            unit: unit_name(unit),
            scope: match group_of[i].as_str() {
                "" => "GPU · SPM".to_owned(),
                group => format!("GPU · {group}"),
            },
            values: (0..samples)
                .map(|s| f64_at(b, base + s * 8))
                .collect::<Result<_, _>>()?,
        });
    }
    Ok(SpmData { stamps, interval, series })
}

/// Places SPM series on `capture`'s time axis. `to_axis` maps a GPU timestamp to that
/// axis and returns None for samples it cannot place, which are dropped and counted.
fn attach_spm(
    capture: &mut Capture,
    data: SpmData,
    to_axis: impl Fn(u64) -> Option<f64>,
) -> usize {
    let placed: Vec<Option<f64>> = data.stamps.iter().map(|s| to_axis(*s)).collect();
    let dropped = placed.iter().filter(|x| x.is_none()).count();
    let raw = data.series.iter().filter(|s| s.raw()).count();
    let silent: Vec<&str> = data
        .series
        .iter()
        .filter(|s| s.raw() && s.values.iter().all(|v| *v == 0.))
        .map(|s| s.name.as_str())
        .collect();
    let notes = [
        (raw > 0).then(|| format!(
            "{raw} SPM counters are raw hardware events from a RADV_SPM_COUNTERS_CONFIG file, sampled every {} clocks and not attributed to a dispatch or wave. RADV sums each across the block instances its config line expands to, which on GFX10 and later need not include every CU: read these as activity within those instances, not GPU totals.",
            data.interval
        )),
        (!silent.is_empty()).then(|| format!(
            "{} read zero in every sample. RADV may not enable counting for these events on this GPU; a zero here is not evidence that the event did not occur.",
            silent.join(", ")
        )),
        (raw < data.series.len()).then(|| format!(
            "SPM counters are sampled every {} clocks and are device-wide; they are not attributed to a dispatch or wave. Byte and hit-rate counters describe memory-side traffic, which write-back caching and the profiler's own trace writes both affect: they are not the workload's requested traffic.",
            data.interval
        )),
    ];
    for s in data.series {
        let points: Vec<[f64; 2]> = placed
            .iter()
            .zip(&s.values)
            .filter_map(|(x, v)| x.map(|x| [x, *v]))
            .collect();
        if let Some(last) = points.last() {
            capture.end = capture.end.max(last[0]);
        }
        capture.counters.push(Series {
            spm_clock: None,
            definition: Some(crate::model::CounterDefinition {
                id: s.name.clone(),
                is_derived: Some(!s.raw()),
                description: s.description,
                block: String::new(),
                expression: String::new(),
                is_constant: None,
            }),
            att: None,
            name: s.name,
            unit: s.unit.into(),
            scope: s.scope,
            points,
        });
    }
    let (first, last) = (data.stamps.first().copied(), data.stamps.last().copied());
    capture
        .metadata
        .insert("spmSamples".into(), data.stamps.len().to_string());
    capture
        .metadata
        .insert("spmSampleInterval".into(), format!("{} clocks", data.interval));
    if let (Some(a), Some(b)) = (first, last) {
        capture.metadata.insert(
            "spmWindow".into(),
            format!("{:.3} ms of GPU timestamp", (b - a) as f64 * GPU_TIMESTAMP_NS / 1e6),
        );
    }
    capture.warnings.extend(notes.into_iter().flatten());
    dropped
}

const CHUNK_CODE_OBJECT_LOADER_EVENTS: u8 = 10;
const CHUNK_PSO_CORRELATION: u8 = 11;

/// A shader ELF from the code-object database, with the load address its RGP loader
/// event recorded. RADV traces carry no in-trace load markers, so this is the only
/// way to place traced PCs.
pub struct PlacedCodeObject {
    pub elf: Vec<u8>,
    pub base: u64,
    pub text_size: u64,
    pub pipeline_hash: Option<[u64; 2]>,
    /// The PSO correlation record for this pipeline: its API hash and name.
    pub api_pso: Option<(u64, String)>,
}

impl PlacedCodeObject {
    /// RADV leaves PSO names empty, so a name comes from the application's debug
    /// labels when it set any (see `pipeline_names`), and otherwise from the API PSO
    /// hash, which RGP also shows and which is stable across runs of one pipeline.
    pub fn label(&self, id: usize, names: &std::collections::HashMap<u64, String>) -> String {
        match &self.api_pso {
            Some((_, name)) if !name.is_empty() => name.clone(),
            Some((hash, _)) => names.get(hash).cloned().unwrap_or_else(|| format!("pipeline {hash:016x}")),
            None => format!("code object {id}"),
        }
    }
}

/// SQ_THREAD_TRACE_USERDATA_2 and _3 as a thread-trace register address.
const USERDATA_2: u64 = 0xC342;
const USERDATA_3: u64 = 0xC343;
/// RGP's API type for vkCmdDispatch and vkCmdDispatchIndirect. Other event markers
/// describe dispatches the driver issues for copies, fills and the like.
const RGP_DISPATCH: [u32; 2] = [6, 7];

/// One hardware dispatch as RADV's RGP instrumentation markers describe it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchMarker {
    pub me: u8,
    pub pipe: u8,
    pub api: u32,
    /// The compute pipeline bound when an application dispatch was recorded.
    pub pipeline: Option<u64>,
    /// The marker's dimensions: workgroup counts for vkCmdDispatch, but thread
    /// counts for the unaligned dispatches RADV issues for its own commands.
    pub dims: Option<[u32; 3]>,
    /// Open debug labels, outermost first.
    pub labels: Vec<String>,
}

/// RADV writes RGP instrumentation markers into the thread trace as register writes
/// to SQ_THREAD_TRACE_USERDATA_2/3, two dwords at a time: debug labels, pipeline
/// binds and one event per hardware dispatch. Returns the dispatches in stream order,
/// and false if a marker could not be sized, after which the rest is not trusted.
pub fn dispatch_markers(body: &[u8]) -> (Vec<DispatchMarker>, bool) {
    let (bits, _) = gfx11_token_bits();
    let nibble = |i: usize| (body[i >> 1] >> ((i & 1) * 4)) & 0xF;
    let total = body.len() * 2;
    let mut streams: std::collections::BTreeMap<(u8, u8), Vec<u32>> = Default::default();
    let mut i = 0;
    while i + 2 <= total {
        let header = (nibble(i) | (nibble(i + 1) << 4)) as usize;
        let len = (bits[header] as usize / 4).max(1);
        if header & 0xF == 0b1001 && i + 16 <= total {
            let v = (0..16).rev().fold(0u64, |v, j| (v << 4) | nibble(i + j) as u64);
            if matches!((v >> 16) & 0xFFFF, USERDATA_2 | USERDATA_3) {
                let queue = (((v >> 9) & 3) as u8, ((v >> 7) & 3) as u8);
                streams.entry(queue).or_default().push((v >> 32) as u32);
            }
        }
        i += len;
    }
    let mut out = vec![];
    for ((me, pipe), dw) in streams {
        let (mut labels, mut bound): (Vec<String>, Option<u64>) = (vec![], None);
        let mut at = 0;
        while at < dw.len() {
            let d0 = dw[at];
            let size = match d0 & 0xF {
                0 if d0 >> 31 == 1 => 6,
                0 | 2 | 0xC => 3,
                1 => 4,
                3 | 4 | 9 => 2,
                5 if (d0 >> 12) & 0xFF == 1 => 1,
                5 => match dw.get(at + 1) {
                    Some(bytes) => 2 + *bytes as usize / 4,
                    None => return (out, false),
                },
                6 => 1,
                _ => return (out, false),
            };
            let Some(m) = dw.get(at..at + size) else { return (out, false) };
            match d0 & 0xF {
                0 => {
                    let api = (d0 >> 7) & 0xFF_FFFF;
                    out.push(DispatchMarker {
                        me,
                        pipe,
                        api,
                        pipeline: bound.filter(|_| RGP_DISPATCH.contains(&api)),
                        dims: (size == 6).then(|| [m[3], m[4], m[5]]),
                        labels: labels.clone(),
                    });
                }
                1 => (labels, bound) = (vec![], None),
                5 => match (d0 >> 12) & 0xFF {
                    1 => drop(labels.pop()),
                    2 => {
                        let bytes: Vec<u8> = m[2..].iter().flat_map(|w| w.to_le_bytes()).collect();
                        labels.push(name_at(&bytes, 0, bytes.len()));
                    }
                    _ => {}
                },
                // Bind point 1 is VK_PIPELINE_BIND_POINT_COMPUTE.
                0xC if (d0 >> 7) & 1 == 1 => bound = Some(m[1] as u64 | (m[2] as u64) << 32),
                _ => {}
            }
            at += size;
        }
    }
    (out, true)
}

/// RGP API types of the commands RADV implements with its own compute shaders.
fn internal_command(api: u32) -> Option<&'static str> {
    Some(match api {
        8 => "vkCmdCopyBuffer",
        9 => "vkCmdCopyImage",
        10 => "vkCmdBlitImage",
        11 => "vkCmdCopyBufferToImage",
        12 => "vkCmdCopyImageToBuffer",
        13 => "vkCmdUpdateBuffer",
        14 => "vkCmdFillBuffer",
        15 => "vkCmdClearColorImage",
        16 => "vkCmdClearDepthStencilImage",
        18 => "vkCmdResolveImage",
        21 => "vkCmdResetQueryPool",
        22 => "vkCmdCopyQueryPoolResults",
        _ => return None,
    })
}

/// Pairs the shader engine's decoded dispatches with RADV's dispatch markers, one per
/// hardware dispatch in the same order, then records each vkCmdDispatch's grid and
/// names the driver's internal shaders after the command they implement. Nothing changes
/// unless the counts and queues match and every application dispatch ran the
/// pipeline its marker says was bound.
fn attach_markers(
    capture: &mut Capture,
    se: u32,
    markers: &[DispatchMarker],
    label: impl Fn(u64) -> Option<String>,
) -> Result<(), String> {
    let decoded: Vec<usize> = (0..capture.dispatches.len())
        .filter(|i| capture.dispatches[*i].shader_engine == Some(se))
        .collect();
    if decoded.len() != markers.len() {
        return Err(format!("{} markers against {} decoded dispatches on SE {se}", markers.len(), decoded.len()));
    }
    let mut renames: std::collections::HashMap<String, Option<String>> = Default::default();
    for (&i, m) in decoded.iter().zip(markers) {
        let d = &capture.dispatches[i];
        if d.queue != format!("ME {} / pipe {}", m.me, m.pipe) {
            return Err(format!("dispatch {} ran on {} but its marker on ME {} / pipe {}", d.id, d.queue, m.me, m.pipe));
        }
        if let Some(expected) = m.pipeline.and_then(&label) {
            if d.kernel != expected {
                return Err(format!("dispatch {} ran {} but its marker bound {expected}", d.id, d.kernel));
            }
        }
        if let Some(command) = internal_command(m.api).filter(|_| d.kernel.starts_with("pipeline ")) {
            let name = format!("{command} (RADV internal)");
            let entry = renames.entry(d.kernel.clone()).or_insert_with(|| Some(name.clone()));
            if entry.as_deref() != Some(name.as_str()) {
                *entry = None;
            }
        }
    }
    for (&i, m) in decoded.iter().zip(markers) {
        let d = &mut capture.dispatches[i];
        let workgroup: Option<Vec<u64>> = d.workgroup.split(" × ").map(|n| n.parse().ok()).collect();
        let groups = m.dims.filter(|_| m.api == RGP_DISPATCH[0]);
        if let (Some(groups), Some(workgroup)) = (groups, workgroup.filter(|w| w.len() == 3)) {
            d.grid = format!("{} × {} × {}", groups[0] as u64 * workgroup[0], groups[1] as u64 * workgroup[1], groups[2] as u64 * workgroup[2]);
        }
    }
    let renames: std::collections::HashMap<String, String> =
        renames.into_iter().filter_map(|(from, to)| Some((from, to?))).collect();
    for d in &mut capture.dispatches {
        if let Some(name) = renames.get(&d.kernel) {
            d.kernel.clone_from(name);
        }
    }
    for interval in &mut capture.intervals {
        if let Some(name) = renames.get(&interval.kernel) {
            interval.kernel.clone_from(name);
        }
    }
    Ok(())
}

/// Names each compute pipeline after the innermost debug label open at its
/// dispatches. Labels often end in that dispatch's workgroup counts, as ggml's
/// `name (x,y,z)` does; that suffix describes one dispatch, not the pipeline.
pub fn pipeline_names(markers: &[DispatchMarker]) -> std::collections::HashMap<u64, String> {
    let stem = |label: &str| -> String {
        match label.strip_suffix(')').and_then(|l| l.rsplit_once(" (")) {
            Some((head, dims)) if !dims.is_empty() && dims.chars().all(|c| c.is_ascii_digit() || c == ',' || c == ' ') => {
                head.trim_end().to_owned()
            }
            _ => label.to_owned(),
        }
    };
    let mut seen: std::collections::HashMap<u64, Vec<(String, usize)>> = Default::default();
    for m in markers {
        let (Some(pipeline), Some(label)) = (m.pipeline, m.labels.last()) else { continue };
        let stems = seen.entry(pipeline).or_default();
        let s = stem(label);
        match stems.iter_mut().find(|(known, _)| *known == s) {
            Some((_, n)) => *n += 1,
            None => stems.push((s, 1)),
        }
    }
    seen.into_iter()
        .filter_map(|(pipeline, mut stems)| {
            stems.retain(|(s, _)| !s.is_empty());
            stems.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            let more = stems.len() > 3;
            let mut name = stems.iter().take(3).map(|(s, _)| s.as_str()).collect::<Vec<_>>().join(" / ");
            if more {
                name.push_str(" / …");
            }
            (!name.is_empty()).then_some((pipeline, name))
        })
        .collect()
}

fn read_code_objects(d: &[u8], c: &Chunk) -> Result<Vec<Vec<u8>>, String> {
    let count = u32_at(d, c.offset + 28)? as usize;
    let end = c.offset + c.size;
    let mut at = c.offset + 32;
    let mut out = Vec::with_capacity(count.min(4096));
    for _ in 0..count {
        let size = u32_at(d, at)? as usize;
        let body = d
            .get(at + 4..at + 4 + size)
            .filter(|_| at + 4 + size <= end)
            .ok_or("RGP code object record exceeds its chunk")?;
        out.push(body.to_vec());
        at = (at + 4 + size + 3) & !3;
    }
    Ok(out)
}

fn read_loader_events(d: &[u8], c: &Chunk) -> Result<Vec<(u64, [u64; 2])>, String> {
    let record = u32_at(d, c.offset + 24)? as usize;
    let count = u32_at(d, c.offset + 28)? as usize;
    if record < 40 || c.offset + 32 + record * count > c.offset + c.size {
        return Err("RGP loader events are malformed".into());
    }
    (0..count)
        .map(|i| {
            let at = c.offset + 32 + i * record;
            Ok((u64_at(d, at + 8)?, [u64_at(d, at + 16)?, u64_at(d, at + 24)?]))
        })
        .collect()
}

/// PSO correlation records: API PSO hash, internal pipeline hash and name.
fn read_pipeline_correlation(d: &[u8], c: &Chunk) -> Result<Vec<(u64, [u64; 2], String)>, String> {
    let record = u32_at(d, c.offset + 24)? as usize;
    let count = u32_at(d, c.offset + 28)? as usize;
    if record < 88 || c.offset + 32 + record * count > c.offset + c.size {
        return Err("RGP pipeline correlation records are malformed".into());
    }
    (0..count)
        .map(|i| {
            let at = c.offset + 32 + i * record;
            Ok((u64_at(d, at)?, [u64_at(d, at + 8)?, u64_at(d, at + 16)?], name_at(d, at + 24, 64)))
        })
        .collect()
}

fn elf_text_size(elf: &[u8]) -> Option<u64> {
    let u16_ = |at: usize| elf.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize);
    let shoff = u64_at(elf, 0x28).ok()? as usize;
    let (entsize, count, names) = (u16_(0x3A)?, u16_(0x3C)?, u16_(0x3E)?);
    let header = |i: usize| shoff + i * entsize;
    let strtab = u64_at(elf, header(names) + 24).ok()? as usize;
    (0..count).find_map(|i| {
        let name = u32_at(elf, header(i)).ok()? as usize;
        (elf.get(strtab + name..strtab + name + 6)? == b".text\0")
            .then(|| u64_at(elf, header(i) + 32).ok())
            .flatten()
    })
}

/// `.internal_pipeline_hash` from the PAL msgpack note: a fixarray of two big-endian
/// uint64 values. It equals the loader event's code-object hash.
fn elf_pipeline_hash(elf: &[u8]) -> Option<[u64; 2]> {
    let key = b".internal_pipeline_hash";
    let at = elf.windows(key.len()).position(|w| w == key)? + key.len();
    let v = elf.get(at..at + 19)?;
    (v[0] == 0x92 && v[1] == 0xcf && v[10] == 0xcf).then(|| {
        [
            u64::from_be_bytes(v[2..10].try_into().unwrap()),
            u64::from_be_bytes(v[11..19].try_into().unwrap()),
        ]
    })
}

pub fn placed_code_objects(d: &[u8], table: &[Chunk]) -> Result<Vec<PlacedCodeObject>, String> {
    let Some(db) = table.iter().find(|c| c.kind == CHUNK_CODE_OBJECT_DATABASE) else {
        return Ok(vec![]);
    };
    let elves = read_code_objects(d, db)?;
    let loads = match table.iter().find(|c| c.kind == CHUNK_CODE_OBJECT_LOADER_EVENTS) {
        Some(c) => read_loader_events(d, c)?,
        None => vec![],
    };
    let pipelines = match table.iter().find(|c| c.kind == CHUNK_PSO_CORRELATION) {
        Some(c) => read_pipeline_correlation(d, c)?,
        None => vec![],
    };
    let mut out = Vec::with_capacity(elves.len());
    for (i, elf) in elves.into_iter().enumerate() {
        let hash = elf_pipeline_hash(&elf);
        let base = loads
            .iter()
            .find(|(_, h)| Some(*h) == hash)
            .or_else(|| (loads.len() == 1).then(|| &loads[0]).filter(|_| i == 0))
            .map(|(base, _)| *base)
            .ok_or("A RADV code object has no loader event recording where it was placed")?;
        let text_size = elf_text_size(&elf).unwrap_or(elf.len() as u64);
        let api_pso = pipelines
            .iter()
            .find(|(_, internal, _)| Some(*internal) == hash)
            .map(|(api, _, name)| (*api, name.clone()));
        out.push(PlacedCodeObject { elf, base, text_size, pipeline_hash: hash, api_pso });
    }
    Ok(out)
}

const EXEC_TOKEN_BITS: u8 = 24;

/// gfx11 SQTT token lengths in bits, indexed by the low 8 header bits, mirroring
/// rocprof-trace-decoder's gfx10 table plus its gfx11 overrides. Two extra 24-bit
/// families (headers `1110` and `x111`) are ALU/VMEM execution tokens that RADV
/// enables and the decoder has no encoding for. Zero marks an unknown header.
fn gfx11_token_bits() -> ([u8; 256], [bool; 256]) {
    let mut bits = [0u8; 256];
    let mut exec = [false; 256];
    let mut add = |pattern: usize, len: u32, token: u8, is_exec: bool| {
        let mut i = pattern;
        while i < 256 {
            bits[i] = token;
            exec[i] = is_exec;
            i += 1 << len;
        }
    };
    for (pattern, len, token) in [
        (0b010, 3, 20), (0b011, 3, 12), (0b1101, 4, 12), (0b00100, 5, 24), (0b10100, 5, 24),
        (0b0100001, 7, 64), (0b01100, 5, 32), (0b11100, 5, 48), (0b00101, 5, 20), (0b10101, 5, 20),
        (0b00110, 5, 52), (0b10110, 5, 28), (0b0110001, 7, 64), (0b1000, 4, 8), (0b0000, 4, 4),
        (0b1010001, 7, 24), (0b01100001, 8, 24), (0b11100001, 8, 32), (0b1001, 4, 64),
        (0b1110001, 7, 64), (0b0000001, 7, 64), (0b0010001, 7, 64),
        // gfx11 overrides
        (0b1010001, 7, 24), (0b0110001, 7, 48), (0b0000001, 7, 48),
    ] {
        add(pattern, len, token, false);
    }
    add(0b1110, 4, EXEC_TOKEN_BITS, true);
    add(0b111, 3, EXEC_TOKEN_BITS, true);
    (bits, exec)
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Sanitized {
    pub blanked: usize,
    pub unknown: usize,
}

/// Overwrites RADV's execution tokens with NOP nibbles so rocprof-trace-decoder parses
/// the stream in sync. A NOP is any nibble whose value is zero and carries no time,
/// so stream length, alignment and timing are unchanged. Unknown headers are counted,
/// not guessed at: a stream that still contains any cannot be decoded reliably.
pub fn sanitize_sqtt(buf: &mut [u8]) -> Sanitized {
    let (bits, exec) = gfx11_token_bits();
    let nibble = |b: &[u8], i: usize| (b[i >> 1] >> ((i & 1) * 4)) & 0xF;
    let total = buf.len() * 2;
    let mut out = Sanitized::default();
    let mut i = 0;
    while i + 2 <= total {
        let header = (nibble(buf, i) | (nibble(buf, i + 1) << 4)) as usize;
        let len = bits[header] as usize / 4;
        if len == 0 {
            out.unknown += 1;
            i += 1;
            continue;
        }
        if exec[header] && i + len <= total {
            for j in i..i + len {
                buf[j >> 1] &= if j & 1 == 1 { 0x0F } else { 0xF0 };
            }
            out.blanked += 1;
        }
        i += len;
    }
    out
}

/// Counts shader-clock discontinuities against the decoder's paired realtime clock.
/// A decode that stayed in sync advances both clocks in a steady ratio; a jump in the
/// shader clock that the realtime clock does not share means tokens were misparsed.
pub fn realtime_anomalies(realtime: &serde_json::Value) -> Vec<(String, usize, usize)> {
    let mut out = vec![];
    let Some(map) = realtime.as_object() else { return out };
    for (key, pairs) in map.iter().filter(|(k, _)| k.starts_with("SE")) {
        let points: Vec<(f64, f64)> = pairs
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|p| Some((p.get(0)?.as_f64()?, p.get(1)?.as_f64()?)))
                    .collect()
            })
            .unwrap_or_default();
        let mut ratios: Vec<f64> = points
            .windows(2)
            .filter(|w| w[1].1 > w[0].1)
            .map(|w| (w[1].0 - w[0].0) / (w[1].1 - w[0].1))
            .filter(|r| *r > 0. && *r < 1e4)
            .collect();
        let mut cadence: Vec<f64> = points
            .windows(2)
            .map(|w| w[1].1 - w[0].1)
            .filter(|d| *d > 0.)
            .collect();
        if ratios.is_empty() || cadence.is_empty() {
            out.push((key.clone(), 0, points.len()));
            continue;
        }
        ratios.sort_by(f64::total_cmp);
        cadence.sort_by(f64::total_cmp);
        let (median, step) = (ratios[ratios.len() / 2], cadence[cadence.len() / 2]);
        // Realtime tokens never go backwards, and the shader clock can neither run far
        // above its median nor stop: RADV inhibits clock gating while tracing. Tokens
        // pause while the GPU idles, so a long interval is legitimate on its own; one
        // across which the shader clock nearly stopped is a misparsed realtime value.
        let anomalies = points
            .windows(2)
            .filter(|w| {
                let (dg, dr) = (w[1].0 - w[0].0, w[1].1 - w[0].1);
                dr <= 0.
                    || dg < 0.
                    || dg > 4. * dr * median + 1000.
                    || (dr > 8. * step && dg * 64. < dr * median)
            })
            .count();
        out.push((key.clone(), anomalies, points.len()));
    }
    out
}

const TIMESTAMP_BITS: u32 = 36;

/// Maps a 64-bit GPU timestamp onto SE 0's decoded shader-clock axis using the
/// decoder's realtime pairs, whose realtime values are the same timestamp truncated
/// to 36 bits. Samples outside the paired window are not extrapolated.
fn realtime_axis(realtime: &serde_json::Value, reference: u64) -> Option<impl Fn(u64) -> Option<f64>> {
    let mask = (1u64 << TIMESTAMP_BITS) - 1;
    let half = 1u64 << (TIMESTAMP_BITS - 1);
    let mut pairs: Vec<(u64, f64)> = realtime
        .get("SE0")?
        .as_array()?
        .iter()
        .filter_map(|p| Some((p.get(1)?.as_u64()?, p.get(0)?.as_f64()?)))
        .map(|(rt, gfx)| {
            let mut full = (reference & !mask) | (rt & mask);
            if full + half < reference {
                full += 1 << TIMESTAMP_BITS;
            } else if full > reference + half {
                full -= 1 << TIMESTAMP_BITS;
            }
            (full, gfx)
        })
        .collect();
    pairs.sort_by_key(|p| p.0);
    pairs.dedup_by_key(|p| p.0);
    (pairs.len() >= 2).then(move || {
        move |stamp: u64| {
            let i = pairs.partition_point(|p| p.0 <= stamp);
            if i == 0 || i == pairs.len() && pairs[i - 1].0 != stamp {
                return None;
            }
            let (r0, g0) = pairs[i - 1];
            let Some(&(r1, g1)) = pairs.get(i) else { return Some(g0) };
            Some(g0 + (stamp - r0) as f64 * (g1 - g0) / (r1 - r0) as f64)
        }
    })
}

/// Decoded wave and occupancy timestamps that fall outside their shader engine's
/// realtime window. Every wave lives inside the trace, so any such timestamp means a
/// desync that per-wave invariants alone cannot catch: a misparsed span is absorbed
/// into one oversized state and still sums correctly.
pub fn timestamps_outside_window(
    realtime: &serde_json::Value,
    filenames: &serde_json::Value,
    occupancy: &serde_json::Value,
) -> Vec<(String, usize, usize)> {
    fn collect(v: &serde_json::Value, out: &mut Vec<f64>) {
        match v {
            serde_json::Value::Array(a) if a.len() == 3 && a[0].is_string() => {
                out.extend(a[1..].iter().filter_map(serde_json::Value::as_f64));
            }
            serde_json::Value::Array(a) => a.iter().for_each(|x| collect(x, out)),
            serde_json::Value::Object(m) => m.values().for_each(|x| collect(x, out)),
            _ => {}
        }
    }
    let mut out = vec![];
    let Some(map) = realtime.as_object() else { return out };
    for (key, pairs) in map.iter().filter(|(k, _)| k.starts_with("SE")) {
        let se = &key[2..];
        let mut gfx: Vec<f64> = pairs
            .as_array()
            .map(|a| a.iter().filter_map(|p| p.get(0)?.as_f64()).collect())
            .unwrap_or_default();
        if gfx.len() < 2 {
            continue;
        }
        gfx.sort_by(f64::total_cmp);
        let mut steps: Vec<f64> = gfx.windows(2).map(|w| w[1] - w[0]).collect();
        steps.sort_by(f64::total_cmp);
        let slack = 3. * steps[steps.len() / 2];
        let (lo, hi) = (gfx[0] - slack, gfx[gfx.len() - 1] + slack);
        let mut times = vec![];
        if let Some(waves) = filenames.get("wave_filenames").and_then(|w| w.get(se)) {
            collect(waves, &mut times);
        }
        if let Some(events) = occupancy.get(se).and_then(|e| e.as_array()) {
            times.extend(events.iter().filter_map(|e| e.get(0)?.as_f64()));
        }
        let outside = times.iter().filter(|t| **t < lo || **t > hi).count();
        out.push((key.clone(), outside, times.len()));
    }
    out
}

/// Per-shader-engine SQTT payloads. An SQTT_DATA offset is from the start of the file.
pub fn sqtt_sections(d: &[u8], table: &[Chunk]) -> Result<Vec<(u32, Vec<u8>)>, String> {
    let mut sections = vec![];
    let mut se = None;
    for c in table {
        if c.kind == CHUNK_SQTT_DESC {
            se = Some(u32_at(d, c.offset + 16)?);
        } else if c.kind == CHUNK_SQTT_DATA {
            let (offset, size) = (u32_at(d, c.offset + 16)? as usize, u32_at(d, c.offset + 20)? as usize);
            let body = d
                .get(offset..offset + size)
                .ok_or("An SQTT data section points outside the file")?;
            sections.push((se.ok_or("SQTT data precedes its descriptor")?, body.to_vec()));
        }
    }
    Ok(sections)
}

const CAPTURE_ADVICE: &str = "For per-instruction data, capture with RADV_THREAD_TRACE_ROCPROF_COMPAT=true on a RADV built with docs/radv-rocprof-compat.patch; for occupancy that is exact at any scale, capture with RADV_THREAD_TRACE_INSTRUCTION_TIMING=false.";

/// What a decoded RADV thread trace can support. The token mask is not recorded in
/// the RGP file, so the mode is inferred from what the stream held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceMode {
    /// No execution tokens, and instructions were attributed: the stream carries
    /// rocprof-trace-decoder's token set, so per-instruction data is complete.
    InstructionTiming,
    /// RADV's default instruction-timing mask; its execution tokens were removed.
    ExecutionTokensRemoved,
    /// Wave lifetimes only.
    Occupancy,
}

impl TraceMode {
    pub fn label(self) -> &'static str {
        match self {
            TraceMode::InstructionTiming => "instruction timing",
            TraceMode::ExecutionTokensRemoved => "instruction timing, execution tokens removed",
            TraceMode::Occupancy => "occupancy",
        }
    }
}

pub fn trace_mode(blanked: usize, decoded: &Capture) -> TraceMode {
    if blanked > 0 {
        TraceMode::ExecutionTokensRemoved
    } else if decoded.instructions.iter().any(|i| i.hits > 0) {
        TraceMode::InstructionTiming
    } else {
        TraceMode::Occupancy
    }
}

/// Decodes a capture's thread trace. Ok(None) means the capture holds no thread
/// trace; Err carries the reason a trace that is present could not be trusted.
fn thread_trace(
    d: &[u8],
    table: &[Chunk],
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<Option<(Capture, serde_json::Value, Sanitized)>, String> {
    let mut sections = sqtt_sections(d, table)?;
    if sections.is_empty() {
        return Ok(None);
    }
    let mut total = Sanitized::default();
    for (_, body) in &mut sections {
        let s = sanitize_sqtt(body);
        total.blanked += s.blanked;
        total.unknown += s.unknown;
    }
    if total.unknown > 0 {
        return Err(format!(
            "the stream contains {} token headers that neither rocprof-trace-decoder nor this importer can size, so none of it can be decoded reliably. This occurs on RADV captures taken with the default instruction-timing token mask. {}",
            total.unknown,
            CAPTURE_ADVICE.trim_end_matches('.')
        ));
    }
    let objects = placed_code_objects(d, table)?;
    // Every shader engine records the same marker stream; the first names pipelines.
    let marker_sets: Vec<(u32, (Vec<DispatchMarker>, bool))> =
        sections.iter().map(|(se, body)| (*se, dispatch_markers(body))).collect();
    let markers = marker_sets.first().map(|(_, (m, _))| m.clone()).unwrap_or_default();
    let complete = marker_sets.iter().all(|(_, (_, complete))| *complete);
    let names = pipeline_names(&markers);
    let workspace = tempfile::tempdir().map_err(err)?;
    let input = workspace.path().join("input");
    fs::create_dir(&input).map_err(err)?;
    let mut traces = vec![];
    for (se, body) in &sections {
        let path = input.join(format!("radv_shader_engine_{se}_0.att"));
        fs::write(&path, body).map_err(err)?;
        traces.push(serde_json::json!({"shaderEngine": se, "path": path}));
    }
    let mut code_objects = vec![];
    for (i, o) in objects.iter().enumerate() {
        let path = input.join(format!("radv_code_object_id_{}.out", i + 1));
        fs::write(&path, &o.elf).map_err(err)?;
        code_objects.push(serde_json::json!({"id": i + 1, "path": path, "base": o.base, "size": o.text_size, "name": o.label(i + 1, &names)}));
    }
    fs::write(
        input.join("radv-manifest.json"),
        serde_json::to_vec(&serde_json::json!({"traces": traces, "codeObjects": code_objects})).map_err(err)?,
    )
    .map_err(err)?;
    let run = crate::decoder::run_decoder_in(
        &workspace,
        &input,
        &crate::decoder::DecoderConfig::default(),
        cancelled,
    )?;
    if let Some(lost) = crate::decoder::decoder_warnings(&run.log)
        .into_iter()
        .find(|w| w.contains("Data Lost"))
    {
        return Err(format!("the decoder reports lost trace data ({lost})"));
    }
    let realtime: serde_json::Value = fs::read(run.root.join("realtime.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .ok_or("the decoder produced no realtime clock pairs to validate the trace against")?;
    for (se, anomalies, pairs) in realtime_anomalies(&realtime) {
        if pairs < 2 {
            return Err(format!("{se} has too few realtime clock pairs ({pairs}) to validate its timing"));
        }
        if anomalies > 0 {
            return Err(format!(
                "the decoded shader clock on {se} jumps {anomalies} times against the realtime clock, which means tokens were misparsed"
            ));
        }
    }
    let read = |name: &str| -> serde_json::Value {
        fs::read(run.root.join(name))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or(serde_json::Value::Null)
    };
    for (se, outside, total) in timestamps_outside_window(&realtime, &read("filenames.json"), &read("occupancy.json")) {
        if outside > 0 {
            return Err(format!(
                "{outside} of {total} decoded wave and occupancy timestamps on {se} lie outside that shader engine's own trace window, which means tokens were misparsed"
            ));
        }
    }
    let mut decoded = crate::load_capture_with_wave_states(&run.root, true)?;
    decoded.metadata.insert("rgpDispatchMarkers".into(), markers.len().to_string());
    if !names.is_empty() {
        decoded.metadata.insert(
            "kernelNames".into(),
            format!("{} of {} pipelines named from the application's debug labels, recorded as RGP markers in the thread trace", names.len(), objects.len()),
        );
    }
    if !complete {
        decoded.warnings.push(
            "The thread trace's RGP instrumentation markers could not all be read, so some kernels may be named by pipeline hash only.".into(),
        );
    }
    let labelled = |hash: u64| {
        let object = objects.iter().enumerate().find(|(_, o)| o.api_pso.as_ref().is_some_and(|(h, _)| *h == hash));
        object.map(|(i, o)| o.label(i + 1, &names))
    };
    for (se, (markers, _)) in marker_sets.iter().filter(|(_, (_, complete))| *complete) {
        if let Err(reason) = attach_markers(&mut decoded, *se, markers, &labelled) {
            decoded.warnings.push(format!(
                "RGP dispatch markers were not matched to decoded dispatches on SE {se} ({reason}), so its dispatch grids are not recorded and driver-internal dispatches may keep pipeline-hash names."
            ));
        }
    }
    decoded.workspace = Some(std::sync::Arc::new(workspace));
    Ok(Some((decoded, realtime, total)))
}

pub fn load(path: &Path) -> Result<Capture, String> {
    load_cancellable(path, &std::sync::atomic::AtomicBool::new(false))
}

pub fn load_cancellable(path: &Path, cancelled: &std::sync::atomic::AtomicBool) -> Result<Capture, String> {
    load_options(path, cancelled, true)
}

/// `decode_thread_trace` false imports counters and identity only, which keeps the
/// result independent of whether the ROCm trace decoder is installed.
pub fn load_options(
    path: &Path,
    cancelled: &std::sync::atomic::AtomicBool,
    decode_thread_trace: bool,
) -> Result<Capture, String> {
    let size = fs::metadata(path).map_err(err)?.len();
    if size > MAX_FILE {
        return Err(format!("{} exceeds the 256 MiB per-file import limit", path.display()));
    }
    let d = fs::read(path).map_err(err)?;
    let table = chunks(&d)?;
    let (major, minor) = version(&d)?;
    let name: String = path.file_name().unwrap_or_default().to_string_lossy().into();

    let mut info = std::collections::BTreeMap::new();
    info.insert("rgpVersion".to_owned(), format!("{major}.{minor}"));
    info.insert(
        "collection".to_owned(),
        "Mesa RADV Vulkan; counters are derived SPM values reported by the driver, not raw hardware event counts".to_owned(),
    );
    let mut asic_name = None;
    if let Some(c) = table.iter().find(|c| c.kind == CHUNK_ASIC_INFO) {
        let a = asic(&d, c)?;
        info.insert("asic".into(), a.name.clone());
        info.insert("deviceId".into(), format!("{:#06x}", a.device_id));
        info.insert(
            "asicReportedShaderClock".into(),
            format!("{} MHz as reported by the driver; the clock during capture can differ", a.shader_clock_hz / 1_000_000),
        );
        if a.shader_engines > 0 {
            info.insert(
                "topology".into(),
                format!("{} shader engines × {} CUs", a.shader_engines, a.cus_per_engine),
            );
        }
        asic_name = Some(a.name);
    }
    if let Some(c) = table.iter().find(|c| c.kind == CHUNK_PSO_CORRELATION) {
        let names: Vec<String> =
            read_pipeline_correlation(&d, c)?.into_iter().map(|(_, _, n)| n).filter(|n| !n.is_empty()).collect();
        if !names.is_empty() {
            info.insert("pipelines".into(), names.join(", "));
        }
    }
    let spm_data = table
        .iter()
        .find(|c| c.kind == CHUNK_DERIVED_SPM)
        .map(|c| spm(&d, c))
        .transpose()?;

    let decoded = if decode_thread_trace { thread_trace(&d, &table, cancelled) } else { Ok(None) };
    match decoded {
        Ok(Some((mut capture, realtime, sanitized))) => {
            capture.name = name;
            capture.format = "RADV RGP · thread trace".into();
            capture.metadata.extend(info);
            let mode = trace_mode(sanitized.blanked, &capture);
            capture.metadata.insert("radvTraceMode".into(), mode.label().into());
            match mode {
                TraceMode::ExecutionTokensRemoved => capture.warnings.push(format!(
                    "Captured with RADV's default instruction-timing token mask. RADV reports instruction execution through {} ALU/VMEM execution tokens that rocprof-trace-decoder cannot interpret; they were removed before decoding. Wave lifetimes, states and timing are complete, but per-instruction hit counts, latencies and VALU activity are mostly missing and must not be used for hotspot analysis. {CAPTURE_ADVICE}",
                    sanitized.blanked
                )),
                TraceMode::Occupancy => capture.warnings.push(
                    "This trace holds wave lifetimes and occupancy only, with no per-instruction data.".into(),
                ),
                TraceMode::InstructionTiming => {}
            }
            if let Some(data) = spm_data {
                let reference = data.stamps.first().copied().unwrap_or(0);
                match realtime_axis(&realtime, reference) {
                    Some(axis) => {
                        let dropped = attach_spm(&mut capture, data, axis);
                        if dropped > 0 {
                            capture.warnings.push(format!(
                                "{dropped} SPM samples fall outside the thread trace's realtime window and were not placed."
                            ));
                        }
                    }
                    None => capture.warnings.push(
                        "SPM counters were present but could not be aligned to the thread trace clock and were not imported.".into(),
                    ),
                }
            }
            Ok(capture)
        }
        other => {
            let mut capture = Capture::empty(name, "RADV RGP", "ns");
            capture.root = path.parent().unwrap_or(Path::new(".")).to_owned();
            capture.architecture = asic_name.filter(|n| !n.is_empty()).unwrap_or_else(|| "Unknown".into());
            capture.metadata.extend(info);
            if let Err(reason) = &other {
                capture.warnings.push(format!("Thread trace not imported: {reason}."));
            }
            let Some(data) = spm_data else {
                return Err(match other {
                    Err(reason) => format!("This RGP capture has no SPM counters, and its thread trace could not be imported: {reason}."),
                    _ => "This RGP capture contains neither SPM counters nor a thread trace. RADV writes counters only when RADV_THREAD_TRACE_CACHE_COUNTERS is enabled.".into(),
                });
            };
            let origin = data.stamps.first().copied().unwrap_or(0);
            attach_spm(&mut capture, data, |s| {
                s.checked_sub(origin).map(|d| d as f64 * GPU_TIMESTAMP_NS)
            });
            capture.start = 0.;
            Ok(capture)
        }
    }
}
