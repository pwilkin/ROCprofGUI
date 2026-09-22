use super::{columns, err, optional, quoted};
use crate::{Capture, CodeObjectRecord};
use rusqlite::Connection;
use std::collections::BTreeSet;
pub fn read(
    db: &Connection,
    tables: &BTreeSet<String>,
    capture: &mut Capture,
) -> Result<(), String> {
    for table in tables.iter().filter(|name| {
        name.as_str() == "rocpd_info_code_object" || name.starts_with("rocpd_info_code_object_")
    }) {
        let cols = columns(db, table)?;
        if !["id", "guid", "nid", "pid"]
            .iter()
            .all(|name| cols.contains(*name))
        {
            capture
                .warnings
                .push(format!("{table}: unsupported code-object column contract"));
            continue;
        }
        let fields = [
            "id",
            "guid",
            "nid",
            "pid",
            "agent_id",
            "uri",
            "storage_type",
            "load_base",
            "load_size",
            "load_delta",
        ]
        .map(|name| format!("CAST({} AS TEXT)", optional(&cols, "O", name)))
        .join(",");
        let mut statement = db
            .prepare(&format!(
                "SELECT {fields} FROM {} O ORDER BY O.guid,O.nid,O.pid,O.id LIMIT 100001",
                quoted(table)
            ))
            .map_err(err)?;
        let mut rows = statement.query([]).map_err(err)?;
        while let Some(row) = rows.next().map_err(err)? {
            if capture.code_objects.len() >= 100000 {
                return Err("ROCPD code-object catalog exceeds 100,000 records".into());
            }
            let object_id: String = row.get(0).map_err(err)?;
            let guid: String = row.get(1).map_err(err)?;
            let node: String = row.get(2).map_err(err)?;
            let process: String = row.get(3).map_err(err)?;
            let id =
                serde_json::to_string(&(table, &guid, &node, &process, &object_id)).map_err(err)?;
            capture.code_objects.push(CodeObjectRecord {
                id,
                object_id,
                scope: format!("{table} · GUID {guid} · node {node} · process {process}"),
                agent: row.get(4).map_err(err)?,
                uri: row.get(5).map_err(err)?,
                storage: row.get(6).map_err(err)?,
                load_base: row.get(7).map_err(err)?,
                load_size: row.get(8).map_err(err)?,
                load_delta: row.get(9).map_err(err)?,
            });
        }
    }
    Ok(())
}
