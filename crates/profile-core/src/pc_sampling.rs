use crate::model::{Capture, Instruction};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};

const REASONS: [&str; 10] = [
    "NONE",
    "NO_INSTRUCTION_AVAILABLE",
    "ALU_DEPENDENCY",
    "WAITCNT",
    "INTERNAL_INSTRUCTION",
    "BARRIER_WAIT",
    "ARBITER_NOT_WIN",
    "ARBITER_WIN_EX_STALL",
    "OTHER_WAIT",
    "SLEEP_WAIT",
];

pub(crate) fn present(tools: &[Value]) -> bool {
    tools.iter().any(|tool| {
        ["pc_sample_stochastic", "pc_sample_host_trap"]
            .iter()
            .any(|key| {
                let value = &tool["buffer_records"][key];
                !value.is_null() && value.as_array().is_none_or(|rows| !rows.is_empty())
            })
    })
}

fn records<'a>(value: &'a Value, name: &str) -> Result<&'a [Value], String> {
    if value.is_null() {
        return Ok(&[]);
    }
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| format!("Invalid PC sampling {name} array"))
}
fn integer(value: &Value, name: &str) -> Result<u64, String> {
    value
        .as_u64()
        .ok_or_else(|| format!("Invalid PC sampling {name}"))
}
fn bit(value: &Value, name: &str) -> Result<bool, String> {
    match value.as_u64() {
        Some(0) => Ok(false),
        Some(1) => Ok(true),
        _ => Err(format!("Invalid PC sampling {name}: expected 0 or 1")),
    }
}

pub(crate) fn methods(tools: &[Value]) -> Result<Vec<String>, String> {
    let mut found = Vec::new();
    for (method, key) in [
        ("host-trap", "pc_sample_host_trap"),
        ("stochastic", "pc_sample_stochastic"),
    ] {
        let mut count = 0usize;
        for tool in tools {
            count += records(&tool["buffer_records"][key], method)?.len();
        }
        if count > 0 {
            found.push(method.into());
        }
    }
    Ok(found)
}

