//! Executable-trace arm of the IRPF importer.
//!
//! The native AMD GPU driver emits `executable-traces` chunks: one raw SQTT
//! ("ATT") buffer per shader engine for each traced operation, plus an
//! `executable-trace-captures` record saying how that buffer was taken. The
//! stream carries no in-trace code-object markers, so this arm stages each trace
//! with the bundle's embedded ELF code objects placed at their recorded load
//! addresses — the placed manifest the RADV arm already uses — runs the same
//! decoder, and attaches the decoded waves and instructions to the dispatch
//! capture.
//!
//! Time. A decoded trace runs on its own shader clock. Its realtime tokens pair
//! that clock with the low 36 bits of the GPU timestamp counter, which is the
//! clock the bundle's dispatch events are stamped with. Each (trace, shader
//! engine) therefore gets an affine map onto the dispatch axis, stored on the
//! capture and applied wherever a wave file is read.
//!
//! Honesty. SQTT samples: one SIMD of one workgroup processor per shader engine.
//! Nothing here turns that into a census; the selection, the buffer size, the
//! bytes written, the dropped counts and the covered time window are all stated
//! in the capture's metadata and warnings. A trace that cannot be decoded or
//! placed is left out with a warning; the dispatch capture still opens.

use crate::model::*;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// `iree_hal_profile_executable_trace_record_t`.
pub(crate) const TRACE_MIN: usize = 88;
/// `iree_hal_profile_executable_trace_capture_record_t`.
pub(crate) const TRACE_CAPTURE_MIN: usize = 80;
/// `IREE_HAL_PROFILE_EXECUTABLE_TRACE_FORMAT_AMDGPU_ATT`.
const FORMAT_AMDGPU_ATT: u32 = 1;
/// `IREE_HAL_PROFILE_EXECUTABLE_TRACE_FLAG_DISPATCH_EVENT`.
const TRACE_FLAG_DISPATCH_EVENT: u32 = 1 << 0;

const CAPTURE_STABLE_POWER_STATE: u32 = 1 << 0;
const CAPTURE_FINISHED: u32 = 1 << 1;
const CAPTURE_BUFFER_FULL: u32 = 1 << 2;
const CAPTURE_WRITE_ERROR: u32 = 1 << 3;
const CAPTURE_COMMAND_BUFFER_SCOPE: u32 = 1 << 4;
const CAPTURE_UNCOLLECTED: u32 = 1 << 5;

/// SQTT realtime tokens carry this many low bits of the GPU timestamp.
const REALTIME_BITS: u32 = 36;

fn u16_at(d: &[u8], at: usize) -> Option<u16> {
    d.get(at..at.checked_add(2)?).map(|b| u16::from_le_bytes([b[0], b[1]]))
}
fn u32_at(d: &[u8], at: usize) -> Option<u32> {
    d.get(at..at.checked_add(4)?)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
fn u64_at(d: &[u8], at: usize) -> Option<u64> {
    d.get(at..at.checked_add(8)?)
        .map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}

/// One `executable-traces` chunk: a record followed by raw trace bytes.
#[derive(Debug)]
pub(crate) struct TraceChunk<'a> {
    pub session: u64,
    pub device: u32,
    pub stream: u64,
    pub trace_id: u64,
    pub shader_engine: u32,
    pub flags: u32,
    pub dispatch_event_id: u64,
    pub submission_id: u64,
    pub command_buffer_id: u64,
    pub bytes: &'a [u8],
    /// The chunk header's TRUNCATED flag and dropped-record count.
    pub truncated: bool,
    pub dropped: u64,
}

/// Parses one trace chunk payload. `Ok(None)` is a format this importer does not
/// decode; `Err` is a malformed chunk.
pub(crate) fn parse_trace_chunk<'a>(
    session: u64,
    device: u32,
    stream: u64,
    truncated: bool,
    dropped: u64,
    payload: &'a [u8],
) -> Result<Option<TraceChunk<'a>>, String> {
    let short = || "IRPF executable-traces chunk is shorter than its 88-byte record".to_owned();
    let record_length = u32_at(payload, 0).ok_or_else(short)? as usize;
    if record_length < TRACE_MIN || record_length > payload.len() {
        return Err(format!(
            "IRPF executable-traces record length {record_length} lies outside its {}-byte chunk",
            payload.len()
        ));
    }
    let format = u32_at(payload, 4).ok_or_else(short)?;
    let data_length = u64_at(payload, 80).ok_or_else(short)?;
    let bytes = &payload[record_length..];
    if data_length != bytes.len() as u64 {
        return Err(format!(
            "IRPF executable-traces chunk declares {data_length} trace bytes but carries {}",
            bytes.len()
        ));
    }
    if format != FORMAT_AMDGPU_ATT {
        return Ok(None);
    }
    Ok(Some(TraceChunk {
        session,
        device,
        stream,
        flags: u32_at(payload, 8).ok_or_else(short)?,
        shader_engine: u32_at(payload, 12).ok_or_else(short)?,
        trace_id: u64_at(payload, 16).ok_or_else(short)?,
        dispatch_event_id: u64_at(payload, 24).ok_or_else(short)?,
        submission_id: u64_at(payload, 32).ok_or_else(short)?,
        command_buffer_id: u64_at(payload, 40).ok_or_else(short)?,
        bytes,
        truncated,
        dropped,
    }))
}

/// One `executable-trace-captures` record: how a trace partition was taken.
#[derive(Debug, Clone)]
pub(crate) struct TraceCapture {
    pub session: u64,
    pub flags: u32,
    pub trace_id: u64,
    pub shader_engine: u32,
    pub shader_engine_mask: u32,
    pub target_compute_unit: u32,
    pub simd_select: u32,
    pub dispatch_count: u32,
    pub buffer_byte_length: u64,
    pub written_byte_length: u64,
    pub dropped_count: u64,
    pub hardware_status: u32,
    pub token_mask: u32,
    pub trace_limit: u32,
    pub skipped_count: u32,
}

