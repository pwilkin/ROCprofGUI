use crate::{Capture, Wave, WaveIndex};
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

pub const MAX_GROUPS: usize = 65_536;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub category: u32,
    pub pipe: Option<String>,
    pub shared: bool,
    pub start: f64,
    pub end: f64,
    pub count: usize,
    pub zoom_range: [f64; 2],
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewWave {
    pub wave: WaveIndex,
    pub cu: Option<u32>,
    pub groups: Vec<Group>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub range: [f64; 2],
    pub utilization: bool,
    pub represented_tokens: usize,
    pub waves: Vec<OverviewWave>,
}

pub fn aggregate(wave: &Wave, range: [f64; 2], buckets: usize) -> Result<Vec<Group>, String> {
    aggregate_cancellable(wave, range, buckets, &AtomicBool::new(false))
}
fn aggregate_cancellable(
    wave: &Wave,
    range: [f64; 2],
    buckets: usize,
    cancelled: &AtomicBool,
) -> Result<Vec<Group>, String> {
    if cancelled.load(Ordering::Relaxed) {
        return Err("Wave query cancelled".into());
    }
    let [start, end] = range;
    if !start.is_finite()
        || !end.is_finite()
        || end <= start
        || !(end - start).is_finite()
        || buckets == 0
        || buckets > 512
    {
        return Err("Invalid wave overview range or bucket count".into());
    }
    let width = (end - start) / buckets as f64;
    if width <= 0. || !width.is_finite() {
        return Err("Unrepresentable overview bucket width".into());
    }
    let mut groups = BTreeMap::<(usize, u32), Group>::new();
    for token in &wave.tokens {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Wave query cancelled".into());
        }
        let token_end = token.start + token.duration.max(token.stall);
        if token.start > end || token_end < start {
            continue;
        }
        if !token_end.is_finite() {
            return Err("Unrepresentable token extent in overview".into());
        }
        let bucket = (((token.start.max(start) - start) / width).floor() as usize).min(buckets - 1);
        let group = groups
            .entry((bucket, token.category))
            .or_insert_with(|| Group {
                category: token.category,
                pipe: None,
                shared: false,
                start: token.start,
                end: token_end,
                count: 0,
                zoom_range: [
                    start + bucket as f64 * width,
                    if bucket + 1 == buckets {
                        end
                    } else {
                        start + (bucket + 1) as f64 * width
                    },
                ],
            });
        group.start = group.start.min(token.start);
        group.end = group.end.max(token_end);
        group.count += 1;
        if groups.len() > MAX_GROUPS {
            return Err("Wave overview exceeds group budget".into());
        }
    }
    Ok(groups.into_values().collect())
}

pub fn load(capture: &Capture, indices: &[usize], range: [f64; 2]) -> Result<Overview, String> {
    load_mode(capture, indices, range, false, false)
}

pub fn load_mode(
    capture: &Capture,
    indices: &[usize],
    range: [f64; 2],
    utilization: bool,
    separate_lds: bool,
) -> Result<Overview, String> {
    load_mode_cancellable(
        capture,
        indices,
        range,
        utilization,
        separate_lds,
        &AtomicBool::new(false),
    )
}
pub fn load_mode_cancellable(
    capture: &Capture,
    indices: &[usize],
    range: [f64; 2],
    utilization: bool,
    separate_lds: bool,
    cancelled: &AtomicBool,
) -> Result<Overview, String> {
    if cancelled.load(Ordering::Relaxed) {
        return Err("Wave query cancelled".into());
    }
    let model = if utilization {
        Some(
            crate::utilization::Model::from_capture(capture)
                .ok_or("Unknown utilization clock model")?,
        )
    } else {
        None
    };
    let isa: BTreeMap<_, _> = if utilization {
        capture
            .instructions
            .iter()
            .map(|i| (i.index, i.text.as_str()))
            .collect()
    } else {
        BTreeMap::new()
    };
    if indices.len() > crate::wave_index::MAX_AGGREGATE_WAVES {
        return Err("Wave overview exceeds 4096 waves".into());
    }
    let buckets = (MAX_GROUPS / (indices.len().max(1) * 14)).clamp(1, 512);
    let mut result = Overview {
        range,
        utilization,
        represented_tokens: 0,
        waves: Vec::new(),
    };
    let mut group_count = 0;
    for index in indices {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Wave query cancelled".into());
        }
        let mut wave = crate::import::load_wave_at_cancellable(capture, *index, cancelled)?;
        if let Some(model) = model {
            let mut projected = Vec::with_capacity(wave.tokens.len());
            for mut token in wave.tokens.drain(..) {
                if cancelled.load(Ordering::Relaxed) {
                    return Err("Wave query cancelled".into());
                }
                let text = isa.get(&token.instruction).copied().unwrap_or("");
                if model.project(&mut token, text)? {
                    projected.push(token);
                }
            }
            wave.tokens = projected;
        }
        let mut groups = aggregate_cancellable(&wave, range, buckets, cancelled)?;
        if let Some(model) = model {
            for group in &mut groups {
                let (pipe, shared) = model.pipe(group.category, separate_lds);
                group.pipe = Some(pipe.into());
                group.shared = shared;
            }
        }
        group_count += groups.len();
        if group_count > MAX_GROUPS {
            return Err("Wave overview exceeds group budget".into());
        }
        result.represented_tokens += groups.iter().map(|g| g.count).sum::<usize>();
        result.waves.push(OverviewWave {
            wave: capture.waves[*index].clone(),
            cu: wave.cu,
            groups,
        });
    }
    if indices.is_empty()
        && (!range[0].is_finite() || !range[1].is_finite() || range[1] <= range[0])
    {
        return Err("Invalid wave overview range".into());
    }
    if cancelled.load(Ordering::Relaxed) {
        return Err("Wave query cancelled".into());
    }
    Ok(result)
}
