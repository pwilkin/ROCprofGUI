use crate::RealtimeCalibration;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlignmentPlan {
    pub anchor_se: u32,
    pub offsets: BTreeMap<u32, i64>,
    pub measured_engines: usize,
    pub uses_only_seed: bool,
    pub ratio_shader_cycles: String,
    pub ratio_realtime_ticks: String,
}

pub fn plan(
    calibration: &RealtimeCalibration,
    required: &BTreeSet<u32>,
) -> Result<AlignmentPlan, String> {
    if calibration.anchors.len() > 100_000 {
        return Err("Too many REALTIME anchors".into());
    }
    let mut engines = BTreeMap::<u32, BTreeMap<u64, i64>>::new();
    for anchor in &calibration.anchors {
        let rt = anchor
            .realtime_clock
            .parse::<u64>()
            .map_err(|_| "Invalid REALTIME clock")?;
        let sc = anchor
            .shader_clock
            .parse::<i64>()
            .map_err(|_| "Invalid shader clock")?;
        if engines
            .entry(anchor.se)
            .or_default()
            .insert(rt, sc)
            .is_some_and(|previous| previous != sc)
        {
            return Err("Conflicting REALTIME anchors".into());
        }
    }
    if engines.len() < 2 {
        return Err(
            "Cross-SE alignment requires at least two engines with REALTIME anchors".into(),
        );
    }
    if required.iter().any(|se| !engines.contains_key(se)) {
        return Err("A populated shader engine has no REALTIME anchors".into());
    }
    let mut ds = 25i128;
    let mut dr = 1i128;
    let mut measured_engines = 0;
    for records in engines.values() {
        let values: Vec<_> = records.values().copied().collect();
        if values.windows(2).any(|p| p[1] <= p[0]) {
            return Err(
                "Nonmonotonic shader clocks require reset/wrap normalization before SE alignment"
                    .into(),
            );
        }
        let (&lo_rt, &lo_sc) = records.first_key_value().unwrap();
        let (&hi_rt, &hi_sc) = records.last_key_value().unwrap();
        if hi_sc > lo_sc && lo_sc > 0 && hi_rt > lo_rt {
            ds = ds
                .checked_add(hi_sc as i128 - lo_sc as i128)
                .ok_or("Shader span overflow")?;
            dr = dr
                .checked_add(hi_rt as i128 - lo_rt as i128)
                .ok_or("Realtime span overflow")?;
            measured_engines += 1;
        }
    }
    let base = engines
        .values()
        .map(|r| *r.first_key_value().unwrap().0)
        .min()
        .unwrap();
    let mut origins = BTreeMap::new();
    for (&se, records) in &engines {
        let (&rt, &sc) = records.first_key_value().unwrap();
        let rt_scaled = (rt as i128 - base as i128)
            .checked_mul(ds)
            .ok_or("Realtime alignment multiplication overflow")?;
        let sc_scaled = (sc as i128)
            .checked_mul(dr)
            .ok_or("Shader alignment multiplication overflow")?;
        origins.insert(
            se,
            rt_scaled
                .checked_sub(sc_scaled)
                .ok_or("Clock origin overflow")?,
        );
    }
    let (&anchor_se, &minimum) = origins
        .iter()
        .min_by_key(|(se, value)| (**value, **se))
        .unwrap();
    let mut offsets = BTreeMap::new();
    for (se, origin) in origins {
        let difference = origin.checked_sub(minimum).ok_or("Clock offset overflow")?;
        let offset = difference / dr;
        if offset > 9_007_199_254_740_991 {
            return Err("SE clock offset exceeds exact frontend precision".into());
        }
        offsets.insert(se, offset as i64);
    }
    Ok(AlignmentPlan {
        anchor_se,
        offsets,
        measured_engines,
        uses_only_seed: measured_engines == 0,
        ratio_shader_cycles: ds.to_string(),
        ratio_realtime_ticks: dr.to_string(),
    })
}

pub(crate) fn shift(time: f64, offset: i64) -> Result<f64, String> {
    let shifted = time + offset as f64;
    if offset.unsigned_abs() > 9_007_199_254_740_991
        || !time.is_finite()
        || !shifted.is_finite()
        || time.abs() > 9_007_199_254_740_991.
        || shifted.abs() > 9_007_199_254_740_991.
    {
        return Err("Aligned timestamp exceeds exact frontend precision".into());
    }
    Ok(shifted)
}

