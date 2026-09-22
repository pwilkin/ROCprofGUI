use profile_core::{spm_clock::align, RealtimeAnchor, RealtimeCalibration};
fn anchors(se: u32, pairs: &[(i64, u64)]) -> RealtimeCalibration {
    RealtimeCalibration {
        frequency_hz: None,
        anchors: pairs
            .iter()
            .map(|(sc, rt)| RealtimeAnchor {
                se,
                shader_clock: sc.to_string(),
                realtime_clock: rt.to_string(),
            })
            .collect(),
    }
}
#[test]
fn measured_intervals_integrate_both_directions_from_nearest_anchor() {
    let result = align(
        &[100, 120, 150, 180],
        &[None, Some(40.), Some(90.), Some(60.)],
        &anchors(2, &[(1000, 130)]),
        2,
    )
    .unwrap();
    assert_eq!(result.shader_clocks, [930., 970., 1060., 1120.]);
    assert_eq!(result.measured_intervals, 3);
    assert_eq!(result.fallback_intervals, 0);
    assert!(result.ranges_overlap);
    assert!(result.extrapolated);
}
#[test]
fn missing_or_zero_cycles_use_selected_se_slope_and_report_it() {
    let mut calibration = anchors(1, &[(500, 100), (800, 200)]);
    calibration
        .anchors
        .extend(anchors(0, &[(0, 100), (10000, 200)]).anchors);
    let result = align(
        &[100, 120, 150],
        &[Some(999.), None, Some(0.)],
        &calibration,
        1,
    )
    .unwrap();
    assert_eq!(result.shader_clocks, [500., 560., 650.]);
    assert_eq!(result.fallback_intervals, 2);
    assert!(!result.extrapolated);
    let outside = align(&[300], &[None], &calibration, 1).unwrap();
    assert_eq!(outside.shader_clocks, [1100.]);
    assert!(!outside.ranges_overlap);
    assert!(outside.extrapolated);
}
#[test]
fn realtime_subtraction_remains_exact_near_u64_maximum() {
    let base = u64::MAX - 100;
    let result = align(
        &[base + 1, base + 2, base + 3],
        &[None, None, None],
        &anchors(0, &[(-20, base), (0, base + 10)]),
        0,
    )
    .unwrap();
    assert_eq!(result.shader_clocks, [-18., -16., -14.]);
}
#[test]
fn rejects_ambiguous_missing_reset_and_imprecise_clocks() {
    let good = anchors(0, &[(100, 10), (200, 20)]);
    assert!(align(&[10, 10], &[None, None], &good, 0).is_err());
    assert!(align(&[20, 10], &[None, None], &good, 0).is_err());
    assert!(align(&[10], &[None], &good, 1).is_err());
    assert!(align(&[10, 20], &[None, Some(f64::NAN)], &good, 0).is_err());
    assert!(align(&[10, 20], &[None, Some(-1.)], &good, 0).is_err());
    for pairs in [
        vec![(100, 10), (101, 10)],
        vec![(200, 10), (100, 20)],
        vec![(100, 10), (100, 20)],
    ] {
        assert!(align(&[10, 20], &[None, None], &anchors(0, &pairs), 0).is_err());
    }
    let one = anchors(0, &[(100, 10)]);
    assert!(align(&[10, 20], &[None, None], &one, 0).is_err());
    assert!(align(&[11], &[None], &one, 0).is_err());
    assert_eq!(
        align(&[10], &[None], &one, 0).unwrap().shader_clocks,
        [100.]
    );
    assert!(align(&[10], &[None], &anchors(0, &[(9007199254740992, 10)]), 0).is_err());
    assert!(align(&[0, u64::MAX], &[None, Some(1.)], &one, 0).is_err());
}
#[test]
fn affine_clock_family_matches_independent_closed_form() {
    for case in 1..=500u64 {
        let origin = u64::MAX - 10000;
        let first = origin + 300;
        let slope = (case % 7 + 1) as f64;
        let sc = (case * 13) as i64;
        let calibration = anchors(3, &[(sc, first), (sc + (slope * 500.) as i64, first + 500)]);
        let times: Vec<_> = (0..12).map(|i| origin + case % 29 + i * 79).collect();
        let cycles: Vec<_> = (0..times.len())
            .map(|i| if i % 2 == 0 { None } else { Some(79. * slope) })
            .collect();
        let result = align(&times, &cycles, &calibration, 3).unwrap();
        for (actual, rt) in result.shader_clocks.iter().zip(times) {
            let expected = sc as f64 + (rt as i128 - first as i128) as f64 * slope;
            assert!((actual - expected).abs() < 1e-9);
        }
    }
}
