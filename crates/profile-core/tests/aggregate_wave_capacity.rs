use profile_core::{wave_index::WaveWindowCache, Capture, WaveIndex};
use std::sync::Arc;

#[test]
fn sequential_aggregates_cover_4096_waves_without_relaxing_exact_transfer() {
    let dir = tempfile::tempdir().unwrap();
    let mut capture = Capture::empty("aggregate".into(), "test", "cycles");
    capture.root = dir.path().to_path_buf();
    for i in 0..4096 {
        let id = format!("wave-{i}.json");
        std::fs::write(
            dir.path().join(&id),
            serde_json::to_vec(&serde_json::json!({
                "wave": {"begin":0,"end":10,"cu":i % 8,
                    "instructions":[[1,6,1,2,7],[9,3,0,3,8]], "timeline":[]}
            }))
            .unwrap(),
        )
        .unwrap();
        capture.waves.push(WaveIndex {
            id,
            se: 0,
            simd: (i % 4) as u32,
            slot: 0,
            wid: i as u32,
            start: 0.,
            end: 10.,
        });
    }
    let capture = Arc::new(capture);
    let mut cache = WaveWindowCache::default();
    assert!(cache
        .select(&capture, 0, 0., 10.)
        .unwrap_err()
        .contains("128"));
    let indices = cache.select_aggregate(&capture, 0, 0., 10.).unwrap();
    assert_eq!(indices, (0..4096).collect::<Vec<_>>());
    assert_eq!(cache.stats().2, 1);
    let overview = profile_core::wave_overview::load(&capture, &indices, [0., 10.]).unwrap();
    assert_eq!(overview.waves.len(), 4096);
    assert_eq!(overview.represented_tokens, 8192);
    assert_eq!(
        overview.waves.iter().map(|w| w.groups.len()).sum::<usize>(),
        8192
    );
    for (i, wave) in overview.waves.iter().enumerate() {
        assert_eq!(wave.wave.id, format!("wave-{i}.json"));
        assert_eq!(wave.cu, Some((i % 8) as u32));
        assert_eq!(wave.groups.iter().map(|g| g.count).sum::<usize>(), 2);
    }
    let stats = profile_core::wave_stats::load(&capture, &indices, [0., 10.]).unwrap();
    assert_eq!(stats.len(), 2);
    assert_eq!(
        (
            stats[0].instruction,
            stats[0].hits,
            stats[0].latency,
            stats[0].stall
        ),
        (7, 4096, 8192., 4096.)
    );
    assert_eq!(
        (
            stats[1].instruction,
            stats[1].hits,
            stats[1].latency,
            stats[1].stall
        ),
        (8, 4096, 12288., 0.)
    );
    let mut too_many = indices.clone();
    too_many.push(0);
    assert!(
        profile_core::wave_stats::load(&capture, &too_many, [0., 10.])
            .unwrap_err()
            .contains("4096")
    );
    assert!(
        profile_core::wave_overview::load(&capture, &too_many, [0., 10.])
            .unwrap_err()
            .contains("4096")
    );
    let mut over = (*capture).clone();
    over.waves.push(over.waves[0].clone());
    assert!(cache
        .select_aggregate(&Arc::new(over), 0, 0., 10.)
        .unwrap_err()
        .contains("4096"));
}

#[test]
fn cancelled_aggregates_do_not_open_wave_files() {
    use std::sync::atomic::AtomicBool;
    let mut capture = Capture::empty("cancelled".into(), "test", "cycles");
    capture.waves.push(WaveIndex {
        id: "missing.json".into(),
        se: 0,
        simd: 0,
        slot: 0,
        wid: 0,
        start: 0.,
        end: 10.,
    });
    let cancelled = AtomicBool::new(true);
    assert_eq!(
        profile_core::wave_stats::load_cancellable(&capture, &[0], [0., 10.], &cancelled)
            .unwrap_err(),
        "Wave query cancelled"
    );
    for utilization in [false, true] {
        assert_eq!(
            profile_core::wave_overview::load_mode_cancellable(
                &capture,
                &[0],
                [0., 10.],
                utilization,
                false,
                &cancelled
            )
            .unwrap_err(),
            "Wave query cancelled"
        );
    }
}
