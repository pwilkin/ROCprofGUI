use profile_core::{capture_archive, load_wave, project};
use std::{
    fs,
    path::Path,
    sync::{atomic::AtomicBool, Arc},
};
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
fn portable_archive_reopens_after_original_removal_and_owns_extracted_files() {
    let source = tempfile::tempdir().unwrap();
    let output = tempfile::tempdir().unwrap();
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/transpose-ui"),
        source.path(),
    );
    let capture = project::load_input(source.path(), Arc::new(AtomicBool::new(false))).unwrap();
    let expected = load_wave(&capture, &capture.waves[0].id).unwrap();
    let path = output.path().join("portable.rocprof-capture");
    capture_archive::write(&path, &capture, &AtomicBool::new(false)).unwrap();
    if let Ok(destination) = std::env::var("ROCPROF_ARCHIVE_TEST_ARTIFACT") {
        fs::copy(&path, destination).unwrap();
    }
    source.close().unwrap();
    let restored = project::load_input(&path, Arc::new(AtomicBool::new(false))).unwrap();
    assert_eq!(restored.waves.len(), capture.waves.len());
    assert_eq!(restored.instructions.len(), capture.instructions.len());
    assert_eq!(restored.counters.len(), capture.counters.len());
    let wave = load_wave(&restored, &restored.waves[0].id).unwrap();
    assert_eq!(
        serde_json::to_value(wave.tokens).unwrap(),
        serde_json::to_value(expected.tokens).unwrap()
    );
    for name in &restored.source_files {
        profile_core::source_files::read_snapshot(&restored, name).unwrap();
    }
    let root = restored.root.clone();
    assert!(root.exists());
    drop(restored);
    assert!(!root.exists());
}
#[test]
fn corrupted_and_cancelled_archives_fail_without_overwriting_destination() {
    let output = tempfile::tempdir().unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/transpose-ui");
    let capture = project::load_input(&fixture, Arc::new(AtomicBool::new(false))).unwrap();
    let path = output.path().join("capture.rocprof-capture");
    let flag = AtomicBool::new(false);
    capture_archive::write(&path, &capture, &flag).unwrap();
    let original = fs::read(&path).unwrap();
    assert!(
        capture_archive::write(&path, &capture, &AtomicBool::new(true))
            .unwrap_err()
            .contains("cancelled")
    );
    assert_eq!(fs::read(&path).unwrap(), original);
    let mut changed = original.clone();
    *changed.last_mut().unwrap() ^= 1;
    fs::write(&path, changed).unwrap();
    assert!(capture_archive::read(&path, &flag)
        .unwrap_err()
        .contains("checksum"));
    let mut changed = original.clone();
    changed[48] ^= 1;
    fs::write(&path, changed).unwrap();
    assert!(capture_archive::read(&path, &flag)
        .unwrap_err()
        .contains("manifest checksum"));
    fs::write(&path, &original[..original.len() - 1]).unwrap();
    assert!(capture_archive::read(&path, &flag)
        .unwrap_err()
        .contains("length mismatch"));
}
#[test]
fn traversal_and_duplicate_entries_are_rejected_even_with_valid_manifest_hash() {
    use sha2::{Digest, Sha256};
    let output = tempfile::tempdir().unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/transpose-ui");
    let capture = project::load_input(&fixture, Arc::new(AtomicBool::new(false))).unwrap();
    let flag = AtomicBool::new(false);
    let path = output.path().join("capture.rocprof-capture");
    capture_archive::write(&path, &capture, &flag).unwrap();
    let bytes = fs::read(&path).unwrap();
    let n = u64::from_le_bytes(bytes[8..16].try_into().unwrap()) as usize;
    let original: serde_json::Value = serde_json::from_slice(&bytes[48..48 + n]).unwrap();
    for name in [
        "../outside",
        "/absolute",
        "C:\\escape",
        "waves/../escape",
        original["files"][1]["path"].as_str().unwrap(),
    ] {
        let mut manifest = original.clone();
        manifest["files"][0]["path"] = name.into();
        let serialized = serde_json::to_vec(&manifest).unwrap();
        let mut bad = bytes[..8].to_vec();
        bad.extend_from_slice(&(serialized.len() as u64).to_le_bytes());
        bad.extend_from_slice(&Sha256::digest(&serialized));
        bad.extend_from_slice(&serialized);
        bad.extend_from_slice(&bytes[48 + n..]);
        fs::write(&path, bad).unwrap();
        assert!(capture_archive::read(&path, &flag)
            .unwrap_err()
            .contains("Invalid capture archive entry"));
    }
}

