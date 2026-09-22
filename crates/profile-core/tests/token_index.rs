use profile_core::{token_index::TokenIndex, wave_viewport::project, Capture, WaveIndex};
use std::sync::atomic::AtomicBool;

fn fixture(rows: usize) -> (tempfile::TempDir, Capture) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("cache")).unwrap();
    let mut capture = Capture::empty("index".into(), "test", "cycles");
    capture.root = dir.path().canonicalize().unwrap();
    capture.waves.push(WaveIndex {
        id: "wave.json".into(),
        se: 0,
        simd: 0,
        slot: 0,
        wid: 0,
        start: 0.,
        end: (rows * 2) as f64,
    });
    let tokens: Vec<_> = (0..rows)
        .map(|i| serde_json::json!([i * 2, 6, 0, 1, i % 17]))
        .collect();
    std::fs::write(dir.path().join("wave.json"), serde_json::to_vec(&serde_json::json!({"wave": {"begin": 0, "end": rows * 2, "instructions": tokens, "timeline": [[2, rows * 2]], "info": {"original": true}, "waitcnt": [[3, [[1, 2]]]]}})).unwrap()).unwrap();
    (dir, capture)
}

#[test]
fn indexed_query_matches_full_projection_and_reads_only_overlapping_blocks() {
    let (dir, capture) = fixture(20000);
    let cancelled = AtomicBool::new(false);
    let index = TokenIndex::build(&capture, 0, &dir.path().join("cache"), &cancelled).unwrap();
    let full = profile_core::load_wave_indices(&capture, &[0])
        .unwrap()
        .remove(0);
    let (actual, bytes) = index.query(39000., 39021., 500000, &cancelled).unwrap();
    let expected = project(full, 39000., 39021.).unwrap();
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert_eq!(bytes, 544 * 56);
    assert_eq!(
        index.query(37000., 37021., 500000, &cancelled).unwrap().1,
        1024 * 56
    );
    assert_eq!(index.disk_bytes(), 20000 * 56);
    assert_eq!(
        index.query(50000., 50001., 500000, &cancelled).unwrap().1,
        0
    );
    assert!(index
        .query(0., 40000., 100, &cancelled)
        .unwrap_err()
        .contains("500,000"));
    drop(index);
    assert_eq!(
        std::fs::read_dir(dir.path().join("cache")).unwrap().count(),
        0
    );
}

#[test]
fn unsorted_tokens_offsets_and_concurrent_reads_preserve_identity() {
    let (dir, mut capture) = fixture(4000);
    let path = dir.path().join("wave.json");
    let mut data: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let tokens = data["wave"]["instructions"].as_array_mut().unwrap();
    for (i, row) in tokens.iter_mut().enumerate() {
        row[0] = serde_json::json!((i * 73 % 3999) / 4);
        row[2] = serde_json::json!(i % 23);
        row[3] = serde_json::json!(i % 31);
        row[4] = serde_json::json!(i as i64 % 17 - 2);
    }
    std::fs::write(path, serde_json::to_vec(&data).unwrap()).unwrap();
    capture.se_time_offsets.insert(0, 125);
    let cancelled = AtomicBool::new(false);
    let index = TokenIndex::build(&capture, 0, &dir.path().join("cache"), &cancelled).unwrap();
    let full = profile_core::load_wave_indices(&capture, &[0])
        .unwrap()
        .remove(0);
    std::thread::scope(|scope| {
        for start in [0., 125., 333., 500., 999., 1100.] {
            let index = &index;
            let full = &full;
            let cancelled = &cancelled;
            scope.spawn(move || {
                let (actual, _) = index.query(start, start + 25., 500000, cancelled).unwrap();
                assert_eq!(
                    serde_json::to_value(actual).unwrap(),
                    serde_json::to_value(project(full.clone(), start, start + 25.).unwrap())
                        .unwrap()
                );
            });
        }
    });
}

