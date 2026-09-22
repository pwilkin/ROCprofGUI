use profile_core::{hidden::analyze_scope, Token, Wave};
use std::collections::BTreeMap;
fn wave(id: &str, tokens: Vec<Token>) -> Wave {
    Wave {
        id: id.into(),
        start: 0.,
        end: 50.,
        cu: Some(0),
        tokens,
        states: vec![],
        waits: serde_json::Value::Null,
        info: serde_json::Value::Null,
    }
}
fn token(start: f64, duration: f64, stall: f64, category: u32, line: i64) -> Token {
    Token {
        start,
        duration,
        stall,
        category,
        instruction: line,
    }
}
#[test]
fn math_hides_memory_issue_but_memory_does_not_hide_math() {
    let result = analyze_scope(
        &[
            wave("math", vec![token(0., 20., 0., 6, 1)]),
            wave("memory", vec![token(10., 20., 0., 3, 2)]),
        ],
        &BTreeMap::new(),
        &[],
    );
    assert_eq!(result.by_instruction[&1].issue, 0);
    assert_eq!(result.by_instruction[&2].issue, 10);
    assert_eq!(result.by_instruction[&2].idle, 10);
}
#[test]
fn matrix_classification_comes_from_isa() {
    let isa = BTreeMap::from([(1, "v_wmma_f32_16x16x16_f16 v0, v1, v2".into())]);
    let result = analyze_scope(
        &[
            wave("matrix", vec![token(0., 20., 0., 6, 1)]),
            wave("valu", vec![token(10., 20., 0., 6, 2)]),
        ],
        &isa,
        &[],
    );
    assert_eq!(result.by_instruction[&1].issue, 0);
    assert_eq!(result.by_instruction[&2].issue, 10);
}
#[test]
fn other_simd_memory_activity_hides_scalar_and_wait_tokens() {
    let result = analyze_scope(
        &[wave(
            "scalar",
            vec![token(10., 20., 5., 2, 1), token(35., 5., 5., 9, 2)],
        )],
        &BTreeMap::new(),
        &[[0., 40.]],
    );
    assert_eq!(result.by_instruction[&1].idle, 10);
    assert_eq!(result.by_instruction[&1].stall, 5);
    assert_eq!(result.by_instruction[&1].issue, 15);
    assert_eq!(result.by_instruction[&2].stall, 5);
}

