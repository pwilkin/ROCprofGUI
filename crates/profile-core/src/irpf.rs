//! IRPF v2.0 importer for native HRX profiles.
//!
//! HRX's `libhrx` runtime emits native profiling bundles through
//! `HRX_PROFILE_FILE` / `HRX_PROFILE_MODE=queue|dispatch`. The bundles are IREE
//! HAL profile ("IRPF") containers: a little-endian binary format with a 24-byte
//! file header, 104-byte record headers, and typed chunks whose payloads are
//! arrays of fixed/variable-length sub-records tagged with
//! `application/vnd.iree.hal.profile.<family>` content types.
//!
//! Every offset and length here follows the pinned producer contract
//! (`iree/hal/profile_file.h` and `iree/hal/profile_schema.h`, HRX System
//! 32b86d42d96a — retained under the 2026-10-02 campaign's `phase2/`). Nothing is
//! guessed: fields are bounds-checked against the record and chunk extents and a
//! malformed bundle is rejected with an explicit error rather than panicking.
//!
//! Time domain. Dispatch events carry raw device ticks. When the device record
//! advertises a timestamp frequency the importer converts elapsed ticks to
//! nanoseconds relative to the first tick, which is honest for durations and
//! within-capture ordering. It never places events on a host timeline: the
//! clock-correlation records in these bundles are flagged
//! `DEVICE_TICK_UNALIGNED`, so a host-time fit is explicitly forbidden by the
//! producer and the importer keeps that limitation in its warnings and metadata.

use crate::model::*;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// "IRPF" little-endian magic at byte 0.
const MAGIC: u32 = 0x4650_5249;
const FILE_HEADER_LEN: usize = 24;
const RECORD_HEADER_LEN: usize = 104;
const SUPPORTED_MAJOR: u16 = 2;

const RECORD_TYPE_CHUNK: u16 = 2;

/// Content-type prefix shared by every HAL profile chunk.
const CONTENT_PREFIX: &str = "application/vnd.iree.hal.profile.";

// Minimum (fixed-prefix) byte lengths of the sub-records this importer reads.
// Each must stay <= its struct's `record_length`; variable-length records
// (functions, code objects) carry trailing bytes after the fixed prefix.
const DEVICE_MIN: usize = 32;
const FUNCTION_MIN: usize = 64;
const CODE_OBJECT_MIN: usize = 48;
const CODE_OBJECT_LOAD_MIN: usize = 40;
const CLOCK_CORRELATION_MIN: usize = 72;
const DISPATCH_EVENT_MIN: usize = 88;
const COMMAND_OPERATION_MIN: usize = 120;
const QUEUE_MIN: usize = 24;

// Device record flag bits.
const DEVICE_FLAG_TIMESTAMP_FREQUENCY: u32 = 1 << 1;
// Clock-correlation flag bits.
const CLOCK_FLAG_DEVICE_TICK_UNALIGNED: u32 = 1 << 4;
// Command-operation type: kernel dispatch.
const COMMAND_OPERATION_TYPE_DISPATCH: u32 = 2;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn u16_at(d: &[u8], at: usize) -> Result<u16, String> {
    d.get(at..at + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| "IRPF bundle is truncated".into())
}
fn u32_at(d: &[u8], at: usize) -> Result<u32, String> {
    d.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| "IRPF bundle is truncated".into())
}
fn u64_at(d: &[u8], at: usize) -> Result<u64, String> {
    d.get(at..at + 8)
        .map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
        .ok_or_else(|| "IRPF bundle is truncated".into())
}
fn i64_at(d: &[u8], at: usize) -> Result<i64, String> {
    u64_at(d, at).map(|v| v as i64)
}

fn cancelled_err(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) {
        Err("Import cancelled".into())
    } else {
        Ok(())
    }
}

/// One parsed top-level file record (session begin/chunk/session end), with
/// borrowed views into the bundle bytes.
struct Record<'a> {
    record_type: u16,
    /// Content-type family after [`CONTENT_PREFIX`], e.g. `"dispatch-events"`.
    family: Option<&'a str>,
    payload: &'a [u8],
}