#[test]
fn cancellation_invalid_sources_and_changes_never_return_stale_results() {
    let (dir, capture) = fixture(100);
    let cache = dir.path().join("cache");
    assert!(TokenIndex::build(&capture, 0, &cache, &AtomicBool::new(true)).is_err());
    assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0);
    let index = TokenIndex::build(&capture, 0, &cache, &AtomicBool::new(false)).unwrap();
    assert!(index
        .query(0., 10., 500000, &AtomicBool::new(true))
        .unwrap_err()
        .contains("cancelled"));
    std::fs::write(dir.path().join("wave.json"), "{}").unwrap();
    assert!(index
        .query(0., 10., 500000, &AtomicBool::new(false))
        .unwrap_err()
        .contains("changed"));
    drop(index);
    assert!(TokenIndex::build(&capture, 0, &cache, &AtomicBool::new(false)).is_err());
    assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn retargeted_symlink_invalidates_an_unchanged_original_file() {
    let (dir, mut capture) = fixture(100);
    std::fs::copy(dir.path().join("wave.json"), dir.path().join("other.json")).unwrap();
    let link = dir.path().join("link.json");
    std::os::unix::fs::symlink("wave.json", &link).unwrap();
    capture.waves[0].id = "link.json".into();
    let index = TokenIndex::build(
        &capture,
        0,
        &dir.path().join("cache"),
        &AtomicBool::new(false),
    )
    .unwrap();
    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink("other.json", &link).unwrap();
    assert!(index
        .query(0., 10., 500000, &AtomicBool::new(false))
        .unwrap_err()
        .contains("changed"));
}

#[test]
fn cache_reuses_indexes_and_retains_disk_accounting_until_active_handles_drop() {
    use profile_core::token_cache::TokenCache;
    use std::sync::Arc;
    let (dir, capture) = fixture(100);
    let capture = Arc::new(capture);
    let cache = TokenCache::default();
    let cancelled = AtomicBool::new(false);
    let index = cache
        .get_or_build(&capture, 0, &dir.path().join("cache"), &cancelled)
        .unwrap()
        .unwrap();
    let again = cache
        .get_or_build(&capture, 0, &dir.path().join("cache"), &cancelled)
        .unwrap()
        .unwrap();
    assert!(Arc::ptr_eq(&index, &again));
    drop(again);
    cache.remove_capture(&capture).unwrap();
    assert_eq!(cache.stats().unwrap(), (0, 5600, 0));
    assert!(cache
        .get_or_build(&capture, 0, &dir.path().join("cache"), &cancelled)
        .is_err());
    drop(index);
    assert_eq!(
        std::fs::read_dir(dir.path().join("cache")).unwrap().count(),
        0
    );
    assert_eq!(cache.stats().unwrap(), (0, 0, 0));
    assert_eq!(
        std::fs::read_dir(dir.path().join("cache")).unwrap().count(),
        0
    );
    let replacement = Arc::new((*capture).clone());
    assert!(cache
        .get_or_build(&replacement, 0, &dir.path().join("cache"), &cancelled)
        .unwrap()
        .is_some());
}

