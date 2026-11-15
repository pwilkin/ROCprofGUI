//! Hardware-counter arm of the IRPF importer.
//!
//! Producers emit three families that belong together: `counter-sets` (the value
//! layout of a set), `counters` (name, unit and value slots of each counter) and
//! `counter-samples` (one vector of deltas per measured scope). The native AMD
//! GPU driver adds `counter-captures`, a cumulative record saying how the set was
//! sampled. This arm maps them onto the capture's existing counter model:
//!
//! - one [`EventCounter`] per sample and counter, linked to the dispatch the
//!   sample names through its dispatch event id;
//! - one [`Series`] per counter, with each joined sample's value placed at its
//!   dispatch midpoint, the way the rocprofv3 dispatch-counter CSV is shown.
//!
//! A counter's value in a sample is the sum of its value slots: one slot per
//! hardware instance (shader engine or workgroup processor).
//!
//! Honesty. A sample is the counter's increase between two reads that bracket a
//! dispatch. The hardware counts every queue of the process that has counting
//! enabled inside that window, so the value is a window sum. Whether a window
//! held only its dispatch is the producer's statement (`counter-captures`), not
//! something the values show; this arm repeats that statement, says which
//! dispatches were sampled, and warns when the bundle cannot tell. Cycle
//! counters are not isolated by the queue enable at all (measured on gfx1151),
//! so they are never presented as a dispatch's own count. Anything
//! malformed leaves the counters out with a warning; the dispatch capture still
//! opens.

use crate::model::*;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

/// `iree_hal_profile_counter_set_record_t`.
pub(crate) const COUNTER_SET_MIN: usize = 32;
/// `iree_hal_profile_counter_record_t`.
pub(crate) const COUNTER_MIN: usize = 48;
/// `iree_hal_profile_counter_sample_record_t`, before its trailing values.
pub(crate) const COUNTER_SAMPLE_MIN: usize = 112;
/// `iree_hal_profile_counter_capture_record_t`.
pub(crate) const COUNTER_CAPTURE_MIN: usize = 88;

/// `IREE_HAL_PROFILE_COUNTER_SAMPLE_FLAG_DISPATCH_EVENT`.
const SAMPLE_FLAG_DISPATCH_EVENT: u32 = 1 << 0;
/// `IREE_HAL_PROFILE_COUNTER_SAMPLE_FLAG_COMMAND_OPERATION`.
const SAMPLE_FLAG_COMMAND_OPERATION: u32 = 1 << 1;

/// Kernels listed in the per-kernel counter digest of the capture's metadata.
const KERNEL_DIGEST_LIMIT: usize = 16;

pub(crate) const CAPTURE_STABLE_POWER_STATE: u32 = 1 << 0;
pub(crate) const CAPTURE_EXCLUSIVE_WINDOW: u32 = 1 << 1;
pub(crate) const CAPTURE_PROCESS_SCOPE: u32 = 1 << 2;
pub(crate) const CAPTURE_LIMIT_REACHED: u32 = 1 << 3;

fn u32_at(d: &[u8], at: usize) -> Result<u32, String> {
    d.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| "a counter record is shorter than its fixed fields".into())
}
fn u64_at(d: &[u8], at: usize) -> Result<u64, String> {
    d.get(at..at + 8)
        .map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
        .ok_or_else(|| "a counter record is shorter than its fixed fields".into())
}

/// One chunk of a counter family with the header context that scopes it.
pub(crate) struct Chunk<'a> {
    pub session: u64,
    pub device: u32,
    pub queue: u32,
    pub stream: u64,
    pub payload: &'a [u8],
}

/// Everything the arm needs from the bundle and from the dispatch import.
pub(crate) struct Context<'a> {
    pub sets: Vec<Chunk<'a>>,
    pub counters: Vec<Chunk<'a>>,
    pub samples: Vec<Chunk<'a>>,
    pub captures: Vec<Chunk<'a>>,
    /// (session, stream, dispatch event id) → index into the capture's
    /// dispatches. Empty when the dispatches are not dispatch events.
    pub dispatches: BTreeMap<(u64, u64, u64), usize>,
    /// Record cap shared with the other arms.
    pub cap: usize,
}

impl Context<'_> {
    pub(crate) fn present(&self) -> bool {
        !(self.sets.is_empty() && self.counters.is_empty() && self.samples.is_empty() && self.captures.is_empty())
    }
}

