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

/// Reads the derived SPM counters RADV writes on GFX10 and later.
fn spm(d: &[u8], c: &Chunk, clock_hz: u64, capture: &mut Capture) -> Result<(), String> {
    let b = &d[c.offset..c.offset + c.size];
    let samples = u32_at(b, 24)? as usize;
    let groups = u32_at(b, 28)? as usize;
    let counters = u32_at(b, 32)? as usize;
    let components = u32_at(b, 36)? as usize;
    let interval = u32_at(b, 40)? as u64;
    if samples == 0 || counters == 0 {
        capture.warnings.push("The capture contains an SPM section with no samples.".into());
        return Ok(());
    }
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

    // Groups name the counters they contain; keep that mapping for the series scope.
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
        named.push((r.name.clone(), r.fields[2]));
        at += r.size;
    }

    // Values are stored as one contiguous f64 run per series, counters first.
    let values = c.size - (counters + components) * samples * 8;
    let origin = stamps.first().copied().unwrap_or(0);
    for (i, (name, unit)) in named.into_iter().enumerate() {
        let base = values + i * samples * 8;
        let mut points = Vec::with_capacity(samples);
        for (s, stamp) in stamps.iter().enumerate() {
            points.push([stamp.saturating_sub(origin) as f64, f64_at(b, base + s * 8)?]);
        }
        let scope = match group_of[i].as_str() {
            "" => "GPU · SPM".to_owned(),
            group => format!("GPU · {group}"),
        };
        capture.counters.push(Series {
            spm_clock: None,
            definition: None,
            att: None,
            name,
            unit: unit_name(unit).into(),
            scope,
            points,
        });
    }
    capture.end = capture
        .end
        .max(stamps.last().copied().unwrap_or(0).saturating_sub(origin) as f64);
    capture.metadata.insert("spmSamples".into(), samples.to_string());
    capture
        .metadata
        .insert("spmSampleInterval".into(), format!("{interval} shader clocks"));
    if clock_hz > 0 {
        let span = interval * samples as u64;
        capture.metadata.insert(
            "spmWindow".into(),
            format!("{:.3} ms at {} MHz", span as f64 / clock_hz as f64 * 1e3, clock_hz / 1_000_000),
        );
    }
    capture.warnings.push(format!(
        "SPM counters are sampled every {interval} shader clocks and are device-wide; they are not attributed to a dispatch or wave."
    ));
    Ok(())
}

pub fn load(path: &Path) -> Result<Capture, String> {
    let size = fs::metadata(path).map_err(err)?.len();
    if size > MAX_FILE {
        return Err(format!(
            "{} exceeds the 256 MiB per-file import limit",
            path.display()
        ));
    }
    let d = fs::read(path).map_err(err)?;
    let table = chunks(&d)?;
    let (major, minor) = version(&d)?;
    let mut capture = Capture::empty(
        path.file_name().unwrap_or_default().to_string_lossy().into(),
        "RADV RGP",
        "shader clocks",
    );
    capture.root = path.parent().unwrap_or(Path::new(".")).to_owned();
    capture
        .metadata
        .insert("rgpVersion".into(), format!("{major}.{minor}"));

    let mut clock_hz = 0;
    if let Some(c) = table.iter().find(|c| c.kind == CHUNK_ASIC_INFO) {
        let a = asic(&d, c)?;
        clock_hz = a.shader_clock_hz;
        capture.architecture = if a.name.is_empty() { "Unknown".into() } else { a.name.clone() };
        capture.metadata.insert("asic".into(), a.name);
        capture.metadata.insert("deviceId".into(), format!("{:#06x}", a.device_id));
        capture
            .metadata
            .insert("shaderClock".into(), format!("{} Hz at capture time", a.shader_clock_hz));
        if a.shader_engines > 0 {
            capture.metadata.insert(
                "topology".into(),
                format!("{} shader engines × {} CUs", a.shader_engines, a.cus_per_engine),
            );
        }
    }

    let sqtt = table.iter().filter(|c| c.kind == CHUNK_SQTT_DATA).count();
    if sqtt > 0 {
        capture.metadata.insert("sqttSections".into(), sqtt.to_string());
        capture.warnings.push(format!(
            "This capture also contains {sqtt} thread-trace sections. Wave reconstruction from RADV thread traces is not implemented; only SPM counters were imported."
        ));
    }
    if table.iter().any(|c| c.kind == CHUNK_CODE_OBJECT_DATABASE) {
        capture
            .metadata
            .insert("codeObjects".into(), "present, not imported".into());
    }
    if table.iter().any(|c| c.kind == CHUNK_SQTT_DESC) {
        capture.metadata.insert("source".into(), "RADV thread trace capture".into());
    }

    match table.iter().find(|c| c.kind == CHUNK_DERIVED_SPM) {
        Some(c) => spm(&d, c, clock_hz, &mut capture)?,
        None => {
            return Err("This RGP capture contains no SPM counter section. RADV writes one only when cache counters are enabled; re-capture with RADV_THREAD_TRACE_CACHE_COUNTERS=true.".into())
        }
    }
    capture.metadata.insert(
        "collection".into(),
        "Mesa RADV Vulkan; counters are derived SPM values reported by the driver, not raw hardware event counts".into(),
    );
    Ok(capture)
}
