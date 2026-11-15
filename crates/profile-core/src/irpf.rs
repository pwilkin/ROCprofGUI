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
const RECORD_TYPE_SESSION_END: u16 = 3;

/// `IREE_HAL_PROFILE_CHUNK_FLAG_TRUNCATED` (profile_sink.h): the chunk is partial
/// because the producer dropped or truncated data. `chunk_flags` is a `uint64_t`.
const CHUNK_FLAG_TRUNCATED: u64 = 1 << 0;

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
/// Smallest self-sized sub-record: families that are only counted need just the
/// leading `uint32 record_length` to be walkable.
const SUBRECORD_MIN: usize = 4;

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
    /// Producer name string that follows the content type (e.g. `"amd-gpu"`);
    /// empty when the producer wrote none.
    name: &'a str,
    payload: &'a [u8],
    /// `session_id`@16 — one profiling session per device in a device group.
    session_id: u64,
    /// `stream_id`@24 — the queue stream a CHUNK belongs to.
    stream_id: u64,
    /// `physical_device_ordinal`@68 of the chunk (UINT32_MAX when not device-scoped).
    physical_device_ordinal: u32,
    /// `queue_ordinal`@72 of the chunk (UINT32_MAX when not queue-scoped).
    queue_ordinal: u32,
    /// `chunk_flags`@80 — carries [`CHUNK_FLAG_TRUNCATED`] for CHUNK records.
    chunk_flags: u64,
    /// `dropped_record_count`@88 — typed records the producer omitted (0 if none/unknown).
    dropped_record_count: u64,
    /// `session_status_code`@96 — nonzero means a SESSION_END record reports failure.
    session_status_code: u32,
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
    // The file-header `flags`@12 field is reserved and must be zero; a nonzero
    // value means a producer wrote a format revision this importer does not read.
    if u32_at(d, 12)? != 0 {
        return Err("IRPF file header reserved flags field is nonzero; refusing to import an unknown format revision".into());
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
        // chunk_flags@80, dropped_record_count@88 and session_status_code@96 sit
        // inside the fixed 104-byte header (bounds already ensured above).
        let session_id = u64_at(logical, pos + 16)?;
        let stream_id = u64_at(logical, pos + 24)?;
        let physical_device_ordinal = u32_at(logical, pos + 68)?;
        let queue_ordinal = u32_at(logical, pos + 72)?;
        let chunk_flags = u64_at(logical, pos + 80)?;
        let dropped_record_count = u64_at(logical, pos + 88)?;
        let session_status_code = u32_at(logical, pos + 96)?;
        let record_type = u16_at(logical, pos + 100)?;
        // The record-header `flags`@102 field is reserved and must be zero.
        if u16_at(logical, pos + 102)? != 0 {
            return Err("IRPF record header reserved flags field is nonzero; refusing to import an unknown format revision".into());
        }
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
            name: std::str::from_utf8(&logical[name_start..payload_start]).unwrap_or(""),
            payload,
            session_id,
            stream_id,
            physical_device_ordinal,
            queue_ordinal,
            chunk_flags,
            dropped_record_count,
            session_status_code,
        });
        pos = end;
    }
    Ok(out)
}

/// Whether any CHUNK of a family carries payload bytes.
fn family_has_payload(records: &[Record], family: &str) -> bool {
    records
        .iter()
        .any(|r| r.record_type == RECORD_TYPE_CHUNK && r.family == Some(family) && !r.payload.is_empty())
}

/// Walks every CHUNK of one content-type family in file order, calling `parse`
/// on each self-sized sub-record together with the chunk record that carried it.
/// The chunk header is the only place a record's session, physical device, queue
/// and stream are stated for most families, and the native AMD driver splits a
/// family across many chunks (one per queue and per flush), so the payloads must
/// not be concatenated into one anonymous array. The record cap is cumulative
/// across the family's chunks.
fn walk_family<T>(
    records: &[Record],
    family: &str,
    min_len: usize,
    cap: usize,
    cancelled: &AtomicBool,
    mut parse: impl FnMut(&Record, &[u8]) -> Result<T, String>,
) -> Result<Vec<T>, String> {
    let mut out = Vec::new();
    for record in records {
        if record.record_type == RECORD_TYPE_CHUNK && record.family == Some(family) {
            walk_subrecords(record.payload, family, min_len, cap, cancelled, &mut out, |sub| {
                parse(record, sub)
            })?;
        }
    }
    Ok(out)
}

/// Whether any CHUNK record of a family is flagged TRUNCATED, and the total
/// producer-side dropped-record count across that family's chunks. A TRUNCATED
/// chunk means the producer dropped or truncated data, so the family is partial.
fn chunk_truncation(records: &[Record], family: &str) -> (bool, u64) {
    let mut truncated = false;
    let mut dropped = 0u64;
    for r in records {
        if r.record_type == RECORD_TYPE_CHUNK && r.family == Some(family) {
            if r.chunk_flags & CHUNK_FLAG_TRUNCATED != 0 {
                truncated = true;
            }
            dropped = dropped.saturating_add(r.dropped_record_count);
        }
    }
    (truncated, dropped)
}

/// Every CHUNK family present in the bundle, in first-seen order — so truncation
/// can be checked per family without the build function naming them all.
fn chunk_families<'a>(records: &[Record<'a>]) -> Vec<&'a str> {
    let mut seen = Vec::new();
    for r in records {
        if r.record_type == RECORD_TYPE_CHUNK {
            if let Some(f) = r.family {
                if !seen.contains(&f) {
                    seen.push(f);
                }
            }
        }
    }
    seen
}

/// Walks a chunk payload as a packed array of self-sized sub-records, calling
/// `parse` on each. Every sub-record begins with a `uint32 record_length`;
/// the walk is bounds-checked and capped by [`crate::caps::max_records`].
/// Parsed records are appended to `out`, so the cap counts across chunks.
fn walk_subrecords<T>(
    payload: &[u8],
    family: &str,
    min_len: usize,
    cap: usize,
    cancelled: &AtomicBool,
    out: &mut Vec<T>,
    mut parse: impl FnMut(&[u8]) -> Result<T, String>,
) -> Result<(), String> {
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
    Ok(())
}

struct Device {
    ordinal: u32,
    frequency_hz: Option<u64>,
    /// Producer name from the chunk header (e.g. `amd-gpu`); may be empty.
    producer: String,
}

impl Device {
    fn label(&self) -> String {
        if self.producer.is_empty() {
            format!("HRX device {}", self.ordinal)
        } else {
            format!("HRX device {} ({})", self.ordinal, self.producer)
        }
    }
}

/// Resolved code-object identity gathered from the code-objects chunk and merged
/// with its native placements.
struct CodeObject {
    executable_id: u64,
    code_object_id: u64,
    data_length: u64,
    is_elf: bool,
    load_delta: Option<i64>,
    load_size: Option<u64>,
    /// Native placements recorded for this image. The schema allows several per
    /// device (queue-private instances); only the first range is surfaced.
    load_count: usize,
}

/// A cheap structural summary of one IRPF bundle, for collection reports: what
/// the producer wrote, without building a capture.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleSummary {
    pub bytes: u64,
    /// False when the file header has no logical length yet (the producer did not
    /// reach its session-end record).
    pub finalized: bool,
    pub sessions: usize,
    /// Producer names of the sessions, in first-seen order.
    pub producers: Vec<String>,
    pub functions: usize,
    pub dispatch_events: usize,
    pub queue_events: usize,
    /// `executable-traces` chunks (one per traced shader engine) and their raw
    /// trace bytes. Zero when the family was not requested or produced nothing.
    pub executable_traces: usize,
    pub executable_trace_bytes: u64,
    /// `counter-samples` records of every producer.
    pub counter_samples: usize,
    /// `counter-sets` records and the names of the counters they define, in
    /// first-seen order. Empty when no producer emitted hardware counters.
    pub counter_sets: usize,
    pub counter_names: Vec<String>,
    /// What the producers said about how the counters were sampled, summed over
    /// the bundle's `counter-captures` records; absent when there is none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counter_capture: Option<CounterCaptureSummary>,
    /// Families with a TRUNCATED chunk, with the producer's dropped-record count.
    pub truncated: Vec<TruncatedFamily>,
    /// Nonzero session-end status codes.
    pub failed_sessions: Vec<u32>,
    /// Record counts per producer (one per device driver of a group), in
    /// first-seen order, so a consumer can tell which device recorded dispatches.
    pub per_producer: Vec<ProducerSummary>,
}

/// The bundle's `counter-captures` records, one per counter set, combined.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CounterCaptureSummary {
    /// Counter sets that have a capture record.
    pub records: usize,
    /// Every record reports an exclusive sample window among its device's queues.
    pub exclusive_window: bool,
    /// Every record reports a stable GPU power state.
    pub stable_power_state: bool,
    /// A record reports its sample bound was reached.
    pub limit_reached: bool,
    /// Largest sample bound, in dispatches.
    pub sample_limit: u64,
    pub sampled_operations: u64,
    pub sampled_dispatches: u64,
    pub skipped_operations: u64,
    pub dropped_samples: u64,
    pub invalid_samples: u64,
    /// Sampled operations that found another collector's perfmon state.
    pub foreign_state: u64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProducerSummary {
    /// Producer name of the sessions counted here; empty when the bundle names none.
    pub producer: String,
    pub sessions: usize,
    pub dispatch_events: usize,
    pub queue_events: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TruncatedFamily {
    pub family: String,
    pub dropped: u64,
}

/// Summarises a bundle on disk. An unfinalized bundle is reported with
/// `finalized: false` rather than as an error, so a collector can tell "the
/// target was killed before it closed its profile" from a malformed file.
pub fn summarize(path: &Path) -> Result<BundleSummary, String> {
    let bytes = std::fs::metadata(path).map_err(err)?.len();
    if bytes > crate::caps::max_file_bytes() {
        return Err(format!(
            "IRPF bundle exceeds the {} MiB per-file import limit",
            crate::caps::max_file_mib()
        ));
    }
    let data = std::fs::read(path).map_err(err)?;
    summarize_bytes(&data)
}

fn summarize_bytes(data: &[u8]) -> Result<BundleSummary, String> {
    let mut summary = BundleSummary {
        bytes: data.len() as u64,
        finalized: false,
        sessions: 0,
        producers: Vec::new(),
        functions: 0,
        dispatch_events: 0,
        queue_events: 0,
        executable_traces: 0,
        executable_trace_bytes: 0,
        counter_samples: 0,
        counter_sets: 0,
        counter_names: Vec::new(),
        counter_capture: None,
        truncated: Vec::new(),
        failed_sessions: Vec::new(),
        per_producer: Vec::new(),
    };
    if data.len() >= FILE_HEADER_LEN && u32_at(data, 0)? == MAGIC && u64_at(data, 16)? == 0 {
        return Ok(summary);
    }
    let records = parse_records(data)?;
    summary.finalized = true;
    let cap = crate::caps::max_records();
    let never = AtomicBool::new(false);
    // Each session's producer is named by the session's first record.
    let mut sessions: Vec<(u64, &str)> = Vec::new();
    for r in &records {
        if !sessions.iter().any(|(id, _)| *id == r.session_id) {
            sessions.push((r.session_id, r.name));
            if !r.name.is_empty() && !summary.producers.iter().any(|p| p == r.name) {
                summary.producers.push(r.name.to_owned());
            }
        }
        if r.record_type == RECORD_TYPE_SESSION_END && r.session_status_code != 0 {
            summary.failed_sessions.push(r.session_status_code);
        }
    }
    summary.sessions = sessions.len();
    // The session of every sub-record of a family, one entry per sub-record.
    let owners = |family: &str, min: usize| -> Result<Vec<u64>, String> {
        walk_family(&records, family, min, cap, &never, |chunk, _| Ok(chunk.session_id))
    };
    summary.functions = owners("executable-functions", FUNCTION_MIN)?.len();
    let dispatch_owners = owners("dispatch-events", DISPATCH_EVENT_MIN)?;
    let queue_owners = owners("queue-events", SUBRECORD_MIN)?;
    summary.dispatch_events = dispatch_owners.len();
    summary.queue_events = queue_owners.len();
    summary.counter_samples = owners("counter-samples", SUBRECORD_MIN)?.len();
    summary.counter_sets = owners("counter-sets", SUBRECORD_MIN)?.len();
    // Counter metadata is descriptive here: damage in it is the import's to
    // report, not a reason to refuse the summary.
    let counter_chunks = |family: &str| -> Vec<crate::irpf_counters::Chunk<'_>> {
        records
            .iter()
            .filter(|r| r.record_type == RECORD_TYPE_CHUNK && r.family == Some(family))
            .map(|r| crate::irpf_counters::Chunk {
                session: r.session_id,
                device: r.physical_device_ordinal,
                queue: r.queue_ordinal,
                stream: r.stream_id,
                payload: r.payload,
            })
            .collect()
    };
    summary.counter_names =
        crate::irpf_counters::counter_names(&counter_chunks("counters"), &never).unwrap_or_default();
    let captures =
        crate::irpf_counters::parse_captures(&counter_chunks("counter-captures"), &never).unwrap_or_default();
    if !captures.is_empty() {
        use crate::irpf_counters::{CAPTURE_LIMIT_REACHED, CAPTURE_STABLE_POWER_STATE};
        let sum = |field: fn(&crate::irpf_counters::CaptureFacts) -> u64| -> u64 {
            captures.values().map(field).fold(0u64, u64::saturating_add)
        };
        summary.counter_capture = Some(CounterCaptureSummary {
            records: captures.len(),
            exclusive_window: captures.values().all(|c| c.exclusive()),
            stable_power_state: captures.values().all(|c| c.flags & CAPTURE_STABLE_POWER_STATE != 0),
            limit_reached: captures.values().any(|c| c.flags & CAPTURE_LIMIT_REACHED != 0),
            sample_limit: captures.values().map(|c| c.sample_limit).max().unwrap_or(0),
            sampled_operations: sum(|c| c.sampled_operation_count),
            sampled_dispatches: sum(|c| c.sampled_dispatch_count),
            skipped_operations: sum(|c| c.skipped_operation_count),
            dropped_samples: sum(|c| c.dropped_sample_count),
            invalid_samples: sum(|c| c.invalid_sample_count),
            foreign_state: sum(|c| c.foreign_state_count),
        });
    }
    for r in &records {
        if r.record_type == RECORD_TYPE_CHUNK && r.family == Some("executable-traces") {
            // Counted from the chunk extent; the import validates the record.
            summary.executable_traces += 1;
            summary.executable_trace_bytes +=
                r.payload.len().saturating_sub(crate::irpf_trace::TRACE_MIN) as u64;
        }
    }
    for (session, producer) in &sessions {
        let owned = |owners: &[u64]| owners.iter().filter(|id| *id == session).count();
        let (dispatch_events, queue_events) = (owned(&dispatch_owners), owned(&queue_owners));
        match summary.per_producer.iter_mut().find(|p| p.producer == *producer) {
            Some(entry) => {
                entry.sessions += 1;
                entry.dispatch_events += dispatch_events;
                entry.queue_events += queue_events;
            }
            None => summary.per_producer.push(ProducerSummary {
                producer: (*producer).to_owned(),
                sessions: 1,
                dispatch_events,
                queue_events,
            }),
        }
    }
    for family in chunk_families(&records) {
        let (truncated, dropped) = chunk_truncation(&records, family);
        if truncated {
            summary.truncated.push(TruncatedFamily { family: family.to_owned(), dropped });
        }
    }
    Ok(summary)
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
    build_with(data, name, cancelled, &crate::irpf_trace::run_decoder(cancelled))
}

