use crate::{Instruction, Wave};
use serde_json::json;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
fn check(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) {
        Err("Wave query cancelled".into())
    } else {
        Ok(())
    }
}

#[derive(Default)]
struct Counter {
    pending: VecDeque<usize>,
    parallel: bool,
}
impl Counter {
    fn drain(&mut self, count: usize, flat: &mut Vec<usize>) -> Vec<usize> {
        if count == 0 {
            self.parallel = false;
        }
        if self.parallel {
            return vec![];
        }
        let mut result = std::mem::take(flat);
        result.extend(
            self.pending
                .drain(..self.pending.len().saturating_sub(count)),
        );
        result
    }
}

#[derive(Default)]
struct ExportCounter {
    pending: VecDeque<(usize, u8)>,
    mixed_wait: bool,
}
impl ExportCounter {
    fn drain(&mut self, count: usize) -> Vec<usize> {
        if count >= 7 {
            return vec![];
        }
        let kinds = self.pending.iter().fold(0u8, |mask, (_, kind)| mask | kind);
        if count > 0 && kinds.count_ones() > 1 {
            self.mixed_wait = true;
            return vec![];
        }
        let take = self.pending.len().saturating_sub(count);
        self.pending
            .drain(..take)
            .map(|(position, _)| position)
            .collect()
    }
}

fn export_kind(text: &str, gfx11: bool) -> Option<u8> {
    let mut words = text.split_whitespace();
    let opcode = words.next()?;
    if opcode == "exp" {
        let target = words.next().unwrap_or("");
        return Some(if target.starts_with("param") {
            1
        } else if target.starts_with("pos") {
            2
        } else {
            4
        });
    }
    if gfx11 && matches!(opcode, "lds_direct_load" | "lds_param_load") {
        return Some(8);
    }
    if opcode.starts_with("ds_")
        && (opcode.starts_with("ds_gws_")
            || opcode == "ds_ordered_count"
            || words.any(|word| word == "gds"))
    {
        return Some(16);
    }
    None
}

fn interp_wait(text: &str) -> Option<usize> {
    let mut words = text.split_whitespace();
    if !matches!(
        words.next()?,
        "v_interp_p10_f32"
            | "v_interp_p2_f32"
            | "v_interp_p10_f16_f32"
            | "v_interp_p2_f16_f32"
            | "v_interp_p10_rtz_f16_f32"
            | "v_interp_p2_rtz_f16_f32"
    ) {
        return None;
    }
    let mut result = None;
    while let Some(word) = words.next() {
        if !word.contains("wait_exp") {
            continue;
        }
        if result.is_some() {
            return None;
        }
        let value = word.strip_prefix("wait_exp:")?;
        let value = if value.is_empty() {
            words.next()?
        } else {
            value
        };
        result = Some(wait_literal(value).filter(|n| *n <= 7)?);
    }
    Some(result.unwrap_or(0))
}

fn wait_literal(text: &str) -> Option<usize> {
    let text = text.trim();
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        if hex.is_empty() || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        usize::from_str_radix(hex, 16).ok()
    } else {
        if text.is_empty() || !text.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        text.parse().ok()
    }
}

fn count(text: &str, name: &str) -> Option<usize> {
    let (opcode, operands) = text.split_once(char::is_whitespace)?;
    if opcode == format!("s_waitcnt_{name}") {
        let (register, value) = operands.split_once(',')?;
        if !matches!(register.trim(), "null" | "s_null") {
            return None;
        }
        return wait_literal(value);
    }
    if opcode == format!("s_wait_{name}") {
        return wait_literal(operands);
    }
    if !matches!(opcode, "s_waitcnt" | "s_wait") {
        return None;
    }
    let mut tail = operands.trim();
    let mut seen = Vec::new();
    let mut result = None;
    while !tail.is_empty() {
        let (counter, rest) = tail.split_once('(')?;
        let counter = counter.trim();
        let allowed = if opcode == "s_waitcnt" {
            &["vmcnt", "lgkmcnt", "expcnt", "vscnt"][..]
        } else {
            &[
                "loadcnt",
                "storecnt",
                "dscnt",
                "kmcnt",
                "expcnt",
                "bvhcnt",
                "samplecnt",
                "tensorcnt",
                "asynccnt",
            ][..]
        };
        if !allowed.contains(&counter) || seen.contains(&counter) {
            return None;
        }
        seen.push(counter);
        let (value, remaining) = rest.split_once(')')?;
        let value = wait_literal(value)?;
        if counter == name {
            result = Some(value);
        }
        if !remaining.is_empty()
            && !remaining.starts_with(char::is_whitespace)
            && !remaining.starts_with('&')
        {
            return None;
        }
        tail = remaining.trim();
        if let Some(after) = tail.strip_prefix('&') {
            tail = after.trim_start();
            if tail.is_empty() {
                return None;
            }
        }
    }
    result
}

fn family(architecture: &str) -> Option<u8> {
    [12, 11, 10, 9]
        .into_iter()
        .find(|major| architecture.starts_with(&format!("gfx{major}")))
}

