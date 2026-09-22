use crate::{Capture, Wave};
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

fn check(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) {
        Err("Wave query cancelled".into())
    } else {
        Ok(())
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenIdentity {
    pub index: usize,
    pub occurrence: usize,
    pub display_slot: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewportWave {
    #[serde(flatten)]
    pub wave: Wave,
    pub token_identities: Vec<TokenIdentity>,
    pub viewport: [f64; 2],
}
impl std::ops::Deref for ViewportWave {
    type Target = Wave;
    fn deref(&self) -> &Wave {
        &self.wave
    }
}

pub fn project(wave: Wave, start: f64, end: f64) -> Result<ViewportWave, String> {
    project_cancellable(wave, start, end, &AtomicBool::new(false))
}
pub fn project_cancellable(
    mut wave: Wave,
    start: f64,
    end: f64,
    cancelled: &AtomicBool,
) -> Result<ViewportWave, String> {
    check(cancelled)?;
    if !start.is_finite() || !end.is_finite() || end <= start {
        return Err("Invalid wave query range".into());
    }
    let mut occurrences = BTreeMap::<i64, usize>::new();
    let mut previous = [0.; 4];
    let mut ends = [0.; 4];
    let mut identities = Vec::new();
    let mut retained = 0;
    for index in 0..wave.tokens.len() {
        check(cancelled)?;
        let token = &wave.tokens[index];
        let occurrence = occurrences.entry(token.instruction).or_default();
        let mut slot = 0;
        while slot < 3 && previous[slot] == token.start && ends[slot] > token.start {
            slot += 1;
        }
        previous[slot] = token.start;
        let token_end = token.start + token.duration.max(token.stall);
        ends[slot] = f64::max(ends[slot], token_end);
        let visible = token.start <= end && token_end >= start;
        if visible {
            identities.push(TokenIdentity {
                index,
                occurrence: *occurrence,
                display_slot: slot,
            });
        }
        *occurrence += 1;
        if visible {
            wave.tokens.swap(retained, index);
            retained += 1;
        }
    }
    wave.tokens.truncate(retained);
    let mut retained = 0;
    for index in 0..wave.states.len() {
        check(cancelled)?;
        let s = wave.states[index];
        if s[0] <= end && s[0] + s[1] >= start {
            wave.states.swap(retained, index);
            retained += 1;
        }
    }
    wave.states.truncate(retained);
    wave.tokens.shrink_to_fit();
    wave.states.shrink_to_fit();
    wave.waits = serde_json::Value::Null;
    check(cancelled)?;
    Ok(ViewportWave {
        wave,
        token_identities: identities,
        viewport: [start, end],
    })
}

pub fn load(
    capture: &Capture,
    indices: &[usize],
    start: f64,
    end: f64,
) -> Result<Vec<ViewportWave>, String> {
    load_cancellable(capture, indices, start, end, &AtomicBool::new(false))
}
pub fn load_cancellable(
    capture: &Capture,
    indices: &[usize],
    start: f64,
    end: f64,
    cancelled: &AtomicBool,
) -> Result<Vec<ViewportWave>, String> {
    check(cancelled)?;
    if !start.is_finite() || !end.is_finite() || end <= start {
        return Err("Invalid wave query range".into());
    }
    crate::wave_index::check_window_waves(indices.len())?;
    let mut waves = Vec::with_capacity(indices.len());
    let mut tokens = 0;
    for index in indices {
        let se = capture
            .waves
            .get(*index)
            .ok_or("Wave is not part of this capture")?
            .se;
        let offset = capture.se_time_offsets.get(&se).copied();
        let mut occurrences = BTreeMap::<i64, usize>::new();
        let mut previous = [0.; 4];
        let mut ends = [0.; 4];
        let mut identities = Vec::new();
        let mut token_index = 0;
        let mut wave = crate::import::load_wave_filtered(capture, *index, cancelled, |token| {
            let time = match offset {
                Some(offset) => crate::se_clock::shift(token.start, offset)?,
                None => token.start,
            };
            let occurrence = occurrences.entry(token.instruction).or_default();
            let mut slot = 0;
            while slot < 3 && previous[slot] == time && ends[slot] > time {
                slot += 1;
            }
            previous[slot] = time;
            let token_end = time + token.duration.max(token.stall);
            ends[slot] = f64::max(ends[slot], token_end);
            let visible = time <= end && token_end >= start;
            if visible {
                if tokens + identities.len() >= crate::wave_index::MAX_VIEWPORT_TOKENS {
                    return Err(crate::wave_index::visible_token_budget_error());
                }
                identities.push(TokenIdentity {
                    index: token_index,
                    occurrence: *occurrence,
                    display_slot: slot,
                });
            }
            *occurrence += 1;
            token_index += 1;
            Ok(visible)
        })?;
        wave.states.retain(|s| s[0] <= end && s[0] + s[1] >= start);
        wave.states.shrink_to_fit();
        wave.waits = serde_json::Value::Null;
        let wave = ViewportWave {
            wave,
            token_identities: identities,
            viewport: [start, end],
        };
        tokens += wave.tokens.len();
        if tokens > crate::wave_index::MAX_VIEWPORT_TOKENS {
            return Err(crate::wave_index::visible_token_budget_error());
        }
        waves.push(wave);
    }
    check(cancelled)?;
    Ok(waves)
}