pub(crate) fn parse_trace_capture(session: u64, r: &[u8]) -> Result<TraceCapture, String> {
    let short = || "IRPF executable-trace-captures record is truncated".to_owned();
    Ok(TraceCapture {
        session,
        flags: u32_at(r, 4).ok_or_else(short)?,
        trace_id: u64_at(r, 8).ok_or_else(short)?,
        shader_engine: u32_at(r, 16).ok_or_else(short)?,
        shader_engine_mask: u32_at(r, 24).ok_or_else(short)?,
        target_compute_unit: u32_at(r, 28).ok_or_else(short)?,
        simd_select: u32_at(r, 32).ok_or_else(short)?,
        dispatch_count: u32_at(r, 36).ok_or_else(short)?,
        buffer_byte_length: u64_at(r, 40).ok_or_else(short)?,
        written_byte_length: u64_at(r, 48).ok_or_else(short)?,
        dropped_count: u64_at(r, 56).ok_or_else(short)?,
        hardware_status: u32_at(r, 64).ok_or_else(short)?,
        token_mask: u32_at(r, 68).ok_or_else(short)?,
        trace_limit: u32_at(r, 72).ok_or_else(short)?,
        skipped_count: u32_at(r, 76).ok_or_else(short)?,
    })
}

/// An embedded ELF code object and where it was loaded.
#[derive(Debug)]
pub(crate) struct PlacedObject<'a> {
    pub session: u64,
    pub executable_id: u64,
    pub code_object_id: u64,
    pub elf: &'a [u8],
    /// `load_delta` of every distinct native placement.
    pub load_deltas: Vec<i64>,
}

/// Virtual-address extent `[start, end)` of an ELF64's executable segments.
pub(crate) fn executable_extent(elf: &[u8]) -> Option<(u64, u64)> {
    if elf.get(..4)? != b"\x7fELF" || *elf.get(4)? != 2 || *elf.get(5)? != 1 {
        return None;
    }
    let phoff = usize::try_from(u64_at(elf, 0x20)?).ok()?;
    let entry_size = u16_at(elf, 0x36)? as usize;
    let count = u16_at(elf, 0x38)? as usize;
    if entry_size < 56 {
        return None;
    }
    let mut extent: Option<(u64, u64)> = None;
    for i in 0..count {
        let at = phoff.checked_add(i.checked_mul(entry_size)?)?;
        let (kind, flags) = (u32_at(elf, at)?, u32_at(elf, at + 4)?);
        // PT_LOAD with PF_X.
        if kind != 1 || flags & 1 == 0 {
            continue;
        }
        let start = u64_at(elf, at + 16)?;
        let end = start.checked_add(u64_at(elf, at + 40)?)?;
        extent = Some(match extent {
            Some((lo, hi)) => (lo.min(start), hi.max(end)),
            None => (start, end),
        });
    }
    extent.filter(|(lo, hi)| hi > lo)
}

/// The traced operation a trace refers to, in its device's tick domain.
#[derive(Debug, Clone, Copy)]
pub(crate) struct OperationWindow {
    pub first_tick: u64,
    pub last_tick: u64,
    pub dispatches: usize,
}

/// How one device's raw ticks land on the capture axis.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DeviceClock {
    pub origin_tick: u64,
    /// Axis units per tick: nanoseconds when the frequency is known, else 1.
    pub axis_per_tick: f64,
}

pub(crate) struct TraceContext<'a> {
    pub chunks: Vec<TraceChunk<'a>>,
    pub captures: Vec<TraceCapture>,
    pub objects: Vec<PlacedObject<'a>>,
    /// Dispatch-event window of each (session, stream, submission).
    pub submissions: BTreeMap<(u64, u64, u64), OperationWindow>,
    /// Dispatch-event window of each (session, stream, event id).
    pub events: BTreeMap<(u64, u64, u64), OperationWindow>,
    pub clocks: BTreeMap<(u64, u32), DeviceClock>,
    /// Chunks of a trace format this importer does not decode.
    pub foreign_format_chunks: usize,
    /// Axis unit of the capture, for messages.
    pub unit: &'static str,
}

/// Runs the decoder on a staged input directory inside `workspace`.
pub(crate) type Decode<'d> =
    dyn Fn(&tempfile::TempDir, &Path, Duration) -> Result<crate::decoder::DecodedRun, String> + 'd;

/// The production decoder: the worker every other raw-trace path uses.
pub(crate) fn run_decoder(
    cancelled: &AtomicBool,
) -> impl Fn(&tempfile::TempDir, &Path, Duration) -> Result<crate::decoder::DecodedRun, String> + '_ {
    move |workspace, input, timeout| {
        let config = crate::decoder::DecoderConfig {
            timeout,
            ..Default::default()
        };
        crate::decoder::run_decoder_in(workspace, input, &config, cancelled)
    }
}

/// A shader clock placed against the GPU timestamp counter.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ClockMap {
    /// `[shader clock, tick relative to the reference]`, sorted, at least two.
    pub knots: Vec<[f64; 2]>,
    /// Anchors the knots stand for.
    pub anchors: usize,
    /// Anchors left out because one of the clocks ran backwards there.
    pub rejected: usize,
}

/// Places `(shader clock, 36-bit realtime)` anchors in full ticks relative to
/// `reference`, a tick known to lie in or next to the trace, and thins them to
/// knots that reproduce every kept anchor within `tolerance` ticks. The shader
/// clock slows when the GPU idles between dispatches, so one line through the
/// trace misplaces waves by tens of microseconds; the anchors themselves are a
/// few microseconds apart.
pub(crate) fn clock_map(pairs: &[(f64, u64)], reference: u64, tolerance: f64) -> Option<ClockMap> {
    let mask = (1u64 << REALTIME_BITS) - 1;
    let half = 1i128 << (REALTIME_BITS - 1);
    let span = 1i128 << REALTIME_BITS;
    let mut points: Vec<[f64; 2]> = pairs
        .iter()
        .filter(|(cycle, _)| cycle.is_finite())
        .map(|&(cycle, realtime)| {
            let mut full = ((reference & !mask) | (realtime & mask)) as i128;
            if full + half < reference as i128 {
                full += span;
            } else if full > reference as i128 + half {
                full -= span;
            }
            [cycle, (full - reference as i128) as f64]
        })
        .collect();
    points.sort_by(|a, b| a[0].total_cmp(&b[0]));
    // Both clocks only run forwards; an anchor that does not is a misparse.
    let mut forward: Vec<[f64; 2]> = Vec::with_capacity(points.len());
    for point in points.iter().copied() {
        match forward.last() {
            Some(last) if point[0] <= last[0] || point[1] < last[1] => {}
            _ => forward.push(point),
        }
    }
    let rejected = points.len() - forward.len();
    if forward.len() < 2 || rejected * 4 > points.len() {
        return None;
    }
    // Swinging door: extend a segment while one slope from its first knot still
    // passes within the tolerance of every anchor it skips.
    let mut knots = vec![forward[0]];
    let mut origin = forward[0];
    let mut previous = forward[0];
    let (mut low, mut high) = (f64::NEG_INFINITY, f64::INFINITY);
    for point in forward.iter().copied().skip(1) {
        let bounds = |from: [f64; 2]| {
            let run = point[0] - from[0];
            ((point[1] - tolerance - from[1]) / run, (point[1] + tolerance - from[1]) / run)
        };
        let (point_low, point_high) = bounds(origin);
        if low.max(point_low) > high.min(point_high) {
            knots.push(previous);
            origin = previous;
            (low, high) = bounds(origin);
        } else {
            (low, high) = (low.max(point_low), high.min(point_high));
        }
        previous = point;
    }
    knots.push(previous);
    Some(ClockMap { knots, anchors: forward.len(), rejected })
}