#[test]
fn standalone_csv_and_database_archives_preserve_all_normalized_measurements() {
    let directory = tempfile::tempdir().unwrap();
    let flag = AtomicBool::new(false);
    let csv = directory.path().join("dispatch.csv");
    fs::write(
        &csv,
        "Kernel_Name,Start_Timestamp,End_Timestamp\nkernel,100,200\n",
    )
    .unwrap();
    let database = directory.path().join("capture.db");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rocpd/sdk-3.0.4.db"),
        &database,
    )
    .unwrap();
    for input in [csv, database] {
        let capture = project::load_input(&input, Arc::new(AtomicBool::new(false))).unwrap();
        let archive = input.with_extension("rocprof-capture");
        capture_archive::write(&archive, &capture, &flag).unwrap();
        fs::remove_file(input).unwrap();
        let mut restored = capture_archive::read(&archive, &flag).unwrap();
        restored.metadata.remove("archive");
        assert_eq!(
            serde_json::to_value(&restored).unwrap(),
            serde_json::to_value(&capture).unwrap()
        );
    }
}

#[test]
fn chunked_catalog_preserves_instruction_order_and_fields() {
    let output = tempfile::tempdir().unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/transpose-ui");
    let flag = AtomicBool::new(false);
    let mut capture = project::load_input(&fixture, Arc::new(AtomicBool::new(false))).unwrap();
    let template = capture.instructions[0].clone();
    capture.instructions = (0..5001)
        .map(|i| {
            let mut instruction = template.clone();
            instruction.index = i;
            instruction.text = format!("instruction {i}");
            instruction
        })
        .collect();
    let path = output.path().join("chunks.rocprof-capture");
    capture_archive::write(&path, &capture, &flag).unwrap();
    let bytes = fs::read(&path).unwrap();
    assert_eq!(&bytes[..8], b"RPCAP002");
    let n = u64::from_le_bytes(bytes[8..16].try_into().unwrap()) as usize;
    let manifest: serde_json::Value = serde_json::from_slice(&bytes[48..48 + n]).unwrap();
    assert_eq!(manifest["instruction_chunks"].as_array().unwrap().len(), 3);
    assert!(manifest["capture"]["instructions"]
        .as_array()
        .unwrap()
        .is_empty());
    for mode in ["duplicate", "missing", "inline"] {
        use sha2::{Digest, Sha256};
        let mut bad = manifest.clone();
        match mode {
            "duplicate" => bad["instruction_chunks"][1] = bad["instruction_chunks"][0].clone(),
            "missing" => bad["instruction_chunks"][0] = "instructions/missing.json".into(),
            "inline" => {
                bad["capture"]["instructions"] =
                    serde_json::to_value(&capture.instructions[..1]).unwrap()
            }
            _ => unreachable!(),
        }
        let body = serde_json::to_vec(&bad).unwrap();
        let mut changed = b"RPCAP002".to_vec();
        changed.extend_from_slice(&(body.len() as u64).to_le_bytes());
        changed.extend_from_slice(&Sha256::digest(&body));
        changed.extend_from_slice(&body);
        changed.extend_from_slice(&bytes[48 + n..]);
        fs::write(&path, changed).unwrap();
        let error = capture_archive::read(&path, &flag).unwrap_err();
        assert!(error.contains("instruction chunks"), "{mode}: {error}");
    }
    fs::write(&path, &bytes).unwrap();
    let restored = capture_archive::read(&path, &flag).unwrap();
    assert_eq!(
        serde_json::to_value(&capture.instructions).unwrap(),
        serde_json::to_value(&restored.instructions).unwrap()
    );
    let mut changed = bytes.clone();
    changed[48 + n + 10] ^= 1;
    fs::write(&path, changed).unwrap();
    assert!(capture_archive::read(&path, &flag)
        .unwrap_err()
        .contains("checksum"));
}

