use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::Instant,
};
use tauri::Manager;

#[derive(Default)]
pub struct Tuning {
    sessions: Mutex<BTreeMap<String, Session>>,
    next: AtomicU64,
}
struct Session {
    directory: PathBuf,
    child: Option<Child>,
    started: Instant,
    error: Option<String>,
}
pub fn stop(child: &mut Child) {
    #[cfg(unix)]
    let _ = Command::new("/bin/kill")
        .args(["-KILL", "--", &format!("-{}", child.id())])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
    let _ = child.wait();
}
impl Drop for Tuning {
    fn drop(&mut self) {
        if let Ok(sessions) = self.sessions.get_mut() {
            for session in sessions.values_mut() {
                if let Some(child) = session.child.as_mut() {
                    stop(child);
                }
            }
        }
    }
}
fn authorize(window: &tauri::WebviewWindow, id: &str) -> Result<(), String> {
    if window.label() != format!("tuning-{id}") {
        return Err("This tuning session belongs to another window".into());
    }
    Ok(())
}

#[tauri::command]
pub async fn open_tuning(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    store: tauri::State<'_, std::sync::Arc<crate::Store>>,
    tuning: tauri::State<'_, std::sync::Arc<Tuning>>,
    capture_id: String,
    dispatch_id: String,
) -> Result<String, String> {
    if window.label() != "main" {
        return Err("Start tuning from the trace workspace".into());
    }
    crate::mcp::ensure_local(&app)?;
    let id = create(&store, &tuning, capture_id, dispatch_id)?;
    show_window(&app, &id)?;
    Ok(id)
}