pub fn normalize(capture: &crate::Capture) -> Result<crate::Capture, String> {
    if capture.unit != "cycles" || !capture.se_time_offsets.is_empty() {
        return Err("Cross-SE normalization requires an unaligned shader-cycle capture".into());
    }
    if capture.dispatches.iter().any(|d| d.shader_engine.is_none())
        || !capture.host_calls.is_empty()
        || !capture.monitor_intervals.is_empty()
    {
        return Err("Capture contains timestamp records without shader-engine attribution".into());
    }
    let calibration = capture.realtime.as_ref().ok_or("No REALTIME calibration")?;
    let mut required = BTreeSet::new();
    required.extend(capture.dispatches.iter().filter_map(|d| d.shader_engine));
    required.extend(capture.waves.iter().map(|x| x.se));
    required.extend(capture.intervals.iter().map(|x| x.se));
    required.extend(capture.decoder_events.iter().map(|x| x.se));
    required.extend(capture.other_simd.iter().map(|x| x.se));
    required.extend(capture.markers.iter().map(|x| x.se));
    let mut counter_engines = Vec::new();
    for series in &capture.counters {
        if series.spm_clock.is_some() {
            return Err("Normalize SE clocks before attaching SPM".into());
        }
        let se = series
            .scope
            .strip_prefix("SE ")
            .and_then(|s| s.split_once(" · "))
            .and_then(|(se, _)| se.parse::<u32>().ok())
            .ok_or("A counter series has no unambiguous shader-engine attribution")?;
        required.insert(se);
        counter_engines.push(se);
    }
    let plan = plan(calibration, &required)?;
    if plan.uses_only_seed {
        return Err("No measured engine clock span; upstream's 25-cycle seed alone is insufficient for automatic alignment".into());
    }
    let mut result = capture.clone();
    let (mut begin, mut end) = (f64::INFINITY, f64::NEG_INFINITY);
    let mut move_time = |value: &mut f64, se: u32| -> Result<(), String> {
        *value = shift(*value, plan.offsets[&se])?;
        begin = begin.min(*value);
        end = end.max(*value);
        Ok(())
    };
    for wave in &mut result.waves {
        move_time(&mut wave.start, wave.se)?;
        move_time(&mut wave.end, wave.se)?;
    }
    for interval in &mut result.intervals {
        move_time(&mut interval.start, interval.se)?;
        move_time(&mut interval.end, interval.se)?;
    }
    for dispatch in &mut result.dispatches {
        let se = dispatch.shader_engine.ok_or("Missing dispatch SE")?;
        move_time(&mut dispatch.start, se)?;
        move_time(&mut dispatch.end, se)?;
    }
    for event in &mut result.decoder_events {
        move_time(&mut event.time, event.se)?;
    }
    for activity in &mut result.other_simd {
        let mut activity_end = activity.start + activity.duration;
        move_time(&mut activity.start, activity.se)?;
        move_time(&mut activity_end, activity.se)?;
    }
    for marker in &mut result.markers {
        move_time(&mut marker.start, marker.se)?;
        if let Some(end) = &mut marker.end {
            move_time(end, marker.se)?;
        }
    }
    for (series, se) in result.counters.iter_mut().zip(counter_engines) {
        for point in &mut series.points {
            move_time(&mut point[0], se)?;
        }
    }
    if begin.is_finite() {
        result.start = begin;
        result.end = end.max(begin + 1.);
    }
    for anchor in &mut result.realtime.as_mut().unwrap().anchors {
        let clock = anchor
            .shader_clock
            .parse::<i64>()
            .map_err(|_| "Invalid shader clock")?;
        anchor.shader_clock = clock
            .checked_add(plan.offsets[&anchor.se])
            .ok_or("Aligned shader anchor overflows i64")?
            .to_string();
    }
    result
        .warnings
        .retain(|w| !w.contains("REALTIME anchors retained but not yet applied."));
    result.warnings.push(format!("Cross-SE REALTIME offsets applied with anchor SE {} and {} measured engine spans; ratio includes upstream's 25-cycle/1-tick seed. Lazy waves receive the same offsets on load.",plan.anchor_se,plan.measured_engines));
    result.metadata.insert(
        "seAlignment".into(),
        serde_json::to_string(&plan).map_err(|e| e.to_string())?,
    );
    result.se_time_offsets = plan.offsets;
    Ok(result)
}
