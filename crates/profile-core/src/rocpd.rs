#[path = "rocpd_host.rs"]
mod host;
#[path = "rocpd_legacy.rs"]
mod legacy;
#[path = "rocpd_objects.rs"]
mod objects;
use crate::{Capture, Dispatch, Series};
use rusqlite::{Connection, OpenFlags};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}
fn columns(db: &Connection, table: &str) -> Result<BTreeSet<String>, String> {
    let mut query = db
        .prepare(&format!("PRAGMA table_info({})", quoted(table)))
        .map_err(err)?;
    let result = query
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err);
    result
}
fn optional(cols: &BTreeSet<String>, alias: &str, name: &str) -> String {
    if cols.contains(name) {
        format!("{alias}.{}", quoted(name))
    } else {
        "NULL".into()
    }
}
pub fn load(path: &Path) -> Result<Capture, String> {
    load_cancellable(path, Arc::new(AtomicBool::new(false)))
}
pub fn load_cancellable(path: &Path, cancelled: Arc<AtomicBool>) -> Result<Capture, String> {
    if cancelled.load(Ordering::Relaxed) {
        return Err("ROCPD import cancelled".into());
    }
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(err)?;
    let started = Instant::now();
    db.progress_handler(
        10_000,
        Some(move || {
            cancelled.load(Ordering::Relaxed) || started.elapsed() > Duration::from_secs(300)
        }),
    );
    db.busy_timeout(Duration::from_secs(2)).map_err(err)?;
    db.execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF;")
        .map_err(err)?;
    let tables: BTreeSet<String> = db
        .prepare("SELECT name FROM sqlite_schema WHERE type='table'")
        .map_err(err)?
        .query_map([], |r| r.get(0))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let dispatch_tables: Vec<_> = tables
        .iter()
        .filter(|name| {
            name.as_str() == "rocpd_kernel_dispatch" || name.starts_with("rocpd_kernel_dispatch_")
        })
        .collect();
    if dispatch_tables.is_empty() && tables.contains("rocpd_op") && tables.contains("rocpd_string")
    {
        return legacy::load(&db, path, &tables);
    }
    if dispatch_tables.is_empty() {
        return Err("Unsupported ROCPD schema: no ROCm kernel-dispatch tables found".into());
    }
    let mut capture = Capture::empty(
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
        "ROCPD",
        "ns",
    );
    capture.root = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut records = vec![];
    let mut origin = i64::MAX;
    for table in &dispatch_tables {
        let suffix = table.strip_prefix("rocpd_kernel_dispatch").unwrap();
        let symbol = format!("rocpd_info_kernel_symbol{suffix}");
        if !tables.contains(&symbol) {
            return Err(format!("Missing ROCPD kernel-symbol table {symbol}"));
        }
        let dc = columns(&db, table)?;
        let sc = columns(&db, &symbol)?;
        for column in [
            "id",
            "guid",
            "nid",
            "pid",
            "agent_id",
            "kernel_id",
            "dispatch_id",
            "queue_id",
            "start",
            "end",
            "grid_size_x",
            "grid_size_y",
            "grid_size_z",
            "workgroup_size_x",
            "workgroup_size_y",
            "workgroup_size_z",
        ] {
            if !dc.contains(column) {
                return Err(format!(
                    "Unsupported ROCPD dispatch schema: missing {column}"
                ));
            }
        }
        for column in ["id", "guid", "kernel_name"] {
            if !sc.contains(column) {
                return Err(format!("Unsupported ROCPD symbol schema: missing {column}"));
            }
        }
        let sql=format!("SELECT D.id,D.guid,D.nid,D.pid,D.dispatch_id,D.agent_id,D.queue_id,D.start,D.end,COALESCE({},S.kernel_name),{},{},{},{},D.grid_size_x,D.grid_size_y,D.grid_size_z,D.workgroup_size_x,D.workgroup_size_y,D.workgroup_size_z,S.id FROM {} D LEFT JOIN {} S ON D.kernel_id=S.id AND D.guid=S.guid ORDER BY D.start,D.id LIMIT 2000001",
            optional(&sc,"S","display_name"),optional(&sc,"S","arch_vgpr_count"),optional(&sc,"S","sgpr_count"),optional(&dc,"D","group_segment_size"),optional(&dc,"D","private_segment_size"),quoted(table),quoted(&symbol));
        let mut statement = db.prepare(&sql).map_err(err)?;
        let mut rows = statement.query([]).map_err(err)?;
        while let Some(row) = rows.next().map_err(err)? {
            if records.len() >= 2_000_000 {
                return Err("ROCPD exceeds two million dispatches".into());
            }
            if row.get::<_, Option<i64>>(20).map_err(err)?.is_none() {
                return Err("ROCPD dispatch references a missing kernel symbol".into());
            }
            let start: i64 = row.get(7).map_err(err)?;
            let end: i64 = row.get(8).map_err(err)?;
            if start < 0 || end < start {
                return Err("Invalid ROCPD dispatch timestamp interval".into());
            }
            origin = origin.min(start);
            let resource = |column| -> Result<Option<u64>, String> {
                row.get::<_, Option<i64>>(column)
                    .map_err(err)?
                    .map(|v| u64::try_from(v).map_err(|_| "Negative ROCPD resource count".into()))
                    .transpose()
            };
            let dimensions = |first| -> Result<String, String> {
                Ok(format!(
                    "{} × {} × {}",
                    resource(first)?.ok_or("Missing launch dimension")?,
                    resource(first + 1)?.ok_or("Missing launch dimension")?,
                    resource(first + 2)?.ok_or("Missing launch dimension")?
                ))
            };
            let dispatch = Dispatch {
                shader_engine: None,
                id: format!(
                    "{}:{}:{}:{}:{}:{}",
                    table,
                    row.get::<_, String>(1).map_err(err)?,
                    row.get::<_, i64>(2).map_err(err)?,
                    row.get::<_, i64>(3).map_err(err)?,
                    row.get::<_, i64>(4).map_err(err)?,
                    row.get::<_, i64>(0).map_err(err)?
                ),
                kernel: row.get(9).map_err(err)?,
                start: 0.,
                end: 0.,
                agent: format!(
                    "{}:{}",
                    row.get::<_, String>(1).map_err(err)?,
                    row.get::<_, i64>(5).map_err(err)?
                ),
                queue: row.get::<_, i64>(6).map_err(err)?.to_string(),
                vgpr: resource(10)?,
                sgpr: resource(11)?,
                lds: resource(12)?,
                scratch: resource(13)?,
                grid: dimensions(14)?,
                workgroup: dimensions(17)?,
            };
            records.push((start, end, dispatch));
        }
    }
    let mut sampled: BTreeMap<(String, String, String), Vec<(i64, f64)>> = BTreeMap::new();
    let mut samples = 0;
    for dispatch_table in &dispatch_tables {
        let suffix = dispatch_table
            .strip_prefix("rocpd_kernel_dispatch")
            .unwrap();
        let events = format!("rocpd_pmc_event{suffix}");
        let info = format!("rocpd_info_pmc{suffix}");
        let sample = format!("rocpd_sample{suffix}");
        if !tables.contains(&events) || !tables.contains(&info) {
            continue;
        }
        let cols = columns(&db, &events)?;
        read_event_counters(
            &db,
            &tables,
            dispatch_table,
            &events,
            &info,
            &cols,
            &mut capture,
        )?;
        if !["sample_id", "xcc", "shader_engine", "instance"]
            .iter()
            .all(|n| cols.contains(*n))
            || !tables.contains(&sample)
        {
            capture.warnings.push(format!("{events}: timestamped SPM schema unavailable; event-associated values are imported separately where supported."));
            continue;
        }
        let sql=format!("SELECT I.name,COALESCE(NULLIF(I.units,''),'unspecified'),I.guid,I.pid,I.agent_id,E.xcc,E.shader_engine,E.instance,S.timestamp,E.value,I.id,S.id FROM {} E LEFT JOIN {} I ON E.pmc_id=I.id AND E.guid=I.guid LEFT JOIN {} S ON E.sample_id=S.id AND E.guid=S.guid WHERE E.sample_id IS NOT NULL LIMIT 2000001",quoted(&events),quoted(&info),quoted(&sample));
        let mut statement = db.prepare(&sql).map_err(err)?;
        let mut rows = statement.query([]).map_err(err)?;
        while let Some(row) = rows.next().map_err(err)? {
            samples += 1;
            if samples > 2_000_000 {
                return Err("ROCPD exceeds two million counter samples".into());
            }
            if row.get::<_, Option<i64>>(10).map_err(err)?.is_none()
                || row.get::<_, Option<i64>>(11).map_err(err)?.is_none()
            {
                return Err(
                    "ROCPD sampled counter references a missing definition or sample".into(),
                );
            }
            let time: i64 = row.get(8).map_err(err)?;
            let value: f64 = row.get(9).map_err(err)?;
            if time < 0 || !value.is_finite() {
                return Err("Invalid ROCPD counter timestamp/value".into());
            }
            origin = origin.min(time);
            let mut dimensions = vec![];
            for (column, name) in [
                (5, "DIMENSION_XCC"),
                (6, "DIMENSION_SHADER_ENGINE"),
                (7, "DIMENSION_INSTANCE"),
            ] {
                let index: i64 = row.get(column).map_err(err)?;
                if index < 0 {
                    return Err("Negative ROCPD counter dimension".into());
                }
                dimensions.push(serde_json::json!({"dimension_name":name,"index":index}));
            }
            let scope = format!(
                "process {} · agent {}:{} · {}",
                row.get::<_, i64>(3).map_err(err)?,
                row.get::<_, String>(2).map_err(err)?,
                row.get::<_, i64>(4).map_err(err)?,
                serde_json::to_string(&dimensions).map_err(err)?
            );
            sampled
                .entry((
                    row.get::<_, String>(0).map_err(err)?,
                    scope,
                    row.get::<_, String>(1).map_err(err)?,
                ))
                .or_default()
                .push((time, value));
        }
    }
    if records.is_empty() && sampled.is_empty() && capture.event_counters.is_empty() {
        return Err("ROCPD contains no supported dispatch or sampled counter records".into());
    }
    let host_calls = host::load(&db, &tables)?;
    for (start, _, _) in &host_calls {
        origin = origin.min(*start);
    }
    if origin == i64::MAX {
        origin = 0;
    }
    capture.origin = origin.to_string();
    for (start, end, mut call) in host_calls {
        if end - origin > 9_007_199_254_740_991 {
            return Err("ROCPD host-call span exceeds exact frontend timestamp precision".into());
        }
        call.start = (start - origin) as f64;
        call.end = (end - origin) as f64;
        capture.end = capture.end.max(call.end);
        capture.host_calls.push(call);
    }
    for ((name, scope, unit), mut points) in sampled {
        points.sort_by_key(|p| p.0);
        if points.last().unwrap().0 - origin > 9_007_199_254_740_991 {
            return Err("ROCPD counter span exceeds exact frontend timestamp precision".into());
        }
        let points: Vec<_> = points
            .into_iter()
            .map(|(t, v)| [(t - origin) as f64, v])
            .collect();
        capture.end = capture.end.max(points.last().unwrap()[0]);
        capture.counters.push(Series {
            spm_clock: None,
            definition: None,
            att: None,
            name,
            scope,
            unit,
            points,
        });
    }
    for (start, end, mut dispatch) in records {
        let relative_end = end - origin;
        if relative_end > 9_007_199_254_740_991 {
            return Err("ROCPD time span exceeds exact frontend timestamp precision".into());
        }
        dispatch.start = (start - origin) as f64;
        dispatch.end = relative_end as f64;
        capture.end = capture.end.max(dispatch.end);
        capture.dispatches.push(dispatch);
    }
    capture
        .dispatches
        .sort_by(|a, b| a.start.total_cmp(&b.start));
    capture.warnings.push("ROCPD kernel dispatch records do not prove graph replay coverage. No detailed SQTT tokens were inferred from dispatch rows.".into());
    capture.metadata.insert(
        "rocpdSchemaAdapter".into(),
        "ROCm SDK dispatch/symbol schema (3.x column contract)".into(),
    );
    capture.metadata.insert("readOnly".into(), "true".into());
    objects::read(&db, &tables, &mut capture)?;
    Ok(capture)
}