/// What the arm did, for the capture's closing coverage sentence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Outcome {
    /// The bundle carries no counter family.
    Absent,
    /// Counter chunks exist but nothing was imported; a warning says why.
    NotImported,
    /// Counter rows were attached.
    Imported,
}

struct Set {
    name: String,
    device: u32,
    value_count: u32,
    declared_counters: u32,
}

struct Counter {
    ordinal: u32,
    unit: u32,
    offset: u32,
    count: u32,
    block: String,
    name: String,
    description: String,
}

/// The cumulative `counter-captures` record of one set.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureFacts {
    pub flags: u32,
    pub shader_engine_count: u32,
    pub instance_count: u32,
    pub sample_limit: u64,
    pub sampled_operation_count: u64,
    pub sampled_dispatch_count: u64,
    pub skipped_operation_count: u64,
    pub dropped_sample_count: u64,
    pub invalid_sample_count: u64,
    pub foreign_state_count: u64,
}

impl CaptureFacts {
    pub fn exclusive(&self) -> bool {
        self.flags & CAPTURE_EXCLUSIVE_WINDOW != 0
    }
}

/// Walks a payload of self-sized records. Structural damage is an error: a
/// record whose length cannot be trusted hides where the next one starts.
fn walk<'a>(
    payload: &'a [u8],
    family: &str,
    min: usize,
    cancelled: &AtomicBool,
    mut each: impl FnMut(&'a [u8]) -> Result<(), String>,
) -> Result<(), String> {
    let mut off = 0usize;
    let mut seen = 0usize;
    while off < payload.len() {
        if seen % 4096 == 0 && cancelled.load(Ordering::Relaxed) {
            return Err("Import cancelled".into());
        }
        seen += 1;
        let length = payload
            .get(off..off + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
            .ok_or_else(|| format!("a {family} record header is truncated"))?;
        if length < min {
            return Err(format!(
                "a {family} record is {length} bytes, below the {min}-byte minimum"
            ));
        }
        let end = off
            .checked_add(length)
            .filter(|end| *end <= payload.len())
            .ok_or_else(|| format!("a {family} record extends past its chunk"))?;
        each(&payload[off..end])?;
        off = end;
    }
    Ok(())
}

fn text(record: &[u8], start: usize, length: usize, what: &str) -> Result<String, String> {
    start
        .checked_add(length)
        .and_then(|end| record.get(start..end))
        .map(|raw| String::from_utf8_lossy(raw).into_owned())
        .ok_or_else(|| format!("a {what} extends past its record"))
}

fn unit_name(unit: u32) -> String {
    match unit {
        0 => "unspecified".into(),
        1 => "count".into(),
        2 => "cycles".into(),
        3 => "bytes".into(),
        other => format!("unit {other}"),
    }
}

/// The capture record of each set, last one wins (the record is cumulative).
pub(crate) fn parse_captures(
    chunks: &[Chunk],
    cancelled: &AtomicBool,
) -> Result<BTreeMap<(u64, u64), CaptureFacts>, String> {
    let mut out = BTreeMap::new();
    for chunk in chunks {
        walk(chunk.payload, "counter-captures", COUNTER_CAPTURE_MIN, cancelled, |r| {
            out.insert(
                (chunk.session, u64_at(r, 8)?),
                CaptureFacts {
                    flags: u32_at(r, 4)?,
                    shader_engine_count: u32_at(r, 20)?,
                    instance_count: u32_at(r, 24)?,
                    sample_limit: u64_at(r, 32)?,
                    sampled_operation_count: u64_at(r, 40)?,
                    sampled_dispatch_count: u64_at(r, 48)?,
                    skipped_operation_count: u64_at(r, 56)?,
                    dropped_sample_count: u64_at(r, 64)?,
                    invalid_sample_count: u64_at(r, 72)?,
                    foreign_state_count: u64_at(r, 80)?,
                },
            );
            Ok(())
        })?;
    }
    Ok(out)
}

/// Counter names of every set, in first-seen order; for the bundle summary.
pub(crate) fn counter_names(chunks: &[Chunk], cancelled: &AtomicBool) -> Result<Vec<String>, String> {
    let mut names: Vec<String> = Vec::new();
    for chunk in chunks {
        walk(chunk.payload, "counters", COUNTER_MIN, cancelled, |r| {
            let block_length = u32_at(r, 36)? as usize;
            let name = text(r, COUNTER_MIN + block_length, u32_at(r, 40)? as usize, "counter name")?;
            if !names.contains(&name) {
                names.push(name);
            }
            Ok(())
        })?;
    }
    Ok(names)
}

fn plural(n: u64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Attaches the bundle's hardware counters to `out`. Only cancellation is an
/// error; every other problem leaves the counters out and says so.
pub(crate) fn attach(
    out: &mut Capture,
    ctx: Context,
    device_label: &dyn Fn(u64, u32) -> String,
    queue_label: &dyn Fn(u32) -> String,
    function_name: &dyn Fn(u64, u64, u32) -> Option<String>,
    cancelled: &AtomicBool,
) -> Result<Outcome, String> {
    if !ctx.present() {
        return Ok(Outcome::Absent);
    }
    match attach_inner(out, &ctx, device_label, queue_label, function_name, cancelled) {
        Ok(outcome) => Ok(outcome),
        Err(reason) if reason == "Import cancelled" => Err(reason),
        Err(reason) => {
            out.warnings.push(format!(
                "Hardware counters not imported: {reason}. The dispatch table is unaffected; no counter row or series was attached."
            ));
            out.metadata.insert("hrxCounterImport".into(), "not imported".into());
            Ok(Outcome::NotImported)
        }
    }
}

fn attach_inner(
    out: &mut Capture,
    ctx: &Context,
    device_label: &dyn Fn(u64, u32) -> String,
    queue_label: &dyn Fn(u32) -> String,
    function_name: &dyn Fn(u64, u64, u32) -> Option<String>,
    cancelled: &AtomicBool,
) -> Result<Outcome, String> {
    // Sets and counters. Producers may repeat metadata on later flushes; the
    // first record of an identity wins.
    let mut sets: BTreeMap<(u64, u64), Set> = BTreeMap::new();
    for chunk in &ctx.sets {
        walk(chunk.payload, "counter-sets", COUNTER_SET_MIN, cancelled, |r| {
            let key = (chunk.session, u64_at(r, 8)?);
            let device = match u32_at(r, 16)? {
                u32::MAX => chunk.device,
                ordinal => ordinal,
            };
            let set = Set {
                name: text(r, COUNTER_SET_MIN, u32_at(r, 28)? as usize, "counter set name")?,
                device,
                declared_counters: u32_at(r, 20)?,
                value_count: u32_at(r, 24)?,
            };
            sets.entry(key).or_insert(set);
            Ok(())
        })?;
    }
    let mut counters: BTreeMap<(u64, u64), Vec<Counter>> = BTreeMap::new();
    for chunk in &ctx.counters {
        walk(chunk.payload, "counters", COUNTER_MIN, cancelled, |r| {
            let key = (chunk.session, u64_at(r, 16)?);
            let block_length = u32_at(r, 36)? as usize;
            let name_length = u32_at(r, 40)? as usize;
            let counter = Counter {
                unit: u32_at(r, 8)?,
                ordinal: u32_at(r, 24)?,
                offset: u32_at(r, 28)?,
                count: u32_at(r, 32)?,
                block: text(r, COUNTER_MIN, block_length, "counter block name")?,
                name: text(r, COUNTER_MIN + block_length, name_length, "counter name")?,
                description: text(
                    r,
                    COUNTER_MIN + block_length + name_length,
                    u32_at(r, 44)? as usize,
                    "counter description",
                )?,
            };
            let list = counters.entry(key).or_default();
            if !list.iter().any(|c| c.ordinal == counter.ordinal) {
                list.push(counter);
            }
            Ok(())
        })?;
    }
    for list in counters.values_mut() {
        list.sort_by_key(|c| c.ordinal);
    }
    let captures = parse_captures(&ctx.captures, cancelled)?;

    // Count before building: the row count is samples × counters and must stay
    // under the record cap as a whole.
    let mut sample_count = 0usize;
    let mut row_count = 0usize;
    for chunk in &ctx.samples {
        walk(chunk.payload, "counter-samples", COUNTER_SAMPLE_MIN, cancelled, |r| {
            sample_count += 1;
            row_count = row_count
                .saturating_add(counters.get(&(chunk.session, u64_at(r, 24)?)).map_or(0, Vec::len));
            Ok(())
        })?;
    }
    if sample_count > ctx.cap || row_count > ctx.cap {
        return Err(format!(
            "{sample_count} counter sample(s) expand to {row_count} counter value(s), above the {} record limit (raise ROCPROF_MAX_RECORDS, or capture fewer samples with a lower hrx.counters.maxSamples)",
            ctx.cap
        ));
    }

    let mut sessions: Vec<u64> = ctx.samples.iter().map(|c| c.session).collect();
    sessions.sort_unstable();
    sessions.dedup();
    let qualify_session = sessions.len() > 1;

    let mut rows: Vec<EventCounter> = Vec::with_capacity(row_count);
    // (session, set, counter ordinal) → [dispatch midpoint, value].
    let mut points: BTreeMap<(u64, u64, u32), Vec<[f64; 2]>> = BTreeMap::new();
    // Per set: samples, joined, without an event id, naming a missing event,
    // with a counter whose slots lie outside the sample.
    #[derive(Default)]
    struct Tally {
        samples: u64,
        joined: u64,
        no_event: u64,
        missing_event: u64,
        short: u64,
        totals: BTreeMap<u32, f64>,
    }
    let mut tallies: BTreeMap<(u64, u64), Tally> = BTreeMap::new();
    let mut orphan_samples = 0u64;
    let mut sampled_dispatches: Vec<usize> = Vec::new();
    // Kernel → (joined samples, counter name → sum over them), for the
    // per-kernel digest in metadata.
    let mut kernels: BTreeMap<String, (u64, Vec<(String, f64)>)> = BTreeMap::new();
    for chunk in &ctx.samples {
        walk(chunk.payload, "counter-samples", COUNTER_SAMPLE_MIN, cancelled, |r| {
            let flags = u32_at(r, 4)?;
            let sample_id = u64_at(r, 16)?;
            let set_id = u64_at(r, 24)?;
            let event_id = u64_at(r, 32)?;
            let value_count = u32_at(r, 104)? as usize;
            let values = value_count
                .checked_mul(8)
                .and_then(|bytes| COUNTER_SAMPLE_MIN.checked_add(bytes))
                .and_then(|end| r.get(COUNTER_SAMPLE_MIN..end))
                .ok_or("a counter sample's values extend past its record")?;
            let key = (chunk.session, set_id);
            let (Some(set), Some(list)) = (sets.get(&key), counters.get(&key)) else {
                orphan_samples += 1;
                return Ok(());
            };
            let tally = tallies.entry(key).or_default();
            tally.samples += 1;
            let stream = if chunk.stream != 0 { chunk.stream } else { u64_at(r, 64)? };
            let queue = match chunk.queue {
                u32::MAX => u32_at(r, 100)?,
                queue => queue,
            };
            let dispatch = if flags & SAMPLE_FLAG_DISPATCH_EVENT != 0 {
                let found = ctx.dispatches.get(&(chunk.session, stream, event_id)).copied();
                if found.is_none() {
                    tally.missing_event += 1;
                }
                found
            } else {
                tally.no_event += 1;
                None
            };
            let joined = dispatch.and_then(|index| out.dispatches.get(index).map(|d| (index, d)));
            if let Some((index, d)) = joined {
                tally.joined += 1;
                sampled_dispatches.push(index);
                kernels.entry(d.kernel.clone()).or_default().0 += 1;
            }
            let place = format!("{} · {}", device_label(chunk.session, set.device), queue_label(queue));
            let subject = match joined {
                Some((_, d)) => format!("{} · grid {} · workgroup {}", d.kernel, d.grid, d.workgroup),
                None => function_name(chunk.session, u64_at(r, 56)?, u32_at(r, 92)?)
                    .map(|name| format!("{name} · dispatch not in this capture"))
                    .unwrap_or_else(|| "kernel not recorded".into()),
            };
            let event = match joined {
                Some((_, d)) => Some(d.id.clone()),
                None if flags & SAMPLE_FLAG_DISPATCH_EVENT != 0 => {
                    Some(format!("dispatch event {event_id} of stream {stream} (not in this capture)"))
                }
                None if flags & SAMPLE_FLAG_COMMAND_OPERATION != 0 => Some(format!(
                    "command buffer {} operation {}",
                    u64_at(r, 48)?,
                    u32_at(r, 88)?
                )),
                None => None,
            };
            let mut short = false;
            for counter in list {
                let slots = (counter.offset as usize)
                    .checked_add(counter.count as usize)
                    .filter(|end| *end <= value_count)
                    .map(|end| &values[counter.offset as usize * 8..end * 8]);
                let value = slots.map(|slots| {
                    slots
                        .chunks_exact(8)
                        .map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]) as f64)
                        .sum::<f64>()
                });
                short |= value.is_none();
                if let Some(value) = value {
                    *tally.totals.entry(counter.ordinal).or_default() += value;
                    if let Some((_, d)) = joined {
                        points
                            .entry((chunk.session, set_id, counter.ordinal))
                            .or_default()
                            .push([(d.start + d.end) / 2., value]);
                        if let Some((_, sums)) = kernels.get_mut(&d.kernel) {
                            match sums.iter_mut().find(|(name, _)| *name == counter.name) {
                                Some((_, sum)) => *sum += value,
                                None => sums.push((counter.name.clone(), value)),
                            }
                        }
                    }
                }
                rows.push(EventCounter {
                    id: if qualify_session {
                        format!(
                            "counter-samples:session{}:s{stream}:{sample_id}:c{}",
                            chunk.session, counter.ordinal
                        )
                    } else {
                        format!("counter-samples:s{stream}:{sample_id}:c{}", counter.ordinal)
                    },
                    counter_id: format!("counters:{}:{set_id}:{}", chunk.session, counter.ordinal),
                    event: event.clone(),
                    dispatches: joined.map(|(_, d)| vec![d.id.clone()]).unwrap_or_default(),
                    name: counter.name.clone(),
                    unit: unit_name(counter.unit),
                    scope: format!(
                        "{place} · {} · sum of {}{} · {subject}",
                        if counter.block.is_empty() { "block not recorded" } else { &counter.block },
                        plural(counter.count as u64, "instance", "instances"),
                        // Cycle counters are not isolated to the dispatch.
                        if counter.unit == 2 { " · hardware-wide during the window" } else { "" },
                    ),
                    value,
                });
            }
            if short {
                tally.short += 1;
            }
            Ok(())
        })?;
    }

    // Describe every set, sampled or not.
    let mut set_lines = Vec::new();
    let mut counter_lines = Vec::new();
    let mut total_lines = Vec::new();
    let mut silent_counters = Vec::new();
    for (key, set) in &sets {
        let label = device_label(key.0, set.device);
        let list = counters.get(key);
        let name = if set.name.is_empty() { "unnamed set" } else { &set.name };
        set_lines.push(format!(
            "{name} ({label}, set {}): {}, {} per sample",
            key.1,
            plural(list.map_or(0, Vec::len) as u64, "counter", "counters"),
            plural(set.value_count as u64, "value slot", "value slots"),
        ));
        let Some(list) = list else {
            out.warnings.push(format!(
                "Counter set {name} ({label}) declares {} but the bundle defines none; its samples cannot be named and were left out.",
                plural(set.declared_counters as u64, "counter", "counters")
            ));
            continue;
        };
        let tally = tallies.get(key);
        for counter in list {
            counter_lines.push(format!(
                "{} [{}, {}, {}]{}",
                counter.name,
                if counter.block.is_empty() { "block not recorded" } else { &counter.block },
                unit_name(counter.unit),
                plural(counter.count as u64, "instance", "instances"),
                if counter.description.is_empty() {
                    String::new()
                } else {
                    format!(": {}", counter.description)
                },
            ));
            match tally.and_then(|t| t.totals.get(&counter.ordinal)) {
                Some(total) => total_lines.push(format!("{} {}", counter.name, *total as u64)),
                None => silent_counters.push(counter.name.clone()),
            }
        }
    }
    for key in counters.keys() {
        if !sets.contains_key(key) {
            out.warnings.push(format!(
                "The bundle defines counters for counter set {} but no such set; their value layout is unknown and their samples were left out.",
                key.1
            ));
        }
    }
    if !set_lines.is_empty() {
        out.metadata.insert("hrxCounterSets".into(), set_lines.join(" | "));
    }
    if !counter_lines.is_empty() {
        out.metadata.insert("hrxCounters".into(), counter_lines.join(" | "));
    }
    if rows.is_empty() {
        out.warnings.push(if sample_count == 0 {
            "This bundle defines hardware counters but carries no counter sample: nothing was sampled (no operation matched the capture filter while the session ran, or every matching operation ran unsampled).".to_owned()
        } else {
            format!(
                "This bundle carries {sample_count} counter sample(s) but none belongs to a counter set the bundle defines; no counter row was attached."
            )
        });
        let (_, notes) = describe_captures(out, &captures, &sets, device_label, 0, 0);
        out.warnings.extend(notes);
        out.metadata.insert("hrxCounterImport".into(), "no samples".into());
        return Ok(Outcome::NotImported);
    }

    // Series: one per counter, joined samples only, in time order.
    for (key, set) in &sets {
        let Some(list) = counters.get(key) else { continue };
        let name = if set.name.is_empty() { "unnamed set" } else { &set.name };
        for counter in list {
            let Some(mut series) = points.remove(&(key.0, key.1, counter.ordinal)) else { continue };
            series.sort_by(|a, b| a[0].total_cmp(&b[0]));
            out.counters.push(Series {
                spm_clock: None,
                definition: None,
                att: None,
                name: counter.name.clone(),
                unit: unit_name(counter.unit),
                scope: format!("{} · dispatch counters · {name}", device_label(key.0, set.device)),
                points: series,
            });
        }
    }
    out.event_counters.extend(rows);

    sampled_dispatches.sort_unstable();
    sampled_dispatches.dedup();
    let total_dispatches = out.dispatches.len() as u64;
    let covered = sampled_dispatches.len() as u64;
    let samples: u64 = tallies.values().map(|t| t.samples).sum();
    let joined: u64 = tallies.values().map(|t| t.joined).sum();
    let no_event: u64 = tallies.values().map(|t| t.no_event).sum();
    let missing_event: u64 = tallies.values().map(|t| t.missing_event).sum();
    let short: u64 = tallies.values().map(|t| t.short).sum();

    out.metadata.insert("hrxCounterImport".into(), "imported".into());
    out.metadata.insert(
        "hrxCounterSamples".into(),
        format!("{samples} sample(s): {joined} joined to a dispatch, {} not joined", samples - joined),
    );
    out.metadata.insert(
        "hrxCounterSampledDispatches".into(),
        format!("{covered} of {total_dispatches}"),
    );
    if !total_lines.is_empty() {
        out.metadata.insert(
            "hrxCounterTotals".into(),
            format!("over {samples} sample(s): {}", total_lines.join(" · ")),
        );
    }
    if !kernels.is_empty() {
        // The kernels with the most sampled dispatches; the rest are counted.
        let mut ranked: Vec<(&String, &(u64, Vec<(String, f64)>))> = kernels.iter().collect();
        ranked.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(b.0)));
        let shown = ranked.len().min(KERNEL_DIGEST_LIMIT);
        let mut lines: Vec<String> = ranked[..shown]
            .iter()
            .map(|(kernel, (dispatches, sums))| {
                format!(
                    "{kernel}: {dispatches} sampled dispatch(es), {}",
                    sums.iter()
                        .map(|(name, sum)| format!("{name} {}", *sum as u64))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
            .collect();
        if ranked.len() > shown {
            lines.push(format!("{} more kernel(s) not listed", ranked.len() - shown));
        }
        out.metadata.insert("hrxCounterKernels".into(), lines.join(" | "));
    }
    out.metadata.insert("hrxCounterValuesAreWindowSums".into(), "true".into());
    let (exclusive, notes) = describe_captures(out, &captures, &sets, device_label, covered, total_dispatches);

    for (key, set) in &sets {
        if !tallies.contains_key(key) {
            continue;
        }
        let name = if set.name.is_empty() { "unnamed set" } else { &set.name };
        out.warnings.push(format!(
            "Hardware counter values of {name} ({}) are window sums. Each value is the counter's increase between a read just before a dispatch and a read just after it, summed over the counter's hardware instances. Inside that window the hardware counts every queue of the profiled process that has counting enabled, not only the dispatch, so a value is the dispatch's own count only when nothing else of the process was counted in the window. {}",
            device_label(key.0, set.device),
            match exclusive.get(key).copied().unwrap_or(Exclusivity::Unknown) {
                Exclusivity::Exclusive => "The producer reports an exclusive window for this capture: it let one queue of its device into a sampled window at a time and enabled counting only inside sampled operations. For event counts (waves, work items, instructions) each window therefore holds exactly its dispatch among that device's queues. That is exact about this device session's queues only; queues of the same process outside the session (a second device instance, or a privileged profiler that forces counting on) would be counted as well, and this capture cannot see them. It does not hold for cycle counters at all (see the warning about cycles).",
                Exclusivity::Foreign => "The producer reports that it found the perfmon registers reprogrammed by another collector (see the warning about foreign perfmon state), so the values cannot be read as per-dispatch counts.",
                Exclusivity::NotClaimed => "The producer did not report an exclusive window, so this capture cannot tell which windows overlapped other queues of the process: read every value as an upper bound on the dispatch's own count, not as that count.",
                Exclusivity::Unknown => "The bundle has no counter-captures record for this set, so this capture cannot tell whether other queues of the process overlapped the sample windows: read every value as an upper bound on the dispatch's own count, not as that count.",
            }
        ));
    }
    out.warnings.extend(notes);
    out.warnings.push(format!(
        "Hardware counters are a sample of the dispatches: {covered} of {total_dispatches} dispatches in this capture carry counter values. Per-counter totals in metadata.hrxCounterTotals cover those sampled dispatches only, not the capture; per-kernel time still comes from every dispatch event."
    ));
    if samples > joined {
        out.warnings.push(format!(
            "{} counter sample(s) are not joined to a dispatch: {no_event} carry no dispatch event id (the dispatch-events family was not captured, or the producer does not emit one), {missing_event} name a dispatch event that is not in this capture (dropped by the producer or filtered out). They are listed in the event counters without a dispatch link and are left out of the counter series.",
            samples - joined
        ));
    }
    if short > 0 {
        out.warnings.push(format!(
            "{short} counter sample(s) carry fewer values than their counter set declares; the affected counter values are shown as not collected, never as zero."
        ));
    }
    if orphan_samples > 0 {
        out.warnings.push(format!(
            "{orphan_samples} counter sample(s) reference a counter set the bundle does not define and were left out."
        ));
    }
    if !silent_counters.is_empty() {
        out.warnings.push(format!(
            "Counter(s) with no sample in this bundle: {}. A missing value is not zero.",
            silent_counters.join(", ")
        ));
    }
    let mut cycle_names: Vec<&str> = Vec::new();
    for row in out.event_counters.iter().filter(|row| row.unit == "cycles") {
        if !cycle_names.contains(&row.name.as_str()) {
            cycle_names.push(&row.name);
        }
    }
    if !cycle_names.is_empty() {
        out.warnings.push(format!(
            "Cycle counters ({}) are never a per-dispatch count, with or without an exclusive window. They count hardware cycles during the sample window, whoever kept the hardware busy: the queue enable that isolates event counts does not isolate them. Measured on gfx1151, a second queue of the same process raised SQG_BUSY_CYCLES about fivefold inside a window whose wave and item counts stayed exact; other processes were not measured. They were not calibrated against a clock and depend on the device's clock level, so compare them with each other, not with the nanosecond axis.",
            cycle_names.join(", ")
        ));
    }
    out.warnings.push(
        "Counter series place each sampled dispatch's value at the dispatch midpoint: they are per-dispatch measurements, not continuous time samples, and dispatches without a sample have no point.".into(),
    );
    let coverage = out.metadata.get("coverage").cloned().unwrap_or_default();
    out.metadata.insert(
        "coverage".into(),
        format!(
            "{coverage}{}hardware counters for {covered} of {total_dispatches} dispatches, as window sums ({})",
            if coverage.is_empty() { "" } else { "; plus " },
            {
                let states: Vec<Exclusivity> = tallies
                    .keys()
                    .map(|key| exclusive.get(key).copied().unwrap_or(Exclusivity::Unknown))
                    .collect();
                if states.iter().any(|s| *s == Exclusivity::Foreign) {
                    "another collector was active"
                } else if states.iter().any(|s| *s == Exclusivity::Unknown) {
                    "no capture record, window exclusivity unknown"
                } else if states.iter().any(|s| *s == Exclusivity::NotClaimed) {
                    "window exclusivity not reported"
                } else {
                    "exclusive among the device's queues, per the producer"
                }
            }
        ),
    );
    Ok(Outcome::Imported)
}

#[derive(Clone, Copy, PartialEq)]
enum Exclusivity {
    /// The capture record has EXCLUSIVE_WINDOW and reports no foreign state.
    Exclusive,
    /// The capture record reports another collector.
    Foreign,
    /// A capture record exists without the EXCLUSIVE_WINDOW flag.
    NotClaimed,
    /// The set has no capture record.
    Unknown,
}

/// Writes the capture-record facts to metadata, classifies what the producer
/// said about each set's sample window, and returns the warnings they call for.
fn describe_captures(
    out: &mut Capture,
    captures: &BTreeMap<(u64, u64), CaptureFacts>,
    sets: &BTreeMap<(u64, u64), Set>,
    device_label: &dyn Fn(u64, u32) -> String,
    covered: u64,
    total_dispatches: u64,
) -> (BTreeMap<(u64, u64), Exclusivity>, Vec<String>) {
    let mut lines = Vec::new();
    let mut windows = Vec::new();
    let mut notes = Vec::new();
    let mut exclusivity = BTreeMap::new();
    for (key, set) in sets {
        let label = device_label(key.0, set.device);
        let Some(facts) = captures.get(key) else {
            exclusivity.insert(*key, Exclusivity::Unknown);
            lines.push(format!("{label} set {}: no counter-captures record", key.1));
            windows.push(format!("{label} set {}: unknown (no counter-captures record)", key.1));
            continue;
        };
        let state = if facts.foreign_state_count > 0 {
            Exclusivity::Foreign
        } else if facts.exclusive() {
            Exclusivity::Exclusive
        } else {
            Exclusivity::NotClaimed
        };
        exclusivity.insert(*key, state);
        windows.push(format!(
            "{label} set {}: {}",
            key.1,
            match state {
                Exclusivity::Exclusive => "true (among the producing device's queues, per the producer)",
                Exclusivity::Foreign => "false (another collector was active)",
                _ => "not reported",
            }
        ));
        let mut facts_text = vec![
            format!(
                "bound {} sampled dispatch(es){}",
                facts.sample_limit,
                if facts.flags & CAPTURE_LIMIT_REACHED != 0 { " (reached)" } else { "" }
            ),
            format!("{} operation(s) bracketed", facts.sampled_operation_count),
            format!("{} sample(s) emitted", facts.sampled_dispatch_count),
            format!("{} matching operation(s) ran unsampled", facts.skipped_operation_count),
            format!("{} sample(s) dropped", facts.dropped_sample_count),
            format!("{} sample(s) without hardware values", facts.invalid_sample_count),
            format!("{} operation(s) found foreign perfmon state", facts.foreign_state_count),
            format!("{} shader engine(s)", facts.shader_engine_count),
        ];
        if facts.instance_count > 0 {
            facts_text.push(format!("{} workgroup-processor instance(s)", facts.instance_count));
        }
        facts_text.push(
            if facts.flags & CAPTURE_STABLE_POWER_STATE != 0 {
                "stable power state held"
            } else {
                "stable power state not held"
            }
            .to_owned(),
        );
        facts_text.push(
            if facts.exclusive() {
                "window exclusive among the device's queues"
            } else {
                "window exclusivity not reported"
            }
            .to_owned(),
        );
        if facts.flags & CAPTURE_PROCESS_SCOPE != 0 {
            facts_text.push("hardware scope: every counting-enabled queue of the process".to_owned());
        }
        lines.push(format!("{label} set {}: {}", key.1, facts_text.join(", ")));

        if facts.foreign_state_count > 0 {
            notes.push(format!(
                "{} sampled operation(s) on {label} found the perfmon registers away from their reset defaults (foreign perfmon state): another collector (rocprofv3 --pmc, a RADV performance query or a second profiling session) was programming the same device-wide registers. Counter values of this capture may have been reset, reselected or inflated by it; do not read them as per-dispatch counts.",
                facts.foreign_state_count
            ));
        }
        if facts.skipped_operation_count > 0 {
            notes.push(format!(
                "{} matching operation(s) on {label} ran unsampled: another queue held the sample window, or the recording was made without counter slots. Their dispatches have no counter values.",
                facts.skipped_operation_count
            ));
        }
        if facts.flags & CAPTURE_LIMIT_REACHED != 0 {
            notes.push(format!(
                "The counter sample bound of {} dispatch(es) was reached on {label}: later matching dispatches ran unsampled ({covered} of {total_dispatches} dispatches in this capture carry values). Raise hrx.counters.maxSamples or narrow hrx.filterFunction to move the sampled set.",
                facts.sample_limit
            ));
        }
        if facts.dropped_sample_count > 0 {
            notes.push(format!(
                "{} counter sample(s) on {label} were lost to host capacity between flushes; shorten the flush interval (hrx.flushIntervalMs) to keep them.",
                facts.dropped_sample_count
            ));
        }
        if facts.invalid_sample_count > 0 {
            notes.push(format!(
                "{} counter sample(s) on {label} were discarded because the hardware never wrote their raw words.",
                facts.invalid_sample_count
            ));
        }
        if facts.flags & CAPTURE_STABLE_POWER_STATE == 0 {
            notes.push(format!(
                "No stable GPU power state was held on {label} while sampling. Event counts do not depend on it; cycle counters then run at whatever clock level the device chose."
            ));
        }
    }
    if !lines.is_empty() {
        out.metadata.insert("hrxCounterCapture".into(), lines.join(" | "));
    }
    if !windows.is_empty() {
        out.metadata.insert("hrxCounterWindowExclusive".into(), windows.join(" | "));
    }
    (exclusivity, notes)
}