/// Parses the file header and every record within the logical bundle length.
fn parse_records(d: &[u8]) -> Result<Vec<Record<'_>>, String> {
    if d.len() < FILE_HEADER_LEN {
        return Err("Not an IRPF bundle: shorter than a 24-byte file header".into());
    }
    if u32_at(d, 0)? != MAGIC {
        return Err("Not an IRPF bundle: missing IRPF signature".into());
    }
    let major = u16_at(d, 4)?;
    let minor = u16_at(d, 6)?;
    if major != SUPPORTED_MAJOR {
        return Err(format!(
            "Unsupported IRPF bundle version {major}.{minor}; this importer reads v{SUPPORTED_MAJOR}.x"
        ));
    }
    let header_length = u32_at(d, 8)? as usize;
    if header_length < FILE_HEADER_LEN {
        return Err("IRPF file header length is smaller than 24 bytes".into());
    }
    let file_length = u64_at(d, 16)? as usize;
    if file_length == 0 {
        return Err(
            "IRPF bundle is not finalized (no session-end record); the capture did not complete. \
             HRX executable/all modes produce this when no capture filter is supplied."
                .into(),
        );
    }
    if file_length < header_length || file_length > d.len() {
        return Err("IRPF declared file length lies outside the actual bytes".into());
    }
    let logical = &d[..file_length];
    let mut out = Vec::new();
    let mut pos = header_length;
    while pos < logical.len() {
        if pos + RECORD_HEADER_LEN > logical.len() {
            return Err("IRPF record header is truncated".into());
        }
        let record_length = u64_at(logical, pos)? as usize;
        let payload_length = u64_at(logical, pos + 8)? as usize;
        let rec_header_length = u32_at(logical, pos + 56)? as usize;
        let content_type_length = u32_at(logical, pos + 60)? as usize;
        let name_length = u32_at(logical, pos + 64)? as usize;
        let record_type = u16_at(logical, pos + 100)?;
        if rec_header_length < RECORD_HEADER_LEN {
            return Err("IRPF record header length is smaller than 104 bytes".into());
        }
        let contents = rec_header_length
            .checked_add(content_type_length)
            .and_then(|v| v.checked_add(name_length))
            .and_then(|v| v.checked_add(payload_length))
            .ok_or("IRPF record size overflow")?;
        if record_length < contents {
            return Err("IRPF record length is smaller than its own contents".into());
        }
        let end = pos
            .checked_add(record_length)
            .ok_or("IRPF record offset overflow")?;
        if end > logical.len() {
            return Err("IRPF record extends past the logical file length".into());
        }
        let ct_start = pos + rec_header_length;
        let name_start = ct_start + content_type_length;
        let payload_start = name_start + name_length;
        let content_type = std::str::from_utf8(&logical[ct_start..name_start])
            .map_err(|_| "IRPF content type is not UTF-8")?;
        let payload = &logical[payload_start..payload_start + payload_length];
        out.push(Record {
            record_type,
            family: content_type.strip_prefix(CONTENT_PREFIX),
            payload,
        });
        pos = end;
    }
    Ok(out)
}

/// Concatenates every CHUNK payload of one content-type family. These bundles
/// emit a single chunk per family, but concatenation keeps the parser correct if
/// a producer splits a family across chunks.
fn chunk_bytes(records: &[Record], family: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for r in records {
        if r.record_type == RECORD_TYPE_CHUNK && r.family == Some(family) {
            out.extend_from_slice(r.payload);
        }
    }
    out
}

/// Walks a chunk payload as a packed array of self-sized sub-records, calling
/// `parse` on each. Every sub-record begins with a `uint32 record_length`;
/// the walk is bounds-checked and capped by [`crate::caps::max_records`].
fn walk_subrecords<T>(
    payload: &[u8],
    family: &str,
    min_len: usize,
    cap: usize,
    cancelled: &AtomicBool,
    mut parse: impl FnMut(&[u8]) -> Result<T, String>,
) -> Result<Vec<T>, String> {
    let mut out = Vec::new();
    let mut off = 0;
    while off < payload.len() {
        if out.len() % 4096 == 0 {
            cancelled_err(cancelled)?;
        }
        if off + 4 > payload.len() {
            return Err(format!("IRPF {family} sub-record header is truncated"));
        }
        let rl = u32_at(payload, off)? as usize;
        if rl < min_len {
            return Err(format!(
                "IRPF {family} sub-record length {rl} is below the {min_len}-byte minimum"
            ));
        }
        let end = off
            .checked_add(rl)
            .ok_or_else(|| format!("IRPF {family} sub-record overflow"))?;
        if end > payload.len() {
            return Err(format!("IRPF {family} sub-record extends past its chunk"));
        }
        out.push(parse(&payload[off..end])?);
        if out.len() > cap {
            return Err(format!(
                "IRPF import exceeds the {cap} record limit; raise ROCPROF_MAX_RECORDS or capture a smaller window"
            ));
        }
        off = end;
    }
    Ok(out)
}

struct Device {
    ordinal: u32,
    frequency_hz: Option<u64>,
}

