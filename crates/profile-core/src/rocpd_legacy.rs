use super::{columns, err};
use crate::{Capture, Dispatch};
use rusqlite::Connection;
use std::{collections::BTreeSet, path::Path};
pub fn load(db: &Connection, path: &Path, tables: &BTreeSet<String>) -> Result<Capture, String> {
    for (table, required) in [
        (
            "rocpd_op",
            vec![
                "id",
                "gpuId",
                "queueId",
                "start",
                "end",
                "description_id",
                "opType_id",
            ],
        ),
        ("rocpd_string", vec!["id", "string"]),
    ] {
        let cols = columns(db, table)?;
        if !required.iter().all(|c| cols.contains(*c)) {
            return Err(format!("Unsupported legacy ROCPD table {table}"));
        }
    }
    let linked = tables.contains("rocpd_kernelapi") && tables.contains("rocpd_api_ops");
    let (cte, join, fields, filter) = if linked {
        for (table, required) in [
            ("rocpd_api_ops", vec!["op_id", "api_id"]),
            (
                "rocpd_kernelapi",
                vec![
                    "api_ptr_id",
                    "kernelName_id",
                    "gridX",
                    "gridY",
                    "gridZ",
                    "workgroupX",
                    "workgroupY",
                    "workgroupZ",
                    "groupSegmentSize",
                    "privateSegmentSize",
                ],
            ),
        ] {
            let cols = columns(db, table)?;
            if !required.iter().all(|c| cols.contains(*c)) {
                return Err(format!("Unsupported legacy ROCPD table {table}"));
            }
        }
        ("WITH K AS (SELECT A.op_id, COUNT(DISTINCT B.api_ptr_id) AS count, MAX(S.string) AS name, MAX(B.gridX) AS gx,MAX(B.gridY) AS gy,MAX(B.gridZ) AS gz,MAX(B.workgroupX) AS wx,MAX(B.workgroupY) AS wy,MAX(B.workgroupZ) AS wz,MAX(B.groupSegmentSize) AS lds,MAX(B.privateSegmentSize) AS scratch FROM rocpd_api_ops A JOIN rocpd_kernelapi B ON A.api_id=B.api_ptr_id LEFT JOIN rocpd_string S ON S.id=B.kernelName_id GROUP BY A.op_id)",
        "LEFT JOIN K ON K.op_id=O.id","COALESCE(K.count,0),K.name,K.gx,K.gy,K.gz,K.wx,K.wy,K.wz,K.lds,K.scratch",
        "T.string='KernelExecution'")
    } else {
        (
            "",
            "",
            "0,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL",
            "T.string='KernelExecution'",
        )
    };
    let sql=format!("{cte} SELECT O.id,O.gpuId,O.queueId,O.start,O.end,D.string,{fields} FROM rocpd_op O LEFT JOIN rocpd_string D ON D.id=O.description_id LEFT JOIN rocpd_string T ON T.id=O.opType_id {join} WHERE {filter} ORDER BY O.start,O.id LIMIT 2000001");
    let mut statement = db.prepare(&sql).map_err(err)?;
    let mut rows = statement.query([]).map_err(err)?;
    let mut c = Capture::empty(
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
        "ROCPD",
        "ns",
    );
    c.root = path.parent().unwrap_or(Path::new(".")).into();
    let mut origin = None;
    let mut ids = BTreeSet::new();
    let mut ambiguous = 0;
    while let Some(row) = rows.next().map_err(err)? {
        if c.dispatches.len() >= 2_000_000 {
            return Err("Legacy ROCPD exceeds two million dispatches".into());
        }
        let id: i64 = row.get(0).map_err(err)?;
        if !ids.insert(id) {
            return Err("Duplicate legacy ROCPD operation identity".into());
        }
        let start: i64 = row.get(3).map_err(err)?;
        let end: i64 = row.get(4).map_err(err)?;
        if start < 0 || end < start {
            return Err("Invalid legacy ROCPD operation interval".into());
        }
        let base = *origin.get_or_insert(start);
        if end - base > 9_007_199_254_740_991 {
            return Err("Legacy ROCPD time span exceeds exact frontend timestamp precision".into());
        }
        let count: i64 = row.get(6).map_err(err)?;
        if count > 1 {
            ambiguous += 1;
        }
        let unique = count == 1;
        let number = |column| -> Result<Option<u64>, String> {
            if !unique {
                return Ok(None);
            }
            row.get::<_, Option<i64>>(column)
                .map_err(err)?
                .map(|n| {
                    u64::try_from(n).map_err(|_| "Negative legacy ROCPD launch resource".into())
                })
                .transpose()
        };
        let dimension = |first| -> Result<String, String> {
            match (number(first)?, number(first + 1)?, number(first + 2)?) {
                (Some(x), Some(y), Some(z)) => Ok(format!("{x} × {y} × {z}")),
                _ => Ok("Not recorded or ambiguous".into()),
            }
        };
        let description: Option<String> = row.get(5).map_err(err)?;
        let kernel = if unique {
            row.get::<_, Option<String>>(7)
                .map_err(err)?
                .or(description)
        } else {
            description
        }
        .ok_or("Legacy ROCPD kernel name references a missing string")?;
        c.end = c.end.max((end - base) as f64);
        c.dispatches.push(Dispatch {
            shader_engine: None,
            id: format!("legacy-op:{id}"),
            kernel,
            start: (start - base) as f64,
            end: (end - base) as f64,
            agent: row.get::<_, i64>(1).map_err(err)?.to_string(),
            queue: row.get::<_, i64>(2).map_err(err)?.to_string(),
            grid: dimension(8)?,
            workgroup: dimension(11)?,
            lds: number(14)?,
            scratch: number(15)?,
            vgpr: None,
            sgpr: None,
        });
    }
    import_measurements(db, tables, &mut c, origin)?;
    c.metadata.insert(
        "rocpdSchemaAdapter".into(),
        "Legacy rocmProfileData operation/string/kernelapi contract".into(),
    );
    c.metadata.insert("readOnly".into(), "true".into());
    if ambiguous > 0 {
        c.warnings.push(format!("{ambiguous} kernel operations have multiple launch API associations; launch dimensions and resources are unavailable."));
    }
    c.warnings.push("Legacy ROCPD GPU operation timestamps are imported as dispatch intervals. CPU API durations are not GPU execution times. SQTT instructions, resource pressure and graph replay coverage are not inferred.".into());
    Ok(c)
}

