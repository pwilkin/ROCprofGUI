use profile_core::load_capture;
use rusqlite::{params, Connection};
fn schema(db: &Connection, suffix: &str) {
    db.execute_batch(&format!("CREATE TABLE rocpd_info_kernel_symbol{suffix} (id INTEGER,guid TEXT,kernel_name TEXT,display_name TEXT,arch_vgpr_count INTEGER,sgpr_count INTEGER);
    CREATE TABLE rocpd_kernel_dispatch{suffix} (id INTEGER,guid TEXT,nid INTEGER,pid INTEGER,agent_id INTEGER,kernel_id INTEGER,dispatch_id INTEGER,queue_id INTEGER,start INTEGER,end INTEGER,group_segment_size INTEGER,private_segment_size INTEGER,grid_size_x INTEGER,grid_size_y INTEGER,grid_size_z INTEGER,workgroup_size_x INTEGER,workgroup_size_y INTEGER,workgroup_size_z INTEGER); ")).unwrap();
}
fn insert(db: &Connection, suffix: &str, guid: &str, start: i64, end: i64) {
    db.execute(
        &format!("INSERT INTO rocpd_info_kernel_symbol{suffix} VALUES(1,?1,?2,NULL,32,NULL)"),
        params![guid, format!("kernel_{guid}")],
    )
    .unwrap();
    db.execute(&format!("INSERT INTO rocpd_kernel_dispatch{suffix} VALUES(1,?1,0,1,2,1,3,4,?2,?3,0,NULL,64,1,1,32,1,1)"),params![guid,start,end]).unwrap();
}
#[test]
fn reads_rocm_dispatches_without_modifying_database_or_losing_timestamp_precision() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("capture.rocpd");
    let db = Connection::open(&path).unwrap();
    schema(&db, "");
    insert(&db, "", "a", 18_000_000_000_000_001, 18_000_000_000_000_008);
    insert(&db, "", "b", 18_000_000_000_000_003, 18_000_000_000_000_012);
    drop(db);
    let before = std::fs::read(&path).unwrap();
    let c = load_capture(&path).unwrap();
    assert_eq!(c.origin, "18000000000000001");
    assert_eq!(c.dispatches.len(), 2);
    assert_eq!(c.dispatches[0].end, 7.);
    assert_eq!(c.dispatches[1].start, 2.);
    assert_eq!(c.dispatches[1].kernel, "kernel_b");
    assert_ne!(c.dispatches[0].id, c.dispatches[1].id);
    assert_eq!(c.dispatches[0].lds, Some(0));
    assert_eq!(c.dispatches[0].scratch, None);
    assert_eq!(c.dispatches[0].sgpr, None);
    assert!(c.waves.is_empty());
    assert_eq!(before, std::fs::read(path).unwrap());
}
#[test]
fn reads_uuid_partitioned_tables_without_double_counting_views() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("capture.db");
    let db = Connection::open(&path).unwrap();
    schema(&db, "_one");
    schema(&db, "_two");
    insert(&db, "_one", "one", 100, 110);
    insert(&db, "_two", "two", 120, 140);
    db.execute_batch("CREATE VIEW rocpd_kernel_dispatch AS SELECT * FROM rocpd_kernel_dispatch_one UNION ALL SELECT * FROM rocpd_kernel_dispatch_two").unwrap();
    let c = load_capture(&path).unwrap();
    assert_eq!(c.dispatches.len(), 2);
    assert_eq!(c.end, 40.);
}
#[test]
fn rejects_invalid_intervals_missing_symbols_and_unsupported_schema() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("capture.sqlite");
    let db = Connection::open(&path).unwrap();
    assert!(load_capture(&path)
        .unwrap_err()
        .contains("Unsupported ROCPD schema"));
    schema(&db, "");
    insert(&db, "", "a", 100, 90);
    assert!(load_capture(&path).unwrap_err().contains("timestamp"));
    db.execute_batch(
        "UPDATE rocpd_kernel_dispatch SET end=120; DELETE FROM rocpd_info_kernel_symbol",
    )
    .unwrap();
    assert!(load_capture(&path)
        .unwrap_err()
        .contains("missing kernel symbol"));
}

