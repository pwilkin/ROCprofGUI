use profile_core::{wave_viewport::project, Token, Wave};

#[test]
fn streamed_viewport_matches_full_projection_with_unsorted_tokens_and_se_offsets() {
    let dir = tempfile::tempdir().unwrap();
    let mut capture = profile_core::Capture::empty("stream".into(), "test", "cycles");
    capture.root = dir.path().canonicalize().unwrap();
    capture.waves.push(profile_core::WaveIndex {
        id: "wave.json".into(),
        se: 2,
        simd: 0,
        slot: 0,
        wid: 0,
        start: 100.,
        end: 2000.,
    });
    let mut seed = 123u64;
    let rows: Vec<_> = (0..2000)
        .map(|i| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            serde_json::json!([
                100 + seed % 500,
                6,
                seed % 13,
                seed % 37,
                i % 17 - 2,
                "extension"
            ])
        })
        .collect();
    let data = serde_json::json!({"ignored": {"instructions": [false]}, "wave": {
        "info": {"kept": true}, "begin": 100, "end": 2000, "cu": 3,
        "instructions": rows, "timeline": [[2, 100], [3, 200], [4, 1600]],
        "waitcnt": [[3, [[1, 2]]]]
    }});
    std::fs::write(
        dir.path().join("wave.json"),
        serde_json::to_vec(&data).unwrap(),
    )
    .unwrap();
    for offset in [-50, 0, 125] {
        capture.se_time_offsets.insert(2, offset);
        let full = profile_core::load_wave_indices(&capture, &[0])
            .unwrap()
            .remove(0);
        for start in [0., 50., 100., 155., 333., 700., 2100.] {
            let expected = project(full.clone(), start, start + 25.).unwrap();
            let actual = profile_core::wave_viewport::load(&capture, &[0], start, start + 25.)
                .unwrap()
                .remove(0);
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                serde_json::to_value(expected).unwrap()
            );
        }
    }
    let mut malformed = data;
    malformed["wave"]["instructions"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!([99999, 6, -1, -1, 2]));
    std::fs::write(
        dir.path().join("wave.json"),
        serde_json::to_vec(&malformed).unwrap(),
    )
    .unwrap();
    assert!(
        profile_core::wave_viewport::load(&capture, &[0], 100., 101.)
            .unwrap_err()
            .contains("Negative")
    );
}
fn wave(tokens: Vec<Token>) -> Wave {
    Wave {
        id: "w".into(),
        start: 0.,
        end: 100.,
        cu: Some(2),
        tokens,
        states: vec![[0., 10., 2.], [10., 20., 3.], [30., 70., 4.]],
        waits: serde_json::json!([[3, [[1, 2]]]]),
        info: serde_json::json!({}),
    }
}
fn token(start: f64, duration: f64, instruction: i64) -> Token {
    Token {
        start,
        duration,
        stall: 0.,
        instruction,
        category: 6,
    }
}
#[test]
fn viewport_retains_original_indices_occurrences_slots_and_full_token_extents() {
    let mut tokens = vec![token(0., 1., 7), token(5., 1., 7)];
    tokens.extend([
        token(10., 1., 7),
        token(10., 20., 7),
        token(10., 30., 8),
        token(10., 40., 7),
        token(10., 50., 7),
    ]);
    tokens.push(token(50., 0., 7));
    let projected = project(wave(tokens), 25., 35.).unwrap();
    assert_eq!(
        projected
            .token_identities
            .iter()
            .map(|i| (i.index, i.occurrence, i.display_slot))
            .collect::<Vec<_>>(),
        [(3, 3, 1), (4, 0, 2), (5, 4, 3), (6, 5, 3)]
    );
    assert_eq!(projected.tokens[0].start, 10.);
    assert_eq!(projected.tokens[0].duration, 20.);
    assert_eq!(projected.states, [[10., 20., 3.], [30., 70., 4.]]);
    assert!(projected.waits.is_null());
    assert_eq!(projected.viewport, [25., 35.]);
    let json = serde_json::to_value(projected).unwrap();
    assert_eq!(json["tokenIdentities"][0]["index"], 3);
    assert_eq!(json["id"], "w");
}
#[test]
fn viewport_keeps_boundary_points_and_stall_overlap_and_rejects_invalid_ranges() {
    let mut stalled = token(0., 1., 1);
    stalled.stall = 20.;
    let w = wave(vec![
        stalled,
        token(10., 0., 2),
        token(20., 0., 3),
        token(21., 0., 4),
    ]);
    assert_eq!(project(w.clone(), 10., 20.).unwrap().tokens.len(), 3);
    for (a, b) in [(0., 0.), (2., 1.), (f64::NAN, 4.), (0., f64::INFINITY)] {
        assert!(project(w.clone(), a, b).is_err());
    }
}

#[test]
#[ignore = "requires ROCPROF_WAVE_ARCHIVE"]
fn archived_wave_query_budget_counts_visible_tokens_and_preserves_every_identity() {
    let path = std::env::var_os("ROCPROF_WAVE_ARCHIVE").expect("archive path");
    let capture = profile_core::load_capture(std::path::Path::new(&path)).unwrap();
    assert!(
        profile_core::load_wave_window(&capture, 0, capture.start, capture.end)
            .unwrap_err()
            .contains("500,000")
    );
    let end = capture.start + (capture.end - capture.start) / 3.;
    let windows = profile_core::load_wave_window(&capture, 0, capture.start, end).unwrap();
    let mut total = 0;
    for view in windows {
        let full = profile_core::load_wave(&capture, &view.id).unwrap();
        let expected: Vec<_> = full
            .tokens
            .iter()
            .enumerate()
            .filter(|(_, t)| t.start <= end && t.start + t.duration.max(t.stall) >= capture.start)
            .collect();
        assert_eq!(view.tokens.len(), expected.len());
        let mut occurrences = std::collections::BTreeMap::new();
        let mut original_occurrences = Vec::new();
        for token in &full.tokens {
            let count = occurrences.entry(token.instruction).or_insert(0);
            original_occurrences.push(*count);
            *count += 1;
        }
        for ((token, identity), (index, original)) in
            view.tokens.iter().zip(&view.token_identities).zip(expected)
        {
            assert_eq!(identity.index, index);
            assert_eq!(identity.occurrence, original_occurrences[index]);
            assert_eq!(
                serde_json::to_value(token).unwrap(),
                serde_json::to_value(original).unwrap()
            );
        }
        total += view.tokens.len();
    }
    assert!(total > 100_000 && total < 500_000, "{total}");
    eprintln!("Qualified {total} visible tokens with original identities");
}

#[test]
fn cancelled_projection_returns_no_partial_viewport() {
    let cancelled = std::sync::atomic::AtomicBool::new(true);
    assert_eq!(
        profile_core::wave_viewport::project_cancellable(
            wave(vec![token(10., 1., 7)]),
            0.,
            20.,
            &cancelled
        )
        .unwrap_err(),
        "Wave query cancelled"
    );
}