#[test]
fn cache_capacity_preserves_active_queries_and_evicts_unleased_entries() {
    use profile_core::token_cache::TokenCache;
    use std::sync::Arc;
    let (dir, capture) = fixture(100);
    let captures: Vec<_> = (0..9).map(|_| Arc::new(capture.clone())).collect();
    let cache = TokenCache::default();
    let cancelled = AtomicBool::new(false);
    let mut handles = Vec::new();
    for capture in &captures[..8] {
        handles.push(
            cache
                .get_or_build(capture, 0, &dir.path().join("cache"), &cancelled)
                .unwrap()
                .unwrap(),
        );
    }
    assert_eq!(cache.stats().unwrap(), (8, 8 * 5600, 0));
    assert!(cache
        .get_or_build(&captures[8], 0, &dir.path().join("cache"), &cancelled)
        .unwrap()
        .is_none());
    let fallback = cache
        .load_window(
            &captures[8],
            &[0],
            0.,
            21.,
            &dir.path().join("cache"),
            &cancelled,
        )
        .unwrap();
    let expected = profile_core::wave_viewport::load(&captures[8], &[0], 0., 21.).unwrap();
    assert_eq!(
        serde_json::to_value(fallback).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert_eq!(cache.stats().unwrap().0, 8);
    handles.clear();
    assert!(cache
        .get_or_build(&captures[8], 0, &dir.path().join("cache"), &cancelled)
        .unwrap()
        .is_some());
    assert_eq!(cache.stats().unwrap(), (8, 8 * 5600, 0));
    cache.clear().unwrap();
    assert_eq!(cache.stats().unwrap(), (0, 0, 0));
}

#[test]
fn cached_window_enforces_the_total_budget_across_waves() {
    use profile_core::token_cache::TokenCache;
    use std::sync::Arc;
    let (dir, capture) = fixture(200000);
    let capture = Arc::new(capture);
    let cache = TokenCache::default();
    let cancelled = AtomicBool::new(false);
    let directory = dir.path().join("cache");
    let error = cache
        .load_window(&capture, &[0, 0, 0], 0., 400000., &directory, &cancelled)
        .unwrap_err();
    assert!(error.contains("500,000"));
    let narrow = cache
        .load_window(&capture, &[0, 0, 0], 0., 21., &directory, &cancelled)
        .unwrap();
    assert_eq!(narrow.iter().map(|w| w.tokens.len()).sum::<usize>(), 33);
    assert_eq!(cache.stats().unwrap().0, 1);
    assert!(cache
        .load_window(&capture, &[], 0., 1., &directory, &AtomicBool::new(true))
        .is_err());
}

#[test]
fn unavailable_index_storage_falls_back_without_masking_source_errors_or_cancellation() {
    use profile_core::token_cache::TokenCache;
    use std::sync::Arc;
    let (dir, capture) = fixture(100);
    let capture = Arc::new(capture);
    let cache = TokenCache::default();
    let cancelled = AtomicBool::new(false);
    let blocked = dir.path().join("blocked");
    std::fs::write(&blocked, "not a directory").unwrap();
    let actual = cache
        .load_window(&capture, &[0], 0., 21., &blocked, &cancelled)
        .unwrap();
    let expected = profile_core::wave_viewport::load(&capture, &[0], 0., 21.).unwrap();
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert_eq!(cache.stats().unwrap(), (0, 0, 0));
    assert!(cache
        .load_window(&capture, &[0], 0., 21., &blocked, &AtomicBool::new(true))
        .unwrap_err()
        .contains("cancelled"));
    std::fs::write(dir.path().join("wave.json"), "{}").unwrap();
    assert!(cache
        .load_window(&capture, &[0], 0., 21., &blocked, &cancelled)
        .is_err());
    assert_eq!(cache.stats().unwrap(), (0, 0, 0));
}

#[test]
fn cache_rebuilds_changed_sources_and_releases_failed_reservations() {
    use profile_core::token_cache::TokenCache;
    use std::sync::Arc;
    let (dir, capture) = fixture(100);
    let capture = Arc::new(capture);
    let cache = TokenCache::default();
    let cancelled = AtomicBool::new(false);
    let first = cache
        .get_or_build(&capture, 0, &dir.path().join("cache"), &cancelled)
        .unwrap()
        .unwrap();
    std::fs::write(
        dir.path().join("wave.json"),
        r#"{"wave":{"begin":0,"end":1,"instructions":[[0,6,0,1,0]]}}"#,
    )
    .unwrap();
    let second = cache
        .get_or_build(&capture, 0, &dir.path().join("cache"), &cancelled)
        .unwrap()
        .unwrap();
    assert!(!Arc::ptr_eq(&first, &second));
    assert_eq!(second.disk_bytes(), 56);
    assert_eq!(cache.stats().unwrap(), (1, 5656, 0));
    drop(first);
    drop(second);
    std::fs::write(dir.path().join("wave.json"), "{}").unwrap();
    assert!(cache
        .get_or_build(&capture, 0, &dir.path().join("cache"), &cancelled)
        .is_err());
    assert_eq!(cache.stats().unwrap(), (0, 0, 0));
}