fn atomic_returns(text: &str, family: u8) -> Option<bool> {
    if family == 9 {
        return Some(true);
    }
    let mut result = None;
    let words: Vec<_> = text.split_whitespace().collect();
    for (index, word) in words.iter().enumerate() {
        let flag = if family < 12 {
            match *word {
                "glc" | "glc:1" => Some(true),
                "noglc" | "glc:0" => Some(false),
                _ if word.starts_with("glc:") => return None,
                _ => continue,
            }
        } else if let Some(hint) = word.strip_prefix("th:") {
            let hint = if hint.is_empty() {
                *words.get(index + 1)?
            } else {
                hint
            };
            match hint {
                "TH_ATOMIC_RETURN" | "TH_ATOMIC_RT_RETURN" | "TH_ATOMIC_NT_RETURN" => Some(true),
                "TH_ATOMIC_RT" | "TH_ATOMIC_NT" => Some(false),
                _ => {
                    let value = if let Some(hex) = hint.strip_prefix("0x") {
                        u8::from_str_radix(hex, 16).ok()?
                    } else {
                        hint.parse::<u8>().ok()?
                    };
                    if value > 7 {
                        return None;
                    }
                    Some(value & 1 != 0)
                }
            }
        } else {
            continue;
        };
        if result.is_some() {
            return None;
        }
        result = flag;
    }
    Some(result.unwrap_or(false))
}
pub fn reconstruct_capture(wave: &mut Wave, capture: &crate::Capture) {
    let _ = reconstruct_capture_cancellable(wave, capture, &AtomicBool::new(false));
}
pub fn reconstruct_capture_cancellable(
    wave: &mut Wave,
    capture: &crate::Capture,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    check(cancelled)?;
    if wave.waits.as_array().is_some_and(|rows| !rows.is_empty()) {
        return Ok(());
    }
    let named = family(&capture.architecture);
    let declared = capture
        .metadata
        .get("gfxip")
        .and_then(|value| value.parse::<u8>().ok())
        .filter(|major| (9..=12).contains(major));
    if named.is_some() && declared.is_some() && named != declared {
        if !wave.info.is_object() {
            wave.info = json!({"capturedInfo":wave.info});
        }
        wave.info["waitAnalysis"] = json!({
            "basis":"named ISA counters in recorded token order", "occurrences":[],
            "unavailableReason":"Architecture label and decoder gfxip disagree; wait reconstruction was skipped."
        });
        return Ok(());
    }
    if let Some(major) = named.or(declared) {
        reconstruct_cancellable(
            wave,
            &format!("gfx{major}"),
            &capture.instructions,
            cancelled,
        )?;
        if wave.info["waitAnalysis"].is_object() {
            wave.info["waitAnalysis"]["gfxip"] = json!(major);
            wave.info["waitAnalysis"]["architectureBasis"] = json!(if named.is_some() {
                "architecture label"
            } else {
                "decoder gfxip"
            });
        }
    }
    check(cancelled)
}