fn captured(
    cus: [Option<u32>; 2],
    second_simd: u32,
    other: bool,
) -> (tempfile::TempDir, profile_core::Capture) {
    use serde_json::json;
    let dir = tempfile::tempdir().unwrap();
    let mut names = json!({"wave_filenames": {"0": {}}, "other_simd_filenames": {}});
    for (i, cu) in cus.into_iter().enumerate() {
        let simd = if i == 0 { 0 } else { second_simd }.to_string();
        let file = format!("wave{i}.json");
        names["wave_filenames"]["0"][&simd][i.to_string()]["0"] = json!([file, 0, 50]);
        let tokens = if i == 0 {
            json!([[0, 6, 0, 20, 1]])
        } else {
            json!([[10, 2, 0, 20, 2]])
        };
        std::fs::write(
            dir.path().join(&file),
            json!({"wave":{"begin":0,"end":50,"cu":cu,"instructions":tokens}}).to_string(),
        )
        .unwrap();
    }
    if other {
        names["other_simd_filenames"]["0"] = json!([["other.json", 0, 40]]);
        std::fs::write(
            dir.path().join("other.json"),
            json!({"instructions_schema":["time","duration"],"instructions":[[0,40]]}).to_string(),
        )
        .unwrap();
    }
    std::fs::write(dir.path().join("filenames.json"), names.to_string()).unwrap();
    let capture = profile_core::load_capture(dir.path()).unwrap();
    (dir, capture)
}
#[test]
fn capture_hidden_costs_never_cross_known_cu_boundaries() {
    let (_dir, capture) = captured([Some(0), Some(4)], 0, false);
    let separated = profile_core::hidden::analyze_capture(&capture).unwrap();
    assert_eq!(separated.by_instruction[&2].issue, 0);
    assert_eq!(separated.by_wave.len(), 2);
    let (_dir, capture) = captured([Some(0), Some(0)], 0, false);
    let together = profile_core::hidden::analyze_capture(&capture).unwrap();
    assert_eq!(together.by_instruction[&2].issue, 10);
    assert!(together.warnings.is_empty());
}
#[test]
fn recorded_other_simd_wgp_scopes_hidden_costs_without_crossing_hardware() {
    for (cus, wgp, expected) in [
        ([Some(0), Some(4)], 4, 20),
        ([Some(0), Some(4)], 0, 0),
        ([Some(4), Some(4)], 0, 0),
        ([Some(4), Some(4)], 4, 20),
        ([Some(0), None], 0, 0),
    ] {
        let (_dir, mut capture) = captured(cus, 1, true);
        capture.architecture = "gfx1100".into();
        capture.other_simd[0].wgp = Some(wgp);
        let result = profile_core::hidden::analyze_capture(&capture).unwrap();
        assert_eq!(
            result.by_instruction[&2].issue, expected,
            "{cus:?} WGP {wgp}"
        );
    }
    for (architecture, declared) in [("gfx942", "9"), ("unknown", ""), ("gfx1100", "9")] {
        let (_dir, mut capture) = captured([Some(4), Some(4)], 1, true);
        capture.architecture = architecture.into();
        capture.metadata.insert("gfxip".into(), declared.into());
        capture.other_simd[0].wgp = Some(4);
        let result = profile_core::hidden::analyze_capture(&capture).unwrap();
        assert_eq!(result.by_instruction[&2].issue, 0);
        assert!(result
            .warnings
            .iter()
            .any(|s| s.contains("other-SIMD activity omitted")));
    }
    let (_dir, mut capture) = captured([Some(0), Some(4)], 1, true);
    capture.architecture = "navi".into();
    capture.metadata.insert("gfxip".into(), "11".into());
    capture.other_simd[0].wgp = Some(4);
    let result = profile_core::hidden::analyze_capture(&capture).unwrap();
    assert_eq!(result.by_instruction[&2].issue, 20);
    assert!(result.warnings.is_empty());
}

#[test]
fn other_simd_association_is_checked_across_the_entire_shader_engine() {
    let (_dir, capture) = captured([Some(0), Some(4)], 1, true);
    let result = profile_core::hidden::analyze_capture(&capture).unwrap();
    assert_eq!(result.by_instruction[&2].issue, 0);
    assert!(result
        .warnings
        .iter()
        .any(|s| s.contains("other-SIMD activity omitted")));
    let (_dir, capture) = captured([Some(4), Some(4)], 1, true);
    let result = profile_core::hidden::analyze_capture(&capture).unwrap();
    assert_eq!(result.by_instruction[&2].issue, 20);
    assert!(result.warnings.is_empty());
}
#[test]
fn mixed_unknown_cus_are_isolated_but_legacy_assumptions_are_explicit() {
    let (_dir, capture) = captured([Some(0), None], 0, false);
    let result = profile_core::hidden::analyze_capture(&capture).unwrap();
    assert_eq!(result.by_instruction[&2].issue, 0);
    assert!(result.warnings[0].contains("analyzed separately"));
    let (_dir, capture) = captured([None, None], 0, false);
    let result = profile_core::hidden::analyze_capture(&capture).unwrap();
    assert_eq!(result.by_instruction[&2].issue, 10);
    assert!(result.warnings[0].contains("assumes one traced CU"));
}

#[test]
fn cancelled_hidden_scope_returns_no_partial_analysis() {
    let flag = std::sync::atomic::AtomicBool::new(true);
    assert_eq!(
        profile_core::hidden::analyze_scope_cancellable(&[], &Default::default(), &[], &flag)
            .unwrap_err(),
        "Wave query cancelled"
    );
}