/// Resolved code-object identity gathered from the code-objects chunk and merged
/// with its per-device load range.
struct CodeObject {
    executable_id: u64,
    code_object_id: u64,
    data_length: u64,
    is_elf: bool,
    load_delta: Option<i64>,
    load_size: Option<u64>,
}

pub fn load(path: &Path) -> Result<Capture, String> {
    load_cancellable(path, &AtomicBool::new(false))
}

pub fn load_cancellable(path: &Path, cancelled: &AtomicBool) -> Result<Capture, String> {
    cancelled_err(cancelled)?;
    let size = std::fs::metadata(path).map_err(err)?.len();
    if size > crate::caps::max_file_bytes() {
        return Err(format!(
            "IRPF bundle exceeds the {} MiB per-file import limit",
            crate::caps::max_file_mib()
        ));
    }
    let data = std::fs::read(path).map_err(err)?;
    cancelled_err(cancelled)?;
    let name = path.file_name().unwrap_or_default().to_string_lossy().into();
    build(&data, name, cancelled)
}

fn build(data: &[u8], name: String, cancelled: &AtomicBool) -> Result<Capture, String> {
    let (major, minor) = (u16_at(data, 4)?, u16_at(data, 6)?);
    let records = parse_records(data)?;
    let cap = crate::caps::max_records();

    // Devices: a timestamp frequency (FREQ flag) is authoritative for elapsed
    // tick conversion.
    let devices = walk_subrecords(
        &chunk_bytes(&records, "devices"),
        "devices",
        DEVICE_MIN,
        cap,
        cancelled,
        |r| {
            let flags = u32_at(r, 4)?;
            let frequency_hz = if flags & DEVICE_FLAG_TIMESTAMP_FREQUENCY != 0 && r.len() >= 40 {
                Some(u64_at(r, 32)?)
            } else {
                None
            };
            Ok(Device {
                ordinal: u32_at(r, 8)?,
                frequency_hz,
            })
        },
    )?;
    let device = devices.first();
    let frequency_hz = device.and_then(|d| d.frequency_hz).filter(|f| *f > 0);
    let device_label = format!("HRX device {}", device.map(|d| d.ordinal).unwrap_or(0));

    // Queue stream id (single queue in the observed bundles) for dispatch scope.
    let queues = walk_subrecords(
        &chunk_bytes(&records, "queues"),
        "queues",
        QUEUE_MIN,
        cap,
        cancelled,
        |r| u64_at(r, 16),
    )?;
    let queue_label = match queues.as_slice() {
        [stream] => format!("HRX stream {stream}"),
        _ => "HRX queue".to_owned(),
    };

    // Executable functions: name resolved by (executable_id, function_ordinal).
    let mut functions: BTreeMap<(u64, u32), String> = BTreeMap::new();
    for (exe, ordinal, nm) in walk_subrecords(
        &chunk_bytes(&records, "executable-functions"),
        "executable-functions",
        FUNCTION_MIN,
        cap,
        cancelled,
        |r| {
            let exe = u64_at(r, 8)?;
            let ordinal = u32_at(r, 16)?;
            let name_length = u32_at(r, 44)? as usize;
            let start = FUNCTION_MIN;
            let raw = r
                .get(start..start + name_length)
                .ok_or("IRPF function name extends past its record")?;
            Ok((exe, ordinal, String::from_utf8_lossy(raw).into_owned()))
        },
    )? {
        functions.insert((exe, ordinal), nm);
    }
    let function_count = functions.len();

    // Code objects (dispatch bundles only): retain ELF identity + load range.
    let mut code_objects: BTreeMap<(u64, u64), CodeObject> = BTreeMap::new();
    for co in walk_subrecords(
        &chunk_bytes(&records, "executable-code-objects"),
        "executable-code-objects",
        CODE_OBJECT_MIN,
        cap,
        cancelled,
        |r| {
            let data_length = u64_at(r, 24)?;
            let body_start = CODE_OBJECT_MIN;
            let is_elf = r
                .get(body_start..body_start + 4)
                .map(|m| m == b"\x7fELF")
                .unwrap_or(false);
            Ok(CodeObject {
                executable_id: u64_at(r, 8)?,
                code_object_id: u64_at(r, 16)?,
                data_length,
                is_elf,
                load_delta: None,
                load_size: None,
            })
        },
    )? {
        code_objects.insert((co.executable_id, co.code_object_id), co);
    }
    for (exe, co_id, delta, load_size) in walk_subrecords(
        &chunk_bytes(&records, "executable-code-object-loads"),
        "executable-code-object-loads",
        CODE_OBJECT_LOAD_MIN,
        cap,
        cancelled,
        |r| {
            Ok((
                u64_at(r, 8)?,
                u64_at(r, 16)?,
                i64_at(r, 24)?,
                u64_at(r, 32)?,
            ))
        },
    )? {
        if let Some(co) = code_objects.get_mut(&(exe, co_id)) {
            co.load_delta = Some(delta);
            co.load_size = Some(load_size);
        }
    }

    // Clock correlations: these bundles mark them DEVICE_TICK_UNALIGNED, so the
    // device clock must not be fit to a host timeline.
    let correlations = walk_subrecords(
        &chunk_bytes(&records, "clock-correlations"),
        "clock-correlations",
        CLOCK_CORRELATION_MIN,
        cap,
        cancelled,
        |r| u32_at(r, 4),
    )?;
    let correlation_count = correlations.len();
    let unaligned = correlations
        .iter()
        .any(|flags| flags & CLOCK_FLAG_DEVICE_TICK_UNALIGNED != 0);

    // Dispatches. Prefer device-timestamped dispatch events; fall back to the
    // static command-operation dispatch program when a queue-mode bundle carries
    // no dispatch events.
    let dispatch_events = chunk_bytes(&records, "dispatch-events");
    let (mut dispatches, dispatch_source, origin_tick) = if !dispatch_events.is_empty() {
        let raw = walk_subrecords(
            &dispatch_events,
            "dispatch-events",
            DISPATCH_EVENT_MIN,
            cap,
            cancelled,
            |r| {
                Ok(RawDispatch {
                    event_id: u64_at(r, 8)?,
                    executable_id: u64_at(r, 32)?,
                    function_ordinal: u32_at(r, 44)?,
                    workgroup_count: [u32_at(r, 48)?, u32_at(r, 52)?, u32_at(r, 56)?],
                    workgroup_size: [u32_at(r, 60)?, u32_at(r, 64)?, u32_at(r, 68)?],
                    start_tick: Some(u64_at(r, 72)?),
                    end_tick: Some(u64_at(r, 80)?),
                })
            },
        )?;
        let origin = raw.iter().filter_map(|d| d.start_tick).min();
        (raw, "dispatch-events", origin)
    } else {
        let raw = walk_subrecords(
            &chunk_bytes(&records, "command-operations"),
            "command-operations",
            COMMAND_OPERATION_MIN,
            cap,
            cancelled,
            |r| {
                let op_type = u32_at(r, 4)?;
                if op_type != COMMAND_OPERATION_TYPE_DISPATCH {
                    return Ok(None);
                }
                Ok(Some(RawDispatch {
                    event_id: u64_at(r, 16)?, // command_buffer_id
                    executable_id: u64_at(r, 32)?,
                    function_ordinal: u32_at(r, 40)?,
                    workgroup_count: [u32_at(r, 48)?, u32_at(r, 52)?, u32_at(r, 56)?],
                    workgroup_size: [u32_at(r, 60)?, u32_at(r, 64)?, u32_at(r, 68)?],
                    start_tick: None,
                    end_tick: None,
                }))
            },
        )?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        (raw, "command-operations", None)
    };
    dispatches.sort_by_key(|d| (d.start_tick.unwrap_or(0), d.event_id));

    let unit = if origin_tick.is_some() && frequency_hz.is_some() {
        "ns"
    } else if origin_tick.is_some() {
        "device ticks"
    } else {
        "unordered"
    };
    let mut unresolved = 0usize;
    let mut out = Capture::empty(name, "HRX native profile · IRPF 2.0", unit);
    let mut end = 0f64;
    let tick_to_axis = |tick: u64| -> f64 {
        let origin = origin_tick.unwrap_or(0);
        let elapsed = tick.saturating_sub(origin) as f64;
        match frequency_hz {
            Some(freq) => elapsed / freq as f64 * 1e9,
            None => elapsed,
        }
    };
    let mut model = Vec::with_capacity(dispatches.len());
    for (index, d) in dispatches.iter().enumerate() {
        if index % 4096 == 0 {
            cancelled_err(cancelled)?;
        }
        let kernel = match functions.get(&(d.executable_id, d.function_ordinal)) {
            Some(nm) => nm.clone(),
            None => {
                unresolved += 1;
                format!(
                    "unresolved function (executable {}, ordinal {})",
                    d.executable_id, d.function_ordinal
                )
            }
        };
        let (start, dispatch_end) = match (d.start_tick, d.end_tick) {
            (Some(s), Some(e)) => {
                let start = tick_to_axis(s);
                let finish = tick_to_axis(e.max(s));
                end = end.max(finish);
                (start, finish)
            }
            _ => (0., 0.),
        };
        model.push(Dispatch {
            shader_engine: None,
            id: format!("{dispatch_source}:{}", d.event_id),
            kernel,
            start,
            end: dispatch_end,
            agent: device_label.clone(),
            queue: queue_label.clone(),
            // Register/LDS/scratch metadata is not part of the IRPF dispatch or
            // function records; it is genuinely not collected here.
            vgpr: None,
            sgpr: None,
            lds: None,
            scratch: None,
            // IRPF records workgroup counts; the global grid is count × size,
            // the exact value rocprofv3 reports as Grid_Size (verified on the
            // shared hrx-b99 kernels). Keep workgroup = workgroup size.
            grid: grid_string(&d.workgroup_count, &d.workgroup_size),
            workgroup: format!(
                "{} × {} × {}",
                d.workgroup_size[0], d.workgroup_size[1], d.workgroup_size[2]
            ),
        });
    }
    out.dispatches = model;
    out.origin = origin_tick.map(|t| t.to_string()).unwrap_or_else(|| "0".into());
    out.start = 0.;
    out.end = end.max(1.);

    for ((exe, co_id), co) in &code_objects {
        out.code_objects.push(CodeObjectRecord {
            id: format!("exe{exe}:co{co_id}"),
            object_id: co_id.to_string(),
            scope: format!("HRX executable {exe}"),
            agent: Some(device_label.clone()),
            uri: None,
            storage: Some(if co.is_elf {
                format!("Embedded ELF code object ({} bytes)", co.data_length)
            } else {
                format!("Embedded code object ({} bytes)", co.data_length)
            }),
            load_base: None,
            load_size: co.load_size.map(|s| s.to_string()),
            load_delta: co.load_delta.map(|d| d.to_string()),
        });
    }

    // Architecture is not stated in the IRPF metadata; the retained bundles are
    // gfx1151 but the format itself does not record a gfx target, so leave it
    // explicitly unknown rather than inferring one.
    out.metadata.insert("hrxProfileFormat".into(), format!("IRPF {major}.{minor}"));
    out.metadata.insert(
        "hrxProfileMode".into(),
        if dispatch_source == "dispatch-events" {
            "dispatch (device-timestamped dispatch events)".into()
        } else {
            "queue (static command-operation program; no device timing)".into()
        },
    );
    out.metadata.insert("hrxFunctionCount".into(), function_count.to_string());
    out.metadata
        .insert("hrxDispatchCount".into(), out.dispatches.len().to_string());
    out.metadata
        .insert("hrxCodeObjectCount".into(), code_objects.len().to_string());
    if let Some(freq) = frequency_hz {
        out.metadata
            .insert("hrxTimestampFrequencyHz".into(), freq.to_string());
    }
    out.metadata.insert(
        "coverage".into(),
        "HRX native IRPF dispatch stream; raw device ticks, no host-timeline alignment".into(),
    );
    out.metadata
        .insert("gridConvention".into(), "grid = workgroup count × workgroup size (global work-items), matching rocprofv3 Grid_Size".into());

    out.warnings.push(
        "HRX native profile (IRPF 2.0). Dispatch names, workgroup counts and sizes are the producer's own records.".into(),
    );
    match (origin_tick.is_some(), frequency_hz) {
        (true, Some(freq)) => out.warnings.push(format!(
            "Timestamps are raw device ticks converted to elapsed nanoseconds using the device timestamp frequency ({freq} Hz). Only relative timing within this capture is meaningful."
        )),
        (true, None) => out.warnings.push(
            "The device record carries no timestamp frequency, so dispatch times are left in raw device ticks; durations are not nanoseconds.".into(),
        ),
        (false, _) => out.warnings.push(
            "This queue-mode bundle has no device-timestamped dispatch events; the dispatches are the static command-operation program and carry no execution timing.".into(),
        ),
    }
    if unaligned {
        out.warnings.push(format!(
            "The {correlation_count} clock-correlation records are flagged DEVICE_TICK_UNALIGNED: the device clock must not be fit to a host timeline. Events keep raw device ordering and tick durations only."
        ));
    } else if correlation_count > 0 {
        out.warnings.push(format!(
            "{correlation_count} clock-correlation records are retained; cross-domain host alignment is not applied automatically."
        ));
    }
    if unresolved > 0 {
        out.warnings.push(format!(
            "{unresolved} dispatches reference an executable/function the bundle does not define; their names are kept explicitly unresolved rather than guessed."
        ));
    }
    out.warnings.push(
        "Coverage is unknown from a native dispatch stream alone: wave occupancy, ISA hotspots and hardware counters are not part of these IRPF bundles.".into(),
    );
    Ok(out)
}

