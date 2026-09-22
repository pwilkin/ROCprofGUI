pub mod att_manifest;
pub mod att_runs;
mod cancellable_read;
pub mod capture_archive;
pub mod catalog_transfer;
pub mod code_object_files;
pub mod decoder;
pub mod definition_files;
pub mod diagnostics;
pub mod hidden;
mod import;
mod wave_stream;
pub mod input_fingerprint;
pub mod markers;
pub mod model;
mod pc_sampling;
mod pc_sampling_csv;
pub mod plot_files;
pub mod project;
mod realtime;
pub mod rocpd;
pub mod se_clock;
pub mod source_files;
pub mod source_index;
pub mod spm_clock;
mod spm_dimensions;
pub mod spm_prepare;
pub mod wait_encoding;
pub mod waits;
pub mod wave_index;
pub mod token_index;
pub mod token_cache;
pub mod token_directory;
pub use import::{
    load_capture, load_capture_options, load_capture_with_wave_states, load_wave,
    load_wave_cancellable, sampling_methods,
};
pub use model::*;

pub fn interval_union(intervals: &[(f64, f64)]) -> f64 {
    let mut ranges: Vec<_> = intervals
        .iter()
        .copied()
        .filter(|(a, b)| a.is_finite() && b.is_finite() && b > a)
        .collect();
    ranges.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut total = 0.;
    let mut current: Option<(f64, f64)> = None;
    for (a, b) in ranges {
        match current {
            Some((s, e)) if a <= e => current = Some((s, e.max(b))),
            Some((s, e)) => {
                total += e - s;
                current = Some((a, b));
            }
            None => current = Some((a, b)),
        }
    }
    total + current.map_or(0., |(s, e)| e - s)
}

pub mod utilization;
pub mod wave_overview;
pub mod wave_requests;
pub mod wave_stats;
pub mod wave_viewport;

pub fn load_wave_window(
    capture: &Capture,
    se: u32,
    start: f64,
    end: f64,
) -> Result<Vec<wave_viewport::ViewportWave>, String> {
    if !start.is_finite() || !end.is_finite() || end <= start {
        return Err("Invalid wave query range".into());
    }
    let indices: Vec<_> = capture
        .waves
        .iter()
        .enumerate()
        .filter(|(_, w)| w.se == se && w.start < end && w.end > start)
        .map(|(index, _)| index)
        .collect();
    if indices.len() > wave_index::MAX_WINDOW_WAVES {
        return Err(format!("This window contains more than {} detailed waves. Zoom in before aggregating; no waves were silently dropped.", wave_index::MAX_WINDOW_WAVES));
    }
    wave_viewport::load(capture, &indices, start, end)
}

pub fn load_wave_indices(capture: &Capture, indices: &[usize]) -> Result<Vec<Wave>, String> {
    wave_index::check_window_waves(indices.len())?;
    let mut waves = Vec::with_capacity(indices.len());
    let mut tokens = 0;
    for index in indices {
        let wave = import::load_wave_at(capture, *index)?;
        tokens += wave.tokens.len();
        if tokens > wave_index::MAX_VIEWPORT_TOKENS {
            return Err("Selected waves exceed 500,000 tokens. This query loads whole waves; choose a range overlapping fewer waves or use Target wave. No waves were silently dropped.".into());
        }
        waves.push(wave);
    }
    Ok(waves)
}

pub mod timeline_file;
