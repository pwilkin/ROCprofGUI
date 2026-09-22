use std::{collections::BTreeMap, io::Write};

#[test]
#[ignore = "requires ROCPROF_WAVE_ARCHIVE and ROCPROF_UTILIZATION_ORACLE"]
fn archived_utilization_projection_for_export_validation() {
    let path = std::env::var_os("ROCPROF_WAVE_ARCHIVE").unwrap();
    let output = std::env::var_os("ROCPROF_UTILIZATION_ORACLE").unwrap();
    let capture = profile_core::load_capture(std::path::Path::new(&path)).unwrap();
    let model = profile_core::utilization::Model::from_capture(&capture).unwrap();
    let isa: BTreeMap<_, _> = capture
        .instructions
        .iter()
        .map(|i| (i.index, i.text.as_str()))
        .collect();
    let mut writer = std::io::BufWriter::new(std::fs::File::create(output).unwrap());
    let mut count = 0;
    for index in capture.waves.iter().filter(|w| w.se == 0) {
        let wave = profile_core::load_wave(&capture, &index.id).unwrap();
        let cu = wave
            .cu
            .map(|v| format!("CU {v}"))
            .unwrap_or_else(|| format!("CU unknown [{}]", wave.id));
        for (n, mut token) in wave.tokens.into_iter().enumerate() {
            let text = isa.get(&token.instruction).copied().unwrap_or("");
            if !model.project(&mut token, text).unwrap() {
                continue;
            }
            let (pipe, shared) = model.pipe(token.category, false);
            let scope = if shared {
                "Shared".into()
            } else {
                format!("S{}", index.simd)
            };
            serde_json::to_writer(
                &mut writer,
                &(
                    format!("{}:{n}", index.id),
                    token.start,
                    token.start + token.duration,
                    format!("{cu} · {scope} · {pipe}"),
                ),
            )
            .unwrap();
            writer.write_all(b"\n").unwrap();
            count += 1;
        }
    }
    writer.flush().unwrap();
    assert!(count > 0);
    eprintln!("{count} native projected issue intervals and pipe lanes written");
}
