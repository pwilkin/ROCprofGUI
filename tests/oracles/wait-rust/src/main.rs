use profile_core::{Instruction, Wave};
use serde_json::{json, Value};
use std::io::{self, BufRead};
fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|arg| arg == "--capture") {
        let input = std::path::Path::new(&args[2]);
        let output = std::path::Path::new(&args[3]);
        std::fs::create_dir_all(output).unwrap();
        let flag = std::sync::atomic::AtomicBool::new(false);
        let capture = if input
            .extension()
            .is_some_and(|extension| extension == "rocprof-capture")
        {
            profile_core::capture_archive::read(input, &flag).unwrap()
        } else {
            profile_core::decoder::decode(input, &Default::default(), &flag).unwrap()
        };
        let archive = output.join("decoded.rocprof-capture");
        if archive != input {
            if let Err(error) = profile_core::capture_archive::write(&archive, &capture, &flag) {
                std::fs::write(output.join("archive-error.txt"), error).unwrap();
            }
        }
        if args.iter().any(|arg| arg == "--verify-archive") {
            let restored = profile_core::capture_archive::read(&archive, &flag).unwrap();
            assert_eq!(capture.instructions.len(), restored.instructions.len());
            for (original, reopened) in capture.instructions.iter().zip(&restored.instructions) {
                assert_eq!(
                    serde_json::to_value(original).unwrap(),
                    serde_json::to_value(reopened).unwrap()
                );
            }
            assert_eq!(capture.waves.len(), restored.waves.len());
            for (original, reopened) in capture.waves.iter().zip(&restored.waves) {
                let before = profile_core::load_wave(&capture, &original.id).unwrap();
                let after = profile_core::load_wave(&restored, &reopened.id).unwrap();
                assert_eq!(
                    serde_json::to_value(&before.tokens).unwrap(),
                    serde_json::to_value(&after.tokens).unwrap()
                );
                assert_eq!(before.info["waitAnalysis"], after.info["waitAnalysis"]);
            }
            std::fs::write(output.join("archive-roundtrip.json"), serde_json::to_vec(&json!({"instructions":restored.instructions.len(),"waves":restored.waves.len(),"instructionFieldsEqual":true,"waveTokensEqual":true,"waitAnalysisEqual":true})).unwrap()).unwrap();
        }
        let mut reports = Vec::new();
        let mut streams = Vec::new();
        let mut used = std::collections::HashSet::new();
        let instruction_ids: std::collections::HashSet<_> =
            capture.instructions.iter().map(|line| line.index).collect();
        for index in &capture.waves {
            let wave = profile_core::load_wave(&capture, &index.id).unwrap();
            used.extend(wave.tokens.iter().map(|token| token.instruction));
            streams.push(json!({"wave":index.id,"instructions":wave.tokens.iter().map(|token| token.instruction).collect::<Vec<_>>()}));
            let analysis = &wave.info["waitAnalysis"];
            let missing: Vec<_> = wave
                .tokens
                .iter()
                .filter(|token| !instruction_ids.contains(&token.instruction))
                .map(|token| json!({"instruction":token.instruction,"category":token.category}))
                .collect();
            reports.push(json!({"wave":index.id,"tokens":wave.tokens.len(),"waits":wave.waits,"analysis":analysis,"missing":missing}));
        }
        std::fs::write(
            output.join("instructions.json"),
            serde_json::to_vec(
                &capture
                    .instructions
                    .iter()
                    .filter(|line| used.contains(&line.index))
                    .collect::<Vec<_>>(),
            )
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            output.join("waves.json"),
            serde_json::to_vec(&reports).unwrap(),
        )
        .unwrap();
        std::fs::write(
            output.join("streams.json"),
            serde_json::to_vec(&streams).unwrap(),
        )
        .unwrap();
        println!(
            "{}",
            json!({"architecture":capture.architecture,"waves":capture.waves.len(),"instructions":capture.instructions.len(),"report":output})
        );
        return;
    }

    for line in io::stdin().lock().lines() {
        let input: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let rows = input["lines"].as_array().unwrap();
        if input["expand"] == true {
            println!(
                "{}",
                json!(rows
                    .iter()
                    .map(|text| profile_core::wait_encoding::expand(
                        input["architecture"].as_str().unwrap(),
                        text.as_str().unwrap()
                    ))
                    .collect::<Vec<_>>())
            );
            continue;
        }
        let code: Vec<Instruction> = rows
            .iter()
            .enumerate()
            .map(|(index, text)| {
                serde_json::from_value(json!({
                    "index": index,"text":text,"source":"","address":"","codeObject":"",
                    "hits":0,"latency":0,"stall":0,"idle":0,"pcSamples":0
                }))
                .unwrap()
            })
            .collect();
        let mut wave: Wave = serde_json::from_value(json!({
            "id":"oracle","start":0,"end":rows.len() * 10,"cu":0,"states":[],"waits":null,"info":{},
            "tokens": (0..rows.len()).map(|index| json!({
                "start":index * 10,"duration":5,"stall":0,"category":0,"instruction":index
            })).collect::<Vec<_>>()
        }))
        .unwrap();
        profile_core::waits::reconstruct(&mut wave, input["architecture"].as_str().unwrap(), &code);
        println!(
            "{}",
            json!({"waits":wave.waits,"analysis":wave.info["waitAnalysis"]})
        );
    }
}