fn read_event_counters(
    db: &Connection,
    tables: &BTreeSet<String>,
    dispatch_table: &str,
    events: &str,
    info: &str,
    cols: &BTreeSet<String>,
    capture: &mut Capture,
) -> Result<(), String> {
    if !["id", "guid", "event_id", "pmc_id", "value"]
        .iter()
        .all(|n| cols.contains(*n))
    {
        capture.warnings.push(format!(
            "{events}: unsupported event-counter column contract"
        ));
        return Ok(());
    }
    let suffix = dispatch_table
        .strip_prefix("rocpd_kernel_dispatch")
        .unwrap();
    let event_table = format!("rocpd_event{suffix}");
    if !tables.contains(&event_table) {
        capture.warnings.push(format!(
            "{events}: event-counter associations unavailable without {event_table}"
        ));
        return Ok(());
    }
    let dc = columns(db, dispatch_table)?;
    let mut links: BTreeMap<(String, i64), Vec<String>> = BTreeMap::new();
    if dc.contains("event_id") {
        let mut statement = db.prepare(&format!("SELECT id,guid,nid,pid,dispatch_id,event_id FROM {} WHERE event_id IS NOT NULL LIMIT 2000001", quoted(dispatch_table))).map_err(err)?;
        let mut rows = statement.query([]).map_err(err)?;
        let mut count = 0;
        while let Some(row) = rows.next().map_err(err)? {
            count += 1;
            if count > 2_000_000 {
                return Err("Too many event-dispatch associations".into());
            }
            let guid: String = row.get(1).map_err(err)?;
            let id = format!(
                "{}:{}:{}:{}:{}:{}",
                dispatch_table,
                guid,
                row.get::<_, i64>(2).map_err(err)?,
                row.get::<_, i64>(3).map_err(err)?,
                row.get::<_, i64>(4).map_err(err)?,
                row.get::<_, i64>(0).map_err(err)?
            );
            links
                .entry((guid, row.get(5).map_err(err)?))
                .or_default()
                .push(id);
        }
    }
    let ic = columns(db, info)?;
    let filter = if cols.contains("sample_id") {
        "WHERE E.sample_id IS NULL"
    } else {
        ""
    };
    let sql = format!("SELECT E.id,E.guid,E.event_id,I.name,COALESCE(NULLIF({},''),'unspecified'),{},{},E.value,I.id,V.id FROM {} E LEFT JOIN {} I ON E.pmc_id=I.id AND E.guid=I.guid LEFT JOIN {} V ON E.event_id=V.id AND E.guid=V.guid {} ORDER BY E.id LIMIT 2000001", optional(&ic,"I","units"), optional(&ic,"I","pid"), optional(&ic,"I","agent_id"), quoted(events), quoted(info), quoted(&event_table), filter);
    let mut associations: usize = capture
        .event_counters
        .iter()
        .map(|r| r.dispatches.len())
        .sum();
    let mut statement = db.prepare(&sql).map_err(err)?;
    let mut rows = statement.query([]).map_err(err)?;
    while let Some(row) = rows.next().map_err(err)? {
        if capture.event_counters.len() >= 2_000_000 {
            return Err("ROCPD exceeds two million event counters".into());
        }
        let event: Option<i64> = row.get(2).map_err(err)?;
        if row.get::<_, Option<i64>>(8).map_err(err)?.is_none()
            || (event.is_some() && row.get::<_, Option<i64>>(9).map_err(err)?.is_none())
        {
            return Err("ROCPD event counter references a missing definition or event".into());
        }
        let guid: String = row.get(1).map_err(err)?;
        let value: Option<f64> = row.get(7).map_err(err)?;
        if value.is_some_and(|v| !v.is_finite()) {
            return Err("Nonfinite ROCPD event counter".into());
        }
        let unknown = |index| -> Result<String, String> {
            Ok(row
                .get::<_, Option<i64>>(index)
                .map_err(err)?
                .map(|n| n.to_string())
                .unwrap_or("unknown".into()))
        };
        let dispatch_links = event.and_then(|id| links.get(&(guid.clone(), id)));
        associations += dispatch_links.map_or(0, |v| v.len());
        if associations > 2_000_000 {
            return Err("ROCPD exceeds two million counter-dispatch associations".into());
        }
        capture.event_counters.push(crate::model::EventCounter {
            id: format!("{}:{}:{}", events, guid, row.get::<_, i64>(0).map_err(err)?),
            counter_id: format!("{}:{}:{}", info, guid, row.get::<_, i64>(8).map_err(err)?),
            event: event.map(|id| format!("{}:{}:{}", event_table, guid, id)),
            dispatches: dispatch_links.cloned().unwrap_or_default(),
            name: row.get(3).map_err(err)?,
            unit: row.get(4).map_err(err)?,
            scope: format!("process {} · agent {}:{}", unknown(5)?, guid, unknown(6)?),
            value,
        });
    }
    Ok(())
}
