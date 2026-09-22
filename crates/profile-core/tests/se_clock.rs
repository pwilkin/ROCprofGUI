use profile_core::{se_clock::plan, RealtimeAnchor, RealtimeCalibration};
use std::collections::BTreeSet;
fn calibration(rows: &[(u32, i64, u64)]) -> RealtimeCalibration {
    RealtimeCalibration {
        frequency_hz: None,
        anchors: rows
            .iter()
            .map(|(se, sc, rt)| RealtimeAnchor {
                se: *se,
                shader_clock: sc.to_string(),
                realtime_clock: rt.to_string(),
            })
            .collect(),
    }
}
#[test]
fn measured_origins_use_upstream_seeded_ratio_and_forward_offsets() {
    let c = calibration(&[(0, 25, 1000), (0, 50, 1001), (2, 25, 1002), (2, 50, 1003)]);
    let p = plan(&c, &BTreeSet::from([0, 2])).unwrap();
    assert_eq!(p.anchor_se, 0);
    assert_eq!(p.offsets[&0], 0);
    assert_eq!(p.offsets[&2], 50);
    assert_eq!(p.ratio_shader_cycles, "75");
    assert_eq!(p.ratio_realtime_ticks, "3");
    assert_eq!(p.measured_engines, 2);
    assert!(!p.uses_only_seed);
}
#[test]
fn seed_only_estimates_and_truncation_are_explicit() {
    let c = calibration(&[(0, 25, 1000), (1, 25, 1001)]);
    let p = plan(&c, &BTreeSet::new()).unwrap();
    assert!(p.uses_only_seed);
    assert_eq!(p.offsets[&1], 25);
    let c = calibration(&[(0, 1, 1000), (0, 2, 1001), (1, 1, 1001), (1, 2, 1002)]);
    assert_eq!(plan(&c, &BTreeSet::new()).unwrap().offsets[&1], 9);
}
#[test]
fn timestamp_translation_near_u64_max_preserves_all_offsets() {
    for case in 0..1000u64 {
        let base = u64::MAX - 10000;
        let rows = [
            (0, 25, 100),
            (0, 25025, 1100),
            (1, 25, 100 + case),
            (1, 25025, 1100 + case),
        ];
        let moved: Vec<_> = rows
            .iter()
            .map(|(se, sc, rt)| (*se, *sc, rt + base))
            .collect();
        let p = plan(&calibration(&rows), &BTreeSet::new()).unwrap();
        let q = plan(&calibration(&moved), &BTreeSet::new()).unwrap();
        assert_eq!(p.offsets, q.offsets);
        assert_eq!(q.offsets[&1], (case * 25) as i64);
    }
}
#[test]
fn missing_ambiguous_reset_and_unsafe_alignment_is_rejected() {
    let c = calibration(&[(0, 25, 1000), (1, 25, 1001)]);
    assert!(plan(&c, &BTreeSet::from([2])).is_err());
    for rows in [
        vec![(0, 1, 1)],
        vec![(0, 1, 1), (0, 2, 1), (1, 1, 2)],
        vec![(0, 2, 1), (0, 1, 2), (1, 1, 2)],
        vec![(0, 1, 0), (1, 1, u64::MAX)],
    ] {
        assert!(plan(&calibration(&rows), &BTreeSet::new()).is_err());
    }
}