fn import_measurements(
    db: &Connection,
    tables: &BTreeSet<String>,
    c: &mut Capture,
    origin: Option<i64>,
) -> Result<(), String> {
    if tables.contains("rocpd_counter") {
        let mut statement = db.prepare("SELECT C.id,C.name_id,C.op_id,C.value,S.string,O.gpuId,O.queueId,O.id FROM rocpd_counter C LEFT JOIN rocpd_string S ON C.name_id=S.id LEFT JOIN rocpd_op O ON C.op_id=O.id ORDER BY C.id LIMIT 2000001").map_err(err)?;
        let mut rows = statement.query([]).map_err(err)?;
        let dispatches: BTreeSet<_> = c.dispatches.iter().map(|d| d.id.clone()).collect();
        while let Some(row) = rows.next().map_err(err)? {
            if c.event_counters.len() >= 2_000_000 {
                return Err("Legacy ROCPD exceeds two million operation counters".into());
            }
            if row.get::<_, Option<i64>>(7).map_err(err)?.is_none() {
                return Err("Legacy counter references a missing operation".into());
            }
            let id: i64 = row.get(0).map_err(err)?;
            let name_id: i64 = row.get(1).map_err(err)?;
            let op: i64 = row.get(2).map_err(err)?;
            let value: Option<f64> = row.get(3).map_err(err)?;
            if value.is_some_and(|v| !v.is_finite()) {
                return Err("Invalid legacy counter value".into());
            }
            let name: String = row
                .get(4)
                .map_err(|_| "Legacy counter references a missing name")?;
            let gpu: i64 = row.get(5).map_err(err)?;
            let queue: i64 = row.get(6).map_err(err)?;
            let event = format!("legacy-op:{op}");
            c.event_counters.push(crate::EventCounter {
                id: format!("legacy-counter:{id}"),
                counter_id: format!("legacy-name:{name_id}:gpu:{gpu}:queue:{queue}"),
                dispatches: if dispatches.contains(&event) {
                    vec![event.clone()]
                } else {
                    vec![]
                },
                event: Some(event),
                name,
                unit: "unspecified".into(),
                scope: format!("GPU {gpu} · queue {queue}"),
                value,
            });
        }
    }
    let mut monitors = Vec::new();
    let mut base = origin;
    if tables.contains("rocpd_monitor") {
        let mut statement=db.prepare("SELECT id,deviceType,deviceId,monitorType,start,end,CAST(value AS TEXT),typeof(value) FROM rocpd_monitor ORDER BY start,id LIMIT 2000001").map_err(err)?;
        let mut rows = statement.query([]).map_err(err)?;
        while let Some(row) = rows.next().map_err(err)? {
            if monitors.len() >= 2_000_000 {
                return Err("Legacy ROCPD exceeds two million monitor intervals".into());
            }
            let start: i64 = row.get(4).map_err(err)?;
            let end: i64 = row.get(5).map_err(err)?;
            if start < 0 || end < start {
                return Err("Invalid legacy monitor interval".into());
            }
            let kind: String = row.get(7).map_err(err)?;
            if kind == "blob" {
                return Err("Legacy monitor value must be text or numeric".into());
            }
            let value = row.get::<_, Option<String>>(6).map_err(err)?;
            if value
                .as_ref()
                .is_some_and(|v| v.len() > 8192 || v.contains('\0'))
            {
                return Err("Invalid legacy monitor value".into());
            }
            base = Some(base.map_or(start, |b| b.min(start)));
            monitors.push((
                start,
                end,
                crate::MonitorInterval {
                    id: format!("legacy-monitor:{}", row.get::<_, i64>(0).map_err(err)?),
                    device_type: row.get(1).map_err(err)?,
                    device_id: row.get::<_, i64>(2).map_err(err)?.to_string(),
                    name: row.get(3).map_err(err)?,
                    start: 0.,
                    end: 0.,
                    value,
                    unit: "unspecified".into(),
                },
            ));
        }
    }
    let calls = read_host_calls(db, tables)?;
    for (start, _, _) in &calls {
        base = Some(base.map_or(*start, |b| b.min(*start)));
    }
    let base = base.unwrap_or(0);
    let shift = origin.unwrap_or(base) - base;
    if shift > 9_007_199_254_740_991 {
        return Err("Legacy ROCPD time span exceeds exact frontend timestamp precision".into());
    }
    for d in &mut c.dispatches {
        if d.end + shift as f64 > 9_007_199_254_740_991. {
            return Err("Legacy ROCPD time span exceeds exact frontend timestamp precision".into());
        }
        d.start += shift as f64;
        d.end += shift as f64;
    }
    c.end = (c.end + shift as f64).max(1.);
    for (start, end, mut m) in monitors {
        if end - base > 9_007_199_254_740_991 {
            return Err("Legacy monitor span exceeds exact frontend timestamp precision".into());
        }
        m.start = (start - base) as f64;
        m.end = (end - base) as f64;
        c.end = c.end.max(m.end);
        c.monitor_intervals.push(m);
    }
    for (start, end, mut call) in calls {
        if end - base > 9_007_199_254_740_991 {
            return Err("Legacy host-call span exceeds exact frontend timestamp precision".into());
        }
        call.start = (start - base) as f64;
        call.end = (end - base) as f64;
        c.end = c.end.max(call.end);
        c.host_calls.push(call);
    }
    c.origin = base.to_string();
    if !c.monitor_intervals.is_empty() {
        c.warnings.push("Legacy monitor values retain recorded intervals and text; units are unspecified by this schema. No interpolation or normalization was applied.".into());
    }
    Ok(())
}