/// `(shader clock, realtime)` anchors of each shader engine in `realtime.json`.
fn realtime_pairs(realtime: &serde_json::Value) -> BTreeMap<u32, Vec<(f64, u64)>> {
    let mut out = BTreeMap::new();
    let Some(map) = realtime.as_object() else { return out };
    for (key, pairs) in map {
        let Some(se) = key.strip_prefix("SE").and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        let pairs: Vec<(f64, u64)> = pairs
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|p| Some((p.get(0)?.as_f64()?, p.get(1)?.as_u64()?)))
                    .collect()
            })
            .unwrap_or_default();
        out.insert(se, pairs);
    }
    out
}

fn engines(mask: u32) -> String {
    let list: Vec<String> = (0..32).filter(|i| mask & (1 << i) != 0).map(|i| i.to_string()).collect();
    if list.is_empty() {
        "none".into()
    } else {
        list.join(", ")
    }
}

fn mib(bytes: u64) -> String {
    let value = bytes as f64 / (1024. * 1024.);
    if value >= 10. || value.fract() == 0. {
        format!("{value:.0} MiB")
    } else {
        format!("{value:.2} MiB")
    }
}

/// What one decoded trace contributed, for the capture's provenance.
struct Imported {
    trace_id: u64,
    waves: usize,
    command_buffer_scope: bool,
    /// Trace window on the capture axis.
    window: Option<(f64, f64)>,
    /// The traced operation's dispatch window on the capture axis.
    operation: Option<(f64, f64)>,
    operation_dispatches: usize,
    covered_dispatches: Option<usize>,
    /// Realtime anchors the waves were placed through, and the knots kept.
    anchors: usize,
    knots: usize,
    rejected_anchors: usize,
    lost: bool,
}

/// Stages, decodes and attaches every trace of the bundle to `out`. Only
/// cancellation is an error: any trace that cannot be staged, decoded or placed
/// is left out with a warning and the dispatch capture stays usable.
pub(crate) fn attach(
    out: &mut Capture,
    ctx: TraceContext,
    dispatch_ticks: &dyn Fn(u64, u64, u64) -> Vec<(u64, u64)>,
    cancelled: &AtomicBool,
    decode: &Decode,
) -> Result<(), String> {
    let cancel = || -> Result<(), String> {
        if cancelled.load(Ordering::Relaxed) {
            Err("Import cancelled".into())
        } else {
            Ok(())
        }
    };
    cancel()?;
    if ctx.foreign_format_chunks > 0 {
        out.warnings.push(format!(
            "{} executable-trace chunk(s) use a trace format other than AMD ATT/SQTT and were not imported.",
            ctx.foreign_format_chunks
        ));
    }
    describe_captures(out, &ctx);
    if ctx.chunks.is_empty() {
        return Ok(());
    }
    let skip = |out: &mut Capture, reason: String| {
        out.warnings.push(format!("Thread traces not imported: {reason}"));
        out.metadata.insert("hrxTraceImport".into(), "not imported".into());
    };
    if ctx.chunks.len() > crate::caps::max_att_traces() {
        skip(out, format!(
            "the bundle holds {} trace partitions, above the {} trace-file limit (ROCPROF_MAX_ATT_TRACES).",
            ctx.chunks.len(),
            crate::caps::max_att_traces()
        ));
        return Ok(());
    }

    // Placed code objects, numbered in bundle order. An object without an
    // executable segment or without a load record cannot be placed.
    let mut placed = vec![];
    let mut unplaced = 0usize;
    for object in &ctx.objects {
        match executable_extent(object.elf) {
            Some(extent) if !object.load_deltas.is_empty() => placed.push((object, extent)),
            _ => unplaced += 1,
        }
    }
    if placed.len() > crate::caps::max_att_code_objects() {
        skip(out, format!(
            "the bundle holds {} placed code objects, above the {} code-object limit (ROCPROF_MAX_ATT_CODE_OBJECTS).",
            placed.len(),
            crate::caps::max_att_code_objects()
        ));
        return Ok(());
    }
    if unplaced > 0 {
        out.warnings.push(format!(
            "{unplaced} embedded code object(s) have no executable segment or no load record; instructions executed there stay unattributed."
        ));
    }
    let workspace = match tempfile::tempdir().and_then(|w| w.path().canonicalize().map(|root| (w, root))) {
        Ok(pair) => pair,
        Err(e) => {
            skip(out, format!("cannot create a decode workspace ({e})."));
            return Ok(());
        }
    };
    let (workspace, root) = workspace;
    let staged = (|| -> Result<Vec<serde_json::Value>, String> {
        let objects = root.join("code-objects");
        std::fs::create_dir(&objects).map_err(|e| e.to_string())?;
        placed
            .iter()
            .enumerate()
            .map(|(i, (object, (start, end)))| {
                let path = objects.join(format!("hrx_code_object_id_{}.out", i + 1));
                std::fs::write(&path, object.elf).map_err(|e| e.to_string())?;
                let loads: Vec<serde_json::Value> = object
                    .load_deltas
                    .iter()
                    .map(|delta| serde_json::json!({"base": delta, "start": start, "size": end}))
                    .collect();
                // No "name": the decoder keeps each kernel's own ELF symbol.
                Ok(serde_json::json!({
                    "id": i + 1,
                    "path": path,
                    "base": object.load_deltas[0],
                    "start": start,
                    "size": end,
                    "loads": loads,
                }))
            })
            .collect()
    })();
    let code_objects = match staged {
        Ok(list) => list,
        Err(e) => {
            skip(out, format!("cannot stage the embedded code objects ({e})."));
            return Ok(());
        }
    };

    // One decode per traced operation: its shader engines share one clock fit
    // reference and one instruction table.
    let mut groups: Vec<((u64, u64), Vec<&TraceChunk>)> = vec![];
    for chunk in &ctx.chunks {
        let key = (chunk.session, chunk.trace_id);
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, list)) => list.push(chunk),
            None => groups.push((key, vec![chunk])),
        }
    }
    let mut imported: Vec<Imported> = vec![];
    for ((session, trace_id), chunks) in &groups {
        cancel()?;
        let label = if groups.len() > 1 {
            format!("Thread trace {trace_id}")
        } else {
            "Thread trace".to_owned()
        };
        let result = import_group(
            out,
            &ctx,
            *session,
            *trace_id,
            chunks,
            &code_objects,
            &workspace,
            &root,
            dispatch_ticks,
            decode,
        );
        cancel()?;
        match result {
            Ok(done) => imported.push(done),
            Err(reason) => out.warnings.push(format!("{label} not imported: {reason}")),
        }
    }
    if imported.is_empty() {
        out.metadata.insert("hrxTraceImport".into(), "not imported".into());
        return Ok(());
    }
    out.root = root;
    out.workspace = Some(std::sync::Arc::new(workspace));
    summarize(out, &ctx, &imported, groups.len());
    Ok(())
}