pub fn create(
    store: &crate::Store,
    tuning: &Tuning,
    capture_id: String,
    dispatch_id: String,
) -> Result<String, String> {
    let captures = store.captures.lock().map_err(|e| e.to_string())?;
    let capture = captures.get(&capture_id).ok_or("Capture no longer open")?;
    let dispatch = capture
        .dispatches
        .iter()
        .find(|d| d.id == dispatch_id)
        .ok_or("Dispatch not found")?;
    let id = format!(
        "{}-{}",
        std::process::id(),
        tuning.next.fetch_add(1, Ordering::Relaxed)
    );
    // RADV captures open a single-shader Vulkan session; everything else a HIP one.
    let vulkan = capture.format.starts_with("RADV RGP");
    let root = match std::env::var_os("ROCPROF_TUNING_ROOT") {
        Some(explicit) => PathBuf::from(explicit),
        None => crate::appdir::subdir(if vulkan { "shader-tuning/sessions" } else { "kernel-tuning/sessions" })?,
    };
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let directory = root.join(&id);
    fs::create_dir(&directory).map_err(|e| e.to_string())?;
    let directory = directory.canonicalize().map_err(|e| e.to_string())?;
    let runtime = root.parent().unwrap_or(&root).join("runtime");
    if vulkan {
        let config = vulkan_config(capture.input_path.as_deref(), &runtime);
        let state = json!({"mode":"vulkan","config":config,"version":1,"id":id,"kernel":dispatch.kernel,"traceDispatch":dispatch,"captureId":capture_id,"runs":[],"directory":directory});
        fs::write(directory.join("session.json"), serde_json::to_vec_pretty(&state).unwrap()).map_err(|e| e.to_string())?;
        fs::write(directory.join("worker.py"), include_str!("shader_worker.py")).map_err(|e| e.to_string())?;
        tuning.sessions.lock().map_err(|e| e.to_string())?.insert(
            id.clone(),
            Session { directory, child: None, started: Instant::now(), error: None },
        );
        return Ok(id);
    }
    let occurrence = capture
        .dispatches
        .iter()
        .take_while(|d| d.id != dispatch_id)
        .filter(|d| d.kernel == dispatch.kernel)
        .count();
    let config = hip_config(capture.input_path.as_deref(), &runtime, occurrence);
    let state = json!({"config":config,"version":1,"id":id,"kernel":dispatch.kernel,"traceDispatch":dispatch,"captureId":capture_id,"runs":[],"directory":directory});
    fs::write(
        directory.join("session.json"),
        serde_json::to_vec_pretty(&state).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    fs::write(
        directory.join("worker.py"),
        include_str!("tuning_worker.py"),
    )
    .map_err(|e| e.to_string())?;
    fs::write(
        directory.join("tuning_calibration.py"),
        include_str!("tuning_calibration.py"),
    )
    .map_err(|e| e.to_string())?;
    fs::write(
        directory.join("calibration.hip"),
        include_str!("calibration.hip"),
    )
    .map_err(|e| e.to_string())?;
    tuning.sessions.lock().map_err(|e| e.to_string())?.insert(
        id.clone(),
        Session {
            directory,
            child: None,
            started: Instant::now(),
            error: None,
        },
    );
    Ok(id)
}

/// The command an MCP `profile_run` recorded for this capture, if it came from one:
/// the job folder holding `profile.json` is the capture's parent or grandparent
/// (`job/rgp/*.rgp`, `job/<host>/*_kernel_trace.csv`).
fn recorded_profile(input: Option<&std::path::Path>) -> Value {
    input
        .into_iter()
        .flat_map(|p| p.ancestors().skip(1).take(2))
        .find_map(|dir| {
            let profile: Value = serde_json::from_slice(&fs::read(dir.join("profile.json")).ok()?).ok()?;
            profile["argv"].is_array().then_some(profile)
        })
        .unwrap_or(Value::Null)
}

/// Defaults for a HIP session: the profiled command when there is one, and
/// otherwise an empty command for the user to fill in. The source tree is the
/// repository holding the application.
fn hip_config(input: Option<&std::path::Path>, runtime: &std::path::Path, occurrence: usize) -> Value {
    let profile = recorded_profile(input);
    let cwd = profile["cwd"].as_str().unwrap_or("");
    let source_tree = profile["argv"][0]
        .as_str()
        .map(|app| std::path::Path::new(cwd).join(app))
        .and_then(|app| app.ancestors().find(|dir| dir.join(".git").exists()).map(|dir| dir.display().to_string()))
        .unwrap_or_default();
    json!({
        "sourceTree": source_tree,
        "cwd": cwd,
        "argv": if profile["argv"].is_array() { profile["argv"].clone() } else { json!([]) },
        "env": if profile["env"].is_object() { profile["env"].clone() } else { json!({}) },
        "occurrence": occurrence,
        "captureLibrary": runtime.join("lib/libkerncap.so"),
        "replay": runtime.join("bin/kerncap-replay"),
        "profiler": "/opt/rocm/bin/rocprofv3",
        "objdump": "/opt/rocm/llvm/bin/llvm-objdump",
        "readobj": "/opt/rocm/llvm/bin/llvm-readobj",
    })
}

/// Defaults for a Vulkan session. When the capture came from an MCP `profile_run`,
/// its job folder records the command, so the same application is rerun for the
/// capture; the ggml source tree is found by walking up from the application.
fn vulkan_config(input: Option<&std::path::Path>, runtime: &std::path::Path) -> Value {
    let profile = recorded_profile(input);
    let mut env = serde_json::Map::new();
    if let Some(recorded) = profile["env"].as_object() {
        for (key, value) in recorded {
            if !key.starts_with("MESA_VK_TRACE") {
                env.insert(key.clone(), value.clone());
            }
        }
    }
    // ggml names its pipelines only with debug markers on; the capture layer
    // selects the dispatch by that name.
    env.entry("GGML_VK_DEBUG_MARKERS").or_insert(json!("1"));
    let argv = profile["argv"].clone();
    let source_tree = argv[0]
        .as_str()
        .map(std::path::Path::new)
        .and_then(|app| app.ancestors().find(|dir| dir.join("ggml/src/ggml-vulkan/vulkan-shaders").is_dir()))
        .map(|dir| dir.display().to_string())
        .unwrap_or_default();
    json!({
        "sourceTree": source_tree,
        "cwd": profile["cwd"].as_str().unwrap_or(""),
        "argv": if argv.is_array() { argv } else { json!([]) },
        "env": env,
        "occurrence": 0,
        "layer": runtime.join("lib/VkLayer_rocprof_capture.json"),
        "replay": runtime.join("bin/vkreplay"),
    })
}

pub fn show_window(app: &tauri::AppHandle, id: &str) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(&format!("tuning-{id}")) {
        return window.set_focus().map_err(|e| e.to_string());
    }
    let label = format!("tuning-{id}");
    let tuning_window = tauri::WebviewWindowBuilder::new(
        app,
        &label,
        tauri::WebviewUrl::App(format!("index.html?tuning={id}").into()),
    )
    .title(format!("Kernel tuning · {id}"))
    .inner_size(1400., 950.)
    .min_inner_size(1000., 700.)
    .build()
    .map_err(|e| e.to_string())?;
    let handle = app.clone();
    let owned = id.to_owned();
    tuning_window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            if let Ok(mut sessions) = handle.state::<std::sync::Arc<Tuning>>().sessions.lock() {
                if let Some(mut session) = sessions.remove(&owned) {
                    if let Some(child) = session.child.as_mut() {
                        stop(child);
                    }
                    prune(&session.directory);
                }
            }
        }
    });
    Ok(())
}

