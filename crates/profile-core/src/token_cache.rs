use crate::{token_index::TokenIndex, Capture};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, Weak,
    },
};

const BUILD_BYTES: usize = 2_000_000 * 56;
const MAX_DISK_BYTES: usize = 512 * 1024 * 1024;
const MAX_RESIDENT_BYTES: usize = 64 * 1024 * 1024;
const MAX_ENTRIES: usize = 8;

struct Entry {
    owner: Weak<Capture>,
    ordinal: usize,
    index: Arc<TokenIndex>,
}
struct Pending {
    id: u64,
    owner: Weak<Capture>,
    ordinal: usize,
    cancelled: bool,
}
#[derive(Default)]
struct State {
    entries: Vec<Entry>,
    retired: Vec<Weak<TokenIndex>>,
    pending: Vec<Pending>,
    next: u64,
    closed: Vec<Weak<Capture>>,
    generation: u64,
}
impl State {
    fn prune(&mut self) {
        self.closed.retain(|owner| owner.strong_count() > 0);
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].owner.strong_count() == 0 {
                let entry = self.entries.remove(i);
                self.retired.push(Arc::downgrade(&entry.index));
            } else {
                i += 1;
            }
        }
        self.retired.retain(|index| index.strong_count() > 0);
    }
    fn disk_bytes(&self) -> usize {
        self.entries
            .iter()
            .map(|e| e.index.disk_bytes())
            .sum::<usize>()
            + self
                .retired
                .iter()
                .filter_map(Weak::upgrade)
                .map(|i| i.disk_bytes())
                .sum::<usize>()
            + self.pending.len() * BUILD_BYTES
    }
    fn resident_bytes(&self) -> usize {
        self.entries
            .iter()
            .map(|e| e.index.estimated_resident_bytes())
            .sum::<usize>()
            + self
                .retired
                .iter()
                .filter_map(Weak::upgrade)
                .map(|i| i.estimated_resident_bytes())
                .sum::<usize>()
    }
    fn evict(&mut self) -> bool {
        if let Some(i) = self
            .entries
            .iter()
            .position(|e| Arc::strong_count(&e.index) == 1)
        {
            self.entries.remove(i);
            true
        } else {
            false
        }
    }
}
#[derive(Default)]
pub struct TokenCache {
    state: Mutex<State>,
}
impl TokenCache {
    pub fn load_window(
        &self,
        capture: &Arc<Capture>,
        ordinals: &[usize],
        start: f64,
        end: f64,
        directory: &Path,
        cancelled: &AtomicBool,
    ) -> Result<Vec<crate::wave_viewport::ViewportWave>, String> {
        if !start.is_finite() || !end.is_finite() || end <= start {
            return Err("Invalid wave query range".into());
        }
        crate::wave_index::check_window_waves(ordinals.len())?;
        let mut result = Vec::with_capacity(ordinals.len());
        let mut count = 0;
        for ordinal in ordinals {
            if cancelled.load(Ordering::Relaxed) {
                return Err("Wave query cancelled".into());
            }
            let wave = match self.get_or_build(capture, *ordinal, directory, cancelled)? {
                Some(index) => index.query(start, end, crate::wave_index::MAX_VIEWPORT_TOKENS - count, cancelled)?.0,
                None => crate::wave_viewport::load_cancellable(
                    capture,
                    &[*ordinal],
                    start,
                    end,
                    cancelled,
                )?
                .remove(0),
            };
            count += wave.tokens.len();
            if count > crate::wave_index::MAX_VIEWPORT_TOKENS {
                return Err(crate::wave_index::visible_token_budget_error());
            }
            result.push(wave);
        }
        if cancelled.load(Ordering::Relaxed) {
            return Err("Wave query cancelled".into());
        }
        Ok(result)
    }

    pub fn get_or_build(
        &self,
        capture: &Arc<Capture>,
        ordinal: usize,
        directory: &Path,
        cancelled: &AtomicBool,
    ) -> Result<Option<Arc<TokenIndex>>, String> {
        self.get_or_build_using(capture, ordinal, cancelled, || {
            TokenIndex::build_for_cache(capture, ordinal, directory, cancelled)
        })
    }

