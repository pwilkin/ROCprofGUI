use profile_core::load_capture;
use rusqlite::Connection;
fn schema(db: &Connection) {
    db.execute_batch("CREATE TABLE rocpd_string(id INTEGER PRIMARY KEY,string TEXT);
    CREATE TABLE rocpd_op(id INTEGER PRIMARY KEY,gpuId INTEGER,queueId INTEGER,start INTEGER,end INTEGER,description_id INTEGER,opType_id INTEGER);
    CREATE TABLE rocpd_api_ops(id INTEGER PRIMARY KEY,api_id INTEGER,op_id INTEGER);
    CREATE TABLE rocpd_kernelapi(api_ptr_id INTEGER PRIMARY KEY,kernelName_id INTEGER,gridX INTEGER,gridY INTEGER,gridZ INTEGER,workgroupX INTEGER,workgroupY INTEGER,workgroupZ INTEGER,groupSegmentSize INTEGER,privateSegmentSize INTEGER);
    INSERT INTO rocpd_string VALUES(1,'KernelExecution'),(2,'CopyDeviceToDevice'),(3,'operation name'),(4,'launch name');
    INSERT INTO rocpd_op VALUES(1,2,3,18000000000000001,18000000000000008,3,1),(2,2,3,18000000000000002,18000000000000020,3,2);
    INSERT INTO rocpd_kernelapi VALUES(10,4,64,1,1,32,1,1,0,128);
    INSERT INTO rocpd_api_ops VALUES(1,10,1);").unwrap();
}
#[test]
fn legacy_import_preserves_gpu_clock_precision_resources_and_read_only_input() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("trace.rpd");
    let db = Connection::open(&path).unwrap();
    schema(&db);
    drop(db);
    let before = std::fs::read(&path).unwrap();
    let c = load_capture(&path).unwrap();
    assert_eq!(c.origin, "18000000000000001");
    assert_eq!(c.dispatches.len(), 1);
    let d = &c.dispatches[0];
    assert_eq!(d.end, 7.);
    assert_eq!(d.kernel, "launch name");
    assert_eq!(d.grid, "64 × 1 × 1");
    assert_eq!(d.lds, Some(0));
    assert_eq!(d.scratch, Some(128));
    assert_eq!(d.vgpr, None);
    assert_eq!(d.agent, "2");
    assert_eq!(before, std::fs::read(&path).unwrap());
    let restored = profile_core::project::load_input(
        &path,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    )
    .unwrap();
    assert_eq!(restored.dispatches[0].end, 7.);
    assert!(restored.input_path.is_some());
}
#[test]
fn duplicate_links_do_not_duplicate_dispatches_but_distinct_launches_remain_ambiguous() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("trace.rpd");
    let db = Connection::open(&path).unwrap();
    schema(&db);
    db.execute_batch("INSERT INTO rocpd_api_ops VALUES(2,10,1);")
        .unwrap();
    let c = load_capture(&path).unwrap();
    assert_eq!(c.dispatches.len(), 1);
    assert_eq!(c.dispatches[0].lds, Some(0));
    db.execute_batch("INSERT INTO rocpd_kernelapi SELECT 11,kernelName_id,gridX,gridY,gridZ,workgroupX,workgroupY,workgroupZ,99,0 FROM rocpd_kernelapi WHERE api_ptr_id=10; INSERT INTO rocpd_api_ops VALUES(3,11,1);").unwrap();
    let c = load_capture(&path).unwrap();
    assert_eq!(c.dispatches.len(), 1);
    assert_eq!(c.dispatches[0].kernel, "operation name");
    assert_eq!(c.dispatches[0].lds, None);
    assert!(c.warnings.iter().any(|s| s.contains("multiple launch")));
}
#[test]
fn operation_only_files_work_and_invalid_gpu_times_fail() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("trace.rpd");
    let db = Connection::open(&path).unwrap();
    schema(&db);
    db.execute_batch("DROP TABLE rocpd_kernelapi; DROP TABLE rocpd_api_ops;")
        .unwrap();
    let c = load_capture(&path).unwrap();
    assert_eq!(c.dispatches[0].kernel, "operation name");
    assert_eq!(c.dispatches[0].lds, None);
    db.execute_batch("UPDATE rocpd_op SET end=start-1 WHERE id=1;")
        .unwrap();
    assert!(load_capture(&path).unwrap_err().contains("interval"));
}

