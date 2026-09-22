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
    let root = match std::env::var_os("ROCPROF_TUNING_ROOT") {
        Some(explicit) => PathBuf::from(explicit),
        None => crate::appdir::subdir("kernel-tuning/sessions")?,
    };
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let directory = root.join(&id);
    fs::create_dir(&directory).map_err(|e| e.to_string())?;
    let directory = directory.canonicalize().map_err(|e| e.to_string())?;
    let runtime = root.parent().unwrap_or(&root).join("runtime");
    let occurrence = capture
        .dispatches
        .iter()
        .take_while(|d| d.id != dispatch_id)
        .filter(|d| d.kernel == dispatch.kernel)
        .count();
    let config = json!({"sourceTree":"/devel/tools/llama.cpp","cwd":"/devel/tools/llama.cpp","argv":["/devel/tools/llama.cpp/build/bin/llama-bench","-hf","unsloth/Qwen3.5-9B-GGUF:Q8_0","-p","32","-n","0","-r","1","--no-warmup","--device","ROCm0"],"occurrence":occurrence,"captureLibrary":runtime.join("lib/libkerncap.so"),"replay":runtime.join("bin/kerncap-replay"),"profiler":"/opt/rocm/bin/rocprofv3","objdump":"/opt/rocm/llvm/bin/llvm-objdump","readobj":"/opt/rocm/llvm/bin/llvm-readobj"});
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
    Ok(())
}

pub fn stop_all(tuning: &Tuning) {
    if let Ok(mut sessions) = tuning.sessions.lock() {
        for session in sessions.values_mut() {
            if let Some(mut child) = session.child.take() {
                stop(&mut child);
                session.error = Some("Application shutting down".into());
            }
        }
    }
}
