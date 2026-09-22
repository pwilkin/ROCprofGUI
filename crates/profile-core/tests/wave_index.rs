use profile_core::{wave_index::WaveWindowCache, Capture, WaveIndex};
use std::sync::Arc;
fn wave(id: usize, se: u32, start: f64, end: f64) -> WaveIndex {
    WaveIndex {
        id: format!("{id}.json"),
        se,
        simd: 0,
        slot: 0,
        wid: id as u32,
        start,
        end,
    }
}
#[test]
fn indexed_queries_match_linear_overlap_in_original_order() {
    let mut c = Capture::empty("test".into(), "test", "cycles");
    let mut seed = 71u64;
    for i in 0..4000 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let start = (seed % 100000) as f64;
        c.waves
            .push(wave(i, (i % 4) as u32, start, start + (seed % 1000) as f64));
    }
    let c = Arc::new(c);
    let mut cache = WaveWindowCache::default();
    for i in 0..500 {
        let start = (i * 173 % 100000) as f64;
        let se = (i % 5) as u32;
        let expected: Vec<_> = c
            .waves
            .iter()
            .enumerate()
            .filter(|(_, w)| w.se == se && w.start < start + 300. && w.end > start)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(cache.select(&c, se, start, start + 300.).unwrap(), expected);
    }
    assert_eq!(cache.stats().2, 1);
    assert!(cache.select(&c, 0, f64::NAN, 1.).is_err());
    assert!(cache.select(&c, 0, 1., 1.).is_err());
}
#[test]
fn million_wave_query_prunes_expired_intervals_even_with_a_long_running_wave() {
    let mut c = Capture::empty("large".into(), "test", "cycles");
    c.waves.push(wave(0, 0, 0., 2_000_000.));
    for i in 1..1_000_000 {
        c.waves.push(wave(i, 0, i as f64, i as f64 + 0.5));
    }
    let c = Arc::new(c);
    let mut cache = WaveWindowCache::default();
    assert_eq!(
        cache.select(&c, 0, 999_998.1, 999_998.4).unwrap(),
        [0, 999_998]
    );
    assert!(cache.stats().3 < 100, "visited {} nodes", cache.stats().3);
    assert!(cache.stats().1 < 40 * 1024 * 1024);
    assert_eq!(
        cache.select(&c, 0, 500_000.1, 500_000.4).unwrap(),
        [0, 500_000]
    );
    assert_eq!(cache.stats().2, 1);
    assert!(cache.stats().3 < 100);
}
#[test]
fn boundaries_limits_and_capture_identity_are_preserved() {
    let mut c = Capture::empty("same-id".into(), "test", "cycles");
    c.waves = vec![
        wave(0, 0, 0., 10.),
        wave(1, 0, 10., 20.),
        wave(2, 0, 12., 12.),
    ];
    let c = Arc::new(c);
    let mut cache = WaveWindowCache::default();
    assert_eq!(cache.select(&c, 0, 10., 12.).unwrap(), [1]);
    assert_eq!(cache.select(&c, 0, 11., 13.).unwrap(), [1, 2]);
    let mut replacement = (*c).clone();
    replacement.waves = vec![wave(3, 0, 100., 200.)];
    let replacement = Arc::new(replacement);
    assert_eq!(
        cache.select(&replacement, 0, 10., 12.).unwrap(),
        Vec::<usize>::new()
    );
    assert_eq!(cache.stats().2, 2);
    drop(c);
    cache.select(&replacement, 0, 110., 120.).unwrap();
    assert_eq!(cache.stats().0, 1);
    let mut dense = Capture::empty("dense".into(), "test", "cycles");
    dense.waves = (0..129).map(|i| wave(i, 0, 0., 10.)).collect();
    assert!(cache
        .select(&Arc::new(dense), 0, 1., 2.)
        .unwrap_err()
        .contains("128"));
}
#[test]
fn cache_evicts_old_indexes_and_indexed_loading_matches_existing_loader() {
    let mut cache = WaveWindowCache::default();
    let captures: Vec<_> = (0..10)
        .map(|i| Arc::new(Capture::empty(i.to_string(), "test", "cycles")))
        .collect();
    for c in &captures {
        cache.select(c, 0, 0., 1.).unwrap();
    }
    assert_eq!(cache.stats().0, 8);
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/transpose-ui/filenames.json");
    let c = Arc::new(profile_core::load_capture(&fixture).unwrap());
    let expected = profile_core::load_wave_window(&c, 0, c.start, c.end).unwrap();
    let indices = cache.select(&c, 0, c.start, c.end).unwrap();
    let actual = profile_core::wave_viewport::load(&c, &indices, c.start, c.end).unwrap();
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert!(profile_core::load_wave_indices(&c, &[usize::MAX]).is_err());
}

#[test]
fn mutating_an_exclusively_owned_capture_invalidates_its_weak_cache_identity() {
    let mut capture = Arc::new(Capture::empty("mutable".into(), "test", "cycles"));
    Arc::make_mut(&mut capture).waves.push(wave(0, 0, 0., 10.));
    let mut cache = WaveWindowCache::default();
    assert_eq!(cache.select(&capture, 0, 1., 2.).unwrap(), [0]);
    Arc::make_mut(&mut capture).waves.clear();
    assert!(cache.select(&capture, 0, 1., 2.).unwrap().is_empty());
    assert_eq!(cache.stats().2, 2);
}