#[test]
fn legacy_inline_catalog_remains_readable() {
    use sha2::{Digest, Sha256};
    let output = tempfile::tempdir().unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/transpose-ui");
    let mut capture = project::load_input(&fixture, Arc::new(AtomicBool::new(false))).unwrap();
    capture.waves.clear();
    capture.source_paths.clear();
    let manifest =
        serde_json::to_vec(&serde_json::json!({"capture":capture,"sources":{},"files":[]}))
            .unwrap();
    let mut bytes = b"RPCAP001".to_vec();
    bytes.extend_from_slice(&(manifest.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&Sha256::digest(&manifest));
    bytes.extend_from_slice(&manifest);
    let path = output.path().join("legacy.rocprof-capture");
    fs::write(&path, bytes).unwrap();
    let restored = capture_archive::read(&path, &AtomicBool::new(false)).unwrap();
    assert_eq!(
        serde_json::to_value(&capture.instructions).unwrap(),
        serde_json::to_value(&restored.instructions).unwrap()
    );
}

#[test]
fn oversized_instruction_chunk_preserves_existing_destination() {
    let output = tempfile::tempdir().unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/transpose-ui");
    let mut capture = project::load_input(&fixture, Arc::new(AtomicBool::new(false))).unwrap();
    capture.instructions.truncate(1);
    capture.instructions[0].text = "x".repeat(16 * 1024 * 1024);
    let path = output.path().join("existing.rocprof-capture");
    fs::write(&path, b"existing destination").unwrap();
    assert!(
        capture_archive::write(&path, &capture, &AtomicBool::new(false))
            .unwrap_err()
            .contains("JSON size limit")
    );
    assert_eq!(fs::read(&path).unwrap(), b"existing destination");
}

#[test]
fn observation_archives_chunk_rows_and_validate_references_counts_and_legacy_versions() {
    use sha2::{Digest, Sha256};
    let output = tempfile::tempdir().unwrap();
    let flag = AtomicBool::new(false);
    let mut capture = profile_core::Capture::empty("samples".into(), "test", "ns");
    capture.pc_sampling_observations = Some(Arc::new(profile_core::PcSamplingObservations {
        columns: vec!["Id".into(), "Instruction".into()],
        rows: (0..5001)
            .map(|i| {
                vec![
                    format!("{}", u64::MAX - i),
                    "same \"instruction\"\nµ".into(),
                ]
            })
            .collect(),
    }));
    let path = output.path().join("samples.rocprof-capture");
    capture_archive::write(&path, &capture, &flag).unwrap();
    let bytes = fs::read(&path).unwrap();
    assert_eq!(&bytes[..8], b"RPCAP003");
    let n = u64::from_le_bytes(bytes[8..16].try_into().unwrap()) as usize;
    let manifest: serde_json::Value = serde_json::from_slice(&bytes[48..48 + n]).unwrap();
    assert_eq!(manifest["observation_count"], 5001);
    assert_eq!(manifest["observation_chunks"].as_array().unwrap().len(), 3);
    assert_eq!(
        manifest["capture"]["pcSamplingObservations"]["rows"],
        serde_json::json!([])
    );
    assert_eq!(
        capture_archive::read(&path, &flag)
            .unwrap()
            .pc_sampling_observations
            .unwrap()
            .rows,
        capture.pc_sampling_observations.as_ref().unwrap().rows
    );
    for mode in [
        "duplicate",
        "missing",
        "inline",
        "count",
        "width",
        "version",
    ] {
        let mut bad = manifest.clone();
        match mode {
            "duplicate" => bad["observation_chunks"][1] = bad["observation_chunks"][0].clone(),
            "missing" => bad["observation_chunks"][0] = "observations/missing.json".into(),
            "inline" => {
                bad["capture"]["pcSamplingObservations"]["rows"] = serde_json::json!([["1", "x"]])
            }
            "count" => bad["observation_count"] = 5002.into(),
            "width" => {
                bad["capture"]["pcSamplingObservations"]["columns"] = serde_json::json!(["Id"])
            }
            _ => (),
        }
        let body = serde_json::to_vec(&bad).unwrap();
        let mut changed = if mode == "version" {
            b"RPCAP002".to_vec()
        } else {
            b"RPCAP003".to_vec()
        };
        changed.extend_from_slice(&(body.len() as u64).to_le_bytes());
        changed.extend_from_slice(&Sha256::digest(&body));
        changed.extend_from_slice(&body);
        changed.extend_from_slice(&bytes[48 + n..]);
        fs::write(&path, changed).unwrap();
        assert!(
            capture_archive::read(&path, &flag)
                .unwrap_err()
                .contains("observation"),
            "{mode}"
        );
    }
    let mut corrupt = bytes.clone();
    *corrupt.last_mut().unwrap() ^= 1;
    fs::write(&path, corrupt).unwrap();
    assert!(capture_archive::read(&path, &flag)
        .unwrap_err()
        .contains("checksum"));
    for magic in [b"RPCAP001", b"RPCAP002"] {
        let body =
            serde_json::to_vec(&serde_json::json!({"capture":capture,"sources":{},"files":[]}))
                .unwrap();
        let mut legacy = magic.to_vec();
        legacy.extend_from_slice(&(body.len() as u64).to_le_bytes());
        legacy.extend_from_slice(&Sha256::digest(&body));
        legacy.extend_from_slice(&body);
        fs::write(&path, legacy).unwrap();
        assert_eq!(
            capture_archive::read(&path, &flag)
                .unwrap()
                .pc_sampling_observations
                .unwrap()
                .rows,
            capture.pc_sampling_observations.as_ref().unwrap().rows
        );
    }
}

#[test]
fn observation_chunks_split_by_encoded_size_and_preserve_destination_on_failure() {
    let output = tempfile::tempdir().unwrap();
    let path = output.path().join("wide.rocprof-capture");
    let flag = AtomicBool::new(false);
    let mut c = profile_core::Capture::empty("wide".into(), "test", "ns");
    c.pc_sampling_observations = Some(Arc::new(profile_core::PcSamplingObservations {
        columns: vec!["x".into()],
        rows: vec![vec!["\"".repeat(5_000_000)]; 2],
    }));
    capture_archive::write(&path, &c, &flag).unwrap();
    assert_eq!(
        capture_archive::read(&path, &flag)
            .unwrap()
            .pc_sampling_observations
            .unwrap()
            .rows,
        c.pc_sampling_observations.as_ref().unwrap().rows
    );
    let original = fs::read(&path).unwrap();
    c.pc_sampling_observations = Some(Arc::new(profile_core::PcSamplingObservations {
        columns: vec!["x".into()],
        rows: vec![vec!["x".repeat(16 * 1024 * 1024)]],
    }));
    assert!(capture_archive::write(&path, &c, &flag)
        .unwrap_err()
        .contains("16 MiB"));
    assert_eq!(fs::read(&path).unwrap(), original);
}

#[test]
#[ignore = "requires a retained large CSV fixture"]
fn large_csv_observation_archive_roundtrip() {
    let input = std::env::var("ROCPROF_CSV_ARCHIVE_INPUT").unwrap();
    let destination = std::env::var("ROCPROF_CSV_ARCHIVE_OUTPUT").unwrap();
    let flag = AtomicBool::new(false);
    let capture = profile_core::load_capture(Path::new(&input)).unwrap();
    capture_archive::write_new(Path::new(&destination), &capture, &flag).unwrap();
    let restored = capture_archive::read(Path::new(&destination), &flag).unwrap();
    assert_eq!(
        restored.pc_sampling_observations.as_ref().unwrap().columns,
        capture.pc_sampling_observations.as_ref().unwrap().columns
    );
    assert_eq!(
        restored.pc_sampling_observations.as_ref().unwrap().rows,
        capture.pc_sampling_observations.as_ref().unwrap().rows
    );
    println!(
        "Every field matched across {} archived CSV observations",
        restored
            .pc_sampling_observations
            .as_ref()
            .unwrap()
            .rows
            .len()
    );
}
