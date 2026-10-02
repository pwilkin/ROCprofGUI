use crate::{Capture, Token, Wave};
use std::sync::atomic::{AtomicBool, Ordering};
fn check(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) {
        Err("Wave query cancelled".into())
    } else {
        Ok(())
    }
}
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HiddenCost {
    pub idle: i64,
    pub stall: i64,
    pub issue: i64,
}
impl HiddenCost {
    fn total(self) -> i64 {
        self.idle + self.stall + self.issue
    }
    fn add(&mut self, other: Self) {
        self.idle += other.idle;
        self.stall += other.stall;
        self.issue += other.issue;
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HiddenToken {
    pub start: f64,
    pub instruction: i64,
    pub duration: f64,
    pub stall: f64,
    pub idle: f64,
    pub hidden: HiddenCost,
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HiddenAnalysis {
    pub by_instruction: BTreeMap<i64, HiddenCost>,
    pub by_wave: BTreeMap<String, Vec<HiddenToken>>,
    pub model: String,
    #[serde(default)]
    pub warnings: Vec<String>,
}
#[derive(Debug, Clone, Copy)]
struct Util {
    end: i64,
    cycles: i64,
}
impl Util {
    fn begin(self) -> i64 {
        self.end - self.cycles
    }
}

// Matches RCV compute_util, including accumulated overlapping pipe durations.
fn compute_util(mut input: Vec<(i64, i64)>) -> Vec<Util> {
    input.sort_by_key(|i| i.0);
    let Some(&(first, _)) = input.first() else {
        return vec![];
    };
    let mut intervals = vec![(first, 0)];
    for (clock, cycles) in input {
        let last = intervals.last_mut().unwrap();
        if clock > last.0 + last.1 {
            intervals.push((clock, cycles));
        } else {
            last.1 += cycles;
        }
    }
    intervals
        .into_iter()
        .map(|(clock, cycles)| Util {
            end: clock + cycles,
            cycles,
        })
        .collect()
}
fn intersection(intervals: &[Util], start: i64, duration: i64) -> i64 {
    if duration <= 0 {
        return 0;
    }
    let end = start + duration;
    intervals[intervals.partition_point(|u| u.end < start)..]
        .iter()
        .take_while(|u| u.begin() <= end)
        .map(|u| {
            let delta = (u.end - start).min(end - u.begin());
            delta.min(duration.min(u.cycles)).max(0)
        })
        .sum()
}
fn union(a: &[Util], b: &[Util]) -> Vec<Util> {
    let mut inputs: Vec<_> = a.iter().chain(b).copied().collect();
    inputs.sort_by_key(|u| u.begin());
    let mut out: Vec<Util> = vec![];
    for u in inputs {
        if u.cycles <= 0 {
            continue;
        }
        if let Some(last) = out.last_mut() {
            if u.begin() <= last.end {
                let begin = last.begin();
                last.end = last.end.max(u.end);
                last.cycles = last.end - begin;
                continue;
            }
        }
        out.push(u);
    }
    out
}
fn cost(util: &[Util], last: i64, t: &Token) -> HiddenCost {
    let start = t.start as i64;
    let stall = t.stall as i64;
    HiddenCost {
        idle: intersection(util, last, start - last),
        stall: intersection(util, start, stall),
        issue: intersection(util, start + stall, t.duration as i64 - stall),
    }
}
fn greatest(a: HiddenCost, b: HiddenCost) -> HiddenCost {
    if a.total() > b.total() {
        a
    } else {
        b
    }
}
fn no_issue(mut c: HiddenCost) -> HiddenCost {
    c.issue = 0;
    c
}
fn category(t: &Token, isa: &BTreeMap<i64, String>) -> u32 {
    if isa.get(&t.instruction).is_some_and(|text| {
        ["v_mfma", "v_smfma", "v_wmma", "v_swmma"]
            .iter()
            .any(|prefix| text.starts_with(prefix))
    }) {
        13
    } else {
        t.category
    }
}

pub fn analyze_scope(
    waves: &[Wave],
    isa: &BTreeMap<i64, String>,
    other_simd: &[[f64; 2]],
) -> HiddenAnalysis {
    analyze_scope_cancellable(waves, isa, other_simd, &AtomicBool::new(false))
        .expect("uncancelled analysis")
}
pub fn analyze_scope_cancellable(
    waves: &[Wave],
    isa: &BTreeMap<i64, String>,
    other_simd: &[[f64; 2]],
    cancelled: &AtomicBool,
) -> Result<HiddenAnalysis, String> {
    check(cancelled)?;
    let (mut matrix, mut valu, mut vmem, mut scalar) = (vec![], vec![], vec![], vec![]);
    for wave in waves {
        for t in &wave.tokens {
            check(cancelled)?;
            let clock = (t.start + t.stall) as i64;
            let cycles = (t.duration - t.stall) as i64;
            match category(t, isa) {
                13 => {
                    matrix.push((clock, cycles));
                    valu.push((clock, 3 * cycles / 4));
                }
                6 => valu.push((clock, cycles)),
                3..=5 => vmem.push((clock, cycles)),
                1 | 2 => scalar.push((clock, cycles)),
                _ => {}
            }
        }
    }
    vmem.extend(
        other_simd
            .iter()
            .filter(|r| r[1] > 0.)
            .map(|r| (r[0] as i64, r[1] as i64)),
    );
    check(cancelled)?;
    let matrix = compute_util(matrix);
    check(cancelled)?;
    let valu = compute_util(valu);
    check(cancelled)?;
    let vmem = compute_util(vmem);
    check(cancelled)?;
    let scalar = compute_util(scalar);
    check(cancelled)?;
    let math = union(&valu, &matrix);
    check(cancelled)?;
    let vector = union(&math, &vmem);
    check(cancelled)?;
    let all = union(&vector, &scalar);
    let mut result=HiddenAnalysis{model:"RCV pipe priority: MATRIX > VALU > VMEM/LDS/FLAT > SMEM/SALU > other; approximation, including upstream self-overlap behavior".into(),..Default::default()};
    for wave in waves {
        let mut last = wave.start as i64;
        let mut tokens = vec![];
        for t in &wave.tokens {
            check(cancelled)?;
            let hidden = match category(t, isa) {
                13 => greatest(
                    no_issue(cost(&valu, last, t)),
                    no_issue(cost(&matrix, last, t)),
                ),
                6 => greatest(no_issue(cost(&valu, last, t)), cost(&matrix, last, t)),
                3..=5 => greatest(cost(&math, last, t), no_issue(cost(&vmem, last, t))),
                1 | 2 => greatest(cost(&vector, last, t), no_issue(cost(&scalar, last, t))),
                _ => cost(&all, last, t),
            };
            result
                .by_instruction
                .entry(t.instruction)
                .or_default()
                .add(hidden);
            tokens.push(HiddenToken {
                start: t.start,
                instruction: t.instruction,
                duration: t.duration,
                stall: t.stall,
                idle: (t.start - last as f64).max(0.),
                hidden,
            });
            last = (t.start + t.duration) as i64;
        }
        result.by_wave.insert(wave.id.clone(), tokens);
    }
    check(cancelled)?;
    Ok(result)
}
pub fn analyze_capture(capture: &Capture) -> Result<HiddenAnalysis, String> {
    analyze_capture_cancellable(capture, &AtomicBool::new(false))
}
pub fn analyze_capture_cancellable(
    capture: &Capture,
    cancelled: &AtomicBool,
) -> Result<HiddenAnalysis, String> {
    check(cancelled)?;
    if capture.waves.is_empty() {
        return Err("Hidden latency requires detailed SQTT waves".into());
    }
    let isa = capture
        .instructions
        .iter()
        .map(|i| (i.index, i.text.clone()))
        .collect();
    let engines: BTreeSet<_> = capture.waves.iter().map(|w| w.se).collect();
    let mut result = HiddenAnalysis::default();
    let mut count = 0;
    let max_tokens = crate::caps::max_hidden_tokens();
    for se in engines {
        check(cancelled)?;
        let mut loaded = vec![];
        let mut known_cus = BTreeSet::new();
        let mut unknown = 0;
        for (position, index) in capture.waves.iter().enumerate().filter(|(_, w)| w.se == se) {
            let wave = crate::import::load_wave_at_cancellable(capture, position, cancelled)?;
            count += wave.tokens.len();
            if count > max_tokens {
                return Err(format!("Hidden-latency analysis exceeds {max_tokens} tokens; import a smaller capture until the indexed analysis worker is available"));
            }
            if let Some(cu) = wave.cu {
                known_cus.insert(cu);
            } else {
                unknown += 1;
            }
            loaded.push((index.simd, wave));
        }
        let legacy = known_cus.is_empty();
        if legacy {
            result.warnings.push(format!("SE {se}: detailed waves have no CU identity; hidden latency assumes one traced CU in this shader engine (legacy decoder convention)."));
        } else if unknown > 0 {
            result.warnings.push(format!("SE {se}: {unknown} waves have unknown CU identity and are analyzed separately from other waves."));
        }
        let mut groups: BTreeMap<(u32, Option<u32>, Option<String>), Vec<Wave>> = BTreeMap::new();
        for (simd, wave) in loaded {
            check(cancelled)?;
            let identity = if wave.cu.is_none() && !legacy {
                Some(wave.id.clone())
            } else {
                None
            };
            groups
                .entry((simd, wave.cu, identity))
                .or_default()
                .push(wave);
        }
        let other: Vec<_> = capture.other_simd.iter().filter(|r| r.se == se).collect();
        let allow_other = legacy || (known_cus.len() == 1 && unknown == 0);
        let named_gfx = [9, 10, 11, 12]
            .into_iter()
            .find(|major| capture.architecture.starts_with(&format!("gfx{major}")));
        let declared_gfx = capture
            .metadata
            .get("gfxip")
            .and_then(|s| s.parse::<u32>().ok());
        let wgp_identity = named_gfx
            .or(declared_gfx)
            .is_some_and(|n| (10..=12).contains(&n))
            && !(named_gfx.is_some() && declared_gfx.is_some() && named_gfx != declared_gfx);
        let mut other_by_cu: BTreeMap<Option<u32>, Vec<[f64; 2]>> = BTreeMap::new();
        let mut omitted = 0;
        for record in other {
            check(cancelled)?;
            let target = match record.wgp {
                Some(wgp) if wgp_identity && known_cus.contains(&wgp) => Some(Some(wgp)),
                None if allow_other => Some(known_cus.iter().next().copied()),
                _ => None,
            };
            if let Some(target) = target {
                other_by_cu
                    .entry(target)
                    .or_default()
                    .push([record.start, record.duration]);
            } else {
                omitted += 1;
            }
        }
        if omitted > 0 {
            result.warnings.push(format!("SE {se}: {omitted} other-SIMD activity omitted because no matching recorded WGP scope or supported legacy association is available; hidden latency may be underestimated."));
        }
        for ((_, cu, _), waves) in &groups {
            let scope = analyze_scope_cancellable(
                waves,
                &isa,
                other_by_cu.get(cu).map(Vec::as_slice).unwrap_or(&[]),
                cancelled,
            )?;
            result.model = scope.model;
            result.by_wave.extend(scope.by_wave);
            for (line, cost) in scope.by_instruction {
                result.by_instruction.entry(line).or_default().add(cost);
            }
        }
    }
    check(cancelled)?;
    Ok(result)
}
