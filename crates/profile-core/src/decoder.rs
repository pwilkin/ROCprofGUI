use crate::{load_capture_with_wave_states, Capture};
use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, OnceLock,
    },
    time::{Duration, Instant},
};

static BUNDLED_PYTHON_PACKAGE: OnceLock<std::path::PathBuf> = OnceLock::new();

pub fn set_bundled_python_package(path: std::path::PathBuf) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!(
            "Bundled decoder Python package is missing: {}",
            path.display()
        ));
    }
    BUNDLED_PYTHON_PACKAGE
        .set(path)
        .map_err(|_| "Bundled decoder package was already configured".into())
}

pub fn bundled_python_package_configured() -> bool {
    BUNDLED_PYTHON_PACKAGE.get().is_some()
}

pub struct DecoderConfig {
    pub python: std::ffi::OsString,
    pub library: Option<std::ffi::OsString>,
    pub python_package: Option<std::path::PathBuf>,
    pub timeout: Duration,
}
impl Default for DecoderConfig {
    fn default() -> Self {
        Self {
            python: std::env::var_os("ROCPROF_GUI_PYTHON").unwrap_or_else(|| "python3".into()),
            library: std::env::var_os("ROCPROF_GUI_DECODER_LIB"),
            python_package: std::env::var_os("ROCPROF_GUI_DECODER_PACKAGE")
                .map(std::path::PathBuf::from)
                .or_else(|| BUNDLED_PYTHON_PACKAGE.get().cloned()),
            timeout: Duration::from_secs(300),
        }
    }
}
pub fn decode(
    path: &Path,
    config: &DecoderConfig,
    cancelled: &AtomicBool,
) -> Result<Capture, String> {
    decode_with_wave_states(path, config, cancelled, true)
}
pub struct DecodedRun {
    pub root: std::path::PathBuf,
    pub log: String,
    pub python_package: Option<std::path::PathBuf>,
}

/// Parses `DECODER-WARNING:` lines the worker prints for decoder INFO records.
pub fn decoder_warnings(log: &str) -> Vec<String> {
    log.lines()
        .filter_map(|line| line.strip_prefix("DECODER-WARNING: "))
        .map(str::to_owned)
        .collect()
}

pub fn decode_with_wave_states(
    path: &Path,
    config: &DecoderConfig,
    cancelled: &AtomicBool,
    load_wave_states: bool,
) -> Result<Capture, String> {
    let workspace = Arc::new(tempfile::tempdir().map_err(|e| e.to_string())?);
    let run = run_decoder_in(&workspace, path, config, cancelled)?;
    let mut capture = load_capture_with_wave_states(&run.root, load_wave_states)?;
    capture.format = "raw ATT · decoder V2".into();
    capture.name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into();
    capture
        .metadata
        .insert("rawInput".into(), path.display().to_string());
    capture.metadata.insert(
        "decoderLog".into(),
        run.log.chars().take(12000).collect(),
    );
    for warning in decoder_warnings(&run.log) {
        capture.warnings.push(format!("Trace decoder: {warning}"));
    }
    if let Some(package) = run.python_package {
        capture
            .metadata
            .insert("decoderPythonPackage".into(), package.display().to_string());
    }
    capture.workspace = Some(workspace);
    Ok(capture)
}

/// Runs the decoder worker inside `workspace`, which the caller keeps alive for as
/// long as the decoded files are read (waves load lazily from it).
pub fn run_decoder_in(
    workspace: &tempfile::TempDir,
    path: &Path,
    config: &DecoderConfig,
    cancelled: &AtomicBool,
) -> Result<DecodedRun, String> {
    let python_package = config
        .python_package
        .as_ref()
        .map(|path| {
            path.canonicalize()
                .map_err(|e| format!("Cannot open decoder Python package {}: {e}", path.display()))
        })
        .transpose()?;
    let script = workspace.path().join("decode.py");
    fs::write(&script, include_str!("decode.py")).map_err(|e| e.to_string())?;
    let manifest_input = workspace.path().join("input.rocprof-att");
    let input = if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("rocprof-att"))
    {
        let manifest = crate::att_manifest::read(path)?;
        fs::write(
            &manifest_input,
            serde_json::to_vec(&manifest).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        manifest_input.as_path()
    } else {
        path
    };
    let output = workspace.path().join("output");
    fs::create_dir(&output).map_err(|e| e.to_string())?;
    let log = workspace.path().join("decoder.log");
    let file = fs::File::create(&log).map_err(|e| e.to_string())?;
    let mut command = Command::new(&config.python);
    command
        .arg(&script)
        .arg(input)
        .arg(&output)
        .stdin(Stdio::null())
        .stdout(file.try_clone().map_err(|e| e.to_string())?)
        .stderr(file);
    if let Some(library) = &config.library {
        command.arg("--lib").arg(library);
    }
    if let Some(package) = &python_package {
        command.arg("--python-package").arg(package);
    }
    let mut child = command.spawn().map_err(|e|format!("Cannot start decoder Python: {e}. Set ROCPROF_GUI_PYTHON to an available Python interpreter."))?;
    let start = Instant::now();
    let status = loop {
        let stop = if cancelled.load(Ordering::Relaxed) {
            Some("Raw trace import cancelled")
        } else if start.elapsed() > config.timeout {
            Some("Raw trace decoder timed out")
        } else if fs::metadata(&log)
            .map(|m| m.len() > 16 * 1024 * 1024)
            .unwrap_or(false)
        {
            Some("Decoder diagnostic output exceeded 16 MiB")
        } else {
            None
        };
        if let Some(reason) = stop {
            let _ = child.kill();
            let _ = child.wait();
            return Err(reason.into());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e.to_string());
            }
        }
    };
    let diagnostic = fs::read_to_string(&log).unwrap_or_default();
    if !status.success() {
        return Err(format!("Raw trace decoder failed ({status}). Check the decoder Python package, ROCm disassembly tools and ROCPROF_GUI_DECODER_LIB for decoder V2.\n{}",diagnostic.chars().rev().take(12000).collect::<String>().chars().rev().collect::<String>()));
    }
    let mut roots = vec![];
    if output.join("filenames.json").is_file() {
        roots.push(output.clone());
    }
    for entry in fs::read_dir(&output).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_type().map_err(|e| e.to_string())?.is_dir()
            && entry.path().join("filenames.json").is_file()
        {
            roots.push(entry.path());
        }
    }
    if roots.len() != 1 {
        return Err(format!(
            "Expected one decoded run, found {}. Open a specific ATT file to select one run.",
            roots.len()
        ));
    }
    Ok(DecodedRun {
        root: roots.remove(0),
        log: diagnostic,
        python_package,
    })
}
