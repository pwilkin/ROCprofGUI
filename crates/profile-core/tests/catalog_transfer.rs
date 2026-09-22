use profile_core::{
    catalog_transfer::{CatalogTransfer, MAX_PAGE_BYTES, MAX_PAGE_ROWS},
    Capture, Instruction,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

fn instruction(index: i64) -> Instruction {
    Instruction {
        index,
        text: format!("v_add_u32 v0, v{index}, v2 // \"µ\"\n"),
        source: "kernel.hip:19".into(),
        address: format!("0x{:x}", index * 4),
        code_object: "object".into(),
        hits: 42,
        latency: 1.25,
        stall: 0.5,
        idle: 0.125,
        pc_samples: 11,
        pc_issued: Some(9),
        pc_stalled: Some(2),
        stall_reasons: Some(vec![0, 1, 2]),
    }
}
fn capture(count: usize) -> Arc<Capture> {
    let mut c = Capture::empty("transfer fixture".into(), "test", "cycles");
    c.id = "capture-17".into();
    c.instructions = (0..count).map(|i| instruction(i as i64)).collect();
    c.metadata.insert("provenance".into(), "test".into());
    c.sources
        .insert("kernel.hip".into(), "void kernel() {}".into());
    Arc::new(c)
}
fn transfer(c: Arc<Capture>, flag: Arc<AtomicBool>) -> CatalogTransfer {
    CatalogTransfer::new(c, "transfer-23".into(), flag).unwrap()
}
#[test]
fn ordered_pages_preserve_every_instruction_field_and_commit_the_same_capture() {
    let original = capture(MAX_PAGE_ROWS + 1);
    let mut t = transfer(original.clone(), Arc::new(AtomicBool::new(false)));
    let header = t.header().unwrap();
    assert_eq!(header.transport, "instruction-pages-v1");
    assert_eq!(header.instruction_count, MAX_PAGE_ROWS + 1);
    let mut expected_header = serde_json::to_value(&*original).unwrap();
    expected_header["instructions"] = serde_json::json!([]);
    assert_eq!(
        serde_json::to_value(&header.capture).unwrap(),
        expected_header
    );
    let first = t.page(0, MAX_PAGE_ROWS).unwrap();
    assert_eq!(first.instructions.len(), MAX_PAGE_ROWS);
    assert_eq!(first.capture_id, original.id);
    assert_eq!(first.transfer_id, header.transfer_id);
    let retry = t.page(0, MAX_PAGE_ROWS).unwrap();
    assert_eq!(
        serde_json::to_value(&retry).unwrap(),
        serde_json::to_value(&first).unwrap()
    );
    let final_page = t.page(MAX_PAGE_ROWS, MAX_PAGE_ROWS).unwrap();
    assert_eq!(final_page.instructions.len(), 1);
    let all: Vec<_> = first
        .instructions
        .into_iter()
        .chain(final_page.instructions)
        .collect();
    assert_eq!(
        serde_json::to_value(all).unwrap(),
        serde_json::to_value(&original.instructions).unwrap()
    );
    assert!(Arc::ptr_eq(&original, &t.commit().unwrap()));
}
#[test]
fn pages_obey_encoded_byte_limits_and_do_not_advance_on_oversized_records() {
    let mut c = capture(3);
    Arc::get_mut(&mut c).unwrap().instructions[0].text = "\n".repeat(MAX_PAGE_BYTES / 3);
    Arc::get_mut(&mut c).unwrap().instructions[1].text = "x".repeat(MAX_PAGE_BYTES);
    let mut t = transfer(c, Arc::new(AtomicBool::new(false)));
    let first = t.page(0, 3).unwrap();
    assert_eq!(first.instructions.len(), 1);
    assert!(serde_json::to_vec(&first).unwrap().len() <= MAX_PAGE_BYTES);
    assert!(t.page(1, 3).unwrap_err().contains("offset 1"));
    assert_eq!(t.page(0, 3).unwrap().instructions.len(), 1);
    assert!(t.commit().is_err());
}
#[test]
fn exact_empty_end_and_invalid_page_requests_are_unambiguous() {
    let mut empty = transfer(capture(0), Arc::new(AtomicBool::new(false)));
    assert_eq!(empty.header().unwrap().instruction_count, 0);
    assert!(empty.page(0, 1).unwrap().instructions.is_empty());
    assert!(empty.commit().unwrap().instructions.is_empty());
    let mut t = transfer(capture(5), Arc::new(AtomicBool::new(false)));
    for (offset, count) in [(1, 2), (usize::MAX, 1), (0, 0), (0, MAX_PAGE_ROWS + 1)] {
        assert!(t.page(offset, count).is_err());
    }
    t.page(0, 2).unwrap();
    assert!(t.page(0, 1).is_err());
    t.page(2, 2).unwrap();
    assert!(t.page(0, 2).is_err());
    t.page(4, 2).unwrap();
    assert!(t.page(5, 1).unwrap().instructions.is_empty());
    assert_eq!(t.commit().unwrap().instructions.len(), 5);
}
#[test]
fn cancellation_blocks_header_pages_retries_and_commit() {
    let flag = Arc::new(AtomicBool::new(false));
    let mut t = transfer(capture(1), flag.clone());
    t.page(0, 1).unwrap();
    flag.store(true, Ordering::Relaxed);
    assert!(t.header().is_err());
    assert!(t.page(0, 1).is_err());
    assert!(t.commit().is_err());
    assert!(CatalogTransfer::new(capture(0), "new".into(), flag).is_err());
}
#[test]
fn incomplete_or_abandoned_transfers_release_the_owned_workspace() {
    for commit in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().to_owned();
        let mut c = capture(2);
        Arc::get_mut(&mut c).unwrap().workspace = Some(Arc::new(directory));
        let t = transfer(c, Arc::new(AtomicBool::new(false)));
        assert!(path.exists());
        if commit {
            assert!(t.commit().is_err());
        } else {
            drop(t);
        }
        assert!(!path.exists());
    }
}

