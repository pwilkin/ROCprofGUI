use crate::{spm_clock, Capture, RealtimeCalibration, Series};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Deserialize)]
struct Dimension {
    dimension_name: String,
    index: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibratedGroup {
    pub domain: String,
    pub xcc: u64,
    pub other_se_intervals: usize,
    pub realtime_timestamps: Vec<String>,
    pub clock: spm_clock::AlignedClock,
    pub counters: Vec<Series>,
}

struct Coordinates {
    domain: String,
    xcc: u64,
    se: u64,
    instance: u64,
    counter: String,
}
fn coordinates(series: &Series) -> Result<Coordinates, String> {
    let (prefix, dimensions) = series
        .scope
        .rsplit_once(" · [")
        .ok_or("SPM counter has no dimension metadata")?;
    if !prefix.starts_with("process ") {
        return Err("Expected a normalized SPM scope".into());
    }
    let (partition, id) = prefix
        .rsplit_once(" · counter ")
        .ok_or("SPM calibration requires recorded counter IDs")?;
    id.parse::<u64>().map_err(|_| "Invalid SPM counter ID")?;
    let dims: Vec<Dimension> = serde_json::from_str(&format!("[{dimensions}"))
        .map_err(|e| format!("Invalid SPM dimensions: {e}"))?;
    let mut seen = BTreeSet::new();
    let (mut xcc, mut se, mut instance) = (0, 0, 0);
    let mut extra = BTreeMap::new();
    for dim in dims {
        if dim.dimension_name.is_empty() || !seen.insert(dim.dimension_name.clone()) {
            return Err("Repeated or empty SPM dimension".into());
        }
        match dim.dimension_name.as_str() {
            "DIMENSION_XCC" => xcc = dim.index,
            "DIMENSION_SHADER_ENGINE" => se = dim.index,
            "DIMENSION_INSTANCE" => instance = dim.index,
            _ => {
                extra.insert(dim.dimension_name, dim.index);
            }
        }
    }
    Ok(Coordinates {
        domain: format!("{partition} · {}", serde_json::to_string(&extra).unwrap()),
        xcc,
        se,
        instance,
        counter: id.into(),
    })
}

pub fn prepare(
    capture: &Capture,
    calibration: &RealtimeCalibration,
    se: u32,
) -> Result<Vec<CalibratedGroup>, String> {
    if !matches!(
        capture.format.as_str(),
        "rocprofv3 SPM JSON"
            | "rocprofv3 SPM + host-trap PC sampling JSON"
            | "rocprofv3 SPM + stochastic PC sampling JSON"
    ) || capture.unit != "ns"
    {
        return Err("Select a timestamped rocprofv3 SPM capture for calibration".into());
    }
    let origin = capture
        .origin
        .parse::<u64>()
        .map_err(|_| "Invalid SPM realtime origin")?;
    if capture
        .counters
        .iter()
        .any(|series| series.spm_clock.is_some())
    {
        return Err("SPM counters are already calibrated; select the original SPM capture".into());
    }
    let mut groups = BTreeMap::<(String, u64), Vec<(Coordinates, &Series)>>::new();
    let mut samples = 0usize;
    for series in &capture.counters {
        samples = samples
            .checked_add(series.points.len())
            .ok_or("Too many SPM samples")?;
        if samples > 1_000_000 {
            return Err("SPM calibration exceeds one million input samples".into());
        }
        let coords = coordinates(series)?;
        groups
            .entry((coords.domain.clone(), coords.xcc))
            .or_default()
            .push((coords, series));
    }
    let timestamp = |relative: f64| -> Result<u64, String> {
        if !relative.is_finite()
            || relative < 0.
            || relative.fract() != 0.
            || relative > 9_007_199_254_740_991.
        {
            return Err("Invalid or inexact relative SPM timestamp".into());
        }
        origin
            .checked_add(relative as u64)
            .ok_or("SPM absolute timestamp overflows u64".into())
    };
    groups.into_iter().map(|((domain, xcc), items)| {
        let mut times = BTreeSet::new();
        let mut cycles = BTreeMap::<u64, BTreeMap<u64, (f64, usize)>>::new();
        let mut cycle_ids = BTreeSet::new();
        let mut coordinates_seen = BTreeSet::new();
        for (coords, series) in &items {
            if !coordinates_seen.insert((coords.counter.clone(), coords.se, coords.instance)) {
                return Err("Duplicate SPM counter spatial coordinate".into());
            }
            let selected = series.name == "SQ_CYCLES";
            if selected { cycle_ids.insert(coords.counter.clone()); }
            let mut previous = None;
            for &[relative, value] in &series.points {
                let time = timestamp(relative)?;
                if previous.is_some_and(|t| time <= t) { return Err("Repeated or unordered SPM samples require resolution before calibration".into()); }
                previous = Some(time);
                times.insert(time);
                if selected {
                    if !value.is_finite() || value < 0. { return Err("Invalid captured SQ_CYCLES value".into()); }
                    if value > 0. {
                        let total = cycles.entry(time).or_default().entry(coords.se).or_default();
                        total.0 += value;
                        total.1 += 1;
                    }
                }
            }
        }
        if cycle_ids.len() > 1 { return Err("Multiple SQ_CYCLES counter IDs in one domain; select an unambiguous collection".into()); }
        let times: Vec<_> = times.into_iter().collect();
        let mut other_se_intervals = 0;
        let intervals: Vec<_> = times.iter().enumerate().map(|(i, time)| cycles.get(time).map(|engines| {
            if let Some((sum, count)) = engines.get(&u64::from(se)) {
                sum / *count as f64
            } else {
                if i > 0 { other_se_intervals += 1; }
                engines.values().map(|(sum, count)| sum / *count as f64).sum::<f64>() / engines.len() as f64
            }
        })).collect();
        let clock = spm_clock::align(&times, &intervals, calibration, se)?;
        let mapped: BTreeMap<_, _> = times.iter().copied().zip(clock.shader_clocks.iter().copied()).collect();
        let mut counters = Vec::new();
        for (_, original) in items {
            let mut series = original.clone();
            let mut realtime_timestamps = Vec::with_capacity(series.points.len());
            for point in &mut series.points {
                let time = timestamp(point[0])?;
                realtime_timestamps.push(time.to_string());
                point[0] = mapped[&time];
            }
            series.spm_clock = Some(crate::SpmSampleClock { anchor_se: se, realtime_timestamps });
            counters.push(series);
        }
        Ok(CalibratedGroup { domain, xcc, other_se_intervals, realtime_timestamps: times.iter().map(u64::to_string).collect(), clock, counters })
    }).collect()
}

pub fn attach(trace: &Capture, spm: &Capture, se: u32) -> Result<Capture, String> {
    if trace.unit != "cycles" || trace.realtime.is_none() {
        return Err("SPM attachment requires a shader-cycle trace with REALTIME anchors".into());
    }
    if trace.counters.iter().any(|s| s.spm_clock.is_some()) {
        return Err(
            "This capture already has calibrated SPM counters; use the original trace".into(),
        );
    }
    let groups = prepare(spm, trace.realtime.as_ref().unwrap(), se)?;
    if groups.is_empty() {
        return Err("No SPM counter groups to attach".into());
    }
    let reports: Vec<_> = groups.iter().map(|g| serde_json::json!({
        "domain": g.domain, "xcc": g.xcc, "samples": g.realtime_timestamps.len(),
        "measuredIntervals": g.clock.measured_intervals, "fallbackIntervals": g.clock.fallback_intervals,
        "otherSeIntervals": g.other_se_intervals, "extrapolated": g.clock.extrapolated,
        "rangesOverlap": g.clock.ranges_overlap,
    })).collect();
    let mut combined = trace.clone();
    combined.name = format!("{} + {}", trace.name, spm.name);
    combined.metadata.insert(
        "spmAttachment".into(),
        serde_json::json!({
            "sourceName": spm.name, "sourceOrigin": spm.origin,
            "sourceFingerprint": spm.input_fingerprint, "anchorSe": se, "groups": reports,
        })
        .to_string(),
    );
    combined.warnings.push(format!("SPM calibrated to SE {se}'s shader clock. Attachment does not shift trace clocks; capture association is user-selected, not independently verified."));
    if spm.metadata.contains_key("pc_sampling_method") {
        combined.warnings.push("Only SPM counters were attached. PC sampling observations remain in the source capture; no association with this trace's ISA was inferred.".into());
    }
    for warning in &spm.warnings {
        if !warning.starts_with("SPM is shown on its own timestamp axis.") {
            combined.warnings.push(format!("Attached SPM: {warning}"));
        }
    }
    for group in groups {
        combined.warnings.push(format!("Attached SPM {} / XCC {}: {} measured intervals, {} REALTIME-slope intervals, {} other-SE intervals; extrapolated={}, realtime ranges overlap={}.", group.domain, group.xcc, group.clock.measured_intervals, group.clock.fallback_intervals, group.other_se_intervals, group.clock.extrapolated, group.clock.ranges_overlap));
        for series in group.counters {
            for point in &series.points {
                combined.start = combined.start.min(point[0]);
                combined.end = combined.end.max(point[0]);
            }
            combined.counters.push(series);
        }
    }
    Ok(combined)
}