#[test]
fn official_sdk_304_fixture_imports_dispatches_and_dimensioned_spm() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/rocpd/sdk-3.0.4.db");
    let c = load_capture(&path).unwrap();
    assert_eq!(c.dispatches.len(), 2);
    assert_eq!(c.counters.len(), 2);
    assert_eq!(c.origin, "999500000");
    assert_eq!(c.host_calls.len(), 3);
    assert!(c.host_calls.iter().all(|call| call.operations.is_empty()));
    assert!(c
        .counters
        .iter()
        .all(|s| s.name == "SQ_WAVES" && s.points == vec![[550000., 16.]]));
    assert_ne!(c.counters[0].scope, c.counters[1].scope);
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("spm-only.db");
    std::fs::copy(path, &copy).unwrap();
    let db = Connection::open(&copy).unwrap();
    let table:String=db.query_row("SELECT name FROM sqlite_schema WHERE type='table' AND name LIKE 'rocpd_kernel_dispatch_%'",[],|r|r.get(0)).unwrap();
    db.execute_batch(&format!("DELETE FROM \"{table}\""))
        .unwrap();
    let region: String = db
        .query_row(
            "SELECT name FROM sqlite_schema WHERE type='table' AND name LIKE 'rocpd_region_%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    db.execute(&format!("DELETE FROM \"{region}\""), [])
        .unwrap();
    let c = load_capture(&copy).unwrap();
    assert!(c.dispatches.is_empty());
    assert_eq!(c.origin, "1000050000");
    assert_eq!(c.counters[0].points, vec![[0., 16.]]);
}

#[test]
fn cancelled_rocpd_import_does_not_open_database() {
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    assert!(
        profile_core::rocpd::load_cancellable(std::path::Path::new("nonexistent.rocpd"), flag)
            .unwrap_err()
            .contains("cancelled")
    );
}

