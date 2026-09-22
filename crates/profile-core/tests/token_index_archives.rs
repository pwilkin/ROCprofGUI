use profile_core::{token_cache::TokenCache, token_directory::TokenDirectory};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{atomic::AtomicBool, Arc, Weak},
};

#[test]
#[ignore = "requires ROCPROF_INDEX_CAPTURES and ROCPROF_INDEX_NATIVE_REPORT for native comparison"]
fn archived_native_window_oracles() {
    let paths: Vec<String> =
        serde_json::from_str(&std::env::var("ROCPROF_INDEX_CAPTURES").unwrap()).unwrap();
    let mut cases = Vec::new();
    for path in paths {
        let capture = profile_core::load_capture(Path::new(&path)).unwrap();
        let wave = &capture.waves[0];
        let span = (wave.end - wave.start).max(1.);
        let start = wave.start + span / 2.;
        let end = start + (span / 100.).max(1.);
        let expected = profile_core::load_wave_window(&capture, wave.se, start, end).unwrap();
        assert!(expected.iter().map(|w| w.tokens.len()).sum::<usize>() > 0);
        let first = profile_core::load_wave_indices(&capture, &[0])
            .unwrap()
            .remove(0);
        cases.push(serde_json::json!({"input":path,"instructions":capture.instructions.len(),"waves":capture.waves.len(),"first_wave":wave.id,"first_tokens":first.tokens.len(),"se":wave.se,"start":start,"end":end,"expected":expected}));
    }
    std::fs::write(
        std::env::var("ROCPROF_INDEX_NATIVE_REPORT").unwrap(),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "requires ROCPROF_INDEX_CAPTURES JSON array of recorded capture paths"]
fn mixed_archived_indexes_match_streaming_through_eviction_and_reuse() {
    let paths: Vec<String> =
        serde_json::from_str(&std::env::var("ROCPROF_INDEX_CAPTURES").unwrap()).unwrap();
    assert!(paths.len() >= 3);
    let captures: Vec<_> = paths
        .iter()
        .map(|path| Arc::new(profile_core::load_capture(Path::new(path)).unwrap()))
        .collect();
    let total_waves: usize = captures.iter().map(|c| c.waves.len()).sum();
    assert!(total_waves > 8);
    let parent = tempfile::tempdir().unwrap();
    let directory = TokenDirectory::create(parent.path()).unwrap();
    let cache = TokenCache::default();
    let cancelled = AtomicBool::new(false);
    let mut previous =
        BTreeMap::<(usize, usize), Weak<profile_core::token_index::TokenIndex>>::new();
    let mut rebuilt = 0;
    let mut rows = Vec::new();
    for cycle in 0..2 {
        for ordinal in 0..captures.iter().map(|c| c.waves.len()).max().unwrap() {
            for (capture_index, capture) in captures.iter().enumerate() {
                let Some(wave) = capture.waves.get(ordinal) else {
                    continue;
                };
                if cycle > 0 && previous[&(capture_index, ordinal)].upgrade().is_none() {
                    rebuilt += 1;
                }
                let index = cache
                    .get_or_build(capture, ordinal, directory.path(), &cancelled)
                    .unwrap()
                    .expect("recorded wave must fit cache admission");
                let warm = cache
                    .get_or_build(capture, ordinal, directory.path(), &cancelled)
                    .unwrap()
                    .unwrap();
                assert!(Arc::ptr_eq(&index, &warm));
                previous.insert((capture_index, ordinal), Arc::downgrade(&index));
                let span = (wave.end - wave.start).max(1.);
                for (start, end) in [
                    (wave.start, wave.start + span),
                    (wave.start, wave.start + span / 7.),
                    (wave.start + span / 2., wave.start + span / 2. + 1.),
                    (wave.end, wave.end + 1.),
                    (wave.end + 1., wave.end + 2.),
                ] {
                    let expected =
                        profile_core::wave_viewport::load(capture, &[ordinal], start, end);
                    let actual = cache.load_window(
                        capture,
                        &[ordinal],
                        start,
                        end,
                        directory.path(),
                        &cancelled,
                    );
                    let result = match (expected, actual) {
                        (Ok(expected), Ok(actual)) => {
                            let tokens: usize = actual.iter().map(|v| v.tokens.len()).sum();
                            let expected = serde_json::to_vec(&expected).unwrap();
                            let actual = serde_json::to_vec(&actual).unwrap();
                            assert!(
                                actual == expected,
                                "capture {capture_index} wave {ordinal} range {start}..{end}"
                            );
                            serde_json::json!({"tokens": tokens, "sha256": format!("{:x}", Sha256::digest(&actual))})
                        }
                        (Err(expected), Err(actual)) => {
                            assert_eq!(actual, expected);
                            assert!(actual.contains("500,000"), "{actual}");
                            serde_json::json!({"budget_error": actual})
                        }
                        (expected, actual) => panic!(
                            "Index/stream result mismatch: expected {:?}, actual {:?}",
                            expected.as_ref().map(|waves| waves.len()),
                            actual.as_ref().map(|waves| waves.len())
                        ),
                    };
                    rows.push(serde_json::json!({"cycle":cycle,"capture":capture_index,"wave":ordinal,"range":[start,end],"result":result}));
                }
                let (entries, disk, pending) = cache.stats().unwrap();
                assert!(entries <= 8 && disk <= 512 * 1024 * 1024);
                assert_eq!(pending, 0);
            }
        }
    }
    assert!(
        rebuilt > 0,
        "The mixed working set must exercise eviction and rebuild"
    );
    cache.clear().unwrap();
    assert_eq!(cache.stats().unwrap(), (0, 0, 0));
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    let report = serde_json::json!({"paths":paths,"waves":total_waves,"queries":rows.len(),"evicted_rebuilds":rebuilt,"results":rows});
    if let Ok(path) = std::env::var("ROCPROF_INDEX_REPORT") {
        std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
    eprintln!("Compared {} indexed/streaming queries over {total_waves} recorded waves; {rebuilt} evicted indexes rebuilt", rows.len());
}
