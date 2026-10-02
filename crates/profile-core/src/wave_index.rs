use crate::Capture;
use std::{
    collections::BTreeMap,
    sync::{Arc, Weak},
};
const MAX_INDEX_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_WINDOW_WAVES: usize = 128;
pub const MAX_AGGREGATE_WAVES: usize = 4096;
pub const MAX_VIEWPORT_TOKENS: usize = 500_000;

/// Formats a count with ASCII thousands separators ("500,000"), matching the
/// frontend's `Number.toLocaleString("en-US")` so the interpolated budget
/// messages below still match `src/waveOverview.ts::isWaveDetailBudgetError`.
fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let bytes = digits.as_bytes();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}

pub fn check_window_waves(count: usize) -> Result<(), String> {
    if count > MAX_WINDOW_WAVES {
        return Err(format!("Wave query exceeds {MAX_WINDOW_WAVES} waves"));
    }
    Ok(())
}

pub fn visible_token_budget_error() -> String {
    format!("Visible wave query exceeds {} tokens. Zoom in or use Target wave. No tokens were silently dropped.", thousands(MAX_VIEWPORT_TOKENS))
}

/// The aggregate-wave (statistics / overview) budget rejection, with the
/// MAX_AGGREGATE_WAVES constant interpolated so the number cannot drift from the
/// bound. `what` names the query ("Wave statistics query", "Wave overview").
pub fn aggregate_waves_budget_error(what: &str) -> String {
    format!("{what} exceeds {MAX_AGGREGATE_WAVES} waves")
}

#[cfg(test)]
mod budget_contract {
    #[test]
    fn budget_messages_stay_in_step_with_the_frontend_matcher() {
        // src/waveOverview.ts::isWaveDetailBudgetError matches these by substring to
        // choose the bounded overview. These assert_eq! pins catch a message that
        // drifts from the interpolated constant (e.g. a raised MAX_VIEWPORT_TOKENS
        // that leaves the literal behind), which would silently break that matcher.
        assert_eq!(
            super::check_window_waves(super::MAX_WINDOW_WAVES + 1).unwrap_err(),
            "Wave query exceeds 128 waves"
        );
        assert_eq!(
            super::visible_token_budget_error(),
            "Visible wave query exceeds 500,000 tokens. Zoom in or use Target wave. No tokens were silently dropped."
        );
        assert_eq!(
            super::aggregate_waves_budget_error("Wave statistics query"),
            "Wave statistics query exceeds 4096 waves"
        );
        assert_eq!(
            super::aggregate_waves_budget_error("Wave overview"),
            "Wave overview exceeds 4096 waves"
        );
        assert_ne!(super::MAX_WINDOW_WAVES, super::MAX_AGGREGATE_WAVES);
    }

