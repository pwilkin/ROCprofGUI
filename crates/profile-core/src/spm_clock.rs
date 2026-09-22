use crate::RealtimeCalibration;
use serde::Serialize;
use std::collections::BTreeMap;

const EXACT: i128 = 9_007_199_254_740_991;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlignedClock {
    pub se: u32,
    pub shader_clocks: Vec<f64>,
    pub measured_intervals: usize,
    pub fallback_intervals: usize,
    pub extrapolated: bool,
    pub ranges_overlap: bool,
}

fn exact_delta(value: i128) -> Result<f64, String> {
    if value.abs() > EXACT {
        Err("Clock calibration exceeds the exact integer range; rebase or split the capture".into())
    } else {
        Ok(value as f64)
    }
}

pub fn align(
    timestamps: &[u64],
    interval_cycles: &[Option<f64>],
    calibration: &RealtimeCalibration,
    se: u32,
) -> Result<AlignedClock, String> {
    if timestamps.is_empty()
        || timestamps.len() != interval_cycles.len()
        || timestamps.len() > 1_000_000
    {
        return Err(
            "SPM calibration requires one to one million matching timestamps and intervals".into(),
        );
    }
    if timestamps.windows(2).any(|p| p[0] >= p[1]) {
        return Err("SPM timestamps must be strictly increasing before calibration".into());
    }
    let mut anchors = BTreeMap::new();
    for anchor in calibration.anchors.iter().filter(|a| a.se == se) {
        let rt = anchor
            .realtime_clock
            .parse::<u64>()
            .map_err(|_| "Invalid REALTIME clock")?;
        let sc = anchor
            .shader_clock
            .parse::<i64>()
            .map_err(|_| "Invalid shader clock")?;
        if anchors
            .insert(rt, sc)
            .is_some_and(|previous| previous != sc)
        {
            return Err("Conflicting REALTIME anchors on the selected shader engine".into());
        }
    }
    let (&first_rt, &first_sc) = anchors
        .first_key_value()
        .ok_or("No REALTIME anchors for selected shader engine")?;
    let (&last_rt, &last_sc) = anchors.last_key_value().unwrap();
    let values: Vec<_> = anchors.values().copied().collect();
    if values.windows(2).any(|p| p[1] <= p[0]) {
        return Err("Shader clock reset, wrap or nonmonotonic REALTIME anchors require explicit normalization".into());
    }
    let first_clock = exact_delta(first_sc as i128)?;
    let slope = if last_rt > first_rt {
        Some(
            exact_delta(last_sc as i128 - first_sc as i128)?
                / exact_delta(last_rt as i128 - first_rt as i128)?,
        )
    } else {
        None
    };
    let mut intervals = vec![0.; timestamps.len()];
    let mut measured_intervals = 0;
    let mut fallback_intervals = 0;
    for i in 1..timestamps.len() {
        let duration = exact_delta(timestamps[i] as i128 - timestamps[i - 1] as i128)?;
        intervals[i] = match interval_cycles[i] {
            Some(cycles) if cycles.is_finite() && cycles > 0. => {
                measured_intervals += 1;
                cycles
            }
            Some(cycles) if !cycles.is_finite() || cycles < 0. => {
                return Err("Invalid SQ_CYCLES interval".into())
            }
            _ => {
                fallback_intervals += 1;
                slope.ok_or("Missing SQ_CYCLES interval and no two-anchor REALTIME slope")?
                    * duration
            }
        };
    }
    let pivot = timestamps
        .iter()
        .enumerate()
        .min_by_key(|(_, time)| time.abs_diff(first_rt))
        .unwrap()
        .0;
    let mut clock = first_clock;
    if timestamps[pivot] != first_rt {
        let rate = if timestamps.len() > 1 {
            let interval = if timestamps[pivot] < first_rt && pivot + 1 < timestamps.len() {
                pivot + 1
            } else {
                pivot.max(1)
            };
            intervals[interval]
                / exact_delta(timestamps[interval] as i128 - timestamps[interval - 1] as i128)?
        } else {
            slope.ok_or("A displaced single SPM sample requires two REALTIME anchors")?
        };
        clock += exact_delta(timestamps[pivot] as i128 - first_rt as i128)? * rate;
    }
    let mut shader_clocks = vec![0.; timestamps.len()];
    shader_clocks[pivot] = clock;
    for i in (1..=pivot).rev() {
        shader_clocks[i - 1] = shader_clocks[i] - intervals[i];
    }
    for i in pivot + 1..timestamps.len() {
        shader_clocks[i] = shader_clocks[i - 1] + intervals[i];
    }
    if shader_clocks
        .iter()
        .any(|v| !v.is_finite() || v.abs() > EXACT as f64)
        || shader_clocks.windows(2).any(|p| p[1] <= p[0])
    {
        return Err("Calibrated clock exceeds numeric precision or is not increasing".into());
    }
    Ok(AlignedClock {
        se,
        shader_clocks,
        measured_intervals,
        fallback_intervals,
        extrapolated: timestamps[0] < first_rt || *timestamps.last().unwrap() > last_rt,
        ranges_overlap: timestamps[0] <= last_rt && *timestamps.last().unwrap() >= first_rt,
    })
}
