use profile_core::{capture_archive, load_capture};
use serde_json::json;
use std::{fs, path::Path, sync::atomic::AtomicBool};
fn copy_tree(source: &Path, target: &Path) {
    fs::create_dir_all(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let dst = target.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &dst);
        } else {
            fs::copy(entry.path(), dst).unwrap();
        }
    }
}

#[test]
fn malformed_att_records_cannot_silently_change_hardware_identity() {
    let root = tempfile::tempdir().unwrap();
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/transpose-ui"),
        root.path(),
    );
    let file = root.path().join("se0_perfcounter.json");
    let good = json!([500, 1, 2, 3, 4, 2, 0]);
    for (column, invalid, message) in [
        (0, json!(9007199254740992u64), "timestamp"),
        (0, json!(-9007199254740992i64), "timestamp"),
        (0, json!(0.5), "timestamp"),
        (1, json!(-1), "value"),
        (2, json!(65536), "value"),
        (3, json!(1.5), "value"),
        (4, json!(null), "value"),
        (5, json!(256), "CU"),
        (5, json!(4294967296u64), "CU"),
        (5, json!(-1), "CU"),
        (5, json!("2"), "CU"),
        (6, json!(2), "bank"),
        (6, json!(null), "bank"),
        (6, json!(0.5), "bank"),
    ] {
        let mut row = good.clone();
        row[column] = invalid;
        fs::write(&file, json!({"data": [good, row]}).to_string()).unwrap();
        assert!(load_capture(root.path()).unwrap_err().contains(message));
    }
    for malformed in [
        json!({}),
        json!({"data": {}}),
        json!({"data": [null]}),
        json!({"data": [[0,1,2,3,4,5]]}),
        json!({"data": [[0,1,2,3,4,5,0,7]]}),
    ] {
        fs::write(&file, malformed.to_string()).unwrap();
        assert!(load_capture(root.path())
            .unwrap_err()
            .contains("Malformed ATT counter"));
    }
}

#[test]
fn att_abi_bounds_and_empty_emitter_output_are_preserved() {
    let root = tempfile::tempdir().unwrap();
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/transpose-ui"),
        root.path(),
    );
    let file = root.path().join("se0_perfcounter.json");
    fs::write(&file, json!({"data": [[-9007199254740991i64, 0, 65535, 0, 65535, 255, 1], [9007199254740991u64, 65535, 0, 65535, 0, 255, 1]]}).to_string()).unwrap();
    let capture = load_capture(root.path()).unwrap();
    let counters: Vec<_> = capture
        .counters
        .iter()
        .filter(|s| s.scope == "SE 0 · CU 255 · bank 1")
        .collect();
    assert_eq!(counters.len(), 4);
    assert_eq!(
        counters[0].points,
        vec![[-9007199254740991., 0.], [9007199254740991., 65535.]]
    );
    assert!(capture
        .warnings
        .iter()
        .any(|s| s.contains("do not record counter types or physical units")));
    for empty in [json!(null), json!([])] {
        for entry in fs::read_dir(root.path()).unwrap() {
            let path = entry.unwrap().path();
            if path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with("_perfcounter.json")
            {
                fs::write(path, json!({"data": empty}).to_string()).unwrap();
            }
        }
        let capture = load_capture(root.path()).unwrap();
        assert!(!capture
            .counters
            .iter()
            .any(|s| s.scope.contains(" · bank ")));
        assert!(!capture
            .warnings
            .iter()
            .any(|s| s.contains("do not record counter types")));
    }
}
#[test]
fn att_masks_preserve_raw_identity_samples_and_archive_metadata() {
    let root = tempfile::tempdir().unwrap();
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/transpose-ui"),
        root.path(),
    );
    let file = root.path().join("filenames.json");
    let mut names: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    let baseline = load_capture(root.path()).unwrap();
    for mask in 0..16u8 {
        names["counter_names"][0] = format!("SQ_WAVES:{mask:X}").into();
        names["counter_names"][1] = "SQ_WAVES".into();
        names["counter_names"][2] = "SQ_WAVES:10".into();
        names["counter_names"][3] = "SQ_WAVES:xyz".into();
        fs::write(&file, serde_json::to_vec(&names).unwrap()).unwrap();
        let capture = load_capture(root.path()).unwrap();
        for (actual, original) in capture.counters.iter().zip(&baseline.counters) {
            assert_eq!(actual.points, original.points);
            assert_eq!(actual.scope, original.scope);
        }
        let selected = capture
            .counters
            .iter()
            .find(|s| s.name == format!("SQ_WAVES:{mask:X}"))
            .unwrap();
        let metadata = selected.att.as_ref().unwrap();
        assert_eq!(metadata.counter, "SQ_WAVES");
        assert_eq!(metadata.simd_mask, mask);
        assert_eq!(
            metadata.simds,
            (0..4).filter(|n| mask & (1 << n) != 0).collect::<Vec<_>>()
        );
        assert!(metadata.explicit);
        let default = capture
            .counters
            .iter()
            .find(|s| s.name == "SQ_WAVES")
            .unwrap()
            .att
            .as_ref()
            .unwrap();
        assert_eq!(default.simd_mask, 15);
        assert!(!default.explicit);
        assert!(capture
            .counters
            .iter()
            .filter(|s| s.name.ends_with(":10") || s.name.ends_with(":xyz"))
            .all(|s| s.att.is_none()));
        assert!(capture
            .warnings
            .iter()
            .any(|w| w.contains("unsupported SIMD mask")));
        if mask == 5 {
            let output = tempfile::tempdir().unwrap();
            let archive = output.path().join("selection.rocprof-capture");
            let flag = AtomicBool::new(false);
            capture_archive::write(&archive, &capture, &flag).unwrap();
            let restored = capture_archive::read(&archive, &flag).unwrap();
            assert_eq!(
                serde_json::to_value(capture.counters).unwrap(),
                serde_json::to_value(restored.counters).unwrap()
            );
        }
    }
}
