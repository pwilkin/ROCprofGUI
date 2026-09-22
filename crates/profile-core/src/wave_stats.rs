use crate::{Capture, Token};
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

pub const MAX_ROWS: usize = 65_536;

#[derive(Debug, Default, Serialize)]
pub struct InstructionStats {
    pub instruction: i64,
    pub hits: usize,
    pub latency: f64,
    pub stall: f64,
}

#[cfg(test)]
fn accumulate(
    stats: &mut BTreeMap<i64, InstructionStats>,
    tokens: &[Token],
    range: [f64; 2],
) -> Result<(), String> {
    accumulate_cancellable(stats, tokens, range, &AtomicBool::new(false))
}
fn accumulate_cancellable(
    stats: &mut BTreeMap<i64, InstructionStats>,
    tokens: &[Token],
    range: [f64; 2],
    cancelled: &AtomicBool,
) -> Result<(), String> {
    for token in tokens {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Wave query cancelled".into());
        }
        if token.start < range[0] || token.start >= range[1] {
            continue;
        }
        if !token.start.is_finite() || !token.duration.is_finite() || !token.stall.is_finite() {
            return Err("Non-finite wave statistics input".into());
        }
        let row = stats
            .entry(token.instruction)
            .or_insert_with(|| InstructionStats {
                instruction: token.instruction,
                ..Default::default()
            });
        row.hits += 1;
        row.latency += token.duration;
        row.stall += token.stall;
        if !row.latency.is_finite() || !row.stall.is_finite() {
            return Err("Wave statistics total overflow".into());
        }
        if stats.len() > MAX_ROWS {
            return Err(
                "Wave statistics exceed 65,536 distinct instructions; narrow the range".into(),
            );
        }
    }
    Ok(())
}

pub fn load(
    capture: &Capture,
    indices: &[usize],
    range: [f64; 2],
) -> Result<Vec<InstructionStats>, String> {
    load_cancellable(capture, indices, range, &AtomicBool::new(false))
}
pub fn load_cancellable(
    capture: &Capture,
    indices: &[usize],
    range: [f64; 2],
    cancelled: &AtomicBool,
) -> Result<Vec<InstructionStats>, String> {
    if cancelled.load(Ordering::Relaxed) {
        return Err("Wave query cancelled".into());
    }
    if !range[0].is_finite() || !range[1].is_finite() || range[1] <= range[0] {
        return Err("Invalid wave statistics range".into());
    }
    if indices.len() > crate::wave_index::MAX_AGGREGATE_WAVES {
        return Err("Wave statistics query exceeds 4096 waves".into());
    }
    let mut stats = BTreeMap::new();
    for index in indices {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Wave query cancelled".into());
        }
        let wave = crate::import::load_wave_at_cancellable(capture, *index, cancelled)?;
        accumulate_cancellable(&mut stats, &wave.tokens, range, cancelled)?;
    }
    if cancelled.load(Ordering::Relaxed) {
        return Err("Wave query cancelled".into());
    }
    Ok(stats.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn token(start: f64, duration: f64) -> Token {
        Token {
            start,
            duration,
            stall: 2.,
            category: 0,
            instruction: 7,
        }
    }
    #[test]
    fn half_open_start_filter_and_cross_wave_totals() {
        let mut stats = BTreeMap::new();
        accumulate(
            &mut stats,
            &[
                token(-1., 50.),
                token(0., 3.),
                token(9., 8.),
                token(10., 4.),
            ],
            [0., 10.],
        )
        .unwrap();
        accumulate(&mut stats, &[token(2., 5.)], [0., 10.]).unwrap();
        let row = &stats[&7];
        assert_eq!(row.hits, 3);
        assert_eq!(row.latency, 16.);
        assert_eq!(row.stall, 6.);
    }
    #[test]
    fn more_than_timeline_token_budget_is_exact() {
        let mut stats = BTreeMap::new();
        accumulate(&mut stats, &vec![token(0., 3.); 500_001], [0., 1.]).unwrap();
        assert_eq!(stats[&7].hits, 500_001);
        assert_eq!(stats[&7].latency, 1_500_003.);
    }
    #[test]
    fn overflow_is_an_error() {
        assert!(accumulate(
            &mut BTreeMap::new(),
            &[token(0., f64::MAX), token(0., f64::MAX)],
            [0., 1.]
        )
        .is_err());
    }
}