pub(crate) fn load(
    path: &Path,
    tools: &[Value],
    selected: Option<&str>,
) -> Result<Capture, String> {
    let available = methods(tools)?;
    if let Some(method) = selected {
        if !available.iter().any(|m| m == method) {
            return Err("Selected PC sampling method is unavailable in this input".into());
        }
    } else if available.len() > 1 {
        return Err(
            "Mixed host-trap and stochastic sampling methods require an explicit method selection"
                .into(),
        );
    }
    let host_trap = selected.unwrap_or_else(|| {
        available
            .first()
            .map(String::as_str)
            .unwrap_or("stochastic")
    }) == "host-trap";
    let method = if host_trap { "host-trap" } else { "stochastic" };
    let record_key = if host_trap {
        "pc_sample_host_trap"
    } else {
        "pc_sample_stochastic"
    };
    let mut capture = Capture::empty(
        path.file_name().unwrap().to_string_lossy().into(),
        &format!("rocprofv3 {method} PC sampling JSON"),
        "ns",
    );
    if selected.is_some() {
        capture.name = format!("{} · {method}", capture.name);
        capture
            .metadata
            .insert("pc_sampling_selection".into(), method.into());
    }
    if available.len() > 1 {
        let other_key = if host_trap {
            "pc_sample_stochastic"
        } else {
            "pc_sample_host_trap"
        };
        let other_count: usize = tools
            .iter()
            .map(|t| {
                t["buffer_records"][other_key]
                    .as_array()
                    .map_or(0, Vec::len)
            })
            .sum();
        capture.metadata.insert(
            "pc_sampling_other_method_records".into(),
            other_count.to_string(),
        );
        capture.warnings.push(format!("Selected {method} sampling. {other_count} records from the other sampling method remain in the input; open it again to inspect that method in a separate tab. SPM counters, when present, remain available with either choice."));
    }
    let mut indices = BTreeMap::new();
    let (mut total, mut unresolved, mut lock_errors) = (0usize, 0usize, 0usize);
    let (mut first, mut last) = (u64::MAX, 0u64);
    for (process, tool) in tools.iter().enumerate() {
        let samples = records(&tool["buffer_records"][record_key], method)?;
        if samples.is_empty() {
            continue;
        }
        let instructions = records(
            &tool["strings"]["pc_sample_instructions"],
            "instruction strings",
        )?;
        let comments = records(&tool["strings"]["pc_sample_comments"], "source comments")?;
        for sample in samples {
            total += 1;
            if total > 2_000_000 {
                return Err("PC sampling import exceeds 2 million records".into());
            }
            let record = &sample["record"];
            let timestamp = integer(&record["timestamp"], "timestamp")?;
            first = first.min(timestamp);
            last = last.max(timestamp);
            let status = if host_trap {
                None
            } else {
                let issued = bit(&record["wave_issued"], "wave_issued")?;
                if bit(&record["snapshot"]["lck_err"], "snapshot.lck_err")? {
                    lock_errors += 1;
                }
                let reason = if issued {
                    0
                } else {
                    record["snapshot"]["stall_reason"]
                        .as_str()
                        .and_then(|s| {
                            s.strip_prefix("ROCPROFILER_PC_SAMPLING_INSTRUCTION_NOT_ISSUED_REASON_")
                        })
                        .and_then(|s| REASONS.iter().position(|r| *r == s))
                        .ok_or("Unknown or missing PC sampling stall reason")?
                };
                Some((issued, reason))
            };
            let object = integer(&record["pc"]["code_object_id"], "code object ID")?;
            let offset = integer(&record["pc"]["code_object_offset"], "code object offset")?;
            let inst_index = sample["inst_index"]
                .as_i64()
                .ok_or("Invalid PC sampling instruction string index")?;
            if object == 0 && inst_index == -1 {
                unresolved += 1;
                continue;
            }
            if object == 0 || inst_index < 0 {
                return Err("Inconsistent PC sampling code object and instruction identity".into());
            }
            let text = instructions
                .get(inst_index as usize)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .ok_or("Missing PC sampling instruction string")?;
            let source = comments
                .get(inst_index as usize)
                .and_then(Value::as_str)
                .ok_or("Missing PC sampling source comment")?;
            let key = (process, object, offset);
            let index = *indices.entry(key).or_insert_with(|| {
                let index = capture.instructions.len();
                capture.instructions.push(Instruction {
                    index: index as i64,
                    text: text.into(),
                    source: source.into(),
                    address: format!("0x{offset:x}"),
                    code_object: format!("process[{process}]:{object}"),
                    hits: 0,
                    latency: 0.,
                    stall: 0.,
                    idle: 0.,
                    pc_samples: 0,
                    pc_issued: (!host_trap).then_some(0),
                    pc_stalled: (!host_trap).then_some(0),
                    stall_reasons: (!host_trap).then(|| vec![0; REASONS.len()]),
                });
                index
            });
            let instruction = &mut capture.instructions[index];
            if instruction.text != text || instruction.source != source {
                return Err("Conflicting PC sampling instruction/source for the same process, code object and offset".into());
            }
            instruction.pc_samples += 1;
            if let Some((issued, reason)) = status {
                if issued {
                    *instruction.pc_issued.as_mut().unwrap() += 1;
                } else {
                    *instruction.pc_stalled.as_mut().unwrap() += 1;
                    instruction.stall_reasons.as_mut().unwrap()[reason] += 1;
                }
            }
        }
    }
    if total == 0 {
        return Err("PC sampling JSON contains no samples".into());
    }
    let span = last - first;
    if span > 9_007_199_254_740_991 {
        return Err("PC sampling timestamp span exceeds exact integer range".into());
    }
    capture.origin = first.to_string();
    capture.end = (span as f64).max(1.);
    for (key, value) in [
        ("pc_sampling_records", total),
        ("pc_sampling_unresolved", unresolved),
        ("pc_sampling_lock_errors", lock_errors),
    ] {
        capture.metadata.insert(key.into(), value.to_string());
    }
    capture
        .metadata
        .insert("instruction_costs".into(), "incomplete".into());
    capture
        .metadata
        .insert("pc_sampling_method".into(), method.into());
    if host_trap {
        capture.metadata.remove("pc_sampling_lock_errors");
        capture.warnings.push("Host-trap samples provide PC observations only; issued/stalled status, stall reasons and sampling-lock contention were not measured.".into());
    }
    capture.warnings.push("Statistical PC samples only: counts are not instruction executions, cycles, or wave occupancy. ISA addresses are code-object offsets. Other API/dispatch records in this JSON are not imported by this sampling view.".into());
    if unresolved > 0 {
        capture.warnings.push(format!("{unresolved} of {total} samples have unresolved code objects and are excluded from ISA totals; their count is retained in capture metadata."));
    }
    if lock_errors > 0 {
        capture.warnings.push(format!("{lock_errors} samples report sampling lock contention. These samples are retained; other waves may have missed samples, so collection frequency can bias the distribution."));
    }
    Ok(capture)
}
