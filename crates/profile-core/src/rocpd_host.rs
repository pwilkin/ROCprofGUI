use super::{columns, err, optional, quoted};
use crate::HostCall;
use rusqlite::Connection;
use std::collections::{BTreeMap, BTreeSet};
type Key = (String, i64, i64, String);
fn key(
    guid: String,
    nid: i64,
    pid: i64,
    event: Option<i64>,
    correlation: Option<i64>,
) -> Option<Key> {
    correlation
        .filter(|id| *id > 0)
        .map(|id| format!("correlation:{id}"))
        .or_else(|| event.map(|id| format!("event:{id}")))
        .map(|id| (guid, nid, pid, id))
}
pub fn load(
    db: &Connection,
    tables: &BTreeSet<String>,
) -> Result<Vec<(i64, i64, HostCall)>, String> {
    let mut calls = Vec::new();
    let mut text_bytes = 0usize;
    let mut associations = 0usize;
    for table in tables
        .iter()
        .filter(|t| t.as_str() == "rocpd_region" || t.starts_with("rocpd_region_"))
    {
        let suffix = table.strip_prefix("rocpd_region").unwrap();
        let strings = format!("rocpd_string{suffix}");
        let events = format!("rocpd_event{suffix}");
        let args = format!("rocpd_arg{suffix}");
        let dispatches = format!("rocpd_kernel_dispatch{suffix}");
        let rc = columns(db, table)?;
        if !["id", "guid", "nid", "pid", "tid", "start", "end", "name_id"]
            .iter()
            .all(|n| rc.contains(*n))
            || !tables.contains(&strings)
        {
            return Err(format!("Unsupported ROCPD region schema: {table}"));
        }
        let event_join = rc.contains("event_id") && tables.contains(&events);
        let ec = if event_join {
            columns(db, &events)?
        } else {
            BTreeSet::new()
        };
        let correlation = optional(&ec, "E", "correlation_id");
        let mut linked: BTreeMap<Key, Vec<String>> = BTreeMap::new();
        if event_join
            && tables.contains(&dispatches)
            && columns(db, &dispatches)?.contains("event_id")
        {
            let sql=format!("SELECT D.id,D.guid,D.nid,D.pid,D.dispatch_id,D.event_id,{correlation} FROM {} D LEFT JOIN {} E ON D.event_id=E.id AND D.guid=E.guid LIMIT 2000001",quoted(&dispatches),quoted(&events));
            let mut statement = db.prepare(&sql).map_err(err)?;
            let mut rows = statement.query([]).map_err(err)?;
            let mut count = 0;
            while let Some(row) = rows.next().map_err(err)? {
                count += 1;
                if count > 2_000_000 {
                    return Err("ROCPD host links exceed two million dispatches".into());
                }
                let id: i64 = row.get(0).map_err(err)?;
                let guid: String = row.get(1).map_err(err)?;
                let nid: i64 = row.get(2).map_err(err)?;
                let pid: i64 = row.get(3).map_err(err)?;
                let dispatch: i64 = row.get(4).map_err(err)?;
                if let Some(k) = key(
                    guid.clone(),
                    nid,
                    pid,
                    row.get(5).map_err(err)?,
                    row.get(6).map_err(err)?,
                ) {
                    linked
                        .entry(k)
                        .or_default()
                        .push(format!("{dispatches}:{guid}:{nid}:{pid}:{dispatch}:{id}"));
                }
            }
        }
        let mut arguments: BTreeMap<(String, i64), Vec<serde_json::Value>> = BTreeMap::new();
        if event_join && tables.contains(&args) {
            let sql=format!("SELECT guid,event_id,position,type,name,value FROM {} ORDER BY guid,event_id,position,id LIMIT 2000001",quoted(&args));
            let mut statement = db.prepare(&sql).map_err(err)?;
            let mut rows = statement.query([]).map_err(err)?;
            let mut count = 0;
            while let Some(row) = rows.next().map_err(err)? {
                count += 1;
                if count > 2_000_000 {
                    return Err("ROCPD exceeds two million host arguments".into());
                }
                let guid: String = row.get(0).map_err(err)?;
                let event: i64 = row.get(1).map_err(err)?;
                let position: i64 = row.get(2).map_err(err)?;
                let kind: String = row.get(3).map_err(err)?;
                let name: String = row.get(4).map_err(err)?;
                let value: Option<String> = row.get(5).map_err(err)?;
                text_bytes = text_bytes.saturating_add(
                    kind.len() + name.len() + value.as_ref().map_or(0, String::len),
                );
                if text_bytes > 64 * 1024 * 1024 {
                    return Err("ROCPD host text exceeds 64 MiB".into());
                }
                arguments.entry((guid, event)).or_default().push(
                    serde_json::json!({"position":position,"type":kind,"name":name,"value":value}),
                );
            }
        }
        let joins = if event_join {
            format!(
                "LEFT JOIN {} E ON R.event_id=E.id AND R.guid=E.guid",
                quoted(&events)
            )
        } else {
            String::new()
        };
        let category = if event_join && ec.contains("category_id") {
            format!(
                "LEFT JOIN {} C ON E.category_id=C.id AND E.guid=C.guid",
                quoted(&strings)
            )
        } else {
            String::new()
        };
        let cat = if category.is_empty() {
            "NULL"
        } else {
            "C.string"
        };
        let corr = if event_join {
            correlation.as_str()
        } else {
            "NULL"
        };
        let event = optional(&rc, "R", "event_id");
        let sql=format!("SELECT R.id,R.guid,R.nid,R.pid,R.tid,R.start,R.end,S.string,{event},{corr},{cat} FROM {} R LEFT JOIN {} S ON R.name_id=S.id AND R.guid=S.guid {joins} {category} ORDER BY R.start,R.id LIMIT 2000001",quoted(table),quoted(&strings));
        let mut statement = db.prepare(&sql).map_err(err)?;
        let mut rows = statement.query([]).map_err(err)?;
        while let Some(row) = rows.next().map_err(err)? {
            if calls.len() >= 2_000_000 {
                return Err("ROCPD exceeds two million CPU regions".into());
            }
            let id: i64 = row.get(0).map_err(err)?;
            let guid: String = row.get(1).map_err(err)?;
            let nid: i64 = row.get(2).map_err(err)?;
            let pid: i64 = row.get(3).map_err(err)?;
            let tid: i64 = row.get(4).map_err(err)?;
            let start: i64 = row.get(5).map_err(err)?;
            let end: i64 = row.get(6).map_err(err)?;
            if start < 0 || end < start {
                return Err("Invalid ROCPD CPU region interval".into());
            }
            let name: String = row
                .get(7)
                .map_err(|_| "ROCPD region references a missing name")?;
            let event: Option<i64> = row.get(8).map_err(err)?;
            let correlation: Option<i64> = row.get(9).map_err(err)?;
            let args = event
                .and_then(|event| arguments.get(&(guid.clone(), event)))
                .map(serde_json::to_string_pretty)
                .transpose()
                .map_err(err)?;
            text_bytes =
                text_bytes.saturating_add(name.len() + args.as_ref().map_or(0, String::len));
            if text_bytes > 64 * 1024 * 1024 {
                return Err("ROCPD host text exceeds 64 MiB".into());
            }
            let operations = key(guid.clone(), nid, pid, event, correlation)
                .and_then(|k| linked.get(&k))
                .cloned()
                .unwrap_or_default();
            associations += operations.len();
            if associations > 2_000_000 {
                return Err("ROCPD exceeds two million host-dispatch associations".into());
            }
            calls.push((
                start,
                end,
                HostCall {
                    id: format!("{table}:{guid}:{nid}:{pid}:{tid}:{id}"),
                    scope: format!("{table} · GUID {guid} · node {nid}"),
                    category: row.get(10).map_err(err)?,
                    link_basis: Some(
                        if correlation.is_some_and(|id| id > 0) {
                            "Recorded correlation ID within GUID/node/process"
                        } else {
                            "Shared recorded event within GUID/node/process"
                        }
                        .into(),
                    ),
                    pid: pid.to_string(),
                    tid: tid.to_string(),
                    name,
                    arguments: args,
                    start: 0.,
                    end: 0.,
                    operations,
                },
            ));
        }
    }
    calls.sort_by_key(|(start, _, _)| *start);
    Ok(calls)
}