/// `build` with the thread-trace decoder supplied by the caller, so the trace
/// arm can be exercised without the ROCm decoder library.
fn build_with(
    data: &[u8],
    name: String,
    cancelled: &AtomicBool,
    decode: &crate::irpf_trace::Decode,
) -> Result<Capture, String> {
    let (major, minor) = (u16_at(data, 4)?, u16_at(data, 6)?);
    let records = parse_records(data)?;
    let cap = crate::caps::max_records();

    // Sessions. A device group profiles each device in its own session inside one
    // bundle (e.g. a GPU and an NPU), so nothing below may assume one device.
    let mut sessions: Vec<u64> = Vec::new();
    for r in &records {
        if !sessions.contains(&r.session_id) {
            sessions.push(r.session_id);
        }
    }

    // Devices, keyed by (session, physical ordinal). A timestamp frequency (FREQ
    // flag) is authoritative for that device's elapsed tick conversion. Producers
    // re-emit device records on later flushes; the first one wins.
    let mut devices: BTreeMap<(u64, u32), Device> = BTreeMap::new();
    for (key, device) in walk_family(&records, "devices", DEVICE_MIN, cap, cancelled, |chunk, r| {
        let flags = u32_at(r, 4)?;
        let frequency_hz = if flags & DEVICE_FLAG_TIMESTAMP_FREQUENCY != 0 && r.len() >= 40 {
            Some(u64_at(r, 32)?).filter(|f| *f > 0)
        } else {
            None
        };
        let ordinal = u32_at(r, 8)?;
        Ok((
            (chunk.session_id, ordinal),
            Device {
                ordinal,
                frequency_hz,
                producer: chunk.name.to_owned(),
            },
        ))
    })? {
        devices.entry(key).or_insert(device);
    }
    // The device a chunk belongs to: its own (session, ordinal); else the only
    // device of its session; else the only device of the bundle. Never a guess
    // among several — an unmatched chunk in a multi-device bundle stays unrecorded
    // (the schema's default ordinal is UINT32_MAX).
    let device_of = |session: u64, ordinal: u32| -> Option<&Device> {
        if let Some(d) = devices.get(&(session, ordinal)) {
            return Some(d);
        }
        let mut in_session = devices.iter().filter(|((s, _), _)| *s == session).map(|(_, d)| d);
        match (in_session.next(), in_session.next()) {
            (Some(d), None) => Some(d),
            (None, _) if devices.len() == 1 => devices.values().next(),
            _ => None,
        }
    };
    let device_label = |session: u64, ordinal: u32| -> String {
        device_of(session, ordinal)
            .map(Device::label)
            .unwrap_or_else(|| "HRX device (unrecorded)".into())
    };

    // Queues: (session, device, queue ordinal) → stream id.
    let mut queues: BTreeMap<(u64, u32, u32), u64> = BTreeMap::new();
    for (key, stream) in walk_family(&records, "queues", QUEUE_MIN, cap, cancelled, |chunk, r| {
        Ok(((chunk.session_id, u32_at(r, 4)?, u32_at(r, 8)?), u64_at(r, 16)?))
    })? {
        queues.entry(key).or_insert(stream);
    }
    let single_stream = match queues.len() {
        1 => queues.values().next().copied(),
        _ => None,
    };
    let queue_label = |queue: u32| -> String {
        match single_stream {
            Some(stream) => format!("HRX stream {stream}"),
            None if queue != u32::MAX => format!("HRX queue {queue}"),
            None => "HRX queue".to_owned(),
        }
    };

    // Executable functions: name resolved by (session, executable_id,
    // function_ordinal). Executable identifiers are session-local, so two devices'
    // sessions may reuse one.
    let mut functions: BTreeMap<(u64, u64, u32), String> = BTreeMap::new();
    for (key, nm) in walk_family(
        &records,
        "executable-functions",
        FUNCTION_MIN,
        cap,
        cancelled,
        |chunk, r| {
            let exe = u64_at(r, 8)?;
            let ordinal = u32_at(r, 16)?;
            let name_length = u32_at(r, 44)? as usize;
            let start = FUNCTION_MIN;
            let raw = r
                .get(start..start + name_length)
                .ok_or("IRPF function name extends past its record")?;
            Ok((
                (chunk.session_id, exe, ordinal),
                String::from_utf8_lossy(raw).into_owned(),
            ))
        },
    )? {
        functions.entry(key).or_insert(nm);
    }
    let function_count = functions.len();

    // Code objects: retain ELF identity + the first native placement.
    let mut code_objects: BTreeMap<(u64, u64, u64), CodeObject> = BTreeMap::new();
    for (session, co) in walk_family(
        &records,
        "executable-code-objects",
        CODE_OBJECT_MIN,
        cap,
        cancelled,
        |chunk, r| {
            let data_length = u64_at(r, 24)?;
            let body_start = CODE_OBJECT_MIN;
            let is_elf = r
                .get(body_start..body_start + 4)
                .map(|m| m == b"\x7fELF")
                .unwrap_or(false);
            Ok((
                chunk.session_id,
                CodeObject {
                    executable_id: u64_at(r, 8)?,
                    code_object_id: u64_at(r, 16)?,
                    data_length,
                    is_elf,
                    load_delta: None,
                    load_size: None,
                    load_count: 0,
                },
            ))
        },
    )? {
        code_objects
            .entry((session, co.executable_id, co.code_object_id))
            .or_insert(co);
    }
    // Every distinct placement of every image, for placing thread traces.
    let mut load_deltas: BTreeMap<(u64, u64, u64), Vec<i64>> = BTreeMap::new();
    for (session, exe, co_id, delta, load_size) in walk_family(
        &records,
        "executable-code-object-loads",
        CODE_OBJECT_LOAD_MIN,
        cap,
        cancelled,
        |chunk, r| {
            Ok((
                chunk.session_id,
                u64_at(r, 8)?,
                u64_at(r, 16)?,
                i64_at(r, 24)?,
                u64_at(r, 32)?,
            ))
        },
    )? {
        let deltas = load_deltas.entry((session, exe, co_id)).or_default();
        if !deltas.contains(&delta) {
            deltas.push(delta);
        }
        if let Some(co) = code_objects.get_mut(&(session, exe, co_id)) {
            if co.load_count == 0 {
                co.load_delta = Some(delta);
                co.load_size = Some(load_size);
            }
            // Producers re-emit the same load record on later flushes; only a
            // different range is another placement.
            if co.load_count == 0 || co.load_delta != Some(delta) || co.load_size != Some(load_size) {
                co.load_count += 1;
            }
        }
    }
    let multi_placement = code_objects.values().filter(|co| co.load_count > 1).count();

    // Clock correlations: these bundles mark them DEVICE_TICK_UNALIGNED, so the
    // device clock must not be fit to a host timeline.
    let correlations = walk_family(
        &records,
        "clock-correlations",
        CLOCK_CORRELATION_MIN,
        cap,
        cancelled,
        |_, r| u32_at(r, 4),
    )?;
    let correlation_count = correlations.len();
    let unaligned = correlations
        .iter()
        .any(|flags| flags & CLOCK_FLAG_DEVICE_TICK_UNALIGNED != 0);

    // Queue events are counted, not imported: they are host-timestamped admission
    // records without a kernel identity. The count tells a reader the session did
    // run work even when no dispatch events were recorded.
    let queue_event_count =
        walk_family(&records, "queue-events", SUBRECORD_MIN, cap, cancelled, |_, _| Ok(()))?.len();

    // Dispatches. Prefer device-timestamped dispatch events; fall back to the
    // static command-operation dispatch program ONLY for a genuine queue-mode
    // bundle (no dispatch-events chunk at all). A TRUNCATED dispatch-events chunk
    // with zero payload must not silently fall through to the queue arm: that
    // would relabel a partial dispatch capture as a queue-mode capture with
    // entirely different timing semantics. The producer contract says precise
    // execution timelines should fail the session rather than truncate, so we
    // reject it explicitly instead.
    let has_dispatch_events = family_has_payload(&records, "dispatch-events");
    let (dispatch_truncated, dispatch_dropped) = chunk_truncation(&records, "dispatch-events");
    if !has_dispatch_events && dispatch_truncated {
        return Err(format!(
            "IRPF dispatch-events chunk is TRUNCATED (producer dropped {dispatch_dropped} record(s)) and carries no payload. Refusing to import a partial dispatch capture as complete, and refusing to relabel it as a queue-mode capture. The producer should fail the session rather than truncate dispatch/device timelines (profile_sink.h); re-capture with a bounded filter or a larger sink."
        ));
    }
    let (mut dispatches, dispatch_source) = if has_dispatch_events {
        let raw = walk_family(
            &records,
            "dispatch-events",
            DISPATCH_EVENT_MIN,
            cap,
            cancelled,
            |chunk, r| {
                Ok(RawDispatch {
                    session: chunk.session_id,
                    device: chunk.physical_device_ordinal,
                    queue: chunk.queue_ordinal,
                    stream: chunk.stream_id,
                    event_id: u64_at(r, 8)?,
                    submission: u64_at(r, 16)?,
                    executable_id: u64_at(r, 32)?,
                    function_ordinal: u32_at(r, 44)?,
                    workgroup_count: [u32_at(r, 48)?, u32_at(r, 52)?, u32_at(r, 56)?],
                    workgroup_size: [u32_at(r, 60)?, u32_at(r, 64)?, u32_at(r, 68)?],
                    start_tick: Some(u64_at(r, 72)?),
                    end_tick: Some(u64_at(r, 80)?),
                })
            },
        )?;
        (raw, "dispatch-events")
    } else {
        let raw = walk_family(
            &records,
            "command-operations",
            COMMAND_OPERATION_MIN,
            cap,
            cancelled,
            |chunk, r| {
                let op_type = u32_at(r, 4)?;
                if op_type != COMMAND_OPERATION_TYPE_DISPATCH {
                    return Ok(None);
                }
                Ok(Some(RawDispatch {
                    session: chunk.session_id,
                    device: chunk.physical_device_ordinal,
                    queue: chunk.queue_ordinal,
                    stream: chunk.stream_id,
                    event_id: u64_at(r, 16)?, // command_buffer_id
                    submission: 0,
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
        (raw, "command-operations")
    };

    // A bundle with no dispatch record at all is not an empty capture: every view
    // of this tool is dispatch-based, and opening it silently as "0 dispatches"
    // hides why. This is what the native AMD drivers write when the session was
    // captured without the dispatch-events family (the only family set a mixed
    // GPU + NPU device group accepts without per-device selection is
    // queue-events + executable-metadata), and what an aborted session leaves
    // behind. Say what the bundle does hold and how to get dispatches.
    if dispatches.is_empty() {
        let producers: Vec<String> = devices.values().map(Device::label).collect();
        return Err(format!(
            "IRPF bundle holds no dispatch records: it has no dispatch-events and no command-operations dispatch program. It records {} profiling session(s){}, {function_count} executable function(s) and {queue_event_count} queue event(s). Either the session was captured without the dispatch-events data family, no kernel ran while profiling was active, or profiling stopped before any work (a GPU + NPU device group rejects dispatch-events on the NPU and aborts the whole group). Re-capture with --device_profiling_mode=queue-events,dispatch-events,executable-metadata on a GPU-only device group; see docs/hrx-profiling.md for the mixed GPU + NPU case.",
            sessions.len(),
            if producers.is_empty() {
                String::new()
            } else {
                format!(" ({})", producers.join(", "))
            },
        ));
    }

    // Device clock domains. Every device that recorded dispatch ticks has its own
    // origin (its first tick) and its own frequency; ticks of two devices are never
    // compared.
    let has_ticks = dispatches.iter().any(|d| d.start_tick.is_some());
    let mut origins: BTreeMap<(u64, u32), u64> = BTreeMap::new();
    for d in &dispatches {
        if let Some(start) = d.start_tick {
            origins
                .entry((d.session, d.device))
                .and_modify(|o| *o = (*o).min(start))
                .or_insert(start);
        }
    }
    let tick_devices: Vec<(u64, u32)> = origins.keys().copied().collect();
    let frequency_of =
        |session: u64, device: u32| device_of(session, device).and_then(|d| d.frequency_hz);
    // Nanoseconds only when every tick-bearing device states its frequency.
    let in_ns = has_ticks && tick_devices.iter().all(|(s, d)| frequency_of(*s, *d).is_some());
    let unit = if in_ns {
        "ns"
    } else if has_ticks {
        "device ticks"
    } else {
        "unordered"
    };
    let tick_to_axis = |d: &RawDispatch, tick: u64| -> f64 {
        let origin = origins.get(&(d.session, d.device)).copied().unwrap_or(0);
        let elapsed = tick.saturating_sub(origin) as f64;
        match frequency_of(d.session, d.device) {
            Some(freq) if in_ns => elapsed / freq as f64 * 1e9,
            _ => elapsed,
        }
    };
    dispatches.sort_by(|a, b| {
        let key = |d: &RawDispatch| d.start_tick.map(|t| tick_to_axis(d, t)).unwrap_or(0.);
        key(a)
            .total_cmp(&key(b))
            .then(a.stream.cmp(&b.stream))
            .then(a.event_id.cmp(&b.event_id))
    });
    // `event_id` is unique only within one dispatch-event stream; qualify the id
    // with the stream once a bundle carries more than one.
    let mut streams: Vec<(u64, u64)> = dispatches.iter().map(|d| (d.session, d.stream)).collect();
    streams.sort_unstable();
    streams.dedup();
    let qualify_stream = streams.len() > 1;

    let mut unresolved = 0usize;
    let mut out = Capture::empty(name, "HRX native profile · IRPF 2.0", unit);
    let mut end = 0f64;
    let mut model = Vec::with_capacity(dispatches.len());
    for (index, d) in dispatches.iter().enumerate() {
        if index % 4096 == 0 {
            cancelled_err(cancelled)?;
        }
        let kernel = match functions.get(&(d.session, d.executable_id, d.function_ordinal)) {
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
                let start = tick_to_axis(d, s);
                let finish = tick_to_axis(d, e.max(s));
                end = end.max(finish);
                (start, finish)
            }
            _ => (0., 0.),
        };
        model.push(Dispatch {
            shader_engine: None,
            id: if qualify_stream {
                format!("{dispatch_source}:s{}:{}", d.stream, d.event_id)
            } else {
                format!("{dispatch_source}:{}", d.event_id)
            },
            kernel,
            start,
            end: dispatch_end,
            agent: device_label(d.session, d.device),
            queue: queue_label(d.queue),
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
            workgroup: if d.workgroup_size.iter().all(|s| *s == 0) {
                "Not recorded".into()
            } else {
                format!(
                    "{} × {} × {}",
                    d.workgroup_size[0], d.workgroup_size[1], d.workgroup_size[2]
                )
            },
        });
    }
    out.dispatches = model;
    out.origin = match tick_devices.as_slice() {
        [only] => origins[only].to_string(),
        _ => "0".into(),
    };
    out.start = 0.;
    out.end = end.max(1.);

    // Session-qualify code-object ids only when two sessions carry code objects,
    // so single-session bundles keep their established ids.
    let mut code_sessions: Vec<u64> = code_objects.keys().map(|(s, _, _)| *s).collect();
    code_sessions.dedup();
    let qualify_session = code_sessions.len() > 1;
    for ((session, exe, co_id), co) in &code_objects {
        let mut in_session = devices.iter().filter(|((s, _), _)| s == session).map(|(_, d)| d);
        let agent = match (in_session.next(), in_session.next()) {
            (Some(d), None) => Some(d.label()),
            _ => None,
        };
        out.code_objects.push(CodeObjectRecord {
            id: if qualify_session {
                format!("s{session}:exe{exe}:co{co_id}")
            } else {
                format!("exe{exe}:co{co_id}")
            },
            object_id: co_id.to_string(),
            scope: format!("HRX executable {exe}"),
            agent,
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
    out.metadata
        .insert("hrxSessionCount".into(), sessions.len().to_string());
    out.metadata
        .insert("hrxQueueEventCount".into(), queue_event_count.to_string());
    if !devices.is_empty() {
        out.metadata.insert(
            "hrxDevices".into(),
            devices.values().map(Device::label).collect::<Vec<_>>().join("; "),
        );
    }
    // Frequencies of the devices that carry dispatch ticks; of every device when
    // the bundle has no ticks (queue mode).
    let mut frequencies: Vec<u64> = if has_ticks {
        tick_devices.iter().filter_map(|(s, d)| frequency_of(*s, *d)).collect()
    } else {
        devices.values().filter_map(|d| d.frequency_hz).collect()
    };
    frequencies.sort_unstable();
    frequencies.dedup();
    if !frequencies.is_empty() {
        out.metadata.insert(
            "hrxTimestampFrequencyHz".into(),
            frequencies.iter().map(u64::to_string).collect::<Vec<_>>().join(", "),
        );
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
    match (has_ticks, in_ns) {
        (true, true) => out.warnings.push(format!(
            "Timestamps are raw device ticks converted to elapsed nanoseconds using the device timestamp frequency ({} Hz). Only relative timing within this capture is meaningful.",
            frequencies.iter().map(u64::to_string).collect::<Vec<_>>().join(", ")
        )),
        (true, false) => out.warnings.push(
            "The device record carries no timestamp frequency, so dispatch times are left in raw device ticks; durations are not nanoseconds.".into(),
        ),
        (false, _) => out.warnings.push(
            "This queue-mode bundle has no device-timestamped dispatch events; the dispatches are the static command-operation program and carry no execution timing.".into(),
        ),
    }
    if tick_devices.len() > 1 {
        out.warnings.push(format!(
            "Dispatch events come from {} devices, each in its own device clock domain. Every device's ticks are shown relative to that device's first event; ordering and gaps between devices are not meaningful.",
            tick_devices.len()
        ));
    }
    // Devices of the bundle that contributed no dispatch record. In a mixed device
    // group this is the normal state of a driver that does not implement the
    // dispatch-events family (the AMD XDNA NPU driver), and it means that device's
    // work is absent here — say so instead of presenting a GPU-only table as whole.
    let silent: Vec<String> = devices
        .iter()
        .filter(|((s, o), _)| !dispatches.iter().any(|d| d.session == *s && d.device == *o))
        .map(|(_, d)| d.label())
        .collect();
    if !silent.is_empty() && devices.len() > 1 {
        out.warnings.push(format!(
            "This bundle profiles {} devices, but {} recorded no dispatch records: {}. Work executed there is absent from the dispatch table and from every per-kernel total (a driver that does not implement the dispatch-events data family, such as the AMD XDNA NPU driver, records none).",
            devices.len(),
            silent.len(),
            silent.join(", ")
        ));
    }
    if multi_placement > 0 {
        out.warnings.push(format!(
            "{multi_placement} code object(s) have more than one native placement (queue-private instances); only the first load range of each is shown."
        ));
    }
    // A TRUNCATED chunk of any family means the producer dropped data; surface it
    // loudly (naming the family and dropped count) so partial data is never
    // presented as a complete representation. dispatch-events is handled above
    // (empty+truncated is rejected); a non-empty-but-truncated dispatch stream is
    // imported but still warned here.
    for family in chunk_families(&records) {
        // A truncated thread trace is not a dropped-record ring: its capture
        // records say what was lost, and the trace arm reports that.
        if family == "executable-traces" {
            continue;
        }
        let (truncated, dropped) = chunk_truncation(&records, family);
        if truncated {
            out.warnings.push(format!(
                "IRPF {family} chunk is TRUNCATED: the producer dropped {dropped} record(s) from this stream. The imported {family} data is partial, not a complete representation of the capture. The native AMD GPU driver keeps a bounded ring per queue between flushes; re-capture with a periodic flush (--device_profiling_flush_interval_ms) or a narrower filter."
            ));
        }
    }
    // A SESSION_END record with a nonzero status means the profiled session ended
    // in failure; the capture still imports but must say so rather than present as
    // a clean run (file_length != 0 only proves finalization, not success).
    for r in &records {
        if r.record_type == RECORD_TYPE_SESSION_END && r.session_status_code != 0 {
            out.warnings.push(format!(
                "The HRX profiling session ended with a nonzero status code ({}); this capture may be incomplete or reflect a failed run.",
                r.session_status_code
            ));
        }
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
    // Thread traces: stage, decode and place them on the dispatch axis.
    let mut trace = crate::irpf_trace::TraceContext {
        chunks: Vec::new(),
        captures: Vec::new(),
        objects: Vec::new(),
        submissions: BTreeMap::new(),
        events: BTreeMap::new(),
        clocks: BTreeMap::new(),
        foreign_format_chunks: 0,
        unit,
    };
    for r in &records {
        if r.record_type != RECORD_TYPE_CHUNK || r.family != Some("executable-traces") {
            continue;
        }
        match crate::irpf_trace::parse_trace_chunk(
            r.session_id,
            r.physical_device_ordinal,
            r.stream_id,
            r.chunk_flags & CHUNK_FLAG_TRUNCATED != 0,
            r.dropped_record_count,
            r.payload,
        )? {
            Some(chunk) => trace.chunks.push(chunk),
            None => trace.foreign_format_chunks += 1,
        }
    }
    trace.captures = walk_family(
        &records,
        "executable-trace-captures",
        crate::irpf_trace::TRACE_CAPTURE_MIN,
        cap,
        cancelled,
        |chunk, r| crate::irpf_trace::parse_trace_capture(chunk.session_id, r),
    )?;
    let has_traces = !trace.chunks.is_empty();
    if has_traces {
        // The embedded images, borrowed straight from the bundle.
        for r in &records {
            if r.record_type != RECORD_TYPE_CHUNK || r.family != Some("executable-code-objects") {
                continue;
            }
            let mut off = 0;
            while off < r.payload.len() {
                cancelled_err(cancelled)?;
                let length = u32_at(r.payload, off)? as usize;
                let end = off
                    .checked_add(length)
                    .filter(|end| length >= CODE_OBJECT_MIN && *end <= r.payload.len())
                    .ok_or("IRPF executable-code-objects sub-record extends past its chunk")?;
                let sub = &r.payload[off..end];
                let (exe, co_id) = (u64_at(sub, 8)?, u64_at(sub, 16)?);
                let data_length = u64_at(sub, 24)? as usize;
                let elf = CODE_OBJECT_MIN
                    .checked_add(data_length)
                    .and_then(|end| sub.get(CODE_OBJECT_MIN..end))
                    .ok_or("IRPF code object image extends past its record")?;
                let known = trace.objects.iter().any(|o| {
                    o.session == r.session_id && o.executable_id == exe && o.code_object_id == co_id
                });
                if !known {
                    trace.objects.push(crate::irpf_trace::PlacedObject {
                        session: r.session_id,
                        executable_id: exe,
                        code_object_id: co_id,
                        elf,
                        load_deltas: load_deltas
                            .get(&(r.session_id, exe, co_id))
                            .cloned()
                            .unwrap_or_default(),
                    });
                }
                off = end;
            }
        }
        for d in &dispatches {
            let (Some(start), Some(end)) = (d.start_tick, d.end_tick) else { continue };
            let window = crate::irpf_trace::OperationWindow {
                first_tick: start,
                last_tick: end.max(start),
                dispatches: 1,
            };
            trace.events.insert((d.session, d.stream, d.event_id), window);
            trace
                .submissions
                .entry((d.session, d.stream, d.submission))
                .and_modify(|w| {
                    w.first_tick = w.first_tick.min(window.first_tick);
                    w.last_tick = w.last_tick.max(window.last_tick);
                    w.dispatches += 1;
                })
                .or_insert(window);
        }
        for (key, origin) in &origins {
            trace.clocks.insert(
                *key,
                crate::irpf_trace::DeviceClock {
                    origin_tick: *origin,
                    axis_per_tick: match frequency_of(key.0, key.1) {
                        Some(freq) if in_ns => 1e9 / freq as f64,
                        _ => 1.,
                    },
                },
            );
        }
    }
    let ticks_of = |session: u64, stream: u64, submission: u64| -> Vec<(u64, u64)> {
        dispatches
            .iter()
            .filter(|d| d.session == session && d.stream == stream && d.submission == submission)
            .filter_map(|d| Some((d.start_tick?, d.end_tick?)))
            .collect()
    };
    crate::irpf_trace::attach(&mut out, trace, &ticks_of, cancelled, decode)?;

    // Hardware counters: rows joined to dispatches by dispatch event id.
    let counter_chunks = |family: &str| -> Vec<crate::irpf_counters::Chunk<'_>> {
        records
            .iter()
            .filter(|r| r.record_type == RECORD_TYPE_CHUNK && r.family == Some(family))
            .map(|r| crate::irpf_counters::Chunk {
                session: r.session_id,
                device: r.physical_device_ordinal,
                queue: r.queue_ordinal,
                stream: r.stream_id,
                payload: r.payload,
            })
            .collect()
    };
    let mut counters = crate::irpf_counters::Context {
        sets: counter_chunks("counter-sets"),
        counters: counter_chunks("counters"),
        samples: counter_chunks("counter-samples"),
        captures: counter_chunks("counter-captures"),
        dispatches: BTreeMap::new(),
        cap,
    };
    // Only dispatch events carry the event ids counter samples refer to.
    if counters.present() && dispatch_source == "dispatch-events" {
        for (index, d) in dispatches.iter().enumerate() {
            counters.dispatches.insert((d.session, d.stream, d.event_id), index);
        }
    }
    let counter_outcome = crate::irpf_counters::attach(
        &mut out,
        counters,
        &device_label,
        &queue_label,
        &|session, executable, ordinal| functions.get(&(session, executable, ordinal)).cloned(),
        cancelled,
    )?;
    use crate::irpf_counters::Outcome;
    let traced = !out.waves.is_empty() || !out.instructions.is_empty();
    out.warnings.push(
        match (counter_outcome, traced, has_traces) {
            (Outcome::Absent, true, _) => "Hardware counters are not part of this IRPF bundle; the thread trace above is its only microarchitectural data.",
            (Outcome::Absent, false, true) => "This bundle carries thread traces, but none was imported (see the warnings above), so wave occupancy and ISA hotspots are absent; hardware counters are not part of these IRPF bundles.",
            (Outcome::Absent, false, false) => "Coverage is unknown from a native dispatch stream alone: wave occupancy, ISA hotspots and hardware counters are not part of these IRPF bundles.",
            (Outcome::Imported, true, _) => "This bundle's microarchitectural data are the thread trace above, a sample of waves, and the hardware counters above, window sums for the sampled dispatches. Neither is a census of the capture.",
            (Outcome::Imported, false, true) => "This bundle carries thread traces, but none was imported (see the warnings above), so wave occupancy and ISA hotspots are absent; its hardware counters cover the sampled dispatches only, as window sums.",
            (Outcome::Imported, false, false) => "Wave occupancy and ISA hotspots are not part of this bundle; its hardware counters cover the sampled dispatches only, as window sums (see the counter warnings above).",
            (Outcome::NotImported, true, _) => "The thread trace above is this capture's only microarchitectural data: the bundle's hardware counters were not imported (see the warnings above).",
            (Outcome::NotImported, false, true) => "This bundle carries thread traces and hardware counters, but neither was imported (see the warnings above), so wave occupancy, ISA hotspots and counter values are absent.",
            (Outcome::NotImported, false, false) => "Wave occupancy and ISA hotspots are not part of this bundle, and its hardware counters were not imported (see the warnings above).",
        }
        .into(),
    );
    Ok(out)
}

/// Per-dimension global work items (workgroup count × workgroup size), rendered
/// like the other importers. Returns "Not recorded" when either the count or the
/// workgroup size is all-zero: the schema defines an all-zero workgroup size as a
/// dynamic/indirect dispatch (INDIRECT_PARAMETERS), so fabricating `count × 1`
/// would present an unrecorded grid as a measured one.
fn grid_string(count: &[u32; 3], size: &[u32; 3]) -> String {
    if count.iter().all(|c| *c == 0) || size.iter().all(|s| *s == 0) {
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
    /// Profiling session of the chunk that carried this record.
    session: u64,
    /// Physical device ordinal of the chunk header.
    device: u32,
    /// Queue ordinal of the chunk header (UINT32_MAX when not queue-scoped).
    queue: u32,
    /// Stream id of the chunk header; `event_id` is unique only within it.
    stream: u64,
    event_id: u64,
    /// Queue submission epoch of a dispatch event; 0 for a command operation.
    submission: u64,
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
        flags: u64,
        dropped: u64,
        /// Record-header context: session, stream, physical device, queue, producer.
        session: u64,
        stream: u64,
        device: u32,
        queue: u32,
        producer: String,
    }
    fn chunk(family: &str, payload: Vec<u8>) -> Chunk {
        Chunk {
            family: format!("{CONTENT_PREFIX}{family}"),
            payload,
            ..Chunk::default()
        }
    }
    fn chunk_truncated(family: &str, payload: Vec<u8>, dropped: u64) -> Chunk {
        Chunk {
            flags: CHUNK_FLAG_TRUNCATED,
            dropped,
            ..chunk(family, payload)
        }
    }
    /// A chunk carrying the header context the native AMD drivers write.
    fn chunk_on(
        family: &str,
        payload: Vec<u8>,
        producer: &str,
        session: u64,
        device: u32,
        queue: u32,
        stream: u64,
    ) -> Chunk {
        Chunk {
            session,
            stream,
            device,
            queue,
            producer: producer.into(),
            ..chunk(family, payload)
        }
    }
    fn bundle(chunks: &[Chunk]) -> Vec<u8> {
        bundle_ex(chunks, 0)
    }
    fn bundle_ex(chunks: &[Chunk], session_status: u32) -> Vec<u8> {
        let mut body = Vec::new();
        let mut push_record = |rtype: u16, c: &Chunk, status: u32| {
            let header_len = RECORD_HEADER_LEN as u64;
            let ctl = c.family.len() as u32;
            let total = header_len + c.family.len() as u64 + c.producer.len() as u64 + c.payload.len() as u64;
            let mut h = vec![0u8; RECORD_HEADER_LEN];
            h[0..8].copy_from_slice(&total.to_le_bytes());
            h[8..16].copy_from_slice(&(c.payload.len() as u64).to_le_bytes());
            h[16..24].copy_from_slice(&c.session.to_le_bytes()); // session_id
            h[24..32].copy_from_slice(&c.stream.to_le_bytes()); // stream_id
            h[56..60].copy_from_slice(&(RECORD_HEADER_LEN as u32).to_le_bytes());
            h[60..64].copy_from_slice(&ctl.to_le_bytes());
            h[64..68].copy_from_slice(&(c.producer.len() as u32).to_le_bytes()); // name_length
            h[68..72].copy_from_slice(&c.device.to_le_bytes()); // physical_device_ordinal
            h[72..76].copy_from_slice(&c.queue.to_le_bytes()); // queue_ordinal
            h[80..88].copy_from_slice(&c.flags.to_le_bytes()); // chunk_flags
            h[88..96].copy_from_slice(&c.dropped.to_le_bytes()); // dropped_record_count
            h[96..100].copy_from_slice(&status.to_le_bytes()); // session_status_code
            h[100..102].copy_from_slice(&rtype.to_le_bytes());
            body.extend_from_slice(&h);
            body.extend_from_slice(c.family.as_bytes());
            body.extend_from_slice(c.producer.as_bytes());
            body.extend_from_slice(&c.payload);
        };
        // One session-begin / session-end pair per distinct session, like a device
        // group writing each device's session into the shared bundle.
        let mut sessions: Vec<(u64, String)> = Vec::new();
        for c in chunks {
            if !sessions.iter().any(|(s, _)| *s == c.session) {
                sessions.push((c.session, c.producer.clone()));
            }
        }
        if sessions.is_empty() {
            sessions.push((0, String::new()));
        }
        let marker = |session: u64, producer: &str| Chunk {
            session,
            producer: producer.into(),
            ..chunk("session", Vec::new())
        };
        for (session, producer) in &sessions {
            push_record(1, &marker(*session, producer), 0);
        }
        for c in chunks {
            push_record(RECORD_TYPE_CHUNK, c, 0);
        }
        for (session, producer) in &sessions {
            push_record(3, &marker(*session, producer), session_status);
        }
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

    // ---- executable traces -------------------------------------------------

    /// A minimal ELF64 with one executable load segment at `vaddr`.
    fn elf_image(vaddr: u64, memsz: u64) -> Vec<u8> {
        let mut elf = vec![0u8; 64 + 56];
        elf[..4].copy_from_slice(b"\x7fELF");
        elf[4] = 2;
        elf[5] = 1;
        elf[0x20..0x28].copy_from_slice(&le_u64(64));
        elf[0x36..0x38].copy_from_slice(&56u16.to_le_bytes());
        elf[0x38..0x3a].copy_from_slice(&1u16.to_le_bytes());
        elf[64..68].copy_from_slice(&le_u32(1));
        elf[68..72].copy_from_slice(&le_u32(5));
        elf[64 + 16..64 + 24].copy_from_slice(&le_u64(vaddr));
        elf[64 + 40..64 + 48].copy_from_slice(&le_u64(memsz));
        elf
    }
    fn trace_record(trace_id: u64, se: u32, flags: u32, event: u64, submission: u64, bytes: &[u8]) -> Vec<u8> {
        let mut r = vec![0u8; 88];
        r[0..4].copy_from_slice(&le_u32(88));
        r[4..8].copy_from_slice(&le_u32(1));
        r[8..12].copy_from_slice(&le_u32(flags));
        r[12..16].copy_from_slice(&le_u32(se));
        r[16..24].copy_from_slice(&le_u64(trace_id));
        r[24..32].copy_from_slice(&le_u64(event));
        r[32..40].copy_from_slice(&le_u64(submission));
        r[40..48].copy_from_slice(&le_u64(22));
        r[80..88].copy_from_slice(&le_u64(bytes.len() as u64));
        r.extend_from_slice(bytes);
        r
    }
    fn trace_capture(trace_id: u64, se: u32, flags: u32, written: u64, dropped: u64) -> Vec<u8> {
        let mut r = vec![0u8; 80];
        r[0..4].copy_from_slice(&le_u32(80));
        r[4..8].copy_from_slice(&le_u32(flags));
        r[8..16].copy_from_slice(&le_u64(trace_id));
        r[16..20].copy_from_slice(&le_u32(se));
        r[24..28].copy_from_slice(&le_u32(1)); // shader_engine_mask
        r[28..32].copy_from_slice(&le_u32(4)); // target_compute_unit
        r[32..36].copy_from_slice(&le_u32(1)); // simd_select
        r[36..40].copy_from_slice(&le_u32(2)); // dispatch_count
        r[40..48].copy_from_slice(&le_u64(1 << 20));
        r[48..56].copy_from_slice(&le_u64(written));
        r[56..64].copy_from_slice(&le_u64(dropped));
        r[72..76].copy_from_slice(&le_u32(1)); // trace_limit
        r[76..80].copy_from_slice(&le_u32(3)); // skipped_count
        r
    }
    /// Two dispatch events of submission 10 at ticks 1000..1100 and 1200..1500
    /// (100 MHz, so 10 ns per tick), one placed code object, and `extra` chunks.
    fn traced_bundle(extra: Vec<Chunk>) -> Vec<u8> {
        let mut events = dispatch_event(1, 1, 0, [32, 1, 1], [32, 1, 1], 1000, 1100);
        events.extend(dispatch_event(2, 1, 0, [3, 2, 1], [256, 1, 1], 1200, 1500));
        for event in events.chunks_mut(88) {
            event[16..24].copy_from_slice(&le_u64(10));
        }
        let mut chunks = vec![
            chunk("devices", device(100_000_000)),
            chunk("executable-functions", function(1, 0, "kernel_a")),
            chunk("executable-code-objects", code_object(1, 1, &elf_image(0x1000, 0x240))),
            chunk("executable-code-object-loads", code_object_load(1, 1, 0x7000_0000, 0x2000)),
            chunk("dispatch-events", events),
        ];
        chunks.extend(extra);
        bundle(&chunks)
    }
    /// Stand-in for the trace decoder: records the staged manifest and writes a
    /// one-wave decoded directory whose shader clock runs 10 cycles per tick and
    /// starts 10 ticks before the first dispatch (tick 990).
    fn fake_decoder<'a>(
        seen: &'a std::cell::RefCell<Vec<serde_json::Value>>,
        log: &'static str,
    ) -> impl Fn(&tempfile::TempDir, &Path, std::time::Duration) -> Result<crate::decoder::DecodedRun, String> + 'a {
        move |workspace, input, _| {
            let mut manifest: serde_json::Value =
                serde_json::from_slice(&std::fs::read(input.join("radv-manifest.json")).unwrap()).unwrap();
            let att = std::fs::read(manifest["traces"][0]["path"].as_str().unwrap()).unwrap();
            manifest["attBytes"] = serde_json::json!(att);
            seen.borrow_mut().push(manifest);
            let out = workspace.path().join("output");
            std::fs::create_dir(&out).unwrap();
            let write = |name: &str, value: serde_json::Value| {
                std::fs::write(out.join(name), serde_json::to_vec(&value).unwrap()).unwrap()
            };
            write("filenames.json", serde_json::json!({
                "gfxip": 11, "gfxv": "navi", "counter_names": [],
                "wave_filenames": {"0": {"0": {"0": {"0": ["se0_sm0_sl0_wv0.json", 100, 300]}}}},
            }));
            write("se0_sm0_sl0_wv0.json", serde_json::json!({"wave": {
                "cu": 0, "id": 0, "simd": 0, "slot": 0, "begin": 100, "end": 300,
                "instructions": [[150, 1, 0, 20, 1], [200, 1, 10, 40, 2]],
                "timeline": [[1, 50], [2, 150]],
                "waitcnt": [],
            }}));
            write("realtime.json", serde_json::json!({
                "metadata": {"frequency": 0},
                "SE0": [[0, 990], [1000, 1090], [3000, 1290]],
            }));
            write("code.json", serde_json::json!({
                "header": ["ISA", "_", "LineNumber", "Source", "Codeobj", "Vaddr", "Hit", "Latency", "Stall", "Idle"],
                "code": [
                    ["; kernel_a", 0, 1, "", 1, 4096, 0, 0, 0, 0],
                    ["v_add_f32 v0, v0, v1", 0, 2, "", 1, 4100, 1, 20, 0, 0],
                    ["s_endpgm", 0, 3, "", 1, 4104, 1, 40, 10, 5],
                ],
            }));
            write("occupancy.json", serde_json::json!({
                "0": [[100, 0, 0, 0, 1, "2", 0, 0], [300, 0, 0, 0, 0, "2", 0, 0]],
                "dispatches": {"2": "kernel_a"},
            }));
            Ok(crate::decoder::DecodedRun { root: out, log: log.into(), python_package: None })
        }
    }
    const NEVER: &dyn Fn(&tempfile::TempDir, &Path, std::time::Duration) -> Result<crate::decoder::DecodedRun, String> =
        &|_, _, _| panic!("a bundle without thread traces must not start the decoder");

    #[test]
    fn bundles_without_traces_never_start_the_decoder() {
        let flag = AtomicBool::new(false);
        let plain = build_with(&dispatch_bundle(), "trace.irpf".into(), &flag, NEVER).unwrap();
        assert!(plain.waves.is_empty() && plain.instructions.is_empty() && plain.wave_time_maps.is_empty());
        assert!(plain.workspace.is_none());
        assert_eq!(
            plain.warnings.last().unwrap(),
            "Coverage is unknown from a native dispatch stream alone: wave occupancy, ISA hotspots and hardware counters are not part of these IRPF bundles."
        );
        assert!(!plain.metadata.keys().any(|k| k.starts_with("hrxTrace")), "{:?}", plain.metadata);
        assert_eq!(plain.metadata["coverage"], "HRX native IRPF dispatch stream; raw device ticks, no host-timeline alignment");
    }

    #[test]
    fn trace_is_staged_placed_and_decoded_onto_the_dispatch_axis() {
        let payload = [7u8; 96];
        let b = traced_bundle(vec![
            chunk_truncated("executable-traces", trace_record(1, 0, 0, 0, 10, &payload), 5),
            chunk("executable-trace-captures", trace_capture(1, 0, 1 | 2 | 16, 96, 5)),
        ]);
        let seen = std::cell::RefCell::new(vec![]);
        let decode = fake_decoder(&seen, "DECODER-WARNING: hrx_shader_engine_0_0.att: Data Lost: bandwidth\n");
        let c = build_with(&b, "trace.irpf".into(), &AtomicBool::new(false), &decode).unwrap();

        // Staging: the raw bytes under the decoder's file-name convention, and
        // the ELF placed by its load delta and executable extent.
        let manifest = seen.borrow()[0].clone();
        assert!(manifest["traces"][0]["path"].as_str().unwrap().ends_with("hrx_shader_engine_0_0.att"));
        assert_eq!(manifest["traces"][0]["shaderEngine"], 0);
        assert_eq!(manifest["attBytes"].as_array().unwrap().len(), 96);
        let object = &manifest["codeObjects"][0];
        assert_eq!((object["id"].as_u64(), object["base"].as_u64()), (Some(1), Some(0x7000_0000)));
        assert_eq!((object["start"].as_u64(), object["size"].as_u64()), (Some(0x1000), Some(0x1240)));
        assert_eq!(object["loads"], serde_json::json!([{"base": 0x7000_0000u64, "start": 0x1000, "size": 0x1240}]));
        assert!(object.get("name").is_none(), "kernels keep their ELF symbol names");
        assert_eq!(&std::fs::read(object["path"].as_str().unwrap()).unwrap()[..4], b"\x7fELF");

        // The dispatch capture is intact and the trace sits on its axis: shader
        // cycles 100..300 are ticks 1000..1020, i.e. 0..200 ns after the origin.
        assert_eq!((c.unit.as_str(), c.dispatches.len()), ("ns", 2));
        assert_eq!(c.dispatches[0].start, 0.);
        assert_eq!(c.waves.len(), 1);
        let wave = &c.waves[0];
        assert!(wave.id.ends_with("/output/se0_sm0_sl0_wv0.json") && wave.id.starts_with("trace1-"), "{}", wave.id);
        assert!((wave.start - 0.).abs() < 1e-6 && (wave.end - 200.).abs() < 1e-6, "{wave:?}");
        assert_eq!(c.intervals.len(), 1);
        assert_eq!(c.intervals[0].kernel, "kernel_a");
        assert!((c.intervals[0].end - 200.).abs() < 1e-6);
        // Lazily loaded tokens and states use the same map: 10 cycles are 1 tick.
        let loaded = crate::load_wave(&c, &wave.id).unwrap();
        assert!((loaded.start - 0.).abs() < 1e-6 && (loaded.end - 200.).abs() < 1e-6);
        assert!((loaded.tokens[0].start - 50.).abs() < 1e-6 && (loaded.tokens[0].duration - 20.).abs() < 1e-6);
        assert!((loaded.tokens[1].stall - 10.).abs() < 1e-6 && (loaded.tokens[1].duration - 40.).abs() < 1e-6);
        assert!((loaded.states[1][0] - 50.).abs() < 1e-6 && (loaded.states[1][1] - 150.).abs() < 1e-6);
        assert_eq!(c.instructions.len(), 3);
        assert_eq!((c.instructions[2].hits, c.instructions[2].latency), (1, 40.));
        assert_eq!(c.wave_time_maps.len(), 1);
        assert!(c.workspace.is_some() && c.root.is_dir());

        // Provenance: the selection, what was dropped, the covered window.
        assert_eq!(c.metadata["hrxTraceSelection"], "workgroup processor 4, SIMD 1, shader engine(s) 0");
        assert_eq!(c.metadata["hrxTraceStablePstate"], "held");
        assert_eq!(c.metadata["hrxTraceDroppedTokens"], "5");
        assert_eq!(c.metadata["hrxTraceBufferBytes"], "1048576 per shader engine");
        assert_eq!(c.metadata["hrxTraceSkippedOperations"], "3");
        assert_eq!(c.metadata["hrxTraceWaves"], "1");
        assert_eq!(c.metadata["hrxTraceWavesAreASample"], "true");
        assert!(c.metadata["hrxTracePartitions"].contains("whole recorded execution of 2 dispatches"));
        assert!(c.metadata["hrxTraceWindow"].contains("2 of 2 dispatch event(s) start inside the traced window"), "{}", c.metadata["hrxTraceWindow"]);
        assert!(c.metadata["coverage"].contains("The trace is a sample") && c.metadata["coverage"].contains("roughly 2% of waves"));
        let said = |needle: &str| c.warnings.iter().any(|w| w.contains(needle));
        assert!(said("a hardware sample, not a census"));
        assert!(said("dropped 5 trace token(s)"));
        assert!(said("Trace decoder (trace 1): hrx_shader_engine_0_0.att: Data Lost"));
        assert!(said("lost data"));
        assert!(said("3 matching operation(s) ran untraced"));
        assert!(said("Hardware counters are not part of this IRPF bundle"));
        // The ring-buffer advice is for dropped records, not for a truncated trace.
        assert!(!said("IRPF executable-traces chunk is TRUNCATED"));
    }

    #[test]
    fn trace_that_cannot_be_decoded_or_placed_leaves_the_dispatch_capture_open() {
        let flag = AtomicBool::new(false);
        let traced = |submission: u64| {
            traced_bundle(vec![
                chunk("executable-traces", trace_record(1, 0, 0, 0, submission, &[1u8; 64])),
                chunk("executable-trace-captures", trace_capture(1, 0, 2 | 16, 64, 0)),
            ])
        };
        // The decoder is missing or fails.
        let failing = |_: &tempfile::TempDir, _: &Path, _: std::time::Duration| -> Result<crate::decoder::DecodedRun, String> {
            Err("Cannot start decoder Python: not found\nsecond line".into())
        };
        let c = build_with(&traced(10), "trace.irpf".into(), &flag, &failing).unwrap();
        assert_eq!(c.dispatches.len(), 2);
        assert!(c.waves.is_empty() && c.instructions.is_empty() && c.workspace.is_none());
        assert!(c.warnings.iter().any(|w| w == "Thread trace not imported: the trace decoder failed (Cannot start decoder Python: not found)"), "{:?}", c.warnings);
        assert!(c.warnings.last().unwrap().contains("none was imported"));
        assert_eq!(c.metadata["hrxTraceImport"], "not imported");
        // The capture records are reported even so.
        assert_eq!(c.metadata["hrxTraceSelection"], "workgroup processor 4, SIMD 1, shader engine(s) 0");
        assert!(c.metadata["hrxTraceStablePstate"] == "not held" && c.warnings.iter().any(|w| w.contains("No stable GPU power state")));

        // A trace naming a submission with no dispatch event cannot be placed;
        // the decoder is not even started.
        let c = build_with(&traced(99), "trace.irpf".into(), &flag, NEVER).unwrap();
        assert!(c.warnings.iter().any(|w| w.contains("no dispatch event of the submission it names is in the bundle")), "{:?}", c.warnings);
        assert!(c.waves.is_empty());

        // A malformed trace chunk is an explicit rejection, never a panic.
        let mut lying = trace_record(1, 0, 0, 0, 10, &[1u8; 64]);
        lying[80..88].copy_from_slice(&le_u64(1 << 40));
        let e = build_with(&traced_bundle(vec![chunk("executable-traces", lying)]), "x".into(), &flag, NEVER).unwrap_err();
        assert!(e.contains("declares 1099511627776 trace bytes but carries 64"), "{e}");
        let e = build_with(&traced_bundle(vec![chunk("executable-traces", vec![1, 2, 3])]), "x".into(), &flag, NEVER).unwrap_err();
        assert!(e.contains("shorter than its 88-byte record"), "{e}");
    }

    #[test]
    fn trace_import_honours_cancellation() {
        let b = traced_bundle(vec![
            chunk("executable-traces", trace_record(1, 0, 0, 0, 10, &[1u8; 64])),
            chunk("executable-trace-captures", trace_capture(1, 0, 2 | 16, 64, 0)),
        ]);
        let flag = AtomicBool::new(false);
        let cancelling = |_: &tempfile::TempDir, _: &Path, _: std::time::Duration| -> Result<crate::decoder::DecodedRun, String> {
            flag.store(true, Ordering::Relaxed);
            Err("Raw trace import cancelled".into())
        };
        assert_eq!(build_with(&b, "x".into(), &flag, &cancelling).unwrap_err(), "Import cancelled");
    }

    #[test]
    fn summary_counts_trace_partitions_and_their_bytes() {
        let b = traced_bundle(vec![
            chunk_truncated("executable-traces", trace_record(1, 0, 0, 0, 10, &[7u8; 96]), 5),
            chunk("executable-traces", trace_record(1, 1, 0, 0, 10, &[7u8; 32])),
        ]);
        let summary = summarize_bytes(&b).unwrap();
        assert_eq!((summary.executable_traces, summary.executable_trace_bytes), (2, 128));
        assert_eq!(summary.counter_samples, 0);
        assert_eq!(summary.truncated.len(), 1);
        assert_eq!((summary.truncated[0].family.as_str(), summary.truncated[0].dropped), ("executable-traces", 5));
        let plain = summarize_bytes(&dispatch_bundle()).unwrap();
        assert_eq!((plain.executable_traces, plain.executable_trace_bytes), (0, 0));
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

    #[test]
    fn truncated_dispatch_events_with_zero_payload_is_rejected_not_relabelled_queue_mode() {
        // The M1 scenario: a dispatch-mode bundle whose dispatch-events chunk is
        // TRUNCATED with zero payload must NOT fall back to the command-operations
        // (queue-mode) arm, which would silently change the timing semantics.
        let functions = function(1, 0, "ggml_rope_f32");
        let b = bundle(&[
            chunk("devices", device(100_000_000)),
            chunk("executable-functions", functions),
            chunk_truncated("dispatch-events", Vec::new(), 512),
        ]);
        let e = build(&b, "trace.irpf".into(), &AtomicBool::new(false)).unwrap_err();
        assert!(e.contains("TRUNCATED") && e.contains("512"), "{e}");
        assert!(!e.contains("queue-mode") || e.contains("refusing to relabel"), "{e}");
    }

    #[test]
    fn truncated_aggregate_family_imports_with_a_loud_warning() {
        // An aggregate observability stream (the contract permits these to
        // truncate) imports, but the partial data must be flagged, not silent.
        let mut functions = function(1, 0, "ggml_rope_f32");
        functions.extend(function(2, 0, "ggml_get_rows_f32"));
        let mut events = dispatch_event(1, 1, 0, [32, 1, 1], [32, 1, 1], 1000, 1100);
        events.extend(dispatch_event(2, 2, 0, [3, 2, 1], [256, 1, 1], 1200, 1500));
        let b = bundle(&[
            chunk("devices", device(100_000_000)),
            chunk("executable-functions", functions),
            chunk("dispatch-events", events),
            chunk_truncated("command-operations", Vec::new(), 7),
        ]);
        let c = build(&b, "trace.irpf".into(), &AtomicBool::new(false)).unwrap();
        assert_eq!(c.dispatches.len(), 2); // dispatch-events still imported
        assert!(
            c.warnings.iter().any(|w| w.contains("command-operations") && w.contains("TRUNCATED") && w.contains('7')),
            "{:?}",
            c.warnings
        );
    }

    #[test]
    fn nonzero_session_status_is_warned() {
        let functions = function(1, 0, "ggml_rope_f32");
        let events = dispatch_event(1, 1, 0, [32, 1, 1], [32, 1, 1], 1000, 1100);
        let b = bundle_ex(
            &[
                chunk("devices", device(100_000_000)),
                chunk("executable-functions", functions),
                chunk("dispatch-events", events),
            ],
            9,
        );
        let c = build(&b, "trace.irpf".into(), &AtomicBool::new(false)).unwrap();
        assert!(
            c.warnings.iter().any(|w| w.contains("nonzero status code") && w.contains('9')),
            "{:?}",
            c.warnings
        );
    }

    #[test]
    fn all_zero_workgroup_size_renders_not_recorded_not_fabricated_grid() {
        // A dynamic/indirect dispatch (all-zero workgroup_size per the schema) must
        // not be rendered as `count × 1`, which would present an unrecorded grid as
        // measured.
        let functions = function(1, 0, "ggml_dynamic");
        let events = dispatch_event(1, 1, 0, [8, 1, 1], [0, 0, 0], 1000, 1100);
        let b = bundle(&[
            chunk("devices", device(100_000_000)),
            chunk("executable-functions", functions),
            chunk("dispatch-events", events),
        ]);
        let c = build(&b, "trace.irpf".into(), &AtomicBool::new(false)).unwrap();
        assert_eq!(c.dispatches[0].grid, "Not recorded");
        assert_eq!(c.dispatches[0].workgroup, "Not recorded");
    }

    #[test]
    fn nonzero_reserved_flag_fields_are_rejected() {
        // File-header flags@12 nonzero.
        let mut d = dispatch_bundle();
        d[12..16].copy_from_slice(&1u32.to_le_bytes());
        assert!(build(&d, "x".into(), &AtomicBool::new(false)).unwrap_err().contains("reserved flags"));
        // Record-header flags@102 nonzero (first record header starts at FILE_HEADER_LEN).
        let mut d = dispatch_bundle();
        let off = FILE_HEADER_LEN + 102;
        d[off..off + 2].copy_from_slice(&1u16.to_le_bytes());
        assert!(build(&d, "x".into(), &AtomicBool::new(false)).unwrap_err().contains("reserved flags"));
    }

    // The ROCPROF_MAX_RECORDS cap is exercised in tests/configurable_caps.rs,
    // the only test binary allowed to mutate that process-wide variable.
    fn device_at(ordinal: u32, freq: u64) -> Vec<u8> {
        let mut r = device(freq);
        r[8..12].copy_from_slice(&le_u32(ordinal));
        r
    }
    fn queue(device: u32, ordinal: u32, stream: u64) -> Vec<u8> {
        let mut r = vec![0u8; 24];
        r[0..4].copy_from_slice(&le_u32(24));
        r[4..8].copy_from_slice(&le_u32(device));
        r[8..12].copy_from_slice(&le_u32(ordinal));
        r[16..24].copy_from_slice(&le_u64(stream));
        r
    }
    fn queue_event() -> Vec<u8> {
        let mut r = vec![0u8; 104];
        r[0..4].copy_from_slice(&le_u32(104));
        r
    }

    /// The shape the native AMD drivers write for a GPU + NPU device group: one
    /// session per device in one bundle, dispatch events split per queue and per
    /// flush, and an NPU session that records queue events but no dispatches.
    fn device_group_bundle() -> Vec<u8> {
        const NONE: u32 = u32::MAX;
        let mut gpu_functions = function(1, 0, "g_mv_q4_k_k5120_t8_q8");
        gpu_functions.extend(function(2, 0, "g_quant_q8"));
        bundle(&[
            chunk_on("devices", device_at(0, 100_000_000), "amd-gpu", 1, 0, NONE, 0),
            chunk_on("queues", queue(0, 0, 1), "amd-gpu", 1, 0, 0, 1),
            chunk_on("queues", queue(0, 4, 5), "amd-gpu", 1, 0, 4, 5),
            chunk_on("devices", device_at(1, 1_000_000_000), "xdna", 4, 1, 0, 0),
            chunk_on("queues", queue(1, 0, 1), "xdna", 4, 1, 0, 1),
            chunk_on("executable-functions", gpu_functions, "amd-gpu", 1, 0, NONE, 0),
            // The NPU session reuses executable id 1 for a different function.
            chunk_on("executable-functions", function(1, 0, "resident_ffn"), "xdna", 4, 1, NONE, 0),
            // Device records are re-emitted on a later flush.
            chunk_on("devices", device_at(0, 100_000_000), "amd-gpu", 1, 0, NONE, 0),
            // Queue 0, first flush then second flush.
            chunk_on(
                "dispatch-events",
                dispatch_event(1, 1, 0, [8, 40, 1], [256, 1, 1], 5000, 5200),
                "amd-gpu", 1, 0, 0, 1,
            ),
            chunk_on(
                "dispatch-events",
                dispatch_event(2, 2, 0, [3, 8, 1], [64, 1, 1], 5300, 5400),
                "amd-gpu", 1, 0, 0, 1,
            ),
            // Queue 4 restarts its own event ids.
            chunk_on(
                "dispatch-events",
                dispatch_event(1, 2, 0, [3, 8, 1], [64, 1, 1], 5600, 5700),
                "amd-gpu", 1, 0, 4, 5,
            ),
            chunk_on("queue-events", queue_event(), "xdna", 4, 1, 0, 1),
        ])
    }

    #[test]
    fn device_group_bundle_attributes_each_chunk_to_its_own_device_and_queue() {
        let c = build(&device_group_bundle(), "hybrid.irpf".into(), &AtomicBool::new(false)).unwrap();
        assert_eq!(c.dispatches.len(), 3);
        assert_eq!(c.unit, "ns");
        // Names resolve within the dispatch's own session: executable 1 is the GPU
        // kernel, never the NPU session's function with the same id.
        assert_eq!(c.dispatches[0].kernel, "g_mv_q4_k_k5120_t8_q8");
        assert_eq!(c.dispatches[0].grid, "2048 × 40 × 1");
        assert_eq!(c.dispatches[0].workgroup, "256 × 1 × 1");
        assert!(c.dispatches.iter().all(|d| d.agent == "HRX device 0 (amd-gpu)"));
        assert_eq!(
            c.dispatches.iter().map(|d| d.queue.as_str()).collect::<Vec<_>>(),
            ["HRX queue 0", "HRX queue 0", "HRX queue 4"]
        );
        // Two dispatch streams: ids are stream-qualified and stay unique.
        assert_eq!(
            c.dispatches.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(),
            ["dispatch-events:s1:1", "dispatch-events:s1:2", "dispatch-events:s5:1"]
        );
        // 100 MHz GPU clock: 200 ticks = 2000 ns, relative to the GPU's first tick.
        assert_eq!((c.dispatches[0].start, c.dispatches[0].end), (0.0, 2000.0));
        assert_eq!(c.dispatches[2].start, 6000.0);
        assert_eq!(c.metadata["hrxSessionCount"], "2");
        assert_eq!(c.metadata["hrxDevices"], "HRX device 0 (amd-gpu); HRX device 1 (xdna)");
        assert_eq!(c.metadata["hrxQueueEventCount"], "1");
        // The frequency reported is the dispatch-bearing device's, not the NPU's.
        assert_eq!(c.metadata["hrxTimestampFrequencyHz"], "100000000");
        assert!(
            c.warnings
                .iter()
                .any(|w| w.contains("recorded no dispatch records: HRX device 1 (xdna)")),
            "{:?}",
            c.warnings
        );
    }

    #[test]
    fn bundle_without_any_dispatch_record_is_rejected_not_opened_empty() {
        // What a device group writes with queue-events + executable-metadata only:
        // functions and queue events, but neither dispatch family.
        let d = bundle(&[
            chunk_on("devices", device_at(0, 100_000_000), "amd-gpu", 1, 0, u32::MAX, 0),
            chunk_on("executable-functions", function(1, 0, "g_quant_q8"), "amd-gpu", 1, 0, u32::MAX, 0),
            chunk_on("queue-events", queue_event(), "amd-gpu", 1, 0, 0, 1),
        ]);
        let e = build(&d, "queue.irpf".into(), &AtomicBool::new(false)).unwrap_err();
        assert!(e.contains("holds no dispatch records"), "{e}");
        assert!(e.contains("1 executable function(s) and 1 queue event(s)"), "{e}");
        assert!(e.contains("dispatch-events"), "{e}");
    }

    #[test]
    fn summary_reports_structure_truncation_and_unfinalized_bundles() {
        let s = summarize_bytes(&device_group_bundle()).unwrap();
        assert!(s.finalized);
        assert_eq!((s.sessions, s.functions, s.dispatch_events, s.queue_events), (2, 3, 3, 1));
        assert_eq!(s.producers, ["amd-gpu", "xdna"]);
        assert!(s.truncated.is_empty() && s.failed_sessions.is_empty());
        let counts: Vec<(&str, usize, usize, usize)> = s
            .per_producer
            .iter()
            .map(|p| (p.producer.as_str(), p.sessions, p.dispatch_events, p.queue_events))
            .collect();
        // The GPU session owns every dispatch; the NPU session only a queue event.
        assert_eq!(counts, [("amd-gpu", 1, 3, 0), ("xdna", 1, 0, 1)]);

        let truncated = bundle(&[
            chunk("devices", device(100_000_000)),
            chunk_truncated("queue-events", queue_event(), 12216),
        ]);
        let s = summarize_bytes(&truncated).unwrap();
        assert_eq!(s.truncated.len(), 1);
        assert_eq!((s.truncated[0].family.as_str(), s.truncated[0].dropped), ("queue-events", 12216));

        let mut open = dispatch_bundle();
        open[16..24].copy_from_slice(&0u64.to_le_bytes());
        let s = summarize_bytes(&open).unwrap();
        assert!(!s.finalized);
        assert_eq!(s.dispatch_events, 0);
    }

    // ---- hardware counters -------------------------------------------------

    fn counter_set(set_id: u64, counters: u32, values: u32, name: &str) -> Vec<u8> {
        let mut r = vec![0u8; 32];
        r[0..4].copy_from_slice(&le_u32(32 + name.len() as u32));
        r[8..16].copy_from_slice(&le_u64(set_id));
        r[16..20].copy_from_slice(&le_u32(0)); // physical device ordinal
        r[20..24].copy_from_slice(&le_u32(counters));
        r[24..28].copy_from_slice(&le_u32(values));
        r[28..32].copy_from_slice(&le_u32(name.len() as u32));
        r.extend_from_slice(name.as_bytes());
        r
    }
    #[allow(clippy::too_many_arguments)]
    fn counter(
        set_id: u64,
        ordinal: u32,
        unit: u32,
        offset: u32,
        count: u32,
        block: &str,
        name: &str,
        description: &str,
    ) -> Vec<u8> {
        let mut r = vec![0u8; 48];
        let total = 48 + block.len() + name.len() + description.len();
        r[0..4].copy_from_slice(&le_u32(total as u32));
        r[4..8].copy_from_slice(&le_u32(1)); // RAW
        r[8..12].copy_from_slice(&le_u32(unit));
        r[16..24].copy_from_slice(&le_u64(set_id));
        r[24..28].copy_from_slice(&le_u32(ordinal));
        r[28..32].copy_from_slice(&le_u32(offset));
        r[32..36].copy_from_slice(&le_u32(count));
        r[36..40].copy_from_slice(&le_u32(block.len() as u32));
        r[40..44].copy_from_slice(&le_u32(name.len() as u32));
        r[44..48].copy_from_slice(&le_u32(description.len() as u32));
        r.extend_from_slice(block.as_bytes());
        r.extend_from_slice(name.as_bytes());
        r.extend_from_slice(description.as_bytes());
        r
    }
    /// `flags`: bit 0 DISPATCH_EVENT, bit 1 COMMAND_OPERATION.
    fn counter_sample(sample_id: u64, set_id: u64, flags: u32, event_id: u64, values: &[u64]) -> Vec<u8> {
        let mut r = vec![0u8; 112];
        r[0..4].copy_from_slice(&le_u32(112 + 8 * values.len() as u32));
        r[4..8].copy_from_slice(&le_u32(flags));
        r[8..12].copy_from_slice(&le_u32(1)); // scope DISPATCH
        r[16..24].copy_from_slice(&le_u64(sample_id));
        r[24..32].copy_from_slice(&le_u64(set_id));
        r[32..40].copy_from_slice(&le_u64(event_id));
        r[48..56].copy_from_slice(&le_u64(7)); // command buffer
        r[56..64].copy_from_slice(&le_u64(1)); // executable
        r[64..72].copy_from_slice(&le_u64(1)); // stream
        r[88..92].copy_from_slice(&le_u32(3)); // command index
        r[92..96].copy_from_slice(&le_u32(0)); // function ordinal
        r[96..100].copy_from_slice(&le_u32(0));
        r[100..104].copy_from_slice(&le_u32(0));
        r[104..108].copy_from_slice(&le_u32(values.len() as u32));
        for v in values {
            r.extend_from_slice(&le_u64(*v));
        }
        r
    }
    /// `counts`: limit, operations, dispatches, skipped, dropped, invalid, foreign.
    fn counter_capture(set_id: u64, flags: u32, counts: [u64; 7]) -> Vec<u8> {
        let mut r = vec![0u8; 88];
        r[0..4].copy_from_slice(&le_u32(88));
        r[4..8].copy_from_slice(&le_u32(flags));
        r[8..16].copy_from_slice(&le_u64(set_id));
        r[20..24].copy_from_slice(&le_u32(2)); // shader engines
        for (i, v) in counts.iter().enumerate() {
            r[32 + i * 8..40 + i * 8].copy_from_slice(&le_u64(*v));
        }
        r
    }
    const STABLE: u32 = 1;
    const EXCLUSIVE: u32 = 2;
    const PROCESS_SCOPE: u32 = 4;
    const LIMIT_REACHED: u32 = 8;

    /// One GPU session: three dispatches, a five-slot counter set (two
    /// per-engine counters and one single-slot counter) and four samples: two
    /// joined, one naming a dispatch event the bundle lacks, one with no event.
    fn counter_chunks(samples: Vec<u8>, capture: Option<Vec<u8>>) -> Vec<Chunk> {
        let gpu = |family: &str, payload: Vec<u8>, queue: u32, stream: u64| {
            chunk_on(family, payload, "amd-gpu", 1, 0, queue, stream)
        };
        let mut events = dispatch_event(1, 1, 0, [4, 1, 1], [64, 1, 1], 1000, 1200);
        events.extend(dispatch_event(2, 1, 0, [8, 1, 1], [64, 1, 1], 1400, 1800));
        events.extend(dispatch_event(3, 1, 0, [2, 1, 1], [64, 1, 1], 2000, 2100));
        let mut counters = counter(1, 0, 1, 0, 2, "SQG", "SQG_WAVES", "Waves launched");
        counters.extend(counter(1, 1, 2, 2, 2, "SQG", "SQG_BUSY_CYCLES", "Busy cycles"));
        counters.extend(counter(1, 2, 1, 4, 1, "SQ", "SQ_INSTS_ALL", ""));
        let mut chunks = vec![
            gpu("devices", device_at(0, 100_000_000), u32::MAX, 0),
            gpu("queues", queue(0, 0, 1), 0, 1),
            gpu("executable-functions", function(1, 0, "g_gemm_q4"), u32::MAX, 0),
            gpu("dispatch-events", events, 0, 1),
            gpu("counter-sets", counter_set(1, 3, 5, "amdgpu.perfmon"), u32::MAX, 0),
            gpu("counters", counters, u32::MAX, 0),
            gpu("counter-samples", samples, 0, 1),
        ];
        if let Some(capture) = capture {
            chunks.push(gpu("counter-captures", capture, u32::MAX, 0));
        }
        chunks
    }
    fn counter_samples() -> Vec<u8> {
        let mut samples = counter_sample(1, 1, 1, 1, &[3, 1, 100, 50, 7]);
        samples.extend(counter_sample(2, 1, 1, 2, &[5, 3, 400, 20, 9]));
        samples.extend(counter_sample(3, 1, 1, 99, &[1, 1, 1, 1, 1]));
        samples.extend(counter_sample(4, 1, 2, 0, &[2, 0, 6, 0, 1]));
        samples
    }
    fn counter_bundle(capture: Option<Vec<u8>>) -> Capture {
        let d = bundle(&counter_chunks(counter_samples(), capture));
        build(&d, "counters.irpf".into(), &AtomicBool::new(false)).unwrap()
    }
    fn warned(c: &Capture, needle: &str) -> bool {
        c.warnings.iter().any(|w| w.contains(needle))
    }

    #[test]
    fn counter_samples_become_rows_joined_to_dispatches_by_event_id() {
        let c = counter_bundle(Some(counter_capture(
            1,
            STABLE | EXCLUSIVE | PROCESS_SCOPE,
            [4096, 1, 4, 0, 0, 0, 0],
        )));
        assert_eq!(c.dispatches.len(), 3);
        // Four samples × three counters, in sample order.
        assert_eq!(c.event_counters.len(), 12);
        let waves = &c.event_counters[0];
        assert_eq!((waves.name.as_str(), waves.unit.as_str()), ("SQG_WAVES", "count"));
        // Two shader-engine slots summed: 3 + 1.
        assert_eq!(waves.value, Some(4.0));
        assert_eq!(waves.dispatches, [c.dispatches[0].id.clone()]);
        assert_eq!(waves.event.as_deref(), Some(c.dispatches[0].id.as_str()));
        assert_eq!(
            waves.scope,
            "HRX device 0 (amd-gpu) · HRX stream 1 · SQG · sum of 2 instances · g_gemm_q4 · grid 256 × 1 × 1 · workgroup 64 × 1 × 1"
        );
        let cycles = &c.event_counters[1];
        assert_eq!((cycles.name.as_str(), cycles.unit.as_str(), cycles.value), ("SQG_BUSY_CYCLES", "cycles", Some(150.0)));
        assert!(cycles.scope.contains("hardware-wide during the window"), "{}", cycles.scope);
        assert_eq!(c.event_counters[2].value, Some(7.0));
        assert_eq!(c.event_counters[2].scope.matches("sum of 1 instance ").count(), 1);
        // The sample naming event 99: flagged, but that dispatch is not here.
        let lost = &c.event_counters[6];
        assert!(lost.dispatches.is_empty());
        assert_eq!(lost.event.as_deref(), Some("dispatch event 99 of stream 1 (not in this capture)"));
        assert!(lost.scope.ends_with("g_gemm_q4 · dispatch not in this capture"), "{}", lost.scope);
        // The sample with no dispatch event id keeps its command location.
        let unflagged = &c.event_counters[9];
        assert!(unflagged.dispatches.is_empty());
        assert_eq!(unflagged.event.as_deref(), Some("command buffer 7 operation 3"));
        assert_eq!(unflagged.value, Some(2.0));
        // Row ids are unique.
        let mut ids: Vec<&str> = c.event_counters.iter().map(|r| r.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 12);

        // One series per counter, joined samples only, at the dispatch midpoints.
        assert_eq!(c.counters.len(), 3);
        assert_eq!(c.counters[0].name, "SQG_WAVES");
        assert_eq!(c.counters[0].scope, "HRX device 0 (amd-gpu) · dispatch counters · amdgpu.perfmon");
        assert_eq!(c.counters[0].points, [[1000.0, 4.0], [6000.0, 8.0]]);
        assert_eq!(c.counters[1].unit, "cycles");
        assert!(c.counters.iter().all(|s| s.scope == c.counters[0].scope));

        assert_eq!(c.metadata["hrxCounterImport"], "imported");
        assert_eq!(c.metadata["hrxCounterSampledDispatches"], "2 of 3");
        assert_eq!(c.metadata["hrxCounterSamples"], "4 sample(s): 2 joined to a dispatch, 2 not joined");
        assert_eq!(
            c.metadata["hrxCounterTotals"],
            "over 4 sample(s): SQG_WAVES 16 · SQG_BUSY_CYCLES 578 · SQ_INSTS_ALL 18"
        );
        // Per-kernel sums over the joined samples only (samples 1 and 2).
        assert_eq!(
            c.metadata["hrxCounterKernels"],
            "g_gemm_q4: 2 sampled dispatch(es), SQG_WAVES 12, SQG_BUSY_CYCLES 570, SQ_INSTS_ALL 16"
        );
        assert_eq!(c.metadata["hrxCounterValuesAreWindowSums"], "true");
        assert_eq!(
            c.metadata["hrxCounterWindowExclusive"],
            "HRX device 0 (amd-gpu) set 1: true (among the producing device's queues, per the producer)"
        );
        assert!(c.metadata["hrxCounterCapture"].contains("bound 4096 sampled dispatch(es), 1 operation(s) bracketed, 4 sample(s) emitted"));
        assert!(c.metadata["hrxCounterCapture"].contains("stable power state held, window exclusive among the device's queues, hardware scope: every counting-enabled queue of the process"));
        assert!(c.metadata["hrxCounters"].contains("SQG_WAVES [SQG, count, 2 instances]: Waves launched"));
        assert!(c.metadata["coverage"].ends_with(
            "; plus hardware counters for 2 of 3 dispatches, as window sums (exclusive among the device's queues, per the producer)"
        ));

        assert!(warned(&c, "Hardware counter values of amdgpu.perfmon (HRX device 0 (amd-gpu)) are window sums."));
        assert!(warned(&c, "The producer reports an exclusive window for this capture"));
        assert!(warned(&c, "That is exact about this device session's queues only"));
        assert!(warned(&c, "2 of 3 dispatches in this capture carry counter values"));
        assert!(warned(&c, "2 counter sample(s) are not joined to a dispatch: 1 carry no dispatch event id"));
        assert!(warned(&c, "Cycle counters (SQG_BUSY_CYCLES) are never a per-dispatch count"));
        assert!(warned(&c, "its hardware counters cover the sampled dispatches only, as window sums"));
        assert!(!warned(&c, "hardware counters are not part of these IRPF bundles"));
        assert!(!warned(&c, "cannot tell"));
    }

    #[test]
    fn counters_without_a_capture_record_warn_that_the_window_is_unknown() {
        let c = counter_bundle(None);
        assert_eq!(c.event_counters.len(), 12);
        assert!(warned(&c, "has no counter-captures record for this set, so this capture cannot tell"));
        assert!(warned(&c, "read every value as an upper bound"));
        assert!(!warned(&c, "The producer reports an exclusive window"));
        assert_eq!(
            c.metadata["hrxCounterWindowExclusive"],
            "HRX device 0 (amd-gpu) set 1: unknown (no counter-captures record)"
        );
        assert!(c.metadata["coverage"].ends_with("(no capture record, window exclusivity unknown)"));
    }

    #[test]
    fn counter_capture_facts_drive_the_warnings() {
        // No exclusivity claim, bound reached, skipped, dropped and invalid.
        let c = counter_bundle(Some(counter_capture(1, LIMIT_REACHED, [2, 1, 2, 5, 3, 1, 0])));
        assert!(warned(&c, "The producer did not report an exclusive window, so this capture cannot tell"));
        assert!(warned(&c, "The counter sample bound of 2 dispatch(es) was reached"));
        assert!(warned(&c, "5 matching operation(s) on HRX device 0 (amd-gpu) ran unsampled"));
        assert!(warned(&c, "3 counter sample(s) on HRX device 0 (amd-gpu) were lost to host capacity"));
        assert!(warned(&c, "1 counter sample(s) on HRX device 0 (amd-gpu) were discarded"));
        assert!(warned(&c, "No stable GPU power state was held"));
        assert_eq!(c.metadata["hrxCounterWindowExclusive"], "HRX device 0 (amd-gpu) set 1: not reported");

        // Another collector overrides an exclusivity claim.
        let c = counter_bundle(Some(counter_capture(1, STABLE | EXCLUSIVE, [4096, 1, 4, 0, 0, 0, 2])));
        assert!(warned(&c, "2 sampled operation(s) on HRX device 0 (amd-gpu) found the perfmon registers away from their reset defaults"));
        assert!(warned(&c, "so the values cannot be read as per-dispatch counts"));
        assert!(!warned(&c, "The producer reports an exclusive window for this capture"));
        assert!(c.metadata["hrxCounterWindowExclusive"].ends_with("false (another collector was active)"));

        // A later capture record replaces an earlier one: the record is cumulative.
        let mut records = counter_capture(1, 0, [4096, 1, 1, 0, 0, 0, 0]);
        records.extend(counter_capture(1, STABLE | EXCLUSIVE, [4096, 2, 4, 0, 0, 0, 0]));
        let c = counter_bundle(Some(records));
        assert!(c.metadata["hrxCounterCapture"].contains("2 operation(s) bracketed, 4 sample(s) emitted"));
        assert!(warned(&c, "The producer reports an exclusive window for this capture"));
    }

    #[test]
    fn truncated_counter_samples_are_imported_and_flagged() {
        let mut chunks = counter_chunks(counter_samples(), None);
        let samples = chunks.iter_mut().find(|c| c.family.ends_with("counter-samples")).unwrap();
        samples.flags = CHUNK_FLAG_TRUNCATED;
        samples.dropped = 37;
        let c = build(&bundle(&chunks), "counters.irpf".into(), &AtomicBool::new(false)).unwrap();
        assert_eq!(c.event_counters.len(), 12);
        assert!(warned(&c, "IRPF counter-samples chunk is TRUNCATED: the producer dropped 37 record(s)"));
    }

    #[test]
    fn malformed_counter_records_leave_the_counters_out_and_keep_the_capture() {
        // A sample that claims more values than its record holds.
        let mut short = counter_sample(1, 1, 1, 1, &[3, 1, 100, 50, 7]);
        short[104..108].copy_from_slice(&le_u32(9));
        // A sample record shorter than the fixed sample header.
        let mut stub = vec![0u8; 16];
        stub[0..4].copy_from_slice(&le_u32(16));
        // A record length that runs past the chunk.
        let mut overrun = counter_sample(1, 1, 1, 1, &[1, 2, 3, 4, 5]);
        overrun[0..4].copy_from_slice(&le_u32(4096));
        for (samples, reason) in [
            (short, "a counter sample's values extend past its record"),
            (stub, "a counter-samples record is 16 bytes, below the 112-byte minimum"),
            (overrun, "a counter-samples record extends past its chunk"),
        ] {
            let d = bundle(&counter_chunks(samples, None));
            let c = build(&d, "counters.irpf".into(), &AtomicBool::new(false)).unwrap();
            assert_eq!(c.dispatches.len(), 3);
            assert!(c.event_counters.is_empty() && c.counters.is_empty());
            assert!(warned(&c, &format!("Hardware counters not imported: {reason}.")), "{:?}", c.warnings);
            assert_eq!(c.metadata["hrxCounterImport"], "not imported");
            assert!(warned(&c, "its hardware counters were not imported"));
        }
        // A counter record whose name runs past its record.
        let mut chunks = counter_chunks(counter_samples(), None);
        let counters = chunks.iter_mut().find(|c| c.family.ends_with(".counters")).unwrap();
        counters.payload[40..44].copy_from_slice(&le_u32(4000));
        let c = build(&bundle(&chunks), "counters.irpf".into(), &AtomicBool::new(false)).unwrap();
        assert!(warned(&c, "Hardware counters not imported: a counter name extends past its record."));
        assert_eq!(c.dispatches.len(), 3);
    }

    #[test]
    fn samples_with_missing_slots_and_sets_without_counters_are_stated() {
        // A sample with three values where the set lays out five: the per-engine
        // SQG_WAVES fits, the other two counters do not.
        let mut samples = counter_sample(1, 1, 1, 1, &[3, 1, 100]);
        // A sample of a set the bundle does not define.
        samples.extend(counter_sample(2, 9, 1, 2, &[1]));
        let c = build(&bundle(&counter_chunks(samples, None)), "c.irpf".into(), &AtomicBool::new(false)).unwrap();
        assert_eq!(c.event_counters.len(), 3);
        assert_eq!(c.event_counters[0].value, Some(4.0));
        assert_eq!(c.event_counters[1].value, None);
        assert_eq!(c.event_counters[2].value, None);
        assert!(warned(&c, "1 counter sample(s) carry fewer values than their counter set declares"));
        assert!(warned(&c, "1 counter sample(s) reference a counter set the bundle does not define"));
        assert!(warned(&c, "Counter(s) with no sample in this bundle: SQG_BUSY_CYCLES, SQ_INSTS_ALL. A missing value is not zero."));
        // Only the counter with values gets a series.
        assert_eq!(c.counters.len(), 1);

        // Counter metadata but no sample at all.
        let c = build(&bundle(&counter_chunks(Vec::new(), None)), "c.irpf".into(), &AtomicBool::new(false)).unwrap();
        assert!(c.event_counters.is_empty());
        assert!(warned(&c, "defines hardware counters but carries no counter sample"));
        assert_eq!(c.metadata["hrxCounterImport"], "no samples");

        // A set with no counter records.
        let mut chunks = counter_chunks(counter_samples(), None);
        chunks.retain(|c| !c.family.ends_with(".counters"));
        let c = build(&bundle(&chunks), "c.irpf".into(), &AtomicBool::new(false)).unwrap();
        assert!(c.event_counters.is_empty());
        assert!(warned(&c, "Counter set amdgpu.perfmon (HRX device 0 (amd-gpu)) declares 3 counters but the bundle defines none"));
    }

    #[test]
    fn another_producers_counter_set_is_imported_under_its_own_device() {
        // The XDNA driver's own set in a second session of the same bundle: one
        // cycle counter, one slot, no dispatch event ids and no capture record.
        let mut chunks = counter_chunks(
            counter_samples(),
            Some(counter_capture(1, STABLE | EXCLUSIVE, [4096, 1, 4, 0, 0, 0, 0])),
        );
        let npu = |family: &str, payload: Vec<u8>| chunk_on(family, payload, "xdna", 2, 1, 0, 1);
        let mut set = counter_set(1, 1, 1, "xdna");
        set[16..20].copy_from_slice(&le_u32(1));
        chunks.push(npu("devices", device_at(1, 0)));
        chunks.push(npu("counter-sets", set));
        chunks.push(npu("counters", counter(1, 0, 2, 0, 1, "partition_shim", "invocation_cycles", "")));
        chunks.push(npu("counter-samples", counter_sample(1, 1, 0, 0, &[1234])));
        let c = build(&bundle(&chunks), "hybrid.irpf".into(), &AtomicBool::new(false)).unwrap();
        assert_eq!(c.event_counters.len(), 13);
        let npu_row = c.event_counters.last().unwrap();
        assert_eq!((npu_row.name.as_str(), npu_row.unit.as_str(), npu_row.value), ("invocation_cycles", "cycles", Some(1234.0)));
        assert!(npu_row.scope.starts_with("HRX device 1 (xdna) · "), "{}", npu_row.scope);
        assert!(npu_row.dispatches.is_empty() && npu_row.event.is_none());
        // Two sessions carry samples: row ids are session-qualified.
        assert!(npu_row.id.starts_with("counter-samples:session2:"), "{}", npu_row.id);
        // Each set gets its own statement; the GPU's claim is not lent to the NPU.
        assert!(warned(&c, "Hardware counter values of amdgpu.perfmon (HRX device 0 (amd-gpu)) are window sums."));
        assert!(warned(&c, "Hardware counter values of xdna (HRX device 1 (xdna)) are window sums."));
        assert!(warned(&c, "has no counter-captures record for this set, so this capture cannot tell"));
        assert_eq!(
            c.metadata["hrxCounterWindowExclusive"],
            "HRX device 0 (amd-gpu) set 1: true (among the producing device's queues, per the producer) | HRX device 1 (xdna) set 1: unknown (no counter-captures record)"
        );
        assert!(c.metadata["coverage"].ends_with("(no capture record, window exclusivity unknown)"));
    }

    #[test]
    fn counter_rows_above_the_record_cap_are_refused_whole() {
        fn chunk_of(payload: &[u8]) -> crate::irpf_counters::Chunk<'_> {
            crate::irpf_counters::Chunk { session: 1, device: 0, queue: 0, stream: 1, payload }
        }
        let sets = counter_set(1, 2, 2, "amdgpu.perfmon");
        let mut counters = counter(1, 0, 1, 0, 1, "SQG", "SQG_WAVES", "");
        counters.extend(counter(1, 1, 1, 1, 1, "SQG", "SQG_ITEMS", ""));
        let mut samples = Vec::new();
        for id in 1..=3 {
            samples.extend(counter_sample(id, 1, 0, 0, &[1, 2]));
        }
        let run = |cap: usize| {
            let mut out = Capture::empty("c".into(), "HRX native profile · IRPF 2.0", "ns");
            let outcome = crate::irpf_counters::attach(
                &mut out,
                crate::irpf_counters::Context {
                    sets: vec![chunk_of(&sets)],
                    counters: vec![chunk_of(&counters)],
                    samples: vec![chunk_of(&samples)],
                    captures: Vec::new(),
                    dispatches: BTreeMap::new(),
                    cap,
                },
                &|_, _| "HRX device 0".into(),
                &|_| "HRX queue 0".into(),
                &|_, _, _| None,
                &AtomicBool::new(false),
            )
            .unwrap();
            (outcome, out)
        };
        // Three samples × two counters = six rows.
        let (outcome, out) = run(6);
        assert_eq!(outcome, crate::irpf_counters::Outcome::Imported);
        assert_eq!(out.event_counters.len(), 6);
        let (outcome, out) = run(5);
        assert_eq!(outcome, crate::irpf_counters::Outcome::NotImported);
        assert!(out.event_counters.is_empty());
        assert!(
            out.warnings[0].contains("3 counter sample(s) expand to 6 counter value(s), above the 5 record limit"),
            "{:?}",
            out.warnings
        );
        // Cancellation is the one error that propagates.
        let cancelled = AtomicBool::new(true);
        let mut out = Capture::empty("c".into(), "HRX native profile · IRPF 2.0", "ns");
        let result = crate::irpf_counters::attach(
            &mut out,
            crate::irpf_counters::Context {
                sets: vec![chunk_of(&sets)],
                counters: Vec::new(),
                samples: Vec::new(),
                captures: Vec::new(),
                dispatches: BTreeMap::new(),
                cap: 10,
            },
            &|_, _| String::new(),
            &|_| String::new(),
            &|_, _, _| None,
            &cancelled,
        );
        assert_eq!(result.unwrap_err(), "Import cancelled");
    }

    #[test]
    fn summary_reports_counter_sets_names_and_capture_facts() {
        let d = bundle(&counter_chunks(
            counter_samples(),
            Some(counter_capture(1, STABLE | EXCLUSIVE | LIMIT_REACHED, [4, 1, 4, 2, 0, 0, 0])),
        ));
        let s = summarize_bytes(&d).unwrap();
        assert_eq!((s.counter_sets, s.counter_samples), (1, 4));
        assert_eq!(s.counter_names, ["SQG_WAVES", "SQG_BUSY_CYCLES", "SQ_INSTS_ALL"]);
        let capture = s.counter_capture.unwrap();
        assert!(capture.exclusive_window && capture.stable_power_state && capture.limit_reached);
        assert_eq!(
            (capture.records, capture.sample_limit, capture.sampled_operations, capture.sampled_dispatches, capture.skipped_operations, capture.foreign_state),
            (1, 4, 1, 4, 2, 0)
        );
        // A bundle without counters says so and keeps its summary shape.
        let s = summarize_bytes(&dispatch_bundle()).unwrap();
        assert_eq!((s.counter_sets, s.counter_samples), (0, 0));
        assert!(s.counter_names.is_empty() && s.counter_capture.is_none());
        assert!(!serde_json::to_string(&s).unwrap().contains("counterCapture"));
    }
}