#[test]
fn event_counters_in_official_schema_remain_separate_from_spm_and_keep_dispatch_identity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    std::fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rocpd/sdk-3.0.4.db"),
        &path,
    )
    .unwrap();
    let db = Connection::open(&path).unwrap();
    let table: String = db
        .query_row(
            "SELECT name FROM sqlite_schema WHERE type='table' AND name LIKE 'rocpd_pmc_event_%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    db.execute_batch(&format!("INSERT INTO \"{table}\" (guid,event_id,sample_id,pmc_id,value) SELECT guid,4,NULL,pmc_id,0 FROM \"{table}\" LIMIT 1;
      INSERT INTO \"{table}\" (guid,event_id,sample_id,pmc_id,value) SELECT guid,NULL,NULL,pmc_id,NULL FROM \"{table}\" LIMIT 1;")).unwrap();
    drop(db);
    let before = std::fs::read(&path).unwrap();
    let c = load_capture(&path).unwrap();
    assert_eq!(c.counters.len(), 2);
    assert_eq!(c.event_counters.len(), 2);
    assert_eq!(c.event_counters[0].value, Some(0.));
    assert_eq!(c.event_counters[0].dispatches, [c.dispatches[0].id.clone()]);
    assert_eq!(c.event_counters[1].value, None);
    assert_eq!(c.event_counters[1].event, None);
    assert!(c.event_counters[1].dispatches.is_empty());
    assert_eq!(before, std::fs::read(path).unwrap());
}
#[test]
fn event_counter_partitions_do_not_cross_link_and_ambiguous_events_remain_explicit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    let db = Connection::open(&path).unwrap();
    for suffix in ["_one", "_two"] {
        schema(&db, suffix);
        insert(&db, suffix, "same-guid", 100, 200);
        db.execute_batch(&format!("ALTER TABLE rocpd_kernel_dispatch{suffix} ADD COLUMN event_id INTEGER;
          UPDATE rocpd_kernel_dispatch{suffix} SET event_id=7;
          CREATE TABLE rocpd_event{suffix}(id INTEGER,guid TEXT);
          INSERT INTO rocpd_event{suffix} VALUES(7,'same-guid');
          CREATE TABLE rocpd_info_pmc{suffix}(id INTEGER,guid TEXT,name TEXT,units TEXT);
          INSERT INTO rocpd_info_pmc{suffix} VALUES(1,'same-guid','SQ_WAVES','waves');
          CREATE TABLE rocpd_pmc_event{suffix}(id INTEGER,guid TEXT,event_id INTEGER,pmc_id INTEGER,value REAL);
          INSERT INTO rocpd_pmc_event{suffix} VALUES(1,'same-guid',7,1,12.5);")).unwrap();
    }
    db.execute_batch("INSERT INTO rocpd_kernel_dispatch_one SELECT 2,guid,nid,pid,agent_id,kernel_id,4,queue_id,start,end,group_segment_size,private_segment_size,grid_size_x,grid_size_y,grid_size_z,workgroup_size_x,workgroup_size_y,workgroup_size_z,event_id FROM rocpd_kernel_dispatch_one LIMIT 1;").unwrap();
    let c = load_capture(&path).unwrap();
    assert_eq!(c.event_counters.len(), 2);
    assert_eq!(c.event_counters[0].dispatches.len(), 2);
    assert!(c.event_counters[0]
        .dispatches
        .iter()
        .all(|id| id.starts_with("rocpd_kernel_dispatch_one:")));
    assert_eq!(c.event_counters[1].dispatches.len(), 1);
    assert!(c.event_counters[1].dispatches[0].starts_with("rocpd_kernel_dispatch_two:"));
    assert!(c.counters.is_empty());
    db.execute_batch("UPDATE rocpd_pmc_event_one SET event_id=999")
        .unwrap();
    assert!(load_capture(&path)
        .unwrap_err()
        .contains("missing definition or event"));
    db.execute_batch("UPDATE rocpd_pmc_event_one SET event_id=7,value=1e999")
        .unwrap();
    assert!(load_capture(&path).unwrap_err().contains("Nonfinite"));
}

#[test]
fn sdk_host_regions_scope_correlations_and_keep_argument_nulls() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("host.db");
    let db = Connection::open(&path).unwrap();
    schema(&db, "");
    insert(&db, "", "a", 100, 110);
    insert(&db, "", "b", 120, 140);
    db.execute_batch("ALTER TABLE rocpd_kernel_dispatch ADD COLUMN event_id INTEGER;
    UPDATE rocpd_kernel_dispatch SET event_id=7;
    CREATE TABLE rocpd_event(id INTEGER,guid TEXT,correlation_id INTEGER,category_id INTEGER);
    INSERT INTO rocpd_event VALUES(7,'a',77,8),(8,'a',77,8),(7,'b',77,8),(8,'b',77,8),(9,'a',0,8),(10,'a',0,8);
    CREATE TABLE rocpd_string(id INTEGER,guid TEXT,string TEXT);
    INSERT INTO rocpd_string VALUES(7,'a','api_a'),(7,'b','api_b'),(8,'a','HIP'),(8,'b','HSA');
    CREATE TABLE rocpd_region(id INTEGER,guid TEXT,nid INTEGER,pid INTEGER,tid INTEGER,start INTEGER,end INTEGER,name_id INTEGER,event_id INTEGER);
    INSERT INTO rocpd_region VALUES(1,'a',0,1,2,90,95,7,8),(2,'a',9,1,2,91,96,7,8),(1,'b',0,1,2,92,97,7,8),(3,'a',0,1,2,93,98,7,9),(4,'a',0,1,2,94,99,7,10);
    CREATE TABLE rocpd_arg(id INTEGER,guid TEXT,event_id INTEGER,position INTEGER,type TEXT,name TEXT,value TEXT);
    INSERT INTO rocpd_arg VALUES(1,'a',8,0,'pointer','input',NULL),(2,'a',8,1,'string','text','<test>');").unwrap();
    let c = load_capture(&path).unwrap();
    assert_eq!(c.origin, "90");
    assert_eq!(c.host_calls.len(), 5);
    let a = &c.host_calls[0];
    assert_eq!(a.operations, vec![c.dispatches[0].id.clone()]);
    assert_eq!(a.category.as_deref(), Some("HIP"));
    let arguments: serde_json::Value = serde_json::from_str(a.arguments.as_ref().unwrap()).unwrap();
    assert!(arguments[0]["value"].is_null());
    assert_eq!(arguments[1]["value"], "<test>");
    assert!(c.host_calls[1].operations.is_empty());
    assert_eq!(c.host_calls[2].operations, vec![c.dispatches[1].id.clone()]);
    assert_eq!(c.host_calls[2].category.as_deref(), Some("HSA"));
    assert!(c.host_calls[3].operations.is_empty());
    assert!(c.host_calls[4].operations.is_empty());
    assert_ne!(c.host_calls[0].scope, c.host_calls[2].scope);
    db.execute_batch("UPDATE rocpd_kernel_dispatch SET event_id=9 WHERE guid='a';")
        .unwrap();
    let shared = load_capture(&path).unwrap();
    assert_eq!(
        shared.host_calls[3].operations,
        vec![shared.dispatches[0].id.clone()]
    );
    assert!(shared.host_calls[4].operations.is_empty());
    db.execute_batch("UPDATE rocpd_region SET end=start-1 WHERE guid='a' AND id=1;")
        .unwrap();
    assert!(load_capture(&path)
        .unwrap_err()
        .contains("CPU region interval"));
}
