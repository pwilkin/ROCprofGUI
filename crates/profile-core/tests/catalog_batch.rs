use profile_core::{catalog_transfer::CatalogBatch, Capture};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

fn capture(id: &str) -> Arc<Capture> {
    let mut capture = profile_core::load_capture(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/transpose-ui"),
    )
    .unwrap();
    capture.id = id.into();
    Arc::new(capture)
}
fn flag() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

#[test]
fn batch_preserves_project_order_and_commits_only_when_all_catalogs_are_complete() {
    let captures = vec![capture("second"), capture("first")];
    let mut batch = CatalogBatch::new(captures.clone(), "project-1".into(), flag()).unwrap();
    let header = batch.header().unwrap();
    assert_eq!(header.transport, "catalog-batch-v1");
    assert_eq!(
        header
            .captures
            .iter()
            .map(|h| h.capture.id.as_str())
            .collect::<Vec<_>>(),
        vec!["second", "first"]
    );
    assert!(header
        .captures
        .iter()
        .all(|h| h.capture.instructions.is_empty()));
    assert_ne!(
        header.captures[0].transfer_id,
        header.captures[1].transfer_id
    );
    for index in [1, 0] {
        let first = batch.page(index, 0, 5).unwrap();
        let retry = batch.page(index, 0, 5).unwrap();
        assert_eq!(
            serde_json::to_value(&first).unwrap(),
            serde_json::to_value(retry).unwrap()
        );
        let rest = batch.page(index, 5, 4096).unwrap();
        let records: Vec<_> = first
            .instructions
            .into_iter()
            .chain(rest.instructions)
            .collect();
        assert_eq!(
            serde_json::to_value(records).unwrap(),
            serde_json::to_value(&captures[index].instructions).unwrap()
        );
    }
    let ready = batch.commit().unwrap();
    assert!(ready.iter().zip(captures).all(|(a, b)| Arc::ptr_eq(a, &b)));
}
#[test]
fn incomplete_batch_does_not_replace_the_existing_workspace_and_releases_all_staging() {
    let current = vec![capture("existing")];
    let mut staged = vec![capture("new-1"), capture("new-2")];
    let mut paths = Vec::new();
    for capture in &mut staged {
        let workspace = tempfile::tempdir().unwrap();
        paths.push(workspace.path().to_owned());
        Arc::get_mut(capture).unwrap().workspace = Some(Arc::new(workspace));
    }
    let mut batch = CatalogBatch::new(staged, "project-2".into(), flag()).unwrap();
    batch.page(0, 0, 4096).unwrap();
    let mut workspace = current.clone();
    let result = batch.commit();
    assert!(result.is_err());
    if let Ok(replacement) = result {
        workspace = replacement;
    }
    assert!(Arc::ptr_eq(&workspace[0], &current[0]));
    assert!(paths.iter().all(|p| !p.exists()));
}
#[test]
fn cancellation_after_all_pages_prevents_batch_commit() {
    let cancelled = flag();
    let mut batch = CatalogBatch::new(
        vec![capture("a"), capture("b")],
        "project-3".into(),
        cancelled.clone(),
    )
    .unwrap();
    batch.page(0, 0, 4096).unwrap();
    batch.page(1, 0, 4096).unwrap();
    cancelled.store(true, Ordering::Relaxed);
    assert!(batch.header().is_err());
    assert!(batch.page(0, 0, 4096).is_err());
    assert!(batch.commit().is_err());
}
#[test]
fn invalid_batch_ids_counts_and_indices_are_rejected_without_progress() {
    assert!(CatalogBatch::new(vec![], "empty".into(), flag()).is_err());
    assert!(CatalogBatch::new(
        vec![capture("same"), capture("same")],
        "duplicate".into(),
        flag()
    )
    .is_err());
    assert!(CatalogBatch::new(vec![capture("")], "blank".into(), flag()).is_err());
    assert!(CatalogBatch::new(vec![capture("a")], "x".repeat(97), flag()).is_err());
    assert!(CatalogBatch::new(
        (0..7).map(|n| capture(&n.to_string())).collect(),
        "too-many".into(),
        flag()
    )
    .is_err());
    let mut batch = CatalogBatch::new(vec![capture("a")], "valid".into(), flag()).unwrap();
    assert!(batch.page(usize::MAX, 0, 1).is_err());
    assert!(batch.page(0, 1, 1).is_err());
    batch.page(0, 0, 4096).unwrap();
    assert_eq!(batch.commit().unwrap().len(), 1);
}

#[test]
fn six_native_captures_and_demo_preserve_existing_project_capacity() {
    let mut captures: Vec<_> = (0..6).map(|n| capture(&n.to_string())).collect();
    captures.push(capture("demo"));
    let mut batch = CatalogBatch::new(captures, "full-project".into(), flag()).unwrap();
    assert_eq!(batch.header().unwrap().captures.len(), 7);
    for index in 0..7 {
        batch.page(index, 0, 4096).unwrap();
    }
    assert_eq!(batch.commit().unwrap().len(), 7);
}