    #[test]
    fn thousands_matches_en_us_grouping() {
        assert_eq!(super::thousands(500_000), "500,000");
        assert_eq!(super::thousands(4096), "4,096");
        assert_eq!(super::thousands(128), "128");
        assert_eq!(super::thousands(0), "0");
    }
}
#[derive(Clone, Debug)]
struct Node {
    start: f64,
    end: f64,
    max_end: f64,
    ordinal: usize,
}
#[derive(Debug)]
struct Index {
    engines: BTreeMap<u32, Vec<Node>>,
    bytes: usize,
}
impl Index {
    fn build(capture: &Capture) -> Result<Self, String> {
        if capture.waves.len() > MAX_INDEX_BYTES / std::mem::size_of::<Node>() {
            return Err("Wave index exceeds 128 MiB".into());
        }
        let mut counts = BTreeMap::<u32, usize>::new();
        for wave in &capture.waves {
            if !wave.start.is_finite() || !wave.end.is_finite() || wave.end < wave.start {
                return Err("Invalid wave interval".into());
            }
            *counts.entry(wave.se).or_default() += 1;
        }
        let mut engines: BTreeMap<_, _> = counts
            .into_iter()
            .map(|(se, n)| (se, Vec::with_capacity(n)))
            .collect();
        for (ordinal, wave) in capture.waves.iter().enumerate() {
            engines.get_mut(&wave.se).unwrap().push(Node {
                start: wave.start,
                end: wave.end,
                max_end: wave.end,
                ordinal,
            });
        }
        fn augment(nodes: &mut [Node]) -> f64 {
            if nodes.is_empty() {
                return f64::NEG_INFINITY;
            }
            let mid = nodes.len() / 2;
            let (left, rest) = nodes.split_at_mut(mid);
            let (node, right) = rest.split_first_mut().unwrap();
            node.max_end = node.end.max(augment(left)).max(augment(right));
            node.max_end
        }
        for nodes in engines.values_mut() {
            nodes.sort_unstable_by(|a, b| {
                a.start.total_cmp(&b.start).then(a.ordinal.cmp(&b.ordinal))
            });
            augment(nodes);
        }
        let bytes = engines
            .values()
            .map(|v| v.capacity() * std::mem::size_of::<Node>())
            .sum();
        Ok(Self { engines, bytes })
    }
    fn query(
        &self,
        se: u32,
        start: f64,
        end: f64,
        limit: usize,
    ) -> Result<(Vec<usize>, usize), String> {
        let mut found = Vec::new();
        let mut visited = 0;
        fn visit(
            nodes: &[Node],
            start: f64,
            end: f64,
            found: &mut Vec<usize>,
            visited: &mut usize,
            limit: usize,
        ) -> Result<(), String> {
            if nodes.is_empty() {
                return Ok(());
            }
            let mid = nodes.len() / 2;
            let node = &nodes[mid];
            *visited += 1;
            if node.max_end <= start {
                return Ok(());
            }
            visit(&nodes[..mid], start, end, found, visited, limit)?;
            if node.start < end {
                if node.end > start {
                    found.push(node.ordinal);
                    if found.len() > limit {
                        return Err(format!("This window contains more than {limit} waves. Narrow the range; no waves were silently dropped."));
                    }
                }
                visit(&nodes[mid + 1..], start, end, found, visited, limit)?;
            }
            Ok(())
        }
        if let Some(nodes) = self.engines.get(&se) {
            visit(nodes, start, end, &mut found, &mut visited, limit)?;
        }
        found.sort_unstable();
        Ok((found, visited))
    }
}
struct Entry {
    capture: Weak<Capture>,
    index: Index,
    used: u64,
}
#[derive(Default)]
pub struct WaveWindowCache {
    entries: Vec<Entry>,
    clock: u64,
    builds: u64,
    visited: usize,
}
impl WaveWindowCache {
    pub fn select(
        &mut self,
        capture: &Arc<Capture>,
        se: u32,
        start: f64,
        end: f64,
    ) -> Result<Vec<usize>, String> {
        self.select_limit(capture, se, start, end, MAX_WINDOW_WAVES)
    }
    pub fn select_aggregate(
        &mut self,
        capture: &Arc<Capture>,
        se: u32,
        start: f64,
        end: f64,
    ) -> Result<Vec<usize>, String> {
        self.select_limit(capture, se, start, end, MAX_AGGREGATE_WAVES)
    }
    fn select_limit(
        &mut self,
        capture: &Arc<Capture>,
        se: u32,
        start: f64,
        end: f64,
        limit: usize,
    ) -> Result<Vec<usize>, String> {
        if !start.is_finite() || !end.is_finite() || end <= start {
            return Err("Invalid wave query range".into());
        }
        self.entries.retain(|e| e.capture.strong_count() > 0);
        self.clock += 1;
        let identity = Arc::downgrade(capture);
        let position = match self
            .entries
            .iter()
            .position(|e| e.capture.ptr_eq(&identity))
        {
            Some(position) => position,
            None => {
                let index = Index::build(capture)?;
                while !self.entries.is_empty()
                    && (self.entries.len() >= 8
                        || self.entries.iter().map(|e| e.index.bytes).sum::<usize>() + index.bytes
                            > MAX_INDEX_BYTES)
                {
                    let oldest = self
                        .entries
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, e)| e.used)
                        .unwrap()
                        .0;
                    self.entries.remove(oldest);
                }
                self.entries.push(Entry {
                    capture: identity,
                    index,
                    used: self.clock,
                });
                self.builds += 1;
                self.entries.len() - 1
            }
        };
        let entry = &mut self.entries[position];
        entry.used = self.clock;
        let (indices, visited) = entry.index.query(se, start, end, limit)?;
        self.visited = visited;
        Ok(indices)
    }
    pub fn stats(&self) -> (usize, usize, u64, usize) {
        (
            self.entries.len(),
            self.entries.iter().map(|e| e.index.bytes).sum(),
            self.builds,
            self.visited,
        )
    }
}
