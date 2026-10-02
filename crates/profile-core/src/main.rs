fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("Usage: profile-core CAPTURE_PATH [WAVE_ID | --bundle | --hidden]");
        std::process::exit(2);
    };
    // Through the same fingerprinted, cancellable router the GUI and MCP use, so
    // the CLI computes the input fingerprint and sets input_path/input_fingerprint
    // (which the capture-archive overwrite guards key on) instead of diverging.
    let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let result = profile_core::project::load_input(std::path::Path::new(&path), cancelled)
        .and_then(|mut capture| match args.next().as_deref() {
            Some("--hidden") => {
                serde_json::to_value(profile_core::hidden::analyze_capture(&capture)?)
                    .map_err(|e| e.to_string())
            }
            Some("--bundle") => {
                let mut bytes = 0;
                for name in capture.source_files.clone() {
                    let source = profile_core::source_files::read_snapshot(&capture, &name)?;
                    bytes += source.text.len();
                    if bytes > 16 * 1024 * 1024 {
                        return Err("Source bundle exceeds 16 MiB".into());
                    }
                    capture.sources.insert(name, source.text);
                }

                let waves: Result<std::collections::BTreeMap<_, _>, _> = capture
                    .waves
                    .iter()
                    .map(|w| {
                        profile_core::load_wave(&capture, &w.id).map(|wave| (w.id.clone(), wave))
                    })
                    .collect();
                Ok(serde_json::json!({"capture":capture,"waves":waves?}))
            }
            Some(id) => profile_core::load_wave(&capture, id)
                .and_then(|w| serde_json::to_value(w).map_err(|e| e.to_string())),
            None => serde_json::to_value(capture).map_err(|e| e.to_string()),
        });
    match result {
        Ok(value) => println!("{}", value),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