#[tauri::command]
pub fn tuning_start(
    window: tauri::WebviewWindow,
    tuning: tauri::State<'_, std::sync::Arc<Tuning>>,
    id: String,
    request: Value,
) -> Result<(), String> {
    authorize(&window, &id)?;
    crate::mcp::ensure_local(window.app_handle())?;
    start(&tuning, &id, request)
}

pub fn start(tuning: &Tuning, id: &str, mut request: Value) -> Result<(), String> {
    if serde_json::to_vec(&request)
        .map_err(|e| e.to_string())?
        .len()
        > 16 * 1024 * 1024
    {
        return Err("Tuning request exceeds 16 MiB".into());
    }
    if !["search", "select", "capture", "run", "save", "calibrate"]
        .contains(&request["op"].as_str().unwrap_or(""))
    {
        return Err("Invalid tuning operation".into());
    }
    let mut sessions = tuning.sessions.lock().map_err(|e| e.to_string())?;
    let session = sessions.get_mut(id).ok_or("Tuning session is closed")?;
    if session.child.is_some() {
        return Err("A tuning operation is already running".into());
    }
    if let Some(patch) = request.get("config").and_then(Value::as_object) {
        let state: Value = serde_json::from_slice(
            &fs::read(session.directory.join("session.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let mut config = state["config"].clone();
        for (key, value) in patch {
            config[key] = value.clone();
        }
        request["config"] = config;
    }
    fs::write(
        session.directory.join("request.json"),
        serde_json::to_vec(&request).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    let log = fs::File::create(session.directory.join("worker.log")).map_err(|e| e.to_string())?;
    let mut command =
        Command::new(std::env::var_os("ROCPROF_TUNING_PYTHON").unwrap_or("python3".into()));
    command
        .arg(session.directory.join("worker.py"))
        .arg(&session.directory)
        .env("TMPDIR", &session.directory)
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    session.child = Some(command.spawn().map_err(|e| e.to_string())?);
    session.started = Instant::now();
    session.error = None;
    Ok(())
}

#[tauri::command]
pub fn tuning_status(
    window: tauri::WebviewWindow,
    tuning: tauri::State<'_, std::sync::Arc<Tuning>>,
    id: String,
) -> Result<Value, String> {
    authorize(&window, &id)?;
    status(&tuning, &id)
}

pub fn status(tuning: &Tuning, id: &str) -> Result<Value, String> {
    let mut sessions = tuning.sessions.lock().map_err(|e| e.to_string())?;
    let session = sessions.get_mut(id).ok_or("Tuning session is closed")?;
    if let Some(child) = session.child.as_mut() {
        let status = child.try_wait().map_err(|e| e.to_string())?;
        let oversized = fs::metadata(session.directory.join("job.log"))
            .map(|m| m.len() > 64 * 1024 * 1024)
            .unwrap_or(false);
        if status.is_some() || session.started.elapsed().as_secs() > 600 || oversized {
            if let Some(status) = status {
                if !status.success() {
                    session.error = Some(
                        fs::read_to_string(session.directory.join("worker.log"))
                            .unwrap_or_else(|_| format!("Worker exited {status}")),
                    );
                }
            } else {
                session.error = Some("Tuning operation exceeded its time or log budget".into());
            }
            stop(child);
            session.child = None;
        }
    }
    let state: Value = serde_json::from_slice(
        &fs::read(session.directory.join("session.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let path = session.directory.join("job.log");
    let log = fs::File::open(path)
        .ok()
        .and_then(|mut file| {
            use std::io::{Read, Seek, SeekFrom};
            let len = file.metadata().ok()?.len();
            file.seek(SeekFrom::Start(len.saturating_sub(16000))).ok()?;
            let mut out = String::new();
            file.read_to_string(&mut out).ok()?;
            Some(out)
        })
        .unwrap_or_default();
    Ok(json!({"running":session.child.is_some(),"error":session.error,"session":state,"log":log}))
}

#[tauri::command]
pub fn tuning_cancel(
    window: tauri::WebviewWindow,
    tuning: tauri::State<'_, std::sync::Arc<Tuning>>,
    id: String,
) -> Result<(), String> {
    authorize(&window, &id)?;
    crate::mcp::ensure_local(window.app_handle())?;
    cancel(&tuning, &id)
}

pub fn cancel(tuning: &Tuning, id: &str) -> Result<(), String> {
    let mut sessions = tuning.sessions.lock().map_err(|e| e.to_string())?;
    let session = sessions.get_mut(id).ok_or("Tuning session is closed")?;
    if let Some(child) = session.child.as_mut() {
        stop(child);
    }
    session.child = None;
    session.error =
        Some("Operation cancelled; completed runs and captured inputs are retained".into());
    Ok(())
}

pub fn busy(tuning: &Tuning) -> bool {
    tuning
        .sessions
        .lock()
        .unwrap()
        .values()
        .any(|s| s.child.is_some())
}
pub fn list(tuning: &Tuning) -> Vec<String> {
    tuning.sessions.lock().unwrap().keys().cloned().collect()
}
pub fn close(tuning: &Tuning, id: &str) -> Result<(), String> {
    let mut session = tuning
        .sessions
        .lock()
        .unwrap()
        .remove(id)
        .ok_or("Tuning session is closed")?;
    if let Some(child) = session.child.as_mut() {
        stop(child)
    }
    prune(&session.directory);
    Ok(())
}

pub fn stop_all(tuning: &Tuning) {
    if let Ok(mut sessions) = tuning.sessions.lock() {
        for session in sessions.values_mut() {
            if let Some(mut child) = session.child.take() {
                stop(&mut child);
                session.error = Some("Application shutting down".into());
            }
            prune(&session.directory);
        }
    }
}

/// Deletes a finished session's bulky inputs: the captured memory and any
/// validation dumps, which for llama.cpp hold the whole model. Results, logs and
/// source backups stay. A session cannot be reopened, so nothing still usable is
/// lost. Returns the bytes freed and records them in `pruned.json`.
pub fn prune(directory: &std::path::Path) -> u64 {
    let mut doomed = Vec::new();
    for entry in fs::read_dir(directory).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        if name.starts_with("capture-") {
            doomed.push(entry.path());
        } else if name.starts_with("run-") {
            for sub in ["reference-validation", "variant-validation", "output"] {
                let path = entry.path().join(sub);
                if fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir()) {
                    doomed.push(path);
                }
            }
        }
    }
    let freed = tree_bytes(&doomed);
    for path in &doomed {
        let _ = fs::remove_dir_all(path);
    }
    if freed > 0 {
        let _ = fs::write(directory.join("pruned.json"), json!({"bytes": freed}).to_string());
    }
    freed
}

/// Bytes held by the files under these trees, counting hard links once.
fn tree_bytes(roots: &[PathBuf]) -> u64 {
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    #[cfg(unix)]
    let identity = |meta: &fs::Metadata, _: &std::path::Path| (meta.dev(), meta.ino(), PathBuf::new());
    #[cfg(not(unix))]
    let identity = |_: &fs::Metadata, path: &std::path::Path| (0u64, 0u64, path.to_path_buf());
    let mut seen = std::collections::HashSet::new();
    let mut stack: Vec<PathBuf> = roots.to_vec();
    let mut total = 0;
    while let Some(path) = stack.pop() {
        let Ok(meta) = fs::symlink_metadata(&path) else { continue };
        if meta.is_dir() {
            stack.extend(fs::read_dir(&path).into_iter().flatten().flatten().map(|e| e.path()));
        } else if meta.is_file() && seen.insert(identity(&meta, &path)) {
            total += meta.len();
        }
    }
    total
}

/// Prunes the sessions of rocprof-studio processes that have exited (a crash, a
/// kill or a restart), never those of another instance that is still running.
/// Session directories are named `<owner pid>-<n>`.
pub fn sweep() -> u64 {
    let roots: Vec<PathBuf> = match std::env::var_os("ROCPROF_TUNING_ROOT") {
        Some(explicit) => vec![PathBuf::from(explicit)],
        None => ["kernel-tuning/sessions", "shader-tuning/sessions"]
            .iter()
            .filter_map(|relative| crate::appdir::root().ok().map(|root| root.join(relative)))
            .collect(),
    };
    sweep_roots(&roots)
}

fn sweep_roots(roots: &[PathBuf]) -> u64 {
    // Without /proc a live owner cannot be told from a dead one.
    if !std::path::Path::new("/proc/self").exists() {
        return 0;
    }
    let mut freed = 0;
    for root in roots {
        for entry in fs::read_dir(root).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(owner) = name.split('-').next().and_then(|pid| pid.parse::<u32>().ok()) else { continue };
            if owner == std::process::id() || owner_running(owner) || entry.path().join("pruned.json").exists() {
                continue;
            }
            freed += prune(&entry.path());
        }
    }
    freed
}

/// Runs `sweep` off the startup path; deleting a model-sized capture takes a moment.
pub fn sweep_in_background() {
    std::thread::spawn(|| {
        let freed = sweep();
        if freed > 0 {
            eprintln!("Pruned {:.1} GiB of captured inputs from tuning sessions of exited processes", freed as f64 / (1u64 << 30) as f64);
        }
    });
}

fn owner_running(pid: u32) -> bool {
    fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|comm| comm.trim_end().starts_with("rocprof-studio"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vulkan_sessions_rerun_the_profiled_command_without_tracing() {
        let root = std::env::temp_dir().join(format!("rpg-tuning-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let shaders = root.join("llama.cpp/ggml/src/ggml-vulkan/vulkan-shaders");
        fs::create_dir_all(&shaders).unwrap();
        fs::create_dir_all(root.join("llama.cpp/build/bin")).unwrap();
        let job = root.join("job");
        fs::create_dir_all(job.join("rgp")).unwrap();
        let app = root.join("llama.cpp/build/bin/llama-bench");
        fs::write(
            job.join("profile.json"),
            serde_json::to_vec(&json!({"tracer":"radv","argv":[app,"-p","512"],"cwd":"/work",
                "env":{"MESA_VK_TRACE":"rgp","MESA_VK_TRACE_PER_SUBMIT":"1","VK_DRIVER_FILES":"/icd.json"}}))
            .unwrap(),
        )
        .unwrap();
        let runtime = root.join("runtime");
        let config = vulkan_config(Some(&job.join("rgp/app_submit7.rgp")), &runtime);
        assert_eq!(config["argv"][2], "512");
        assert_eq!(config["cwd"], "/work");
        assert_eq!(config["env"]["VK_DRIVER_FILES"], "/icd.json");
        assert_eq!(config["env"]["GGML_VK_DEBUG_MARKERS"], "1");
        assert!(config["env"].get("MESA_VK_TRACE").is_none() && config["env"].get("MESA_VK_TRACE_PER_SUBMIT").is_none());
        assert_eq!(config["sourceTree"], root.join("llama.cpp").display().to_string());
        assert_eq!(config["replay"], json!(runtime.join("bin/vkreplay")));

        // A capture opened from anywhere else starts with an empty command.
        let bare = vulkan_config(Some(&root.join("elsewhere.rgp")), &runtime);
        assert_eq!(bare["argv"], json!([]));
        assert_eq!(bare["sourceTree"], "");
        assert_eq!(bare["env"]["GGML_VK_DEBUG_MARKERS"], "1");
        let _ = fs::remove_dir_all(&root);
    }

    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("rpg-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn hip_sessions_rerun_the_profiled_command() {
        let root = scratch("hip-config");
        fs::create_dir_all(root.join("app/.git")).unwrap();
        fs::create_dir_all(root.join("job/host")).unwrap();
        fs::write(
            root.join("job/profile.json"),
            serde_json::to_vec(&json!({"tracer":"rocprofv3","argv":["build/bin/bench","-p","32"],"cwd":root.join("app"),"env":{"HIP_VISIBLE_DEVICES":"0"}})).unwrap(),
        )
        .unwrap();
        let runtime = root.join("runtime");
        let config = hip_config(Some(&root.join("job/host/123_kernel_trace.csv")), &runtime, 4);
        assert_eq!(config["argv"], json!(["build/bin/bench", "-p", "32"]));
        assert_eq!(config["cwd"], root.join("app").display().to_string());
        assert_eq!(config["env"]["HIP_VISIBLE_DEVICES"], "0");
        assert_eq!(config["sourceTree"], root.join("app").display().to_string());
        assert_eq!(config["occurrence"], 4);
        assert_eq!(config["replay"], json!(runtime.join("bin/kerncap-replay")));
        // Opened from anywhere else, nothing is guessed: no stand-in command to download a model for.
        let bare = hip_config(Some(&root.join("trace.csv")), &runtime, 0);
        assert_eq!(bare["argv"], json!([]));
        assert_eq!(bare["sourceTree"], "");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn pruning_removes_captured_inputs_and_dumps_but_keeps_results() {
        let dir = scratch("prune");
        fs::create_dir_all(dir.join("capture-1/memory")).unwrap();
        fs::write(dir.join("capture-1/memory/region.bin"), vec![0u8; 1000]).unwrap();
        fs::create_dir_all(dir.join("run-1-5/reference-validation/memory")).unwrap();
        fs::hard_link(dir.join("capture-1/memory/region.bin"), dir.join("run-1-5/reference-validation/memory/region.bin")).unwrap();
        fs::create_dir_all(dir.join("run-1-5/variant-validation/output")).unwrap();
        fs::write(dir.join("run-1-5/variant-validation/output/region.bin"), vec![0u8; 500]).unwrap();
        for kept in ["session.json", "job.log", "source-backup-1", "run-1-5/result.json"] {
            fs::write(dir.join(kept), b"{}").unwrap();
        }
        // The hard-linked region counts once.
        assert_eq!(prune(&dir), 1500);
        assert!(!dir.join("capture-1").exists());
        assert!(!dir.join("run-1-5/reference-validation").exists() && !dir.join("run-1-5/variant-validation").exists());
        for kept in ["session.json", "job.log", "source-backup-1", "run-1-5/result.json"] {
            assert!(dir.join(kept).exists(), "{kept}");
        }
        let record: Value = serde_json::from_slice(&fs::read(dir.join("pruned.json")).unwrap()).unwrap();
        assert_eq!(record["bytes"], 1500);
        assert_eq!(prune(&dir), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sweeping_prunes_only_sessions_whose_owner_exited() {
        let root = scratch("sweep");
        // Another rocprof-studio that is still running: a copy of sleep with that name.
        fs::copy("/bin/sleep", root.join("rocprof-studio")).unwrap();
        let mut other = Command::new(root.join("rocprof-studio")).arg("30").spawn().unwrap();
        let sessions = root.join("sessions");
        let mine = sessions.join(format!("{}-1", std::process::id()));
        let live = sessions.join(format!("{}-1", other.id()));
        let dead = sessions.join("4294967294-1");
        for dir in [&mine, &live, &dead] {
            fs::create_dir_all(dir.join("capture-1")).unwrap();
            fs::write(dir.join("capture-1/region.bin"), vec![0u8; 64]).unwrap();
        }
        let first = sweep_roots(&[sessions.clone()]);
        let again = sweep_roots(&[sessions.clone()]);
        stop(&mut other);
        assert_eq!((first, again), (64, 0));
        assert!(mine.join("capture-1").exists() && live.join("capture-1").exists());
        assert!(!dead.join("capture-1").exists());
        // Once that instance has exited, its session is pruned too.
        assert_eq!(sweep_roots(&[sessions]), 64);
        assert!(!live.join("capture-1").exists() && mine.join("capture-1").exists());
        let _ = fs::remove_dir_all(&root);
    }
}