/// Reconstructs named-counter dependencies without replacing imported relationships.
pub fn reconstruct(wave: &mut Wave, architecture: &str, instructions: &[Instruction]) {
    let _ = reconstruct_cancellable(wave, architecture, instructions, &AtomicBool::new(false));
}
pub fn reconstruct_cancellable(
    wave: &mut Wave,
    architecture: &str,
    instructions: &[Instruction],
    cancelled: &AtomicBool,
) -> Result<(), String> {
    check(cancelled)?;
    if wave.waits.as_array().is_some_and(|rows| !rows.is_empty()) {
        return Ok(());
    }
    let family = if architecture.starts_with("gfx9") {
        9
    } else if architecture.starts_with("gfx10") || architecture.starts_with("gfx11") {
        10
    } else if architecture.starts_with("gfx12") {
        12
    } else {
        return Ok(());
    };
    let mut code = HashMap::new();
    for i in instructions {
        check(cancelled)?;
        code.insert(
            i.index,
            (
                i.text.trim(),
                crate::wait_encoding::expand(architecture, &i.text),
            ),
        );
    }
    let names: &[&str] = if family == 9 {
        &["lgkmcnt", "vmcnt"]
    } else if family == 10 {
        &["lgkmcnt", "vmcnt", "vscnt"]
    } else {
        &[
            "dscnt",
            "loadcnt",
            "storecnt",
            "kmcnt",
            "expcnt",
            "bvhcnt",
            "samplecnt",
            "tensorcnt",
            "asynccnt",
        ]
    };
    let mut counters: Vec<Counter> = names.iter().map(|_| Counter::default()).collect();
    let mut flat_load = vec![];
    let mut flat_store = vec![];
    let mut relationships = vec![];
    let mut occurrences = vec![];
    let mut unsupported = 0usize;
    let mut missing = 0usize;
    let mut packed = 0usize;
    let mut export = ExportCounter::default();
    let mut export_used = false;
    let mut conservative_image_ordering = false;
    for (position, token) in wave.tokens.iter().enumerate() {
        check(cancelled)?;
        let Some((original, expanded)) = code.get(&token.instruction) else {
            missing += 1;
            continue;
        };
        let text = expanded.as_deref().unwrap_or(original);
        let text = text
            .split(';')
            .next()
            .unwrap_or(text)
            .split("//")
            .next()
            .unwrap_or(text)
            .trim();
        let implicit_export = architecture.starts_with("gfx11") && text.starts_with("v_interp_");
        if expanded.is_some() {
            packed += 1;
        }
        if (text.starts_with("s_wait") && !text.starts_with("s_wait_alu")) || implicit_export {
            let mut dependencies = vec![];
            if implicit_export {
                export_used = true;
                if let Some(value) = interp_wait(text) {
                    dependencies.extend(export.drain(value));
                } else {
                    unsupported += 1;
                }
            } else if family == 12 && text == "s_wait_idle" {
                for counter in &mut counters {
                    dependencies.extend(counter.pending.drain(..));
                    counter.parallel = false;
                }
                dependencies.append(&mut flat_load);
                dependencies.append(&mut flat_store);
            } else {
                let order: &[usize] = if family == 9 {
                    &[0, 1]
                } else if family == 10 {
                    &[2, 1, 0]
                } else {
                    &[5, 4, 6, 3, 2, 1, 7, 8, 0]
                };
                let mut recognized = false;
                if family < 12 && text.contains("expcnt") {
                    export_used = true;
                    if let Some(value) = count(text, "expcnt").filter(|value| *value <= 7) {
                        recognized = true;
                        dependencies.extend(export.drain(value));
                    } else {
                        unsupported += 1;
                    }
                }
                for &index in order {
                    if !text.contains(names[index]) {
                        continue;
                    }
                    let Some(value) = count(text, names[index]) else {
                        unsupported += 1;
                        continue;
                    };
                    recognized = true;
                    let maximum = match names[index] {
                        "lgkmcnt" => Some(if family == 9 { 15 } else { 63 }),
                        "vmcnt" | "vscnt" | "loadcnt" | "storecnt" | "dscnt" | "samplecnt" => {
                            Some(63)
                        }
                        "kmcnt" => Some(31),
                        "expcnt" | "bvhcnt" => Some(7),
                        _ => None,
                    };
                    if maximum.is_some_and(|maximum| value >= maximum) {
                        if maximum.is_some_and(|maximum| value > maximum) {
                            unsupported += 1;
                        }
                        continue;
                    }
                    let mut empty = vec![];
                    let flat = if index == 0 || index == 1 {
                        &mut flat_load
                    } else if index == 2 {
                        &mut flat_store
                    } else {
                        &mut empty
                    };
                    dependencies.extend(counters[index].drain(value, flat));
                    if index == 0 && !counters[index].parallel {
                        dependencies.append(&mut flat_store);
                    }
                }
                if !recognized {
                    unsupported += 1;
                }
            }
            dependencies.sort_unstable();
            dependencies.dedup();
            if !dependencies.is_empty() {
                relationships.push(json!([
                    token.instruction,
                    dependencies
                        .iter()
                        .map(|&index| json!([wave.tokens[index].instruction, 0]))
                        .collect::<Vec<_>>()
                ]));
                occurrences.push(json!({
                    "waitToken": position,
                    "sources": dependencies.iter().map(|&index| json!({
                        "token": index,
                        "issueToWaitCycles": token.start - wave.tokens[index].start,
                        "issueToWaitEndCycles": token.start + token.duration - wave.tokens[index].start
                    })).collect::<Vec<_>>()
                }));
            }
            continue;
        }
        if family < 12 {
            if let Some(kind) = export_kind(text, architecture.starts_with("gfx11")) {
                export.pending.push_back((position, kind));
                export_used = true;
            }
        }
        let scalar = text.starts_with("s_load") || text.starts_with("s_store");
        let opcode = text.split_whitespace().next().unwrap_or("");
        let image_load =
            opcode.starts_with("image_load") || opcode.starts_with("image_get_resinfo");
        let image_sample = opcode.starts_with("image_sample")
            || opcode.starts_with("image_gather4")
            || opcode.starts_with("image_get_lod")
            || opcode.starts_with("image_msaa_load");
        let image_store = opcode.starts_with("image_store");
        let atomic = [
            "global_atomic_",
            "buffer_atomic_",
            "flat_atomic_",
            "image_atomic_",
        ]
        .iter()
        .any(|prefix| opcode.starts_with(prefix));
        let atomic_return = if atomic {
            let Some(returning) = atomic_returns(text, family) else {
                unsupported += 1;
                continue;
            };
            returning
        } else {
            false
        };
        let message = text.starts_with("s_sendmsg");
        let store = opcode.contains("store")
            || (family == 12 && opcode.starts_with("global_wb"))
            || (atomic && !atomic_return);
        let global = text.starts_with("global_")
            || text.starts_with("buffer_")
            || text.starts_with("tbuffer_")
            || text.starts_with("scratch_");
        let flat = text.starts_with("flat_");
        let ds = text.starts_with("ds_") || (family == 12 && text.starts_with("lds_"));
        let index = if family == 12 && text.contains("_bvh") {
            Some(5)
        } else if atomic {
            Some(if store && family != 9 { 2 } else { 1 })
        } else if image_sample {
            Some(if family == 12 { 6 } else { 1 })
        } else if image_load {
            Some(1)
        } else if image_store {
            Some(if family == 9 { 1 } else { 2 })
        } else if family == 12 && ds && (text.contains("param_load") || text.contains("ds_direct"))
        {
            Some(4)
        } else if family == 12 && (text.starts_with("tensor_") || text.starts_with("stensor_")) {
            Some(7)
        } else if family == 12
            && (text.starts_with("global_load_async_") || text.starts_with("global_store_async_"))
        {
            Some(8)
        } else if family == 12 && text.contains("_prefetch_") {
            None
        } else if scalar || message {
            Some(if family == 12 { 3 } else { 0 })
        } else if ds {
            Some(0)
        } else if global {
            Some(if store && family != 9 { 2 } else { 1 })
        } else {
            None
        };
        if flat {
            counters[0].parallel = true;
            let index = if store && family != 9 { 2 } else { 1 };
            counters[index].parallel = true;
            if index == 2 {
                flat_store.push(position);
            } else {
                flat_load.push(position);
            }
        } else if let Some(index) = index {
            counters[index].pending.push_back(position);
            if family < 12
                && (image_load
                    || image_sample
                    || image_store
                    || opcode.starts_with("image_atomic_"))
            {
                counters[index].parallel = true;
                conservative_image_ordering = true;
            }
            if scalar {
                counters[index].parallel = true;
            }
            if family != 9 && text.starts_with("s_sendmsg_rtn") {
                counters[index].pending.push_back(position);
            }
        }
    }
    let mut unordered = false;
    for pair in wave.tokens.windows(2) {
        check(cancelled)?;
        if pair[1].start < pair[0].start {
            unordered = true;
            break;
        }
    }
    if unsupported > 0 || missing > 0 || unordered {
        relationships.clear();
        occurrences.clear();
    }
    check(cancelled)?;
    wave.waits = json!(relationships);
    if !wave.info.is_object() {
        wave.info = json!({"capturedInfo": wave.info});
    }
    wave.info["waitAnalysis"] = json!({
        "basis": "named ISA counters in recorded token order",
        "occurrences": occurrences,
        "unsupportedWaits": unsupported,
        "packedWaitTokens": packed,
        "unmodeledCounters": Vec::<String>::new(),
        "exportCounterModeled": export_used,
        "conservativeExportOrdering": export.mixed_wait,
        "conservativeImageOrdering": conservative_image_ordering,
        "missingInstructionTokens": missing,
        "unorderedTokens": unordered,
        "limitation": "Issue-to-wait intervals are not measured memory completion latency. Unsupported counter encodings are not inferred."
    });
    check(cancelled)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancelled_reconstruction_preserves_wave_without_partial_analysis() {
        let mut wave = run("gfx942", &["s_endpgm"], &[0]);
        let before = serde_json::to_value(&wave).unwrap();
        let cancelled = AtomicBool::new(true);
        assert_eq!(
            reconstruct_cancellable(&mut wave, "gfx942", &[], &cancelled).unwrap_err(),
            "Wave query cancelled"
        );
        assert_eq!(serde_json::to_value(&wave).unwrap(), before);
    }
    fn run(architecture: &str, lines: &[&str], order: &[usize]) -> Wave {
        let instructions: Vec<Instruction> = lines
            .iter()
            .enumerate()
            .map(|(index, text)| {
                serde_json::from_value(json!({
            "index": index, "text": text, "source": "", "address": "", "codeObject": "", "hits": 0,
            "latency": 0, "stall": 0, "idle": 0, "pcSamples": 0
        })).unwrap()
            })
            .collect();
        let mut wave: Wave = serde_json::from_value(json!({
            "id":"w", "start":0, "end":100, "cu":0, "states":[], "waits":null,"info":{},
            "tokens":order.iter().enumerate().map(|(n,i)| json!({
                "start":n * 10,"duration":5,"stall":2,"category":0,"instruction":i
            })).collect::<Vec<_>>()
        }))
        .unwrap();
        reconstruct(&mut wave, architecture, &instructions);
        wave
    }
    #[test]
    fn idle_requires_an_exact_operand_free_instruction() {
        for wait in [
            "s_wait_idle_extra",
            "s_wait_idle 0",
            "s_wait_idle loadcnt(0)",
        ] {
            let wave = run(
                "gfx1200",
                &["global_load_b32 v0, v1, off", wait, "s_wait_loadcnt 0"],
                &[0, 1, 2],
            );
            assert_eq!(wave.waits, json!([]), "{wait}");
            assert!(
                wave.info["waitAnalysis"]["unsupportedWaits"]
                    .as_u64()
                    .unwrap()
                    > 0
            );
        }
        for wait in [
            "s_wait_idle",
            "s_wait_idle ; comment",
            "s_wait_idle // comment",
        ] {
            let wave = run("gfx1200", &["global_load_b32 v0, v1, off", wait], &[0, 1]);
            assert_eq!(wave.waits, json!([[1, [[0, 0]]]]), "{wait}");
        }
    }

    #[test]
    fn interpolation_waits_reject_duplicates_and_unknown_opcodes() {
        for modifier in [
            "wait_exp:0 wait_exp:7",
            "wait_exp:7 wait_exp:0",
            "wait_exp:0junk",
            "wait_exp:0 bogus_wait_exp:7",
            "wait_exp:",
            "wait_exp:8",
        ] {
            let interp = format!("v_interp_p10_f32 v0, v1, v2, v3 {modifier}");
            let wave = run(
                "gfx1100",
                &["lds_direct_load v0, v1", &interp, "s_waitcnt expcnt(0)"],
                &[0, 1, 2],
            );
            assert_eq!(wave.waits, json!([]), "{interp}");
            assert_eq!(wave.info["waitAnalysis"]["unsupportedWaits"], 1);
        }
        let wave = run(
            "gfx1100",
            &["lds_direct_load v0, v1", "v_interp_unknown v0, v1, v2, v3"],
            &[0, 1],
        );
        assert_eq!(wave.waits, json!([]));
        assert_eq!(wave.info["waitAnalysis"]["unsupportedWaits"], 1);
        for opcode in [
            "v_interp_p10_f32",
            "v_interp_p2_f32",
            "v_interp_p10_f16_f32",
            "v_interp_p2_f16_f32",
            "v_interp_p10_rtz_f16_f32",
            "v_interp_p2_rtz_f16_f32",
        ] {
            for modifier in ["", "wait_exp:0", "wait_exp: 0X0"] {
                let interp = format!("{opcode} v0, v1, v2, v3 {modifier}");
                let wave = run("gfx1100", &["lds_direct_load v0, v1", &interp], &[0, 1]);
                assert_eq!(wave.waits, json!([[1, [[0, 0]]]]), "{interp}");
            }
        }
    }

    #[test]
    fn wait_operands_require_complete_constant_syntax() {
        for text in [
            "s_waitcnt vmcnt(0junk)",
            "s_waitcnt vmcnt(0) trailing",
            "s_waitcnt vmcnt(0) lgkmcnt(bad)",
            "s_waitcnt vmcnt(0) vmcnt(1)",
            "s_waitcnt vmcnt(0)&",
            "s_waitcnt vmcnt(0)lgkmcnt(0)",
            "s_waitcnt vmcnt(-1)",
            "s_waitcnt vmcnt(0x)",
        ] {
            let wave = run("gfx942", &["global_load_dword v0, v1, off", text], &[0, 1]);
            assert_eq!(wave.waits, json!([]), "{text}");
            assert!(
                wave.info["waitAnalysis"]["unsupportedWaits"]
                    .as_u64()
                    .unwrap()
                    > 0,
                "{text}"
            );
        }
        for text in [
            "s_waitcnt vmcnt(0)",
            "s_waitcnt vmcnt(0X0) & lgkmcnt(0)",
            "s_waitcnt vmcnt(0x0) expcnt(0) lgkmcnt(0)",
        ] {
            let wave = run("gfx942", &["global_load_dword v0, v1, off", text], &[0, 1]);
            assert_eq!(wave.waits, json!([[1, [[0, 0]]]]), "{text}");
        }
        for register in ["s0", "s[0:1]", "", "null extra"] {
            let wait = format!("s_waitcnt_vscnt {register}, 0");
            let wave = run(
                "gfx1100",
                &["global_store_dword v0, v1, off", &wait],
                &[0, 1],
            );
            assert_eq!(wave.waits, json!([]), "{wait}");
            assert!(
                wave.info["waitAnalysis"]["unsupportedWaits"]
                    .as_u64()
                    .unwrap()
                    > 0
            );
        }
        for register in ["null", "s_null"] {
            let wait = format!("s_waitcnt_vscnt {register}, 0X0");
            let wave = run(
                "gfx1100",
                &["global_store_dword v0, v1, off", &wait],
                &[0, 1],
            );
            assert_eq!(wave.waits, json!([[1, [[0, 0]]]]), "{wait}");
        }
        for wait in [
            "s_wait_loadcnt 0junk",
            "s_wait_loadcnt 0, 1",
            "s_wait_loadcnt s0",
        ] {
            let wave = run("gfx1200", &["global_load_b32 v0, v1, off", wait], &[0, 1]);
            assert_eq!(wave.waits, json!([]), "{wait}");
            assert!(
                wave.info["waitAnalysis"]["unsupportedWaits"]
                    .as_u64()
                    .unwrap()
                    > 0
            );
        }
    }
    #[test]
    fn atomic_return_modifiers_select_load_or_store_dependencies() {
        for architecture in ["gfx900", "gfx1030", "gfx1100", "gfx1200"] {
            let modern = architecture == "gfx1200";
            let returning = if modern { "th:TH_ATOMIC_RETURN" } else { "glc" };
            let load_wait = if modern {
                "s_wait_loadcnt 0"
            } else {
                "s_waitcnt vmcnt(0)"
            };
            let store_wait = if modern {
                "s_wait_storecnt 0"
            } else {
                "s_waitcnt_vscnt null, 0"
            };
            for prefix in ["global", "buffer", "image", "flat"] {
                let first = format!("{prefix}_atomic_add v0, v1, s[0:3] {returning}");
                let second =
                    format!("{prefix}_atomic_add v0, v1, s[0:3] // glc th:TH_ATOMIC_RETURN");
                let wave = run(
                    architecture,
                    &[&first, &second, load_wait, store_wait],
                    if architecture == "gfx900" {
                        &[0, 1, 2]
                    } else {
                        &[0, 1, 2, 3]
                    },
                );
                let expected = if architecture == "gfx900" {
                    json!([[2, [[0, 0], [1, 0]]]])
                } else {
                    json!([[2, [[0, 0]]], [3, [[1, 0]]]])
                };
                assert_eq!(wave.waits, expected, "{architecture} {prefix}");
            }
        }
    }

    #[test]
    fn atomic_hints_accept_numeric_spellings_and_reject_ambiguous_modifiers() {
        for value in 0..8 {
            for hint in [format!("th:{value}"), format!("th: 0x{value:x}")] {
                assert_eq!(
                    atomic_returns(&format!("image_atomic_add v0 {hint}"), 12),
                    Some(value & 1 != 0)
                );
            }
        }
        for hint in [
            "TH_ATOMIC_RETURN",
            "TH_ATOMIC_RT_RETURN",
            "TH_ATOMIC_NT_RETURN",
        ] {
            assert_eq!(
                atomic_returns(&format!("image_atomic_add v0 th:{hint}"), 12),
                Some(true)
            );
        }
        for hint in [
            "th:",
            "th:8",
            "th:TH_LOAD_NT",
            "th:TH_ATOMIC_RETURN th:0",
            "th:1garbage",
        ] {
            let wave = run(
                "gfx1200",
                &[
                    &format!("global_atomic_add_u32 v0, v1 {hint}"),
                    "s_wait_loadcnt 0",
                    "s_wait_storecnt 0",
                ],
                &[0, 1, 2],
            );
            assert_eq!(wave.waits, json!([]));
            assert_eq!(wave.info["waitAnalysis"]["unsupportedWaits"], 1);
        }
        assert_eq!(atomic_returns("global_atomic_add v0 glc glc:0", 10), None);
        assert_eq!(atomic_returns("global_atomic_add v0 glc:2", 10), None);
    }

    #[test]
    fn gfx12_image_operations_use_independent_counters_and_occurrences() {
        let wave = run(
            "gfx1200",
            &[
                "image_load v0, v1, s[0:7] dmask:0x1 dim:SQ_RSRC_IMG_2D",
                "image_store v0, v1, s[0:7] dmask:0x1 dim:SQ_RSRC_IMG_2D",
                "image_sample v0, v[1:2], s[0:7], s[8:11] dmask:0x1 dim:SQ_RSRC_IMG_2D",
                "image_gather4 v0, v[1:2], s[0:7], s[8:11] dmask:0x1 dim:SQ_RSRC_IMG_2D",
                "s_wait_loadcnt 0",
                "s_wait_storecnt 0",
                "s_wait_samplecnt 1",
                "s_wait_samplecnt 0",
            ],
            &[0, 1, 2, 3, 4, 5, 6, 7],
        );
        assert_eq!(
            wave.waits,
            json!([[4, [[0, 0]]], [5, [[1, 0]]], [6, [[2, 0]]], [7, [[3, 0]]]])
        );
        let occurrences = wave.info["waitAnalysis"]["occurrences"].as_array().unwrap();
        for (row, source) in occurrences.iter().zip(0..4) {
            assert_eq!(row["sources"][0]["token"], source);
            assert_eq!(row["sources"][0]["issueToWaitCycles"], 40.0);
        }
        assert_eq!(
            wave.info["waitAnalysis"]["conservativeImageOrdering"],
            false
        );
    }

    #[test]
    fn legacy_images_retain_ambiguous_partial_waits_until_zero() {
        for architecture in ["gfx900", "gfx1030", "gfx1100"] {
            let wave = run(
                architecture,
                &[
                    "global_load_dword v0, v1, off",
                    "image_load v0, v1, s[0:7] dmask:0x1",
                    "image_sample v0, v[1:2], s[0:7], s[8:11] dmask:0x1",
                    "s_waitcnt vmcnt(1)",
                    "s_waitcnt vmcnt(0)",
                ],
                &[0, 1, 2, 3, 4, 0, 0, 3, 4],
            );
            assert_eq!(
                wave.waits,
                json!([[4, [[0, 0], [1, 0], [2, 0]]], [3, [[0, 0]]], [4, [[0, 0]]]])
            );
            let rows = wave.info["waitAnalysis"]["occurrences"].as_array().unwrap();
            assert_eq!(rows[0]["waitToken"], 4);
            assert_eq!(rows[1]["sources"][0]["token"], 5);
            assert_eq!(rows[2]["sources"][0]["token"], 6);
            assert_eq!(wave.info["waitAnalysis"]["conservativeImageOrdering"], true);
        }
        for (architecture, wait) in [
            ("gfx900", "s_waitcnt vmcnt(0)"),
            ("gfx1030", "s_waitcnt_vscnt null, 0"),
            ("gfx1100", "s_waitcnt_vscnt null, 0"),
        ] {
            let wave = run(
                architecture,
                &["image_store v0, v1, s[0:7] dmask:0x1", wait],
                &[0, 1],
            );
            assert_eq!(wave.waits, json!([[1, [[0, 0]]]]));
        }
    }

    #[test]
    fn image_variants_route_by_opcode_and_idle_drains_all() {
        for (opcode, wait) in [
            ("image_load_mip", "s_wait_loadcnt 0"),
            ("image_msaa_load", "s_wait_samplecnt 0"),
            ("image_get_resinfo", "s_wait_loadcnt 0"),
            ("image_store_mip", "s_wait_storecnt 0"),
            ("image_sample_c_lz_o", "s_wait_samplecnt 0"),
            ("image_gather4_c_lz_o", "s_wait_samplecnt 0"),
            ("image_get_lod", "s_wait_samplecnt 0"),
        ] {
            for ending in [wait, "s_wait_idle"] {
                let wave = run(
                    "gfx1200",
                    &[&format!("{opcode} v0, v1, s[0:7]"), ending],
                    &[0, 0, 1],
                );
                assert_eq!(
                    wave.waits,
                    json!([[1, [[0, 0], [0, 0]]]]),
                    "{opcode}: {ending}"
                );
            }
        }
        let wave = run(
            "gfx1200",
            &["s_nop 0 ; image_sample", "s_wait_samplecnt 0"],
            &[0, 1],
        );
        assert_eq!(wave.waits, json!([]));
    }

    #[test]
    fn loop_occurrences_and_partial_waits() {
        let wave = run(
            "gfx1100",
            &[
                "global_load_dword v0, v1, off",
                "s_waitcnt vmcnt(1)",
                "s_waitcnt vmcnt(0)",
            ],
            &[0, 0, 1, 2],
        );
        assert_eq!(wave.waits, json!([[1, [[0, 0]]], [2, [[0, 0]]]]));
        let occurrences = &wave.info["waitAnalysis"]["occurrences"];
        assert_eq!(occurrences[0]["sources"][0]["token"], 0);
        assert_eq!(occurrences[1]["sources"][0]["token"], 1);
        assert_eq!(occurrences[1]["sources"][0]["issueToWaitCycles"], 20.0);
    }
    #[test]
    fn scalar_parallel_and_named_store_counter() {
        let wave = run(
            "gfx1030",
            &[
                "s_load_dword s0, s[2:3], 0",
                "s_waitcnt lgkmcnt(1)",
                "s_waitcnt lgkmcnt(0)",
                "global_store_dword v0, v1, off",
                "s_waitcnt_vscnt null, 0x0",
            ],
            &[0, 1, 2, 3, 4],
        );
        assert_eq!(wave.waits, json!([[2, [[0, 0]]], [4, [[3, 0]]]]));
    }
    #[test]
    fn gfx9_flat_and_gfx12_separate_counters() {
        let wave = run(
            "gfx942",
            &[
                "flat_load_dword v0, v[1:2]",
                "s_waitcnt vmcnt(0) lgkmcnt(0)",
            ],
            &[0, 1],
        );
        assert_eq!(wave.waits, json!([[1, [[0, 0]]]]));
        let wave = run(
            "gfx1200",
            &[
                "ds_read_b32 v0, v1",
                "s_load_dword s0, s[2:3], 0",
                "global_store_dword v0, v1, off",
                "s_wait_dscnt 0",
                "s_wait_kmcnt 0",
                "s_wait_storecnt 0",
            ],
            &[0, 1, 2, 3, 4, 5],
        );
        assert_eq!(
            wave.waits,
            json!([[3, [[0, 0]]], [4, [[1, 0]]], [5, [[2, 0]]]])
        );
    }
    #[test]
    fn idle_drains_flat_and_unknown_encodings_do_not_claim_dependencies() {
        let wave = run(
            "gfx1200",
            &["flat_store_dword v0, v1", "s_wait_idle"],
            &[0, 1],
        );
        assert_eq!(wave.waits, json!([[1, [[0, 0]]]]));
        let wave = run(
            "gfx942",
            &["global_load_dword v0, v1, off", "s_waitcnt invalid"],
            &[0, 1],
        );
        assert_eq!(wave.waits, json!([]));
        assert_eq!(wave.info["waitAnalysis"]["unsupportedWaits"], 1);
        assert!(run("unknown", &["s_wait_idle"], &[0]).waits.is_null());
    }
    #[test]
    fn nonzero_store_wait_preserves_the_newest_store() {
        let wave = run(
            "gfx1100",
            &[
                "global_store_dword v0, v1, off",
                "s_waitcnt_vscnt null, 1",
                "s_waitcnt_vscnt null, 0",
            ],
            &[0, 0, 1, 2],
        );
        let occurrences = &wave.info["waitAnalysis"]["occurrences"];
        assert_eq!(occurrences[0]["waitToken"], 2);
        assert_eq!(occurrences[0]["sources"].as_array().unwrap().len(), 1);
        assert_eq!(occurrences[0]["sources"][0]["token"], 0);
        assert_eq!(occurrences[1]["sources"][0]["token"], 1);
    }
    #[test]
    fn flat_store_wait_does_not_require_a_second_pending_dependency() {
        for (architecture, wait) in [
            ("gfx1100", "s_waitcnt lgkmcnt(0)"),
            ("gfx1200", "s_wait_dscnt 0"),
        ] {
            let wave = run(architecture, &["flat_store_dword v0, v1", wait], &[0, 1]);
            assert_eq!(wave.waits, json!([[1, [[0, 0]]]]));
        }
    }

    #[test]
    fn export_groups_preserve_order_and_defer_mixed_partial_waits() {
        for arch in ["gfx942", "gfx1030", "gfx1100"] {
            let wave = run(
                arch,
                &[
                    "exp mrt0 v0, v1, v2, v3",
                    "s_waitcnt expcnt(1)",
                    "s_waitcnt expcnt(0)",
                ],
                &[0, 0, 1, 2],
            );
            assert_eq!(wave.waits, json!([[1, [[0, 0]]], [2, [[0, 0]]]]));
            let wave = run(
                arch,
                &[
                    "exp pos0 v0, v1, v2, v3",
                    "exp param0 v0, v1, v2, v3",
                    "s_waitcnt expcnt(1)",
                    "s_waitcnt expcnt(0)",
                ],
                &[0, 1, 2, 3],
            );
            assert_eq!(wave.waits, json!([[3, [[0, 0], [1, 0]]]]));
            assert_eq!(
                wave.info["waitAnalysis"]["conservativeExportOrdering"],
                true
            );
        }
    }

    #[test]
    fn gds_tracks_register_lock_and_memory_dependencies_independently() {
        for arch in ["gfx942", "gfx1030", "gfx1100"] {
            for opcode in [
                "ds_write_b32 v0, v1 gds",
                "ds_gws_barrier v0",
                "ds_ordered_count v0, v1",
            ] {
                let wave = run(
                    arch,
                    &[opcode, "s_waitcnt expcnt(0)", "s_waitcnt lgkmcnt(0)"],
                    &[0, 1, 2],
                );
                assert_eq!(
                    wave.waits,
                    json!([[1, [[0, 0]]], [2, [[0, 0]]]]),
                    "{arch} {opcode}"
                );
            }
            let wave = run(
                arch,
                &[
                    "global_store_dword v0, v1, off",
                    "ds_write_b32 v0, v1",
                    "s_waitcnt expcnt(0)",
                ],
                &[0, 1, 2],
            );
            assert_eq!(wave.waits, json!([]));
        }
    }

    #[test]
    fn gfx11_lds_direct_and_interp_export_waits_preserve_occurrences() {
        for opcode in ["lds_direct_load v0", "lds_param_load v0, attr0.x"] {
            for wait in [
                "v_interp_p10_f32 v0, v1, v2, v3",
                "v_interp_p2_f32 v0, v1, v2, v3 wait_exp:0",
                "s_waitcnt expcnt(0)",
                "s_waitcnt_expcnt null, 0x0",
                "v_interp_p2_f32 v0, v1, v2, v3 wait_exp:0x0",
            ] {
                let wave = run("gfx1100", &[opcode, wait], &[0, 1]);
                assert_eq!(wave.waits, json!([[1, [[0, 0]]]]), "{opcode} {wait}");
            }
        }
        let wave = run(
            "gfx1100",
            &[
                "lds_direct_load v0",
                "v_interp_p10_f32 v0, v1, v2, v3 wait_exp:7",
                "s_waitcnt expcnt(0)",
            ],
            &[0, 1, 2],
        );
        assert_eq!(wave.waits, json!([[2, [[0, 0]]]]));
        let wave = run(
            "gfx1100",
            &[
                "lds_direct_load v0",
                "v_interp_p10_f32 v0, v1, v2, v3 wait_exp:8",
                "s_waitcnt expcnt(0)",
            ],
            &[0, 1, 2],
        );
        assert_eq!(wave.waits, json!([]));
        assert!(
            wave.info["waitAnalysis"]["unsupportedWaits"]
                .as_u64()
                .unwrap()
                > 0
        );
    }

    #[test]
    fn export_wait_preserves_supported_counter_links() {
        for architecture in ["gfx942", "gfx1030", "gfx1100"] {
            for export_wait in ["s_waitcnt expcnt(0)", "s_waitcnt expcnt(7)"] {
                let wave = run(
                    architecture,
                    &[
                        "global_load_dword v0, v1, off",
                        export_wait,
                        "s_waitcnt vmcnt(0)",
                    ],
                    &[0, 1, 2],
                );
                assert_eq!(
                    wave.waits,
                    json!([[2, [[0, 0]]]]),
                    "{architecture} {export_wait}"
                );
                assert_eq!(wave.info["waitAnalysis"]["unsupportedWaits"], 0);
                assert_eq!(wave.info["waitAnalysis"]["unmodeledCounters"], json!([]));
            }
            for export_wait in ["s_waitcnt expcnt(8)", "s_waitcnt expcnt(-1)"] {
                let wave = run(
                    architecture,
                    &[
                        "global_load_dword v0, v1, off",
                        export_wait,
                        "s_waitcnt vmcnt(0)",
                    ],
                    &[0, 1, 2],
                );
                assert!(wave.waits.as_array().unwrap().is_empty());
                assert!(
                    wave.info["waitAnalysis"]["unsupportedWaits"]
                        .as_u64()
                        .unwrap()
                        > 0
                );
            }
        }
    }

    #[test]
    fn packed_zero_waits_link_native_occurrences() {
        for architecture in ["gfx942", "gfx1030", "gfx1100"] {
            let wave = run(
                architecture,
                &["global_load_dword v0, v1, off", "s_waitcnt 0"],
                &[0, 1],
            );
            assert_eq!(wave.waits, json!([[1, [[0, 0]]]]));
            assert_eq!(wave.info["waitAnalysis"]["packedWaitTokens"], 1);
            assert_eq!(wave.info["waitAnalysis"]["unmodeledCounters"], json!([]));
        }
        for operation in ["load", "store"] {
            let wave = run(
                "gfx1200",
                &[
                    &format!("global_{operation}_dword v0, v1, off"),
                    "ds_read_b32 v0, v1",
                    &format!("s_wait_{operation}cnt_dscnt 0"),
                ],
                &[0, 1, 2],
            );
            assert_eq!(wave.waits, json!([[2, [[0, 0], [1, 0]]]]));
        }
    }

    #[test]
    fn maximum_wait_does_not_drain_accumulated_issue_history() {
        let mut order = vec![0; 70];
        order.extend([1, 2]);
        let wave = run(
            "gfx1100",
            &[
                "global_load_dword v0, v1, off",
                "s_waitcnt 0xffff",
                "s_waitcnt vmcnt(0)",
            ],
            &order,
        );
        let occurrences = wave.info["waitAnalysis"]["occurrences"].as_array().unwrap();
        assert_eq!(occurrences.len(), 1);
        assert_eq!(occurrences[0]["waitToken"], 71);
        assert_eq!(occurrences[0]["sources"].as_array().unwrap().len(), 70);
    }

    #[test]
    fn imported_links_remain_authoritative() {
        let mut wave = run("gfx942", &[], &[]);
        wave.waits = json!([[42, [[17, 0]]]]);
        reconstruct(&mut wave, "gfx942", &[]);
        assert_eq!(wave.waits, json!([[42, [[17, 0]]]]));
    }
}
