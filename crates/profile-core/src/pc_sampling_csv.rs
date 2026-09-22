use crate::model::{Capture, PcSamplingObservations};
use std::{collections::BTreeSet, path::Path};

pub(crate) fn load<R: std::io::Read>(
    path: &Path,
    mut reader: csv::Reader<R>,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<Capture, String> {
    crate::cancellable_read::check(cancelled)?;
    let columns: Vec<String> = reader
        .headers()
        .map_err(|e| e.to_string())?
        .iter()
        .map(|s| s.trim_start_matches('\u{feff}').trim().to_owned())
        .collect();
    let unique: BTreeSet<_> = columns.iter().collect();
    if unique.len() != columns.len() || columns.iter().any(String::is_empty) {
        return Err("PC sampling CSV has duplicate or empty column names".into());
    }
    let column = |name: &str| columns.iter().position(|s| s == name);
    let required = [
        "Sample_Timestamp",
        "Exec_Mask",
        "Dispatch_Id",
        "Instruction",
        "Instruction_Comment",
        "Correlation_Id",
    ];
    let stochastic_fields = [
        "Wave_Issued_Instruction",
        "Instruction_Type",
        "Stall_Reason",
        "Wave_Count",
    ];
    let stochastic = stochastic_fields.iter().any(|name| column(name).is_some());
    for name in required
        .iter()
        .chain(stochastic_fields.iter().filter(|_| stochastic))
    {
        if column(name).is_none() {
            return Err(format!("PC sampling CSV is missing {name}"));
        }
    }
    let method = if stochastic {
        "stochastic"
    } else {
        "host-trap"
    };
    let mut capture = Capture::empty(
        path.file_name().unwrap().to_string_lossy().into(),
        &format!("rocprofv3 {method} PC sampling CSV"),
        "ns",
    );
    let mut rows = Vec::new();
    let (mut first, mut last) = (u64::MAX, 0);
    for (index, record) in reader.records().enumerate() {
        crate::cancellable_read::check(cancelled)?;
        if index >= 2_000_000 {
            return Err("PC sampling CSV exceeds the 2 million sample limit".into());
        }
        let record = record.map_err(|e| e.to_string())?;
        let integer = |name: &str| -> Result<u64, String> {
            record[column(name).unwrap()]
                .trim()
                .parse()
                .map_err(|_| format!("Invalid {name} in PC sampling CSV row {}", index + 1))
        };
        let timestamp = integer("Sample_Timestamp")?;
        for name in ["Exec_Mask", "Dispatch_Id", "Correlation_Id"] {
            integer(name)?;
        }
        if stochastic {
            if integer("Wave_Issued_Instruction")? > 1 {
                return Err(format!(
                    "Invalid Wave_Issued_Instruction in PC sampling CSV row {}",
                    index + 1
                ));
            }
            integer("Wave_Count")?;
        }
        first = first.min(timestamp);
        last = last.max(timestamp);
        rows.push(record.iter().map(str::to_owned).collect());
    }
    if rows.is_empty() {
        return Err("PC sampling CSV contains no samples".into());
    }
    let span = last - first;
    if span > 9_007_199_254_740_991 {
        return Err("PC sampling CSV timestamp span exceeds exact integer range".into());
    }
    capture.origin = first.to_string();
    capture.end = (span as f64).max(1.);
    capture
        .metadata
        .insert("pc_sampling_method".into(), method.into());
    capture
        .metadata
        .insert("pc_sampling_records".into(), rows.len().to_string());
    capture
        .metadata
        .insert("instruction_costs".into(), "incomplete".into());
    capture.warnings.push("PC sampling CSV observations retain input order and duplicate rows. This format omits resolved PC/code-object identities; instruction text and comments cannot establish ISA or dispatch ownership. Open matching SDK JSON for ISA attribution. Counts are statistical samples, not execution counts or cycles.".into());
    capture.pc_sampling_observations = Some(std::sync::Arc::new(PcSamplingObservations {
        columns,
        rows,
    }));
    crate::cancellable_read::check(cancelled)?;
    Ok(capture)
}

#[cfg(test)]
mod tests {
    use super::*;
    const HEADER: &str =
        "Sample_Timestamp,Exec_Mask,Dispatch_Id,Instruction,Instruction_Comment,Correlation_Id";
    fn parse(text: &str) -> Result<Capture, String> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("samples.csv");
        std::fs::write(&path, text).unwrap();
        crate::load_capture(&path)
    }
    #[test]
    fn csv_preserves_duplicates_exact_fields_and_unknown_columns() {
        let row = "18446744073709551615,18446744073709551615,9007199254740993,\"v_add v0, v1\",\"source:3\",7,extension";
        let capture = parse(&format!("{HEADER},Extra\n{row}\n{row}\n")).unwrap();
        assert!(capture.instructions.is_empty());
        assert!(capture.dispatches.is_empty());
        assert_eq!(capture.origin, u64::MAX.to_string());
        let observations = capture.pc_sampling_observations.as_ref().unwrap();
        let header = capture.clone_without_instructions();
        assert!(std::sync::Arc::ptr_eq(
            observations,
            header.pc_sampling_observations.as_ref().unwrap()
        ));
        assert_eq!(observations.rows.len(), 2);
        assert_eq!(observations.rows[0], observations.rows[1]);
        assert_eq!(observations.rows[0][3], "v_add v0, v1");
        assert_eq!(observations.rows[0][2], "9007199254740993");
        let restored: Capture = serde_json::from_value(
            serde_json::to_value(capture.clone_without_instructions()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            restored.pc_sampling_observations.unwrap().rows,
            observations.rows
        );
    }
    #[test]
    fn csv_archive_retains_observations_without_the_input() {
        let capture = parse(&format!("{HEADER}\n1,18446744073709551615,1,x,,1\n")).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("samples.rocprof-capture");
        let cancel = std::sync::atomic::AtomicBool::new(false);
        crate::capture_archive::write(&path, &capture, &cancel).unwrap();
        let restored = crate::capture_archive::read(&path, &cancel).unwrap();
        assert_eq!(
            restored.pc_sampling_observations.unwrap().rows,
            capture.pc_sampling_observations.unwrap().rows
        );
        assert!(restored.instructions.is_empty());
    }
    #[test]
    fn csv_rejects_partial_stochastic_schema_bad_flags_and_precision_loss() {
        assert!(parse(&format!("{HEADER},Wave_Count\n1,1,1,x,,1,2\n"))
            .unwrap_err()
            .contains("missing Wave_Issued"));
        assert!(parse(&format!(
            "{HEADER}\n0,1,1,x,,1\n9007199254740992,1,1,x,,1\n"
        ))
        .unwrap_err()
        .contains("exact integer"));
        assert!(parse(&format!("{HEADER},Instruction\n1,1,1,x,,1,x\n"))
            .unwrap_err()
            .contains("duplicate"));
        let header =
            format!("{HEADER},Wave_Issued_Instruction,Instruction_Type,Stall_Reason,Wave_Count");
        assert!(parse(&format!("{header}\n1,1,1,x,,1,2,TYPE,REASON,3\n"))
            .unwrap_err()
            .contains("Invalid Wave_Issued"));
        let c = parse(&format!("{header}\n1,1,1,x,,1,1,NEW_TYPE,NEW_REASON,3\n")).unwrap();
        assert_eq!(c.metadata["pc_sampling_method"], "stochastic");
        assert_eq!(c.pc_sampling_observations.unwrap().rows[0][8], "NEW_REASON");
    }
}
