#[test]
#[ignore = "requires ROCPROF_WAVE_ARCHIVE"]
fn archived_stats_match_full_token_oracle() {
    let path = std::env::var_os("ROCPROF_WAVE_ARCHIVE").unwrap();
    let capture = profile_core::load_capture(std::path::Path::new(&path)).unwrap();
    let indices: Vec<_> = capture
        .waves
        .iter()
        .enumerate()
        .filter(|(_, w)| w.se == 0)
        .map(|(i, _)| i)
        .collect();
    let rows =
        profile_core::wave_stats::load(&capture, &indices, [capture.start, capture.end]).unwrap();
    let mut expected = std::collections::BTreeMap::<i64, (usize, f64, f64)>::new();
    for index in indices {
        let wave = profile_core::load_wave(&capture, &capture.waves[index].id).unwrap();
        for token in wave.tokens {
            if token.start < capture.start || token.start >= capture.end {
                continue;
            }
            let row = expected.entry(token.instruction).or_default();
            row.0 += 1;
            row.1 += token.duration;
            row.2 += token.stall;
        }
    }
    assert_eq!(rows.len(), expected.len());
    assert!(rows.iter().map(|r| r.hits).sum::<usize>() > 500_000);
    for row in &rows {
        assert_eq!(
            (row.hits, row.latency, row.stall),
            expected[&row.instruction]
        );
    }
    if let Some(path) = std::env::var_os("ROCPROF_STATS_ORACLE") {
        std::fs::write(
            path,
            serde_json::to_vec(&serde_json::json!({
                "range": [capture.start, capture.end], "rows": rows, "waves": capture.waves
            }))
            .unwrap(),
        )
        .unwrap();
    }
    eprintln!(
        "{} exact events in {} instruction rows",
        rows.iter().map(|r| r.hits).sum::<usize>(),
        rows.len()
    );
}