#[test]
fn operation_counters_and_monitor_intervals_preserve_scope_missingness_and_clock() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("measurements.rpd");
    let db = Connection::open(&path).unwrap();
    schema(&db);
    db.execute_batch("CREATE TABLE rocpd_counter(id INTEGER PRIMARY KEY, value REAL, op_id INTEGER, name_id INTEGER);
    INSERT INTO rocpd_string VALUES(5,'COUNTER');
    INSERT INTO rocpd_counter VALUES(1,0,1,5),(2,NULL,1,5),(3,4,2,5);
    CREATE TABLE rocpd_monitor(id INTEGER PRIMARY KEY,deviceType TEXT,deviceId INTEGER,monitorType TEXT,start INTEGER,end INTEGER,value);
    INSERT INTO rocpd_monitor VALUES(1,'gpu',2,'temp',17999999999999996,18000000000000011,'N/A'),(2,'cpu',2,'temp',18000000000000000,18000000000000001,0),(3,'gpu',2,'power',18000000000000001,18000000000000002,NULL);").unwrap();
    let c = load_capture(&path).unwrap();
    assert_eq!(c.origin, "17999999999999996");
    assert_eq!(c.dispatches[0].start, 5.);
    assert_eq!(c.dispatches[0].end, 12.);
    assert_eq!(c.end, 15.);
    assert_eq!(c.event_counters.len(), 3);
    assert_eq!(c.event_counters[0].value, Some(0.));
    assert_eq!(c.event_counters[1].value, None);
    assert_eq!(c.event_counters[0].dispatches, vec!["legacy-op:1"]);
    assert!(c.event_counters[2].dispatches.is_empty());
    assert_eq!(c.event_counters[2].event.as_deref(), Some("legacy-op:2"));
    assert_eq!(c.monitor_intervals[0].value.as_deref(), Some("N/A"));
    assert_eq!(c.monitor_intervals[0].start, 0.);
    assert_eq!(c.monitor_intervals[0].end, 15.);
    assert_eq!(c.monitor_intervals[1].device_type, "cpu");
    assert_eq!(c.monitor_intervals[1].value.as_deref(), Some("0"));
    assert_eq!(c.monitor_intervals[2].value, None);
    assert!(c.counters.is_empty());
    db.execute_batch("UPDATE rocpd_counter SET op_id=999 WHERE id=3;")
        .unwrap();
    assert!(load_capture(&path)
        .unwrap_err()
        .contains("missing operation"));
    db.execute_batch(
        "DELETE FROM rocpd_counter WHERE id=3; UPDATE rocpd_monitor SET end=start-1 WHERE id=1;",
    )
    .unwrap();
    assert!(load_capture(&path)
        .unwrap_err()
        .contains("monitor interval"));
}

#[test]
fn host_calls_rebase_with_gpu_records_and_preserve_recorded_links() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("host.rpd");
    let db = Connection::open(&path).unwrap();
    schema(&db);
    db.execute_batch("CREATE TABLE rocpd_ustring(id INTEGER PRIMARY KEY,string TEXT);
    INSERT INTO rocpd_ustring VALUES(1,'actual arguments');
    INSERT INTO rocpd_string VALUES(5,'hipLaunchKernel'),(6,'different table arguments');
    CREATE TABLE rocpd_api(id INTEGER PRIMARY KEY,pid INTEGER,tid INTEGER,start INTEGER,end INTEGER,apiName_id INTEGER,args_id INTEGER REFERENCES rocpd_ustring(id));
    INSERT INTO rocpd_api VALUES(10,12,13,17999999999999991,17999999999999996,5,1);
    INSERT INTO rocpd_api_ops VALUES(2,10,1),(3,10,2);").unwrap();
    let c = load_capture(&path).unwrap();
    assert_eq!(c.origin, "17999999999999991");
    assert_eq!(c.host_calls.len(), 1);
    let call = &c.host_calls[0];
    assert_eq!(call.name, "hipLaunchKernel");
    assert_eq!(call.arguments.as_deref(), Some("actual arguments"));
    assert_eq!(call.start, 0.);
    assert_eq!(call.end, 5.);
    assert_eq!(call.operations, ["legacy-op:1", "legacy-op:2"]);
    assert_eq!(c.dispatches[0].start, 10.);
    assert_eq!(c.dispatches[0].end, 17.);
    assert_eq!(c.end, 17.);
    db.execute_batch("UPDATE rocpd_api SET end=start-1 WHERE id=10;")
        .unwrap();
    assert!(load_capture(&path)
        .unwrap_err()
        .contains("host-call interval"));
}