/// States what every capture record says, whether or not its trace decodes:
/// the selection, the buffer, the loss. This is the bundle's own account.
fn describe_captures(out: &mut Capture, ctx: &TraceContext) {
    if ctx.captures.is_empty() {
        if !ctx.chunks.is_empty() {
            out.warnings.push(
                "The bundle carries thread traces but no executable-trace-captures records, so the traced workgroup processor, SIMD, buffer size and dropped counts are not recorded.".into(),
            );
        }
        return;
    }
    let first = &ctx.captures[0];
    let uniform = ctx.captures.iter().all(|c| {
        c.target_compute_unit == first.target_compute_unit
            && c.simd_select == first.simd_select
            && c.shader_engine_mask == first.shader_engine_mask
            && c.buffer_byte_length == first.buffer_byte_length
    });
    let selection = if uniform {
        format!(
            "workgroup processor {}, SIMD {}, shader engine(s) {}",
            first.target_compute_unit,
            first.simd_select,
            engines(first.shader_engine_mask)
        )
    } else {
        "a selection that differs between traces (see the per-trace lines)".to_owned()
    };
    out.metadata.insert("hrxTraceSelection".into(), selection.clone());
    out.metadata.insert(
        "hrxTraceBufferBytes".into(),
        if uniform {
            format!("{} per shader engine", first.buffer_byte_length)
        } else {
            "varies".into()
        },
    );
    let stable = ctx.captures.iter().all(|c| c.flags & CAPTURE_STABLE_POWER_STATE != 0);
    out.metadata.insert(
        "hrxTraceStablePstate".into(),
        if stable { "held" } else { "not held" }.into(),
    );
    let dropped: u64 = ctx.captures.iter().map(|c| c.dropped_count).sum();
    let written: u64 = ctx.captures.iter().map(|c| c.written_byte_length).sum();
    out.metadata.insert("hrxTraceDroppedTokens".into(), dropped.to_string());
    out.metadata.insert("hrxTraceWrittenBytes".into(), written.to_string());
    out.metadata.insert("hrxTraceLimit".into(), first.trace_limit.to_string());
    let skipped: u64 = {
        // One count per trace, repeated on each of its engines.
        let mut per_trace: BTreeMap<(u64, u64), u32> = BTreeMap::new();
        for c in &ctx.captures {
            per_trace.entry((c.session, c.trace_id)).or_insert(c.skipped_count);
        }
        per_trace.values().map(|v| *v as u64).sum()
    };
    out.metadata.insert("hrxTraceSkippedOperations".into(), skipped.to_string());
    let lines: Vec<String> = ctx
        .captures
        .iter()
        .map(|c| {
            let mut state = vec![];
            if c.flags & CAPTURE_UNCOLLECTED != 0 {
                state.push("not collected".to_owned());
            }
            if c.flags & CAPTURE_FINISHED == 0 && c.flags & CAPTURE_UNCOLLECTED == 0 {
                state.push("finish not confirmed".to_owned());
            }
            if c.flags & CAPTURE_BUFFER_FULL != 0 {
                state.push("buffer full".to_owned());
            }
            if c.flags & CAPTURE_WRITE_ERROR != 0 {
                state.push("write error".to_owned());
            }
            if c.dropped_count > 0 {
                state.push(format!("{} tokens dropped", c.dropped_count));
            }
            if state.is_empty() {
                state.push("complete".to_owned());
            }
            format!(
                "trace {} SE {}: WGP {} SIMD {}, {} of {} written, {}; {}",
                c.trace_id,
                c.shader_engine,
                c.target_compute_unit,
                c.simd_select,
                mib(c.written_byte_length),
                mib(c.buffer_byte_length),
                state.join(", "),
                if c.flags & CAPTURE_COMMAND_BUFFER_SCOPE != 0 {
                    format!("whole recorded execution of {} dispatches", c.dispatch_count)
                } else {
                    "one dispatch".to_owned()
                }
            )
        })
        .collect();
    out.metadata.insert("hrxTracePartitions".into(), lines.join(" | "));

    out.warnings.push(format!(
        "Waves and ISA rows are a hardware sample, not a census. SQTT traces one SIMD of one workgroup processor per shader engine — here {selection} — which is roughly 2% of a dispatch's waves on gfx1151. Wave counts, instruction hit counts and the hotspot table describe the sampled waves only; per-kernel time still comes from every dispatch event."
    ));
    if !stable {
        out.warnings.push(
            "No stable GPU power state was held while tracing; large traces lose more data that way.".into(),
        );
    }
    let full: Vec<&TraceCapture> = ctx.captures.iter().filter(|c| c.flags & CAPTURE_BUFFER_FULL != 0).collect();
    if !full.is_empty() {
        out.warnings.push(format!(
            "{} of {} trace buffer(s) filled ({} each): execution after that point was not traced. Re-capture with a larger trace buffer, a narrower filter, or accept the covered window stated in metadata.hrxTraceWindow.",
            full.len(),
            ctx.captures.len(),
            mib(full[0].buffer_byte_length)
        ));
    }
    if dropped > 0 {
        out.warnings.push(format!(
            "The hardware dropped {dropped} trace token(s) for lack of bandwidth; waves that lost tokens are incomplete and instruction counts are lower bounds."
        ));
    }
    let silent = ctx
        .captures
        .iter()
        .filter(|c| c.flags & CAPTURE_UNCOLLECTED != 0 || c.written_byte_length == 0)
        .count();
    if silent > 0 {
        out.warnings.push(format!(
            "{silent} trace partition(s) produced no trace bytes (not collected, or no wave ran on the traced SIMD)."
        ));
    }
    if skipped > 0 {
        out.warnings.push(format!(
            "{skipped} matching operation(s) ran untraced because an earlier trace still held the device's single trace bracket; the bound was {} trace(s).",
            first.trace_limit
        ));
    }
}