    fn get_or_build_using(
        &self,
        capture: &Arc<Capture>,
        ordinal: usize,
        cancelled: &AtomicBool,
        build: impl FnOnce() -> Result<Option<TokenIndex>, String>,
    ) -> Result<Option<Arc<TokenIndex>>, String> {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Wave query cancelled".into());
        }
        let (candidate, generation) = {
            let mut state = self.state.lock().map_err(|e| e.to_string())?;
            state.prune();
            if state
                .closed
                .iter()
                .any(|owner| owner.ptr_eq(&Arc::downgrade(capture)))
            {
                return Err("Capture is closed".into());
            }
            let candidate = state
                .entries
                .iter()
                .position(|e| e.ordinal == ordinal && e.owner.ptr_eq(&Arc::downgrade(capture)))
                .map(|i| {
                    let entry = state.entries.remove(i);
                    let index = entry.index.clone();
                    state.entries.push(entry);
                    index
                });
            (candidate, state.generation)
        };
        if let Some(index) = candidate {
            if index.is_current() {
                return Ok(Some(index));
            }
            let mut state = self.state.lock().map_err(|e| e.to_string())?;
            state.entries.retain(|e| !Arc::ptr_eq(&e.index, &index));
            state.retired.push(Arc::downgrade(&index));
        }
        let ticket = {
            let mut state = self.state.lock().map_err(|e| e.to_string())?;
            state.prune();
            if state.generation != generation
                || state
                    .closed
                    .iter()
                    .any(|owner| owner.ptr_eq(&Arc::downgrade(capture)))
            {
                return Err("Wave query cancelled".into());
            }
            let owner = Arc::downgrade(capture);
            if state
                .pending
                .iter()
                .any(|p| p.ordinal == ordinal && p.owner.ptr_eq(&owner))
                || state
                    .entries
                    .iter()
                    .any(|e| e.ordinal == ordinal && e.owner.ptr_eq(&owner))
            {
                return Ok(None);
            }
            while state.disk_bytes() + BUILD_BYTES > MAX_DISK_BYTES
                || state.entries.len() + state.pending.len() >= MAX_ENTRIES
            {
                if !state.evict() {
                    return Ok(None);
                }
            }
            let id = state.next;
            state.next = state
                .next
                .checked_add(1)
                .ok_or("Token cache request counter exhausted")?;
            state.pending.push(Pending {
                id,
                owner: Arc::downgrade(capture),
                ordinal,
                cancelled: false,
            });
            id
        };
        let built = build();
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        let position = state
            .pending
            .iter()
            .position(|p| p.id == ticket)
            .ok_or("Token cache reservation lost")?;
        let pending = state.pending.remove(position);
        if pending.cancelled || cancelled.load(Ordering::Relaxed) {
            return Err("Wave query cancelled".into());
        }
        let Some(index) = built? else {
            return Ok(None);
        };
        let index = Arc::new(index);
        state.prune();
        let resident_bytes = index.estimated_resident_bytes();
        if resident_bytes > MAX_RESIDENT_BYTES {
            return Ok(None);
        }
        while state.resident_bytes() + resident_bytes > MAX_RESIDENT_BYTES {
            if !state.evict() {
                return Ok(None);
            }
        }
        state.entries.push(Entry {
            owner: Arc::downgrade(capture),
            ordinal,
            index: index.clone(),
        });
        Ok(Some(index))
    }

    pub fn remove_capture(&self, capture: &Arc<Capture>) -> Result<(), String> {
        let owner = Arc::downgrade(capture);
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        if !state.closed.iter().any(|closed| closed.ptr_eq(&owner)) {
            state.closed.push(owner.clone());
        }
        for pending in &mut state.pending {
            if pending.owner.ptr_eq(&owner) {
                pending.cancelled = true;
            }
        }
        let mut i = 0;
        while i < state.entries.len() {
            if state.entries[i].owner.ptr_eq(&owner) {
                let entry = state.entries.remove(i);
                state.retired.push(Arc::downgrade(&entry.index));
            } else {
                i += 1;
            }
        }
        state.prune();
        Ok(())
    }

    pub fn clear(&self) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        state.generation = state.generation.wrapping_add(1);
        for pending in &mut state.pending {
            pending.cancelled = true;
        }
        let entries = std::mem::take(&mut state.entries);
        state
            .retired
            .extend(entries.into_iter().map(|e| Arc::downgrade(&e.index)));
        state.prune();
        Ok(())
    }

    pub fn stats(&self) -> Result<(usize, usize, usize), String> {
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        state.prune();
        Ok((state.entries.len(), state.disk_bytes(), state.pending.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;

    #[test]
    fn concurrent_requests_do_not_reserve_two_builds_for_one_wave() {
        let cache = TokenCache::default();
        let capture = Arc::new(Capture::empty("shared".into(), "test", "cycles"));
        let cancelled = AtomicBool::new(false);
        let duplicate_built = AtomicBool::new(false);
        let barrier = Barrier::new(2);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                cache
                    .get_or_build_using(&capture, 0, &cancelled, || {
                        barrier.wait();
                        barrier.wait();
                        Ok(None)
                    })
                    .unwrap();
            });
            barrier.wait();
            let duplicate = cache.get_or_build_using(&capture, 0, &cancelled, || {
                duplicate_built.store(true, Ordering::Relaxed);
                Ok(None)
            });
            barrier.wait();
            assert!(duplicate.unwrap().is_none());
        });
        assert!(!duplicate_built.load(Ordering::Relaxed));
        assert_eq!(cache.stats().unwrap(), (0, 0, 0));
    }

    #[test]
    fn pending_builds_reserve_capacity_without_holding_the_cache_lock() {
        let cache = TokenCache::default();
        let capture = Arc::new(Capture::empty("pending".into(), "test", "cycles"));
        let cancelled = AtomicBool::new(false);
        let barrier = Barrier::new(5);
        std::thread::scope(|scope| {
            for ordinal in 0..4 {
                let cache = &cache;
                let capture = &capture;
                let cancelled = &cancelled;
                let barrier = &barrier;
                scope.spawn(move || {
                    let result = cache.get_or_build_using(capture, ordinal, cancelled, || {
                        barrier.wait();
                        barrier.wait();
                        Err("deliberate build failure".into())
                    });
                    assert!(result.err().unwrap().contains("cancelled"));
                });
            }
            barrier.wait();
            assert_eq!(cache.stats().unwrap(), (0, 4 * BUILD_BYTES, 4));
            assert!(cache
                .get_or_build_using(&capture, 5, &cancelled, || panic!(
                    "capacity must fall back before building"
                ))
                .unwrap()
                .is_none());
            cache.clear().unwrap();
            assert_eq!(cache.stats().unwrap(), (0, 4 * BUILD_BYTES, 4));
            barrier.wait();
        });
        assert_eq!(cache.stats().unwrap(), (0, 0, 0));
    }

    #[test]
    fn closing_during_a_build_prevents_late_publication_and_removes_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("cache");
        std::fs::create_dir(&cache_dir).unwrap();
        std::fs::write(
            dir.path().join("wave.json"),
            r#"{"wave":{"begin":0,"end":1,"instructions":[[0,6,0,1,0]]}}"#,
        )
        .unwrap();
        let mut capture = Capture::empty("closing".into(), "test", "cycles");
        capture.root = dir.path().canonicalize().unwrap();
        capture.waves.push(crate::WaveIndex {
            id: "wave.json".into(),
            se: 0,
            simd: 0,
            slot: 0,
            wid: 0,
            start: 0.,
            end: 1.,
        });
        let capture = Arc::new(capture);
        let cache = TokenCache::default();
        let cancelled = AtomicBool::new(false);
        let barrier = Barrier::new(2);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let result = cache.get_or_build_using(&capture, 0, &cancelled, || {
                    let index = TokenIndex::build(&capture, 0, &cache_dir, &cancelled)?;
                    barrier.wait();
                    barrier.wait();
                    Ok(Some(index))
                });
                assert!(result.err().unwrap().contains("cancelled"));
            });
            barrier.wait();
            cache.remove_capture(&capture).unwrap();
            assert_eq!(cache.stats().unwrap(), (0, BUILD_BYTES, 1));
            barrier.wait();
        });
        assert_eq!(cache.stats().unwrap(), (0, 0, 0));
        assert_eq!(std::fs::read_dir(cache_dir).unwrap().count(), 0);
    }
}