fn read_host_calls(
    db: &Connection,
    tables: &BTreeSet<String>,
) -> Result<Vec<(i64, i64, crate::HostCall)>, String> {
    if !tables.contains("rocpd_api") {
        return Ok(vec![]);
    }
    let cols = columns(db, "rocpd_api")?;
    if !["id", "pid", "tid", "start", "end", "apiName_id"]
        .iter()
        .all(|n| cols.contains(*n))
    {
        return Err("Unsupported legacy host API schema".into());
    }
    let mut args_table = None;
    if cols.contains("args_id") {
        let mut fk = db
            .prepare("PRAGMA foreign_key_list(rocpd_api)")
            .map_err(err)?;
        let links = fk
            .query_map([], |r| Ok((r.get::<_, String>(2)?, r.get::<_, String>(3)?)))
            .map_err(err)?;
        for link in links {
            let (table, from) = link.map_err(err)?;
            if from == "args_id"
                && ["rocpd_string", "rocpd_ustring"].contains(&table.as_str())
                && tables.contains(&table)
            {
                args_table = Some(table);
            }
        }
        if args_table.is_none() && !tables.contains("rocpd_ustring") {
            args_table = Some("rocpd_string".into());
        }
    }
    let join = args_table
        .as_ref()
        .map(|t| format!("LEFT JOIN {t} X ON A.args_id=X.id"))
        .unwrap_or_default();
    let args = if args_table.is_some() {
        "X.string"
    } else {
        "NULL"
    };
    let sql=format!("SELECT A.id,A.pid,A.tid,A.start,A.end,S.string,{args} FROM rocpd_api A LEFT JOIN rocpd_string S ON A.apiName_id=S.id {join} ORDER BY A.start,A.id LIMIT 2000001");
    let mut statement = db.prepare(&sql).map_err(err)?;
    let mut rows = statement.query([]).map_err(err)?;
    let mut result = Vec::new();
    let mut ids = std::collections::BTreeMap::new();
    let mut bytes = 0usize;
    while let Some(row) = rows.next().map_err(err)? {
        if result.len() >= 2_000_000 {
            return Err("Legacy ROCPD exceeds two million host calls".into());
        }
        let id: i64 = row.get(0).map_err(err)?;
        let start: i64 = row.get(3).map_err(err)?;
        let end: i64 = row.get(4).map_err(err)?;
        if start < 0 || end < start {
            return Err("Invalid legacy host-call interval".into());
        }
        let name: String = row
            .get(5)
            .map_err(|_| "Legacy host call references a missing API name")?;
        let arguments: Option<String> = row.get(6).map_err(err)?;
        bytes = bytes
            .saturating_add(name.len())
            .saturating_add(arguments.as_ref().map_or(0, String::len));
        if name.len() > 8192
            || arguments.as_ref().is_some_and(|s| s.len() > 65536)
            || bytes > 64 * 1024 * 1024
        {
            return Err("Legacy host-call text exceeds import budget".into());
        }
        if ids.insert(id, result.len()).is_some() {
            return Err("Duplicate legacy host-call identity".into());
        }
        result.push((
            start,
            end,
            crate::HostCall {
                id: format!("legacy-api:{id}"),
                scope: "legacy".into(),
                category: None,
                link_basis: Some("Recorded API-operation links".into()),
                pid: row.get::<_, i64>(1).map_err(err)?.to_string(),
                tid: row.get::<_, i64>(2).map_err(err)?.to_string(),
                name,
                arguments,
                start: 0.,
                end: 0.,
                operations: vec![],
            },
        ));
    }
    if tables.contains("rocpd_api_ops") {
        let mut statement=db.prepare("SELECT DISTINCT A.api_id,A.op_id,O.id FROM rocpd_api_ops A LEFT JOIN rocpd_op O ON A.op_id=O.id ORDER BY A.api_id,A.op_id LIMIT 2000001").map_err(err)?;
        let mut rows = statement.query([]).map_err(err)?;
        let mut count = 0;
        while let Some(row) = rows.next().map_err(err)? {
            count += 1;
            if count > 2_000_000 {
                return Err("Legacy ROCPD exceeds two million API-operation links".into());
            }
            let api: i64 = row.get(0).map_err(err)?;
            let op: i64 = row.get(1).map_err(err)?;
            if row.get::<_, Option<i64>>(2).map_err(err)?.is_none() {
                return Err("Legacy API link references a missing operation".into());
            }
            let index = *ids
                .get(&api)
                .ok_or("Legacy API link references a missing host call")?;
            result[index].2.operations.push(format!("legacy-op:{op}"));
        }
    }
    Ok(result)
}