#[test]
#[ignore = "requires ROCPROF_CATALOG_TRANSFER_ARCHIVE"]
fn real_archive_catalog_pages_match_every_original_record() {
    let path = std::env::var("ROCPROF_CATALOG_TRANSFER_ARCHIVE").unwrap();
    let flag = Arc::new(AtomicBool::new(false));
    let original = Arc::new(
        profile_core::project::load_input(std::path::Path::new(&path), flag.clone()).unwrap(),
    );
    let mut t = transfer(original.clone(), flag);
    let header = t.header().unwrap();
    assert_eq!(header.instruction_count, original.instructions.len());
    assert!(header.capture.instructions.is_empty());
    drop(header);
    let mut offset = 0;
    let mut pages = 0;
    let mut largest = 0;
    while offset < original.instructions.len() {
        let page = t.page(offset, MAX_PAGE_ROWS).unwrap();
        assert!(!page.instructions.is_empty());
        let bytes = serde_json::to_vec(&page).unwrap().len();
        assert!(bytes <= MAX_PAGE_BYTES);
        largest = largest.max(bytes);
        let end = offset + page.instructions.len();
        assert_eq!(
            serde_json::to_vec(&page.instructions).unwrap(),
            serde_json::to_vec(&original.instructions[offset..end]).unwrap()
        );
        offset = end;
        pages += 1;
    }
    assert!(Arc::ptr_eq(&original, &t.commit().unwrap()));
    println!("Catalog transfer verified {offset} records in {pages} pages; largest encoded page {largest} bytes");
}

#[test]
fn observation_pages_are_bounded_ordered_retryable_and_required_for_commit() {
    let mut original = (*capture(0)).clone();
    original.pc_sampling_observations = Some(Arc::new(profile_core::PcSamplingObservations {
        columns: vec!["Instruction".into(), "Exact_Id".into()],
        rows: (0..4097)
            .map(|i| vec![format!("sample {i}"), "18446744073709551615".into()])
            .collect(),
    }));
    let original = Arc::new(original);
    let flag = Arc::new(AtomicBool::new(false));
    let mut t = transfer(original.clone(), flag.clone());
    let header = t.header().unwrap();
    assert_eq!(header.transport, "instruction-pages-v2");
    assert_eq!(header.observation_count, Some(4097));
    assert!(header
        .capture
        .pc_sampling_observations
        .unwrap()
        .rows
        .is_empty());
    assert!(transfer(original.clone(), flag.clone()).commit().is_err());
    assert!(t.observation_page(1, 4096).is_err());
    let first = t.observation_page(0, 4096).unwrap();
    assert_eq!(first.observations.len(), 4096);
    assert_eq!(
        serde_json::to_value(&first).unwrap(),
        serde_json::to_value(t.observation_page(0, 4096).unwrap()).unwrap()
    );
    assert!(serde_json::to_vec(&first).unwrap().len() <= MAX_PAGE_BYTES);
    let second = t.observation_page(4096, 4096).unwrap();
    assert_eq!(
        second.observations,
        original.pc_sampling_observations.as_ref().unwrap().rows[4096..]
    );
    assert!(Arc::ptr_eq(&original, &t.commit().unwrap()));
    let mut cancelled = transfer(original, flag.clone());
    flag.store(true, Ordering::Relaxed);
    assert!(cancelled
        .observation_page(0, 4096)
        .unwrap_err()
        .contains("cancelled"));
}

#[test]
fn observation_pages_enforce_encoded_byte_limit_and_batch_completeness() {
    let mut original = (*capture(0)).clone();
    original.pc_sampling_observations = Some(Arc::new(profile_core::PcSamplingObservations {
        columns: vec!["Instruction".into()],
        rows: vec![vec!["\"".repeat(1_100_000)]; 2],
    }));
    let original = Arc::new(original);
    let flag = Arc::new(AtomicBool::new(false));
    let mut batch = profile_core::catalog_transfer::CatalogBatch::new(
        vec![original.clone()],
        "batch-obs".into(),
        flag.clone(),
    )
    .unwrap();
    assert!(serde_json::to_vec(&batch.header().unwrap()).unwrap().len() < 4096);
    let first = batch.observation_page(0, 0, 4096).unwrap();
    assert_eq!(first.observations.len(), 1);
    assert!(serde_json::to_vec(&first).unwrap().len() <= MAX_PAGE_BYTES);
    batch.observation_page(0, 1, 4096).unwrap();
    assert!(Arc::ptr_eq(&batch.commit().unwrap()[0], &original));
    let mut oversized = (*original).clone();
    oversized.pc_sampling_observations = Some(Arc::new(profile_core::PcSamplingObservations {
        columns: vec!["Instruction".into()],
        rows: vec![vec!["x".repeat(MAX_PAGE_BYTES)]],
    }));
    assert!(transfer(Arc::new(oversized), flag)
        .observation_page(0, 1)
        .is_err());
}
