use profile_core::{wave_overview::aggregate, Token, Wave};
#[test]
fn overview_counts_every_overlap_once_and_preserves_categories_and_spans() {
    let wave = Wave {
        id: "w".into(),
        cu: Some(1),
        start: 0.,
        end: 100.,
        states: vec![],
        waits: serde_json::Value::Null,
        info: serde_json::json!({}),
        tokens: vec![
            (0., 30., 6),
            (12., 1., 6),
            (19., 40., 6),
            (20., 0., 5),
            (30., 0., 999),
            (31., 0., 6),
        ]
        .into_iter()
        .map(|(start, duration, category)| Token {
            start,
            duration,
            category,
            stall: 0.,
            instruction: 1,
        })
        .collect(),
    };
    let groups = aggregate(&wave, [10., 30.], 2).unwrap();
    assert_eq!(groups.iter().map(|g| g.count).sum::<usize>(), 5);
    assert_eq!(groups.len(), 3);
    assert_eq!(
        (groups[0].start, groups[0].end, groups[0].count),
        (0., 59., 3)
    );
    assert_eq!(groups[0].zoom_range, [10., 20.]);
    assert!(groups.iter().any(|g| g.category == 999 && g.count == 1));
    for count in [0, 513] {
        assert!(aggregate(&wave, [10., 30.], count).is_err());
    }
    assert!(aggregate(&wave, [0., f64::INFINITY], 2).is_err());
}
#[test]
#[ignore = "requires ROCPROF_WAVE_ARCHIVE"]
fn archived_full_range_overview_represents_all_tokens_with_bounded_groups() {
    let path = std::env::var_os("ROCPROF_WAVE_ARCHIVE").unwrap();
    let capture = profile_core::load_capture(std::path::Path::new(&path)).unwrap();
    let indices: Vec<_> = capture
        .waves
        .iter()
        .enumerate()
        .filter(|(_, w)| w.se == 0)
        .map(|(i, _)| i)
        .collect();
    let overview =
        profile_core::wave_overview::load(&capture, &indices, [capture.start, capture.end])
            .unwrap();
    let groups: usize = overview.waves.iter().map(|w| w.groups.len()).sum();
    assert_eq!(overview.represented_tokens, 723297);
    assert!(groups < profile_core::wave_overview::MAX_GROUPS);
    assert_eq!(
        overview
            .waves
            .iter()
            .flat_map(|w| &w.groups)
            .map(|g| g.count)
            .sum::<usize>(),
        723297
    );
    eprintln!("{} tokens in {groups} groups", overview.represented_tokens);
}

#[test]
#[ignore = "requires ROCPROF_WAVE_ARCHIVE"]
fn archived_utilization_overview_preserves_projected_event_counts() {
    let path = std::env::var_os("ROCPROF_WAVE_ARCHIVE").unwrap();
    let capture = profile_core::load_capture(std::path::Path::new(&path)).unwrap();
    let indices: Vec<_> = capture
        .waves
        .iter()
        .enumerate()
        .filter(|(_, w)| w.se == 0)
        .map(|(i, _)| i)
        .collect();
    let result = profile_core::wave_overview::load_mode(
        &capture,
        &indices,
        [capture.start, capture.end],
        true,
        false,
    )
    .unwrap();
    assert!(result.utilization);
    assert_eq!(result.represented_tokens, 723297);
    assert!(result
        .waves
        .iter()
        .flat_map(|w| &w.groups)
        .all(|g| g.pipe.is_some()));
    let groups: usize = result.waves.iter().map(|w| w.groups.len()).sum();
    assert!(groups <= profile_core::wave_overview::MAX_GROUPS);
    eprintln!(
        "{} projected events in {} groups",
        result.represented_tokens, groups
    );
}