#[allow(clippy::too_many_arguments)]
fn import_group(
    out: &mut Capture,
    ctx: &TraceContext,
    session: u64,
    trace_id: u64,
    chunks: &[&TraceChunk],
    code_objects: &[serde_json::Value],
    workspace: &tempfile::TempDir,
    root: &Path,
    dispatch_ticks: &dyn Fn(u64, u64, u64) -> Vec<(u64, u64)>,
    decode: &Decode,
) -> Result<Imported, String> {
    let first = chunks[0];
    let clock = ctx
        .clocks
        .get(&(session, first.device))
        .copied()
        .ok_or("its device recorded no dispatch ticks to place the trace against")?;
    let direct = first.flags & TRACE_FLAG_DISPATCH_EVENT != 0;
    let operation = if direct {
        ctx.events.get(&(session, first.stream, first.dispatch_event_id)).copied()
    } else {
        ctx.submissions.get(&(session, first.stream, first.submission_id)).copied()
    }
    .ok_or(if direct {
        "the dispatch event it names is not in the bundle"
    } else {
        "no dispatch event of the submission it names is in the bundle"
    })?;

    // Stage: one .att per shader engine plus the placed manifest.
    let stage = tempfile::Builder::new()
        .prefix(&format!("trace{trace_id}-"))
        .tempdir_in(workspace.path())
        .map_err(|e| e.to_string())?;
    let input = stage.path().join("input");
    std::fs::create_dir(&input).map_err(|e| e.to_string())?;
    let mut traces = vec![];
    let mut seen = vec![];
    let mut bytes = 0u64;
    for chunk in chunks {
        if seen.contains(&chunk.shader_engine) {
            return Err(format!(
                "it carries two partitions for shader engine {}",
                chunk.shader_engine
            ));
        }
        seen.push(chunk.shader_engine);
        let path = input.join(format!("hrx_shader_engine_{}_0.att", chunk.shader_engine));
        std::fs::write(&path, chunk.bytes).map_err(|e| e.to_string())?;
        bytes += chunk.bytes.len() as u64;
        traces.push(serde_json::json!({"shaderEngine": chunk.shader_engine, "path": path}));
    }
    std::fs::write(
        input.join("radv-manifest.json"),
        serde_json::to_vec(&serde_json::json!({"traces": traces, "codeObjects": code_objects}))
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    // Decode time grows with the trace; the base matches every other raw path.
    let timeout = Duration::from_secs(300 + 6 * (bytes / (1024 * 1024)));
    let run = decode(&stage, &input, timeout).map_err(|e| {
        let first_line = e.lines().next().unwrap_or("").to_owned();
        format!("the trace decoder failed ({})", first_line.chars().take(400).collect::<String>())
    })?;
    // The decoded files must outlive this function: waves load from them lazily.
    let stage_root = stage.keep();
    let relative = run
        .root
        .canonicalize()
        .ok()
        .and_then(|p| p.strip_prefix(root).ok().map(Path::to_owned))
        .ok_or("the decoder wrote outside its workspace")?;
    let _ = stage_root;
    let prefix = format!("{}/", relative.to_string_lossy());

    let decoder_warnings = crate::decoder::decoder_warnings(&run.log);
    let lost = decoder_warnings.iter().any(|w| w.contains("Data Lost"));
    let realtime = crate::import::sidecar(&run.root, "realtime.json")
        .map_err(|_| "the decoder produced no realtime clock pairs to place the trace with")?;
    for (se, anomalies, pairs) in crate::rgp::realtime_anomalies(&realtime) {
        if pairs >= 2 && anomalies > 0 && !lost {
            return Err(format!(
                "the decoded shader clock on {se} jumps {anomalies} times against the realtime clock, which means tokens were misparsed"
            ));
        }
    }
    let decoded = crate::import::load_ui_unaligned(&run.root, true, true)?;

    // Place each shader engine's clock in the dispatch tick domain.
    let pairs = realtime_pairs(&realtime);
    let to_axis = |relative_tick: f64| -> f64 {
        ((operation.first_tick as i128 - clock.origin_tick as i128) as f64 + relative_tick)
            * clock.axis_per_tick
    };
    // A twentieth of a microsecond when the axis is in nanoseconds.
    let tolerance_ticks = 5.;
    let mut maps: BTreeMap<u32, WaveTimeMap> = BTreeMap::new();
    let mut window: Option<(f64, f64)> = None;
    let (mut anchors, mut knots, mut rejected) = (0usize, 0usize, 0usize);
    for (se, engine_pairs) in &pairs {
        let Some(placed) = clock_map(engine_pairs, operation.first_tick, tolerance_ticks) else {
            continue;
        };
        anchors += placed.anchors;
        knots += placed.knots.len();
        rejected += placed.rejected;
        let axis_knots: Vec<[f64; 2]> =
            placed.knots.iter().map(|k| [k[0], to_axis(k[1])]).collect();
        let (lo, hi) = (axis_knots[0][1], axis_knots[axis_knots.len() - 1][1]);
        window = Some(match window {
            Some((a, b)) => (a.min(lo), b.max(hi)),
            None => (lo, hi),
        });
        maps.insert(*se, WaveTimeMap { prefix: prefix.clone(), se: *se, knots: axis_knots });
    }
    if maps.is_empty() {
        return Err("no shader engine has usable realtime clock pairs, so its waves cannot be placed on the dispatch axis".into());
    }

    // Instruction rows are indexed by the tokens of every wave file. All traces
    // of one bundle are staged with the same code objects, so their tables agree
    // row for row; a trace that executed outside every code object appends its
    // own catch-all row. Anything else would corrupt the join and is refused.
    let shared = out.instructions.len().min(decoded.instructions.len());
    if out
        .instructions
        .iter()
        .zip(&decoded.instructions)
        .any(|(a, b)| a.address != b.address || a.code_object != b.code_object)
    {
        return Err("its instruction table does not match the earlier trace's".into());
    }
    let mean_scale = maps.values().map(WaveTimeMap::mean_scale).sum::<f64>() / maps.len() as f64;
    let in_axis = |row: &Instruction| Instruction {
        latency: row.latency * mean_scale,
        stall: row.stall * mean_scale,
        idle: row.idle * mean_scale,
        ..row.clone()
    };
    for (into, row) in out.instructions.iter_mut().zip(&decoded.instructions) {
        into.hits = into.hits.saturating_add(row.hits);
        into.latency += row.latency * mean_scale;
        into.stall += row.stall * mean_scale;
        into.idle += row.idle * mean_scale;
    }
    out.instructions.extend(decoded.instructions[shared..].iter().map(in_axis));

    let mut unplaced_waves = 0usize;
    let mut waves = 0usize;
    for wave in &decoded.waves {
        let Some(map) = maps.get(&wave.se) else {
            unplaced_waves += 1;
            continue;
        };
        out.waves.push(WaveIndex {
            id: format!("{prefix}{}", wave.id),
            start: map.at(wave.start),
            end: map.at(wave.end),
            ..wave.clone()
        });
        waves += 1;
    }
    for interval in &decoded.intervals {
        if let Some(map) = maps.get(&interval.se) {
            out.intervals.push(Interval {
                start: map.at(interval.start),
                end: map.at(interval.end),
                ..interval.clone()
            });
        }
    }
    for event in &decoded.decoder_events {
        if let Some(map) = maps.get(&event.se) {
            out.decoder_events.push(DecoderEvent {
                id: format!("t{trace_id}:{}", event.id),
                time: map.at(event.time),
                ..event.clone()
            });
        }
    }
    for activity in &decoded.other_simd {
        if let Some(map) = maps.get(&activity.se) {
            out.other_simd.push(OtherSimd {
                start: map.at(activity.start),
                duration: map.span(activity.start, activity.duration),
                ..activity.clone()
            });
        }
    }
    out.wave_time_maps.extend(maps.into_values());
    if unplaced_waves > 0 {
        out.warnings.push(format!(
            "{unplaced_waves} decoded wave(s) of trace {trace_id} sit on a shader engine without realtime clock pairs and were not placed."
        ));
    }
    for warning in &decoder_warnings {
        out.warnings.push(format!("Trace decoder (trace {trace_id}): {warning}"));
    }
    for warning in decoded
        .warnings
        .iter()
        .filter(|w| w.contains("unmatched occupancy") || w.starts_with("Other-SIMD activity"))
    {
        out.warnings.push(format!("Trace {trace_id}: {warning}"));
    }
    if let Some(gfxip) = decoded.metadata.get("gfxip").filter(|g| !g.is_empty()) {
        out.metadata.insert("hrxTraceDecoderGfxip".into(), gfxip.clone());
    }
    if let Some(costs) = decoded.metadata.get("instruction_costs") {
        out.metadata.insert("instruction_costs".into(), costs.clone());
    }

    let operation_axis = (to_axis(0.), to_axis((operation.last_tick - operation.first_tick) as f64));
    let covered_dispatches = window.map(|(_, hi)| {
        let ticks = if direct {
            vec![(operation.first_tick, operation.last_tick)]
        } else {
            dispatch_ticks(session, first.stream, first.submission_id)
        };
        ticks
            .iter()
            .filter(|(start, _)| {
                ((*start as i128 - clock.origin_tick as i128) as f64) * clock.axis_per_tick <= hi
            })
            .count()
    });
    Ok(Imported {
        trace_id,
        waves,
        command_buffer_scope: !direct,
        window,
        operation: Some(operation_axis),
        operation_dispatches: operation.dispatches,
        covered_dispatches,
        anchors,
        knots,
        rejected_anchors: rejected,
        lost: lost || chunks.iter().any(|c| c.truncated || c.dropped > 0),
    })
}

/// Capture-level provenance of the traces that did import.
fn summarize(out: &mut Capture, ctx: &TraceContext, imported: &[Imported], traced: usize) {
    let unit = ctx.unit;
    let waves: usize = imported.iter().map(|t| t.waves).sum();
    let hit_rows = out.instructions.iter().filter(|i| i.hits > 0).count();
    out.metadata.insert("hrxTraceImport".into(), format!("{} of {traced} trace(s) decoded", imported.len()));
    out.metadata.insert("hrxTraceCount".into(), imported.len().to_string());
    out.metadata.insert("hrxTraceWaves".into(), waves.to_string());
    out.metadata.insert("hrxTraceWavesAreASample".into(), "true".into());
    out.metadata.insert("hrxTraceInstructionRows".into(), out.instructions.len().to_string());
    out.metadata.insert("hrxTraceInstructionRowsWithHits".into(), hit_rows.to_string());
    let scale = if unit == "ns" { 1e-6 } else { 1. };
    let label = if unit == "ns" { "ms" } else { unit };
    let windows: Vec<String> = imported
        .iter()
        .map(|t| {
            let scope = if t.command_buffer_scope {
                format!("recorded execution of {} dispatch event(s)", t.operation_dispatches)
            } else {
                "one dispatch".to_owned()
            };
            match (t.window, t.operation) {
                (Some((lo, hi)), Some((begin, end))) => {
                    let traced = (hi.min(end) - lo.max(begin)).max(0.);
                    let whole = (end - begin).max(0.);
                    let share = if whole > 0. { 100. * traced / whole } else { 100. };
                    format!(
                        "trace {}: {} waves over {:.3} {label} ({:.3} to {:.3}), covering {:.1}% of its {scope} ({:.3} {label}){}",
                        t.trace_id,
                        t.waves,
                        (hi - lo) * scale,
                        lo * scale,
                        hi * scale,
                        share,
                        whole * scale,
                        t.covered_dispatches
                            .map(|n| format!("; {n} of {} dispatch event(s) start inside the traced window", t.operation_dispatches))
                            .unwrap_or_default(),
                    )
                }
                _ => format!("trace {}: {} waves, window not established", t.trace_id, t.waves),
            }
        })
        .collect();
    out.metadata.insert("hrxTraceWindow".into(), windows.join(" | "));
    let anchors: usize = imported.iter().map(|t| t.anchors).sum();
    let knots: usize = imported.iter().map(|t| t.knots).sum();
    let rejected: usize = imported.iter().map(|t| t.rejected_anchors).sum();
    out.metadata.insert(
        "hrxTraceClockPlacement".into(),
        format!(
            "waves placed on the dispatch axis through each shader engine's SQTT realtime anchors (the GPU timestamp counter the dispatch events are stamped with): piecewise-linear through {anchors} anchor(s), kept as {knots} knot(s) within {}{}",
            if unit == "ns" { "0.1 µs" } else { "10 ticks" },
            if rejected > 0 {
                format!("; {rejected} anchor(s) where a clock ran backwards were left out")
            } else {
                String::new()
            }
        ),
    );
    let selection = out
        .metadata
        .get("hrxTraceSelection")
        .cloned()
        .unwrap_or_else(|| "a selection the bundle does not record".into());
    out.metadata.insert(
        "coverage".into(),
        format!(
            "HRX native IRPF dispatch stream (every dispatch, raw device ticks, no host-timeline alignment) plus SQTT thread trace of {} traced operation(s). The trace is a sample: {selection}, roughly 2% of waves on gfx1151. {} waves decoded; {}",
            imported.len(),
            waves,
            windows.join("; ")
        ),
    );
    if imported.iter().any(|t| t.lost) {
        out.warnings.push(
            "At least one thread trace lost data (dropped tokens, a full buffer or an unfinished stop). It was imported anyway: waves the decoder completed are exact, the rest are cut short, and the instruction table undercounts.".into(),
        );
    }
    // Instruction costs were converted with the mean of the engines' clock rates.
    out.warnings.push(format!(
        "Thread-trace waves are placed on the dispatch axis ({unit}) through the trace's realtime anchors, so wave and token times follow the shader clock as it slows between dispatches. Instruction latencies in the ISA table are converted with each trace's mean clock rate instead and are approximate in {unit}."
    ));
    // A trace opens just before its first dispatch, so waves of the driver's
    // own setup kernels can precede the first dispatch event.
    out.start = out
        .waves
        .iter()
        .map(|w| w.start)
        .chain(out.intervals.iter().map(|i| i.start))
        .fold(out.start, f64::min);
    out.end = out
        .waves
        .iter()
        .map(|w| w.end)
        .chain(out.intervals.iter().map(|i| i.end))
        .fold(out.end, f64::max);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_map_widens_realtime_and_keeps_a_straight_clock_as_two_knots() {
        // 7 cycles per tick, trace starts 1,000 ticks after the reference.
        let reference = (62u64 << 36) | 54_519_179_296;
        let anchors: Vec<(f64, u64)> = (0..50)
            .map(|i| {
                let tick = reference + 1_000 + i * 4_096;
                (4_665. + (i * 4_096 * 7) as f64, tick & ((1 << 36) - 1))
            })
            .collect();
        let map = clock_map(&anchors, reference, 5.).unwrap();
        assert_eq!(map.knots, vec![[4_665., 1_000.], [4_665. + (49. * 4_096. * 7.), 1_000. + 49. * 4_096.]]);
        assert_eq!((map.anchors, map.rejected), (50, 0));
    }

    #[test]
    fn clock_map_bends_where_the_shader_clock_slows() {
        // Full rate, then an idle gap where the shader clock nearly stops, then
        // full rate again: a single line would misplace the second burst.
        let mut anchors = vec![];
        let (mut cycle, mut tick) = (0f64, 10_000u64);
        for segment in [(40, 4_096., 588), (1, 4_096., 20_000), (40, 4_096., 588)] {
            for _ in 0..segment.0 {
                anchors.push((cycle, tick));
                cycle += segment.1;
                tick += segment.2;
            }
        }
        anchors.push((cycle, tick));
        let map = clock_map(&anchors, 10_000, 5.).unwrap();
        assert_eq!(map.knots.len(), 4, "{:?}", map.knots);
        let placed = WaveTimeMap { prefix: String::new(), se: 0, knots: map.knots.clone() };
        for (cycle, tick) in &anchors {
            assert!((placed.at(*cycle) - (*tick - 10_000) as f64).abs() <= 10., "{cycle}");
        }
        // A token straddling the gap takes the gap's real time, not the mean rate.
        let across = placed.span(40. * 4_096. - 10., 4_106. + 10.);
        assert!(across > 19_000., "{across}");
        // Outside the anchors the nearest segment continues.
        assert!((placed.at(-4_096.) - -588.).abs() < 1e-6);
    }

    #[test]
    fn clock_map_follows_a_counter_that_wrapped_its_36_bits() {
        let reference = (5u64 << 36) - 2_000;
        let anchors: Vec<(f64, u64)> = (0..10)
            .map(|i| (i as f64 * 100., (reference + i * 1_000) & ((1 << 36) - 1)))
            .collect();
        let map = clock_map(&anchors, reference, 5.).unwrap();
        assert_eq!(map.knots.first().unwrap()[1], 0.);
        assert_eq!(map.knots.last().unwrap()[1], 9_000.);
    }

    #[test]
    fn clock_map_refuses_clocks_that_do_not_run_forwards() {
        assert!(clock_map(&[], 0, 5.).is_none());
        assert!(clock_map(&[(1., 5)], 0, 5.).is_none());
        assert!(clock_map(&[(1., 5), (1., 9)], 0, 5.).is_none());
        assert!(clock_map(&[(1., 900), (2., 800), (3., 700)], 800, 5.).is_none());
        // One stray anchor among many is dropped and counted, not followed.
        let mut anchors: Vec<(f64, u64)> = (0..20).map(|i| (i as f64 * 10., 1_000 + i * 100)).collect();
        anchors[7].1 = 5;
        let map = clock_map(&anchors, 1_000, 5.).unwrap();
        assert_eq!(map.rejected, 1);
        assert_eq!(map.knots.len(), 2);
    }

    #[test]
    fn executable_extent_reads_only_executable_load_segments() {
        let mut elf = vec![0u8; 64 + 3 * 56];
        elf[..4].copy_from_slice(b"\x7fELF");
        elf[4] = 2;
        elf[5] = 1;
        elf[0x20..0x28].copy_from_slice(&64u64.to_le_bytes());
        elf[0x36..0x38].copy_from_slice(&56u16.to_le_bytes());
        elf[0x38..0x3a].copy_from_slice(&3u16.to_le_bytes());
        let mut segment = |i: usize, kind: u32, flags: u32, vaddr: u64, memsz: u64| {
            let at = 64 + i * 56;
            elf[at..at + 4].copy_from_slice(&kind.to_le_bytes());
            elf[at + 4..at + 8].copy_from_slice(&flags.to_le_bytes());
            elf[at + 16..at + 24].copy_from_slice(&vaddr.to_le_bytes());
            elf[at + 40..at + 48].copy_from_slice(&memsz.to_le_bytes());
        };
        segment(0, 1, 4, 0, 0x1000); // R
        segment(1, 1, 5, 0x1000, 0x240); // R+X
        segment(2, 2, 6, 0x2000, 0x80); // DYNAMIC
        assert_eq!(executable_extent(&elf), Some((0x1000, 0x1240)));
        // Truncated, non-ELF and segment-less inputs place nothing.
        assert_eq!(executable_extent(&elf[..100]), None);
        assert_eq!(executable_extent(b"not an elf"), None);
        elf[64 + 56 + 4] = 4;
        assert_eq!(executable_extent(&elf), None);
    }

    fn trace_payload(format: u32, declared: u64, bytes: &[u8]) -> Vec<u8> {
        let mut p = vec![0u8; TRACE_MIN];
        p[0..4].copy_from_slice(&(TRACE_MIN as u32).to_le_bytes());
        p[4..8].copy_from_slice(&format.to_le_bytes());
        p[8..12].copy_from_slice(&1u32.to_le_bytes());
        p[12..16].copy_from_slice(&1u32.to_le_bytes());
        p[16..24].copy_from_slice(&7u64.to_le_bytes());
        p[24..32].copy_from_slice(&41u64.to_le_bytes());
        p[32..40].copy_from_slice(&10u64.to_le_bytes());
        p[40..48].copy_from_slice(&22u64.to_le_bytes());
        p[80..88].copy_from_slice(&declared.to_le_bytes());
        p.extend_from_slice(bytes);
        p
    }

    #[test]
    fn trace_chunk_is_bounds_checked() {
        let payload = trace_payload(FORMAT_AMDGPU_ATT, 4, &[1, 2, 3, 4]);
        let chunk = parse_trace_chunk(3, 0, 1, true, 70, &payload).unwrap().unwrap();
        assert_eq!((chunk.trace_id, chunk.shader_engine, chunk.bytes), (7, 1, &[1u8, 2, 3, 4][..]));
        assert_eq!((chunk.dispatch_event_id, chunk.submission_id, chunk.command_buffer_id), (41, 10, 22));
        assert!(chunk.truncated && chunk.dropped == 70);
        // A declared length that disagrees with the payload is rejected, not clipped.
        let lying = trace_payload(FORMAT_AMDGPU_ATT, 9, &[1, 2, 3, 4]);
        assert!(parse_trace_chunk(3, 0, 1, false, 0, &lying).unwrap_err().contains("declares 9 trace bytes but carries 4"));
        assert!(parse_trace_chunk(3, 0, 1, false, 0, &payload[..40]).is_err());
        let mut oversized = payload.clone();
        oversized[0..4].copy_from_slice(&500u32.to_le_bytes());
        assert!(parse_trace_chunk(3, 0, 1, false, 0, &oversized).is_err());
        // Another trace format is skipped, not misread as SQTT.
        let foreign = trace_payload(9, 4, &[1, 2, 3, 4]);
        assert!(parse_trace_chunk(3, 0, 1, false, 0, &foreign).unwrap().is_none());
    }

    #[test]
    fn trace_capture_record_reads_every_field() {
        let mut r = vec![0u8; TRACE_CAPTURE_MIN];
        r[0..4].copy_from_slice(&80u32.to_le_bytes());
        r[4..8].copy_from_slice(&23u32.to_le_bytes());
        r[8..16].copy_from_slice(&1u64.to_le_bytes());
        r[16..20].copy_from_slice(&1u32.to_le_bytes());
        r[24..28].copy_from_slice(&3u32.to_le_bytes());
        r[28..32].copy_from_slice(&4u32.to_le_bytes());
        r[32..36].copy_from_slice(&2u32.to_le_bytes());
        r[36..40].copy_from_slice(&1060u32.to_le_bytes());
        r[40..48].copy_from_slice(&4_194_304u64.to_le_bytes());
        r[48..56].copy_from_slice(&4_194_272u64.to_le_bytes());
        r[56..64].copy_from_slice(&79u64.to_le_bytes());
        r[64..68].copy_from_slice(&0x9004_0000u32.to_le_bytes());
        r[68..72].copy_from_slice(&0x003f_0683u32.to_le_bytes());
        r[72..76].copy_from_slice(&1u32.to_le_bytes());
        r[76..80].copy_from_slice(&5u32.to_le_bytes());
        let c = parse_trace_capture(9, &r).unwrap();
        assert_eq!((c.session, c.trace_id, c.shader_engine, c.flags), (9, 1, 1, 23));
        assert_eq!((c.shader_engine_mask, c.target_compute_unit, c.simd_select), (3, 4, 2));
        assert_eq!((c.dispatch_count, c.buffer_byte_length, c.written_byte_length), (1060, 4_194_304, 4_194_272));
        assert_eq!((c.dropped_count, c.token_mask, c.trace_limit, c.skipped_count), (79, 0x003f_0683, 1, 5));
        assert!(parse_trace_capture(9, &r[..60]).is_err());
    }
}