/// Per-dimension global work items (workgroup count × workgroup size), rendered
/// like the other importers. Returns "Not recorded" when the count is absent.
fn grid_string(count: &[u32; 3], size: &[u32; 3]) -> String {
    if count.iter().all(|c| *c == 0) {
        return "Not recorded".into();
    }
    format!(
        "{} × {} × {}",
        count[0] as u64 * size[0].max(1) as u64,
        count[1] as u64 * size[1].max(1) as u64,
        count[2] as u64 * size[2].max(1) as u64
    )
}

struct RawDispatch {
    event_id: u64,
    executable_id: u64,
    function_ordinal: u32,
    workgroup_count: [u32; 3],
    workgroup_size: [u32; 3],
    start_tick: Option<u64>,
    end_tick: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // Byte-level builder for synthetic IRPF bundles so the parser is exercised
    // without the multi-megabyte campaign artifacts. Layout mirrors the pinned
    // producer structs exactly.
    #[derive(Default)]
    struct Chunk {
        family: String,
        payload: Vec<u8>,
    }
    fn chunk(family: &str, payload: Vec<u8>) -> Chunk {
        Chunk {
            family: format!("{CONTENT_PREFIX}{family}"),
            payload,
        }
    }
    fn bundle(chunks: &[Chunk]) -> Vec<u8> {
        let mut body = Vec::new();
        let mut push_record = |rtype: u16, ct: &str, payload: &[u8]| {
            let header_len = RECORD_HEADER_LEN as u64;
            let ctl = ct.len() as u32;
            let total = header_len + ct.len() as u64 + payload.len() as u64;
            let mut h = vec![0u8; RECORD_HEADER_LEN];
            h[0..8].copy_from_slice(&total.to_le_bytes());
            h[8..16].copy_from_slice(&(payload.len() as u64).to_le_bytes());
            h[56..60].copy_from_slice(&(RECORD_HEADER_LEN as u32).to_le_bytes());
            h[60..64].copy_from_slice(&ctl.to_le_bytes());
            h[64..68].copy_from_slice(&0u32.to_le_bytes()); // name_length
            h[100..102].copy_from_slice(&rtype.to_le_bytes());
            body.extend_from_slice(&h);
            body.extend_from_slice(ct.as_bytes());
            body.extend_from_slice(payload);
        };
        push_record(1, &format!("{CONTENT_PREFIX}session"), &[]);
        for c in chunks {
            push_record(RECORD_TYPE_CHUNK, &c.family, &c.payload);
        }
        push_record(3, &format!("{CONTENT_PREFIX}session"), &[]);
        let mut out = vec![0u8; FILE_HEADER_LEN];
        out[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        out[4..6].copy_from_slice(&2u16.to_le_bytes());
        out[6..8].copy_from_slice(&0u16.to_le_bytes());
        out[8..12].copy_from_slice(&(FILE_HEADER_LEN as u32).to_le_bytes());
        out.extend_from_slice(&body);
        let file_length = out.len() as u64;
        out[16..24].copy_from_slice(&file_length.to_le_bytes());
        out
    }
    fn le_u32(v: u32) -> [u8; 4] {
        v.to_le_bytes()
    }
    fn le_u64(v: u64) -> [u8; 8] {
        v.to_le_bytes()
    }
    fn device(freq: u64) -> Vec<u8> {
        let mut r = vec![0u8; 40];
        r[0..4].copy_from_slice(&le_u32(40));
        r[4..8].copy_from_slice(&le_u32(DEVICE_FLAG_TIMESTAMP_FREQUENCY));
        r[8..12].copy_from_slice(&le_u32(0)); // ordinal
        r[12..16].copy_from_slice(&le_u32(1)); // queue_count
        r[32..40].copy_from_slice(&le_u64(freq));
        r
    }
    fn function(exe: u64, ordinal: u32, name: &str) -> Vec<u8> {
        let mut r = vec![0u8; 64];
        let total = 64 + name.len() as u32;
        r[0..4].copy_from_slice(&le_u32(total));
        r[8..16].copy_from_slice(&le_u64(exe));
        r[16..20].copy_from_slice(&le_u32(ordinal));
        r[44..48].copy_from_slice(&le_u32(name.len() as u32));
        r.extend_from_slice(name.as_bytes());
        r
    }
    fn dispatch_event(
        event_id: u64,
        exe: u64,
        ordinal: u32,
        count: [u32; 3],
        size: [u32; 3],
        start: u64,
        end: u64,
    ) -> Vec<u8> {
        let mut r = vec![0u8; 88];
        r[0..4].copy_from_slice(&le_u32(88));
        r[8..16].copy_from_slice(&le_u64(event_id));
        r[32..40].copy_from_slice(&le_u64(exe));
        r[44..48].copy_from_slice(&le_u32(ordinal));
        for i in 0..3 {
            r[48 + i * 4..52 + i * 4].copy_from_slice(&le_u32(count[i]));
            r[60 + i * 4..64 + i * 4].copy_from_slice(&le_u32(size[i]));
        }
        r[72..80].copy_from_slice(&le_u64(start));
        r[80..88].copy_from_slice(&le_u64(end));
        r
    }
    fn code_object(exe: u64, co_id: u64, body: &[u8]) -> Vec<u8> {
        let mut r = vec![0u8; 48];
        let total = 48 + body.len() as u32;
        r[0..4].copy_from_slice(&le_u32(total));
        r[8..16].copy_from_slice(&le_u64(exe));
        r[16..24].copy_from_slice(&le_u64(co_id));
        r[24..32].copy_from_slice(&le_u64(body.len() as u64));
        r.extend_from_slice(body);
        r
    }
    fn code_object_load(exe: u64, co_id: u64, delta: i64, size: u64) -> Vec<u8> {
        let mut r = vec![0u8; 40];
        r[0..4].copy_from_slice(&le_u32(40));
        r[8..16].copy_from_slice(&le_u64(exe));
        r[16..24].copy_from_slice(&le_u64(co_id));
        r[24..32].copy_from_slice(&le_u64(delta as u64));
        r[32..40].copy_from_slice(&le_u64(size));
        r
    }
    fn clock_correlation(flags: u32) -> Vec<u8> {
        let mut r = vec![0u8; 72];
        r[0..4].copy_from_slice(&le_u32(72));
        r[4..8].copy_from_slice(&le_u32(flags));
        r
    }
    fn command_operation(
        op_type: u32,
        cb: u64,
        exe: u64,
        ordinal: u32,
        count: [u32; 3],
        size: [u32; 3],
    ) -> Vec<u8> {
        let mut r = vec![0u8; 120];
        r[0..4].copy_from_slice(&le_u32(120));
        r[4..8].copy_from_slice(&le_u32(op_type));
        r[16..24].copy_from_slice(&le_u64(cb));
        r[32..40].copy_from_slice(&le_u64(exe));
        r[40..44].copy_from_slice(&le_u32(ordinal));
        for i in 0..3 {
            r[48 + i * 4..52 + i * 4].copy_from_slice(&le_u32(count[i]));
            r[60 + i * 4..64 + i * 4].copy_from_slice(&le_u32(size[i]));
        }
        r
    }

    fn dispatch_bundle() -> Vec<u8> {
        let mut functions = function(1, 0, "ggml_rope_f32");
        functions.extend(function(2, 0, "ggml_get_rows_f32"));
        let mut events = dispatch_event(1, 1, 0, [32, 1, 1], [32, 1, 1], 1000, 1100);
        events.extend(dispatch_event(2, 2, 0, [3, 2, 1], [256, 1, 1], 1200, 1500));
        let mut objects = code_object(1, 1, b"\x7fELFrope-image");
        objects.extend(code_object(2, 2, b"\x7fELFrows-image"));
        let mut loads = code_object_load(1, 1, 0x1000, 64);
        loads.extend(code_object_load(2, 2, 0x2000, 128));
        let mut correlations = clock_correlation(0x1f);
        correlations.extend(clock_correlation(0x1f));
        bundle(&[
            chunk("devices", device(100_000_000)),
            chunk("executable-functions", functions),
            chunk("executable-code-objects", objects),
            chunk("executable-code-object-loads", loads),
            chunk("clock-correlations", correlations),
            chunk("dispatch-events", events),
        ])
    }

    #[test]
    fn dispatch_bundle_resolves_names_grids_and_elapsed_ns() {
        let c = build(&dispatch_bundle(), "trace.irpf".into(), &AtomicBool::new(false)).unwrap();
        assert_eq!(c.format, "HRX native profile · IRPF 2.0");
        assert_eq!(c.unit, "ns");
        assert_eq!(c.dispatches.len(), 2);
        assert_eq!(c.code_objects.len(), 2);
        assert_eq!(c.metadata["hrxFunctionCount"], "2");
        assert_eq!(c.metadata["hrxTimestampFrequencyHz"], "100000000");
        let rope = &c.dispatches[0];
        assert_eq!(rope.kernel, "ggml_rope_f32");
        // grid = count × size (32×32), matching rocprofv3 Grid_Size.
        assert_eq!(rope.grid, "1024 × 1 × 1");
        assert_eq!(rope.workgroup, "32 × 1 × 1");
        // 100 ticks at 100 MHz = 1000 ns, relative to the first tick.
        assert_eq!(rope.start, 0.0);
        assert_eq!(rope.end, 1000.0);
        let rows = &c.dispatches[1];
        assert_eq!(rows.kernel, "ggml_get_rows_f32");
        assert_eq!(rows.grid, "768 × 2 × 1");
        assert_eq!(rows.start, 2000.0);
        // code-object load range retained.
        assert_eq!(c.code_objects[0].load_size.as_deref(), Some("64"));
        assert_eq!(c.code_objects[0].load_delta.as_deref(), Some("4096"));
        assert!(c.code_objects[0].storage.as_deref().unwrap().contains("ELF"));
        assert!(c
            .warnings
            .iter()
            .any(|w| w.contains("DEVICE_TICK_UNALIGNED")));
    }

    #[test]
    fn queue_bundle_falls_back_to_command_operations() {
        let mut functions = function(1, 0, "ggml_rope_f32");
        functions.extend(function(2, 0, "ggml_get_rows_f32"));
        let mut ops = command_operation(COMMAND_OPERATION_TYPE_DISPATCH, 1, 1, 0, [32, 1, 1], [32, 1, 1]);
        ops.extend(command_operation(1, 0, 0, 0, [0, 0, 0], [0, 0, 0])); // BARRIER, skipped
        ops.extend(command_operation(COMMAND_OPERATION_TYPE_DISPATCH, 1, 2, 0, [3, 2, 1], [256, 1, 1]));
        let b = bundle(&[
            chunk("devices", device(100_000_000)),
            chunk("executable-functions", functions),
            chunk("command-operations", ops),
        ]);
        let c = build(&b, "trace.irpf".into(), &AtomicBool::new(false)).unwrap();
        assert_eq!(c.dispatches.len(), 2);
        assert_eq!(c.unit, "unordered");
        assert_eq!(c.dispatches[0].kernel, "ggml_rope_f32");
        assert_eq!(c.dispatches[0].grid, "1024 × 1 × 1");
        assert!(c.dispatches.iter().all(|d| d.start == 0. && d.end == 0.));
        assert!(c
            .warnings
            .iter()
            .any(|w| w.contains("static command-operation program")));
    }

    #[test]
    fn unfinalized_bundle_is_rejected() {
        let mut d = vec![0u8; 24];
        d[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        d[4..6].copy_from_slice(&2u16.to_le_bytes());
        d[8..12].copy_from_slice(&24u32.to_le_bytes());
        // file_length left 0 → unfinalized.
        let e = build(&d, "trace.irpf".into(), &AtomicBool::new(false)).unwrap_err();
        assert!(e.contains("not finalized"), "{e}");
    }

    #[test]
    fn malformed_inputs_are_rejected_not_panicked() {
        let flag = AtomicBool::new(false);
        // Bad magic.
        let mut d = dispatch_bundle();
        d[0] ^= 0xff;
        assert!(build(&d, "x".into(), &flag).unwrap_err().contains("IRPF signature"));
        // Wrong major version.
        let mut d = dispatch_bundle();
        d[4..6].copy_from_slice(&9u16.to_le_bytes());
        assert!(build(&d, "x".into(), &flag).unwrap_err().contains("Unsupported IRPF"));
        // Declared file length past the actual bytes.
        let mut d = dispatch_bundle();
        let huge = (d.len() as u64 + 4096).to_le_bytes();
        d[16..24].copy_from_slice(&huge);
        assert!(build(&d, "x".into(), &flag).unwrap_err().contains("outside the actual bytes"));
        // Truncated tail mid-record.
        let full = dispatch_bundle();
        let mut d = full.clone();
        let cut = d.len() - 20;
        d.truncate(cut);
        // keep the declared length honest to the truncated buffer so the record
        // walk, not the header check, catches the truncation.
        d[16..24].copy_from_slice(&(cut as u64).to_le_bytes());
        assert!(build(&d, "x".into(), &flag).is_err());
    }

    // The ROCPROF_MAX_RECORDS cap is exercised in tests/configurable_caps.rs,
    // the only test binary allowed to mutate that process-wide variable.
}
