use crate::{tuning, Store};
use profile_core::Capture;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::Child,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Condvar, Mutex,
    },
    time::{Duration, Instant, SystemTime},
};
use tauri::Manager;

const PROTOCOLS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"];
const VIEWS: &[&str] = &[
    "reference",
    "trace",
    "hardware",
    "summary",
    "sampling",
    "occupancy",
    "dispatch",
    "hotspots",
    "flame",
    "markers",
    "counters",
    "evidence",
];
#[derive(Clone)]
pub struct Options {
    pub headless: bool,
    enabled: bool,
    port: u16,
    token_file: PathBuf,
}
impl Options {
    pub fn parse() -> Result<Self, String> {
        let mut o = Self {
            headless: false,
            enabled: true,
            port: 8765,
            token_file: PathBuf::new(),
        };
        let mut args = std::env::args().skip(1);
        while let Some(a) = args.next() {
            match a.as_str() {
                "--headless" => o.headless = true,
                "--no-mcp" => o.enabled = false,
                "--mcp-port" => {
                    o.port = args
                        .next()
                        .ok_or("Missing MCP port")?
                        .parse()
                        .map_err(|_| "Invalid MCP port")?
                }
                "--mcp-token-file" => {
                    o.token_file = PathBuf::from(args.next().ok_or("Missing token file")?)
                }
                "--help" => {
                    println!("rocprof-studio [--headless] [--mcp-port 8765] [--mcp-token-file PATH] [--no-mcp]\nA GUI launch reuses the running service on the selected MCP port.\nThe token file defaults to ~/.rocprofgui/mcp/token; set ROCPROFGUI_HOME to relocate the whole data directory.\nMCP: http://127.0.0.1:PORT/mcp (Streamable HTTP/SSE), /sse (legacy SSE). Authorization: Bearer <token file contents>.");
                    std::process::exit(0);
                }
                _ => return Err(format!("Unknown option: {a}")),
            }
        }
        if o.headless && !o.enabled {
            return Err("Headless mode requires MCP".into());
        }
        if o.token_file.as_os_str().is_empty() && o.enabled {
            o.token_file = crate::appdir::subdir("mcp")?.join("token");
        }
        Ok(o)
    }
}
const ATTACH_ENV: &[&str] = &[
    "DISPLAY", "WAYLAND_DISPLAY", "XAUTHORITY", "XDG_RUNTIME_DIR",
    "DBUS_SESSION_BUS_ADDRESS", "GDK_BACKEND", "XDG_SESSION_TYPE",
    "XDG_CURRENT_DESKTOP", "XDG_DATA_DIRS",
];

fn validate_attach_environment(value: &Value) -> Result<BTreeMap<String, String>, String> {
    let object = value["environment"].as_object().ok_or("Missing display environment")?;
    if object.keys().any(|key| !ATTACH_ENV.contains(&key.as_str())) {
        return Err("Unsupported display environment variable".into());
    }
    let mut environment = BTreeMap::new();
    for (key, value) in object {
        let value = value.as_str().ok_or("Display environment values must be strings")?;
        if value.len() > 4096 || value.contains('\0') {
            return Err("Invalid display environment value".into());
        }
        environment.insert(key.clone(), value.to_owned());
    }
    if !environment.get("DISPLAY").is_some_and(|v| !v.is_empty())
        && !environment.get("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty())
    {
        return Err("Launch the GUI from a graphical session with DISPLAY or WAYLAND_DISPLAY set".into());
    }
    Ok(environment)
}

pub fn try_attach(o: &Options) -> Result<bool, String> {
    if !o.enabled || o.port == 0 {
        return Ok(false);
    }
    let address = SocketAddr::from(([127, 0, 0, 1], o.port));
    let mut stream = match TcpStream::connect_timeout(&address, Duration::from_millis(500)) {
        Ok(stream) => stream,
        Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => return Ok(false),
        Err(e) => return Err(format!("Cannot reach MCP port {}: {e}", o.port)),
    };
    stream.set_read_timeout(Some(Duration::from_secs(3))).map_err(|e| e.to_string())?;
    stream.set_write_timeout(Some(Duration::from_secs(3))).map_err(|e| e.to_string())?;
    let token = fs::read_to_string(&o.token_file)
        .map_err(|e| format!("Cannot read MCP token {}: {e}", o.token_file.display()))?;
    let token = token.trim();
    if token.len() < 32 || !token.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
        return Err("Invalid MCP token file".into());
    }
    let environment: serde_json::Map<String, Value> = ATTACH_ENV.iter()
        .filter_map(|key| std::env::var(key).ok().map(|value| ((*key).into(), json!(value))))
        .collect();
    let body = json!({"environment":environment});
    validate_attach_environment(&body)?;
    let body = body.to_string();
    write!(stream, "POST /attach HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        o.port, token, body.len(), body).map_err(|e| e.to_string())?;
    stream.flush().map_err(|e| e.to_string())?;
    let mut response = String::new();
    stream.read_to_string(&mut response).map_err(|e| e.to_string())?;
    let status = response.lines().next().unwrap_or("");
    if (status.contains(" 202 ") && response.ends_with("GUI attachment accepted"))
        || (status.contains(" 200 ") && response.ends_with("GUI focused"))
    {
        return Ok(true);
    }
    let detail = response.split_once("\r\n\r\n").map(|(_, body)| body.trim()).unwrap_or("");
    Err(format!("GUI attachment refused by MCP service: {} {}", status.trim(), detail))
}

struct Active {
    id: Value,
    owner: String,
    name: String,
    /// The tool name and its arguments: an identical call joins this one.
    key: String,
    cancel: Arc<AtomicBool>,
    started: Instant,
    outcome: Arc<Outcome>,
}
/// The response of a running tool call, handed to identical calls that join it.
#[derive(Default)]
struct Outcome {
    response: Mutex<Option<Value>>,
    ready: Condvar,
}
impl Outcome {
    fn set(&self, response: Value) {
        *self.response.lock().unwrap() = Some(response);
        self.ready.notify_all();
    }
    fn wait(&self) -> Value {
        let mut response = self.response.lock().unwrap();
        while response.is_none() {
            response = self.ready.wait(response).unwrap();
        }
        response.clone().unwrap()
    }
}
/// A finished call's response that has not been confirmed delivered.
struct Undelivered {
    owner: String,
    id: Value,
    key: String,
    response: Value,
    at: Instant,
}
/// Tools that only read state, and so may run while an action is active.
const READ_ONLY_TOOLS: [&str; 2] = ["tuning_status", "profile_status"];
/// How a live profile is gated: RADV RGP file triggers, or rocprofv3 attach windows.
enum Gate {
    Radv {
        trigger: PathBuf,
    },
    Rocprofv3 {
        profiler: String,
        /// The attach window currently open, if any.
        window: Option<AttachWindow>,
        /// The collection/filter selection fixed by this process's first window.
        /// rocprofv3 pickles each PID's attach options in `/tmp/rocprofv3_attach_<pid>.pkl`
        /// and reattaches only with identical options, so every window must match it.
        collection: Option<Collection>,
    },
}
/// One rocprofv3 attach window.
struct AttachWindow {
    child: Child,
    #[allow(dead_code)]
    directory: PathBuf,
    /// true when the window carries no `--attach-duration-msec` and detaches on a
    /// newline written to rocprofv3's stdin (its `input()` returns); false for a
    /// timed window that detaches on its own after `windowMs`.
    triggered: bool,
}
/// The per-process collection signature every attach window of one process shares.
#[derive(Clone, PartialEq)]
struct Collection {
    /// Normalized rocprofv3 collection and kernel-filter flags (without the
    /// per-window `--output-*`/`--attach-*` options).
    args: Vec<String>,
    /// Whether windows are triggered (no duration) or timed (`windowMs`).
    triggered: bool,
}
struct LiveProfile {
    id: String,
    child: Child,
    folder: PathBuf,
    program: String,
    started: SystemTime,
    triggers: u32,
    gate: Gate,
}
impl LiveProfile {
    fn radv_trigger(&self) -> Option<&PathBuf> {
        match &self.gate {
            Gate::Radv { trigger } => Some(trigger),
            Gate::Rocprofv3 { .. } => None,
        }
    }
}
#[derive(Default)]
pub struct Control {
    shutting_down: AtomicBool,
    active: Mutex<Option<Active>>,
    live_profile: Mutex<Option<LiveProfile>>,
    /// Shared cancellation flag for the desktop `live_profile_*` commands. They run
    /// outside `active` (so `Active.cancel` cannot reach them), so they need a flag
    /// with a stored owner that `Control::cancel` / `shutdown` can set; a fresh
    /// `AtomicBool::new(false)` per call would have no owner and could never fire.
    live_cancel: Arc<AtomicBool>,
    workspace: Mutex<Value>,
    revision: AtomicU64,
    endpoint: Mutex<Value>,
    app: Mutex<Option<tauri::AppHandle>>,
    attach_request: Mutex<Option<BTreeMap<String, String>>>,
    attaching: AtomicBool,
    undelivered: Mutex<Vec<Undelivered>>,
}
impl Control {
    /// Keeps a finished call's response until its delivery is confirmed. A client
    /// whose connection dropped during a long call typically reconnects and
    /// retries it; the retry then receives this instead of running it again.
    fn remember(&self, owner: &str, id: &Value, key: String, response: Value) {
        let mut kept = self.undelivered.lock().unwrap();
        kept.retain(|e| e.at.elapsed() < Duration::from_secs(1800));
        if kept.len() >= 16 {
            kept.remove(0);
        }
        kept.push(Undelivered { owner: owner.into(), id: id.clone(), key, response, at: Instant::now() });
    }
    fn delivered(&self, owner: &str, id: &Value) {
        self.undelivered.lock().unwrap().retain(|e| !(e.owner == owner && e.id == *id));
    }
    /// The undelivered response of an identical earlier call, answered to this one.
    fn reclaim(&self, key: &str, id: &Value) -> Option<Value> {
        let mut kept = self.undelivered.lock().unwrap();
        let index = kept.iter().position(|e| e.key == key)?;
        let mut response = kept.remove(index).response;
        response["id"] = id.clone();
        Some(response)
    }
    fn check(flag: &AtomicBool) -> Result<(), String> {
        if flag.load(Ordering::SeqCst) {
            Err("MCP execution cancelled".into())
        } else {
            Ok(())
        }
    }
    fn follow(&self, change: Value) {
        let mut w = self.workspace.lock().unwrap();
        if !w.is_object() {
            *w = json!({});
        }
        if change.get("captureId").is_some() {
            for k in [
                "range",
                "waveId",
                "instruction",
                "dispatchId",
                "project",
                "sourceMappings",
            ] {
                w.as_object_mut().unwrap().remove(k);
            }
        }
        for (k, v) in change.as_object().unwrap() {
            w[k] = v.clone();
        }
        if change.get("tuningId").is_some() { w["tuningRevision"]=json!(self.revision.load(Ordering::SeqCst)+1); }
        self.revision.fetch_add(1, Ordering::SeqCst);
    }
    fn cancel(&self, owner: Option<&str>, id: Option<&Value>) {
        if let Some(a) = self.active.lock().unwrap().as_ref() {
            if owner.is_none_or(|o| o == a.owner) && id.is_none_or(|i| *i == a.id) {
                a.cancel.store(true, Ordering::SeqCst);
            }
        }
        // A desktop live-profile op (trigger/stop) runs outside `active`; its shared
        // cancellation flag lets mcp_cancel / shutdown reach an in-flight detach wait.
        self.live_cancel.store(true, Ordering::SeqCst);
    }
    fn status(&self) -> Value {
        let a = self.active.lock().unwrap();
        // A status poll must never block. `live_profile` is held across multi-second
        // child waits (trigger/stop), and `mcp_status` runs on the GUI main thread,
        // so blocking here would freeze the whole app. try_lock keeps the poll
        // nonblocking; a busy lock reports the field as unavailable rather than wait.
        let live = match self.live_profile.try_lock() {
            Ok(guard) => guard.as_ref().map(|p| json!({"id":p.id,"directory":p.folder,"triggerFile":p.radv_trigger()})),
            Err(_) => Some(json!("unavailable")),
        };
        json!({"active":a.as_ref().map(|a|json!({"requestId":a.id,"tool":a.name,"cancelling":a.cancel.load(Ordering::SeqCst),"elapsedSeconds":a.started.elapsed().as_secs()})),"liveProfile":live,"revision":self.revision.load(Ordering::SeqCst),"workspace":*self.workspace.lock().unwrap(),"server":*self.endpoint.lock().unwrap()})
    }
}
pub fn ensure_local(app: &tauri::AppHandle) -> Result<(), String> {
    if app.state::<Arc<Control>>().active.lock().unwrap().is_some() {
        Err("Currently driven by MCP; cancel MCP execution to edit locally".into())
    } else {
        Ok(())
    }
}
#[tauri::command]
pub fn mcp_status(control: tauri::State<'_, Arc<Control>>) -> Value {
    control.status()
}
#[tauri::command]
pub fn mcp_cancel(control: tauri::State<'_, Arc<Control>>) {
    control.cancel(None, None)
}
#[tauri::command]
pub fn mcp_capture(
    store: tauri::State<'_, Arc<Store>>,
    id: String,
) -> Result<Arc<Capture>, String> {
    store
        .captures
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or("Capture closed".into())
}

// Desktop "Live profiling" panel commands. They share the exact core the MCP
// arms call (profile_*_core), so the logic is not forked. The mutating ones run
// on a blocking task because trigger/stop can wait seconds (attach confirmation,
// detach). They are not in `block_local_invoke`'s allowlist, so they are refused
// while an MCP action is active; only the read-only status poll is allowlisted.
// Their errors pass through desktop_error so the panel never names an MCP tool.

/// Rewrites core error strings (written in MCP tool-name vocabulary) into desktop
/// panel wording, so the Live profiling panel never tells a user to run a tool
/// that does not exist in the GUI. The logic is shared; only the words differ.
fn desktop_error(e: String) -> String {
    match e.as_str() {
        "A live profile is already running; use profile_stop first" =>
            "A live profile is already running; stop it here first, or (if the MCP server started it) cancel the MCP session".into(),
        "No live profile; use profile_start first" | "No live profile; use profile_stop to collect a finished run" =>
            "No live profile is running; start one first".into(),
        "No live profile" => "No live profile is running".into(),
        "Profiled process has exited; use profile_stop to collect artifacts" =>
            "The profiled process has exited; stop and collect to gather its artifacts".into(),
        _ => e,
    }
}
#[tauri::command]
pub async fn live_profile_start(control: tauri::State<'_, Arc<Control>>, args: Value) -> Result<Value, String> {
    let control = control.inner().clone();
    tauri::async_runtime::spawn_blocking(move || profile_start_core(&control, &args))
        .await
        .map_err(|e| e.to_string())?
        .map_err(desktop_error)
}
#[tauri::command]
pub async fn live_profile_trigger(control: tauri::State<'_, Arc<Control>>, args: Value) -> Result<Value, String> {
    let control = control.inner().clone();
    control.live_cancel.store(false, Ordering::SeqCst);
    let cancel = control.live_cancel.clone();
    tauri::async_runtime::spawn_blocking(move || profile_trigger_core(&control, &args, &cancel))
        .await
        .map_err(|e| e.to_string())?
        .map_err(desktop_error)
}
#[tauri::command]
pub async fn live_profile_status(control: tauri::State<'_, Arc<Control>>, args: Value) -> Result<Value, String> {
    let control = control.inner().clone();
    tauri::async_runtime::spawn_blocking(move || profile_status_core(&control, &args))
        .await
        .map_err(|e| e.to_string())?
        .map_err(desktop_error)
}
#[tauri::command]
pub async fn live_profile_stop(control: tauri::State<'_, Arc<Control>>, args: Value) -> Result<Value, String> {
    let control = control.inner().clone();
    control.live_cancel.store(false, Ordering::SeqCst);
    let cancel = control.live_cancel.clone();
    tauri::async_runtime::spawn_blocking(move || profile_stop_core(&control, &args, &cancel))
        .await
        .map_err(|e| e.to_string())?
        .map_err(desktop_error)
}

fn random() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| e.to_string())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
fn token(path: &Path) -> Result<String, String> {
    if !path.exists() {
        if let Some(p) = path.parent() {
            fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        let mut o = fs::OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        let mut f = o.open(path).map_err(|e| e.to_string())?;
        f.write_all(random()?.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    let t = fs::read_to_string(path)
        .map_err(|e| e.to_string())?
        .trim()
        .to_owned();
    if t.len() < 32
        || !t
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(
            "MCP token must contain at least 32 alphanumeric, dash or underscore characters".into(),
        );
    }
    Ok(t)
}
struct Client {
    initialized: bool,
    ready: bool,
    version: String,
    sender: Option<mpsc::SyncSender<String>>,
    touched: Instant,
}
struct Server {
    store: Arc<Store>,
    tuning: Arc<tuning::Tuning>,
    control: Arc<Control>,
    token: String,
    port: u16,
    clients: Mutex<BTreeMap<String, Client>>,
    connections: AtomicU64,
}
pub fn headless(
    o: Options,
    store: Arc<Store>,
    tuning: Arc<tuning::Tuning>,
    control: Arc<Control>,
) -> Result<bool, String> {
    if let Ok(exe) = std::env::current_exe() {
        let parent = exe.parent().unwrap();
        let candidates = [
            parent.join("decoder-python.zip"),
            parent.join("../lib/rocprof studio/decoder-python.zip"),
        ];
        if let Some(path) = candidates.into_iter().find(|p| p.exists()) {
            profile_core::decoder::set_bundled_python_package(path)?;
        }
    }
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGINT, stop_signal as *const () as libc::sighandler_t);
        libc::signal(
            libc::SIGTERM,
            stop_signal as *const () as libc::sighandler_t,
        );
    }
    start(o, store, tuning.clone(), control.clone(), None)?;
    tuning::sweep_in_background();
    while !STOP.load(Ordering::SeqCst) {
        if let Some(environment) = control.attach_request.lock().unwrap().take() {
            for key in ATTACH_ENV {
                std::env::remove_var(key);
            }
            for (key, value) in environment {
                std::env::set_var(key, value);
            }
            #[cfg(unix)]
            unsafe {
                libc::signal(libc::SIGINT, libc::SIG_DFL);
                libc::signal(libc::SIGTERM, libc::SIG_DFL);
            }
            return Ok(true);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    shutdown(&control, &tuning);
    Ok(false)
}
pub fn attach_gui(control: &Control, app: tauri::AppHandle) {
    *control.app.lock().unwrap() = Some(app);
    control.endpoint.lock().unwrap()["headless"] = json!(false);
    control.attaching.store(false, Ordering::SeqCst);
}

pub fn start(
    o: Options,
    store: Arc<Store>,
    tuning: Arc<tuning::Tuning>,
    control: Arc<Control>,
    app: Option<tauri::AppHandle>,
) -> Result<(), String> {
    if !o.enabled {
        return Ok(());
    }
    let token = token(&o.token_file)?;
    let listener = tiny_http::Server::http(("127.0.0.1", o.port)).map_err(|e| e.to_string())?;
    let port = listener
        .server_addr()
        .to_ip()
        .ok_or("Missing TCP address")?
        .port();
    *control.endpoint.lock().unwrap() = json!({"url":format!("http://127.0.0.1:{port}/mcp"),"sse":format!("http://127.0.0.1:{port}/sse"),"tokenFile":o.token_file,"headless":o.headless});
    eprintln!(
        "MCP listening on http://127.0.0.1:{port}/mcp and /sse; bearer token file: {}",
        o.token_file.display()
    );
    *control.app.lock().unwrap() = app;
    let server = Arc::new(Server {
        store,
        tuning,
        control,
        token,
        port,
        clients: Mutex::new(BTreeMap::new()),
        connections: AtomicU64::new(0),
    });
    std::thread::spawn(move || {
        for request in listener.incoming_requests() {
            if server.connections.fetch_add(1, Ordering::SeqCst) >= 32 {
                server.connections.fetch_sub(1, Ordering::SeqCst);
                reply(request, 503, "Too many MCP connections", None);
                continue;
            }
            let s = server.clone();
            std::thread::spawn(move || {
                s.handle(request);
                s.connections.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });
    Ok(())
}
fn header<'a>(r: &'a tiny_http::Request, k: &str) -> Option<&'a str> {
    r.headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(k))
        .map(|h| h.value.as_str())
}
fn h(k: &str, v: &str) -> tiny_http::Header {
    tiny_http::Header::from_bytes(k, v).unwrap()
}
fn reply(r: tiny_http::Request, status: u16, body: &str, session: Option<&str>) {
    let mut response = tiny_http::Response::from_string(body)
        .with_status_code(status)
        .with_header(h("Content-Type", "application/json"));
    if let Some(s) = session {
        response.add_header(h("Mcp-Session-Id", s));
    }
    let _ = r.respond(response);
}
fn error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message.into()}})
}
impl Server {
    fn handle(self: &Arc<Self>, mut r: tiny_http::Request) {
        let host = format!("127.0.0.1:{}", self.port);
        let localhost = format!("localhost:{}", self.port);
        if !header(&r, "Host").is_some_and(|v| v == host || v == localhost)
            || header(&r, "Origin").is_some_and(|v| {
                v != format!("http://{host}") && v != format!("http://{localhost}")
            })
        {
            reply(r, 403, "Invalid Host or Origin", None);
            return;
        }
        if header(&r, "Authorization") != Some(format!("Bearer {}", self.token).as_str()) {
            reply(r, 401, "Bearer token required", None);
            return;
        }
        let url = r.url().to_owned();
        let method = r.method().as_str().to_owned();
        if url == "/attach" {
            if method != "POST" || !header(&r, "Content-Type").is_some_and(|v| v.starts_with("application/json")) {
                reply(r, 405, "POST application/json required", None);
                return;
            }
            let mut bytes = Vec::new();
            if r.as_reader().take(8193).read_to_end(&mut bytes).is_err() || bytes.len() > 8192 {
                reply(r, 413, "Invalid attachment request", None);
                return;
            }
            let environment = serde_json::from_slice::<Value>(&bytes)
                .map_err(|e| e.to_string())
                .and_then(|value| validate_attach_environment(&value));
            let environment = match environment {
                Ok(value) => value,
                Err(e) => { reply(r, 400, &e, None); return; }
            };
            if let Some(app) = self.control.app.lock().unwrap().clone() {
                let _ = app.clone().run_on_main_thread(move || {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                });
                reply(r, 200, "GUI focused", None);
                return;
            }
            let active = self.control.active.lock().unwrap();
            let rejected = self.control.endpoint.lock().unwrap()["headless"] != true
                || active.is_some()
                || self.control.attaching.swap(true, Ordering::SeqCst);
            drop(active);
            if rejected {
                reply(r, 409, "MCP action active or GUI attachment in progress", None);
                return;
            }
            *self.control.attach_request.lock().unwrap() = Some(environment);
            reply(r, 202, "GUI attachment accepted", None);
            return;
        }
        if method == "GET" && url == "/sse" {
            let id = match random() {
                Ok(v) => v,
                Err(e) => {
                    reply(r, 500, &e, None);
                    return;
                }
            };
            let (tx, rx) = mpsc::sync_channel(64);
            {
                let mut clients = self.clients.lock().unwrap();
                clients.retain(|_, c| c.touched.elapsed() < Duration::from_secs(3600));
                if clients.len() >= 16 {
                    reply(r, 503, "Too many sessions", None);
                    return;
                }
                clients.insert(
                    id.clone(),
                    Client {
                        initialized: false,
                        ready: false,
                        version: "2024-11-05".into(),
                        sender: Some(tx.clone()),
                        touched: Instant::now(),
                    },
                );
            }
            let _ = tx.send(format!(
                "event: endpoint\ndata: /messages?sessionId={id}\n\n"
            ));
            drop(tx);
            let mut writer = r.into_writer();
            if write!(writer,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n").and_then(|_|writer.flush()).is_ok() {
            loop { let event=match rx.recv_timeout(Duration::from_secs(5)){Ok(v)=>v,Err(mpsc::RecvTimeoutError::Timeout)=>": keepalive\n\n".into(),Err(_)=>break};
                if writer.write_all(event.as_bytes()).and_then(|_|writer.flush()).is_err(){break}
                if !self.clients.lock().unwrap().contains_key(&id){break}
            }
        }
            self.clients.lock().unwrap().remove(&id);
            return;
        }
        let legacy = url.starts_with("/messages?sessionId=");
        if url != "/mcp" && !legacy {
            reply(r, 404, "Unknown endpoint", None);
            return;
        }
        let sid = if legacy {
            url.strip_prefix("/messages?sessionId=").map(str::to_owned)
        } else {
            header(&r, "Mcp-Session-Id").map(str::to_owned)
        };
        if method == "GET" {
            reply(r, 405, "Use POST for Streamable HTTP request streams", None);
            return;
        }
        if method == "DELETE" && !legacy {
            if let Some(id) = sid {
                self.control.cancel(Some(&id), None);
                self.clients.lock().unwrap().remove(&id);
                reply(r, 200, "{}", None)
            } else {
                reply(r, 400, "Missing session", None)
            }
            return;
        }
        if method != "POST" {
            reply(r, 405, "POST required", None);
            return;
        }
        if !header(&r, "Content-Type").is_some_and(|v| v.starts_with("application/json")) {
            reply(r, 415, "JSON required", None);
            return;
        }
        if !legacy
            && !header(&r, "Accept")
                .is_some_and(|v| v.contains("application/json") && v.contains("text/event-stream"))
        {
            reply(
                r,
                406,
                "Accept application/json and text/event-stream",
                None,
            );
            return;
        }
        let mut bytes = Vec::new();
        if r.as_reader()
            .take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .is_err()
            || bytes.len() > 16 * 1024 * 1024
        {
            reply(r, 413, "Request exceeds 16 MiB", None);
            return;
        }
        let v: Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(_) => {
                reply(
                    r,
                    400,
                    &error(Value::Null, -32700, "Invalid JSON").to_string(),
                    None,
                );
                return;
            }
        };
        if v["jsonrpc"] != "2.0"
            || !v["method"].is_string()
            || v.get("id")
                .is_some_and(|id| !id.is_string() && !id.is_number())
        {
            reply(
                r,
                400,
                &error(Value::Null, -32600, "Invalid JSON-RPC request").to_string(),
                None,
            );
            return;
        }
        let init = v["method"] == "initialize";
        let id = match sid {
            Some(id) => id,
            None if init && !legacy => match random() {
                Ok(id) => id,
                Err(e) => {
                    reply(r, 500, &e, None);
                    return;
                }
            },
            _ => {
                reply(r, 400, "Missing session", None);
                return;
            }
        };
        {
            let mut clients = self.clients.lock().unwrap();
            clients.retain(|_, c| c.touched.elapsed() < Duration::from_secs(3600));
            if !clients.contains_key(&id) {
                if !init || legacy {
                    reply(r, 404, "Session expired", None);
                    return;
                }
                if clients.len() >= 16 {
                    reply(r, 503, "Too many sessions", None);
                    return;
                }
                clients.insert(
                    id.clone(),
                    Client {
                        initialized: false,
                        ready: false,
                        version: String::new(),
                        sender: None,
                        touched: Instant::now(),
                    },
                );
            }
            let client = clients.get_mut(&id).unwrap();
            client.touched = Instant::now();
            if !legacy
                && !init
                && header(&r, "MCP-Protocol-Version").unwrap_or("2025-03-26") != client.version
            {
                reply(r, 400, "Protocol version does not match session", None);
                return;
            }
        }
        if v.get("id").is_none() {
            if v["method"] == "notifications/initialized" {
                let mut clients = self.clients.lock().unwrap();
                let Some(client) = clients.get_mut(&id) else {
                    reply(r, 404, "Session expired", None);
                    return;
                };
                if !client.initialized {
                    reply(r, 400, "Initialize first", None);
                    return;
                }
                client.ready = true;
            }
            if v["method"] == "notifications/cancelled" {
                self.control.cancel(Some(&id), v["params"].get("requestId"));
            }
            reply(r, 202, "", None);
            return;
        }
        let tx = if legacy {
            self.clients
                .lock()
                .unwrap()
                .get(&id)
                .and_then(|c| c.sender.clone())
        } else {
            None
        };
        if legacy {
            reply(r, 202, "", None);
            let result = self.rpc(&id, &v);
            if let Some(tx) = tx {
                if tx.try_send(format!("event: message\ndata: {result}\n\n")).is_ok() {
                    self.control.delivered(&id, &v["id"]);
                }
            }
        } else {
            let mut writer = r.into_writer();
            let _=write!(writer,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nMcp-Session-Id: {id}\r\nConnection: close\r\n\r\n").and_then(|_|writer.flush());
            let (tx, rx) = mpsc::sync_channel(1);
            let server = self.clone();
            let (owner, request, started) = (id.clone(), v["id"].clone(), Instant::now());
            std::thread::spawn(move || {
                let _ = tx.send(server.rpc(&id, &v));
            });
            loop {
                match rx.recv_timeout(Duration::from_secs(2)) {
                    Ok(result) => {
                        // A response that cannot be written stays kept for a retry.
                        // The first write to a connection the client has closed still
                        // succeeds, so a long call probes it first: if the client left
                        // since the last keep-alive, the response write then fails.
                        let probed = started.elapsed() < Duration::from_secs(2)
                            || writer.write_all(b": done\n\n").and_then(|_| writer.flush()).is_ok() && {
                                std::thread::sleep(Duration::from_millis(50));
                                true
                            };
                        if probed
                            && write!(writer, "event: message\ndata: {result}\n\n")
                                .and_then(|_| writer.flush())
                                .is_ok()
                        {
                            self.control.delivered(&owner, &request);
                        }
                        break;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if writer
                            .write_all(b": working\n\n")
                            .and_then(|_| writer.flush())
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    }
    fn rpc(&self, owner: &str, v: &Value) -> Value {
        let id = v["id"].clone();
        if self.control.shutting_down.load(Ordering::SeqCst) {
            return error(id, -32000, "Server shutting down");
        }
        let params = &v["params"];
        let result = match v["method"].as_str().unwrap() {
            "initialize" => {
                let proposed = params["protocolVersion"].as_str().unwrap_or("");
                let version = if PROTOCOLS.contains(&proposed) {
                    proposed
                } else {
                    "2025-06-18"
                };
                let mut clients = self.clients.lock().unwrap();
                let Some(c) = clients.get_mut(owner) else {
                    return error(id, -32002, "Session expired");
                };
                if c.initialized {
                    return error(id, -32600, "Already initialized");
                }
                c.initialized = true;
                c.version = version.into();
                json!({"protocolVersion":version,"capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"rocprof-studio","version":env!("CARGO_PKG_VERSION")},"instructions":"Local profiling and kernel tuning. Tool calls are serialized, except tuning_status and profile_status, which answer while another call runs. After a dropped connection, repeat the same call with the same arguments: it joins the running call, or receives its result if it already finished. Cancel with notifications/cancelled or the desktop Cancel MCP execution button. Read tool schemas before mutation; artifacts default to ~/.rocprofgui (override with ROCPROFGUI_HOME)."})
            }
            "ping" => json!({}),
            _ if !self
                .clients
                .lock()
                .unwrap()
                .get(owner)
                .is_some_and(|c| c.ready) =>
            {
                return error(
                    id,
                    -32002,
                    "Send initialize and notifications/initialized first",
                )
            }
            "tools/list" => json!({"tools":tools()}),
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                let catalog = tools();
                let Some(spec) = catalog.iter().find(|t| t["name"] == name) else {
                    return error(id, -32602, "Unknown tool");
                };
                if !args.is_object() {
                    return error(id, -32602, "arguments must be an object");
                }
                if let Err(e) = validate(&args, &spec["inputSchema"]) {
                    return error(id, -32602, e);
                }
                let flag = Arc::new(AtomicBool::new(false));
                if READ_ONLY_TOOLS.contains(&name) {
                    return match self.tool(name, &args, flag) {
                        Ok(data) => json!({"jsonrpc":"2.0","id":id,"result":{"content":[{"type":"text","text":data.to_string()}],"isError":false}}),
                        Err(e) => json!({"jsonrpc":"2.0","id":id,"result":{"content":[{"type":"text","text":e}],"isError":true}}),
                    };
                }
                let key = format!("{name}\n{args}");
                let outcome_slot = Arc::new(Outcome::default());
                {
                    let mut active = self.control.active.lock().unwrap();
                    if let Some(response) = self.control.reclaim(&key, &id) {
                        return response;
                    }
                    if let Some(running) = active.as_ref() {
                        // A retry of the running call, typically after the client's
                        // connection dropped: wait for it and answer this request.
                        if running.key == key && !running.cancel.load(Ordering::SeqCst) {
                            let (slot, first_owner, first_id) = (running.outcome.clone(), running.owner.clone(), running.id.clone());
                            drop(active);
                            let mut response = slot.wait();
                            self.control.delivered(&first_owner, &first_id);
                            response["id"] = id.clone();
                            return response;
                        }
                        return error(id, -32001, "Another MCP action is running");
                    }
                    if self.control.attaching.load(Ordering::SeqCst) {
                        return error(id, -32001, "GUI attachment in progress");
                    }
                    if self.store.import.lock().unwrap().is_some() || tuning::busy(&self.tuning) {
                        return error(
                            id,
                            -32001,
                            "A desktop operation is running; wait or cancel it first",
                        );
                    }
                    *active = Some(Active {
                        id: id.clone(),
                        owner: owner.into(),
                        name: name.into(),
                        key: key.clone(),
                        cancel: flag.clone(),
                        started: Instant::now(),
                        outcome: outcome_slot.clone(),
                    });
                }
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    self.tool(name, &args, flag.clone())
                }))
                .unwrap_or_else(|_| Err("Internal MCP execution failure".into()));
                let mut active = self.control.active.lock().unwrap();
                let response = if flag.load(Ordering::SeqCst) {
                    let detail = match outcome {
                        Err(message) if name == "profile_run" => format!("MCP execution cancelled; worker stopped. {message}"),
                        _ => "MCP execution cancelled; worker stopped. Completed artifacts are retained.".into(),
                    };
                    error(id.clone(), -32800, detail)
                } else {
                    let result = match outcome {
                        Ok(data) => json!({"content":[{"type":"text","text":data.to_string()}],"isError":false}),
                        Err(e) => json!({"content":[{"type":"text","text":e}],"isError":true}),
                    };
                    json!({"jsonrpc":"2.0","id":id,"result":result})
                };
                // Published before the action ends, so a retry either joins it or
                // finds the response kept for it; it is never run twice.
                self.control.remember(owner, &id, key, response.clone());
                outcome_slot.set(response.clone());
                active.take();
                return response;
            }
            _ => return error(id, -32601, "Method not found"),
        };
        json!({"jsonrpc":"2.0","id":id,"result":result})
    }
    fn capture(&self, a: &Value) -> Result<Arc<Capture>, String> {
        self.store
            .captures
            .lock()
            .unwrap()
            .get(text(a, "captureId")?)
            .cloned()
            .ok_or("Capture not open".into())
    }
    fn insert(&self, mut c: Capture, flag: &AtomicBool) -> Result<Value, String> {
        Control::check(flag)?;
        let mut captures = self.store.captures.lock().unwrap();
        if captures.len() >= 6 {
            return Err("Six-capture limit; close a capture first".into());
        }
        c.id = self.store.next.fetch_add(1, Ordering::Relaxed).to_string();
        let summary = summary(&c);
        let id = c.id.clone();
        captures.insert(id.clone(), Arc::new(c));
        let ids: Vec<_> = captures.keys().cloned().collect();
        drop(captures);
        self.control
            .follow(json!({"captureIds":ids,"captureId":id,"view":"dispatch"}));
        Ok(summary)
    }
    fn tool(&self, name: &str, a: &Value, flag: Arc<AtomicBool>) -> Result<Value, String> {
        Control::check(&flag)?;
        match name {
            "workspace" => Ok(
                json!({"control":self.control.status(),"captures":self.store.captures.lock().unwrap().values().map(|c|summary(c)).collect::<Vec<_>>(),"tuningSessions":tuning::list(&self.tuning)}),
            ),
            "capture_open" => {
                let c = profile_core::project::load_input_options(
                    Path::new(text(a, "path")?),
                    flag.clone(),
                    a["loadWaveStates"].as_bool().unwrap_or(true),
                    a["samplingMethod"].as_str(),
                )?;
                self.insert(c, &flag)
            }
            "capture_close" => {
                let id = text(a, "captureId")?;
                self.store
                    .captures
                    .lock()
                    .unwrap()
                    .remove(id)
                    .ok_or("Capture not open")?;
                let ids: Vec<_> = self
                    .store
                    .captures
                    .lock()
                    .unwrap()
                    .keys()
                    .cloned()
                    .collect();
                self.control
                    .follow(json!({"captureIds":ids,"captureId":ids.first()}));
                Ok(json!({"closed":id}))
            }
            "capture_query" => {
                let c = self.capture(a)?;
                let dataset = text(a, "dataset")?;
                let data = match dataset {
                    "summary" => return Ok(summary(&c)),
                    "dispatches" => json!(c.dispatches),
                    "instructions" => json!(c.instructions),
                    "waves" => json!(c.waves),
                    "counters" => json!(c.counters),
                    "markers" => json!(c.markers),
                    "hostCalls" => json!(c.host_calls),
                    "codeObjects" => json!(c.code_objects),
                    "intervals" => json!(c.intervals),
                    "events" => json!(c.decoder_events),
                    "eventCounters" => json!(c.event_counters),
                    "monitorIntervals" => json!(c.monitor_intervals),
                    "otherSimd" => json!(c.other_simd),
                    "metadata" => return Ok(json!(c.metadata)),
                    "sources" => {
                        return Ok(json!({"paths":profile_core::source_files::recorded_paths(&c)}))
                    }
                    "compileReports" => json!(compile_report_rows(&c)),
                    _ => return Err("Unknown dataset".into()),
                };
                let view = match dataset {
                    "dispatches" => "dispatch",
                    "instructions" => "hotspots",
                    "counters" | "eventCounters" => "counters",
                    "markers" => "markers",
                    _ => "evidence",
                };
                self.control.follow(json!({"captureId":c.id,"view":view}));
                page(data, a, &flag)
            }
            "workspace_select" => {
                let c = self.capture(a)?;
                let view = a["view"].as_str().unwrap_or("trace");
                if !VIEWS.contains(&view) {
                    return Err("Unknown workspace view".into());
                }
                let mut change = json!({"captureId":c.id,"view":view});
                for k in [
                    "range",
                    "waveId",
                    "instruction",
                    "dispatchId",
                    "timeline",
                    "se",
                ] {
                    if let Some(v) = a.get(k) {
                        change[k] = v.clone();
                    }
                }
                if let Some(r) = a.get("range") {
                    let _: [f64; 2] = range(r)?;
                }
                self.control.follow(change.clone());
                Ok(change)
            }
            "wave_read" => {
                let c = self.capture(a)?;
                let id = text(a, "waveId")?;
                self.control
                    .follow(json!({"captureId":c.id,"view":"trace","waveId":id}));
                Ok(json!(profile_core::load_wave_cancellable(&c, id, &flag)?))
            }
            "wave_overview" => {
                let c = self.capture(a)?;
                let indices: Vec<usize> =
                    serde_json::from_value(a["indices"].clone()).map_err(|e| e.to_string())?;
                let r = range(&a["range"])?;
                self.control.follow(json!({"captureId":c.id,"view":"trace","range":r,"timeline":if a["utilization"]==true{"util"}else{"cu"}}));
                Ok(json!(profile_core::wave_overview::load_mode_cancellable(
                    &c,
                    &indices,
                    r,
                    a["utilization"].as_bool().unwrap_or(false),
                    a["separateLds"].as_bool().unwrap_or(false),
                    &flag
                )?))
            }
            "wave_analyze" => {
                let c = self.capture(a)?;
                let indices: Vec<usize> =
                    serde_json::from_value(a["indices"].clone()).map_err(|e| e.to_string())?;
                let r = range(&a["range"])?;
                self.control
                    .follow(json!({"captureId":c.id,"view":"hotspots","range":r}));
                Ok(json!(profile_core::wave_stats::load_cancellable(
                    &c, &indices, r, &flag
                )?))
            }
            "hidden_latency" => {
                let c = self.capture(a)?;
                self.control
                    .follow(json!({"captureId":c.id,"view":"hotspots"}));
                Ok(json!(profile_core::hidden::analyze_capture_cancellable(
                    &c, &flag
                )?))
            }
            "definition_file" => {
                let path = Path::new(text(a, "path")?);
                if a["op"] == "read" {
                    Ok(json!(profile_core::definition_files::read(path)?))
                } else if a["op"] == "write" {
                    new_target(path)?;
                    profile_core::definition_files::write(path, text(a, "text")?)?;
                    Ok(json!({"path":path}))
                } else {
                    Err("Unknown definition file operation".into())
                }
            }
            "plot_export" => {
                let path = Path::new(text(a, "path")?);
                new_target(path)?;
                let bytes = if let Some(text) = a["text"].as_str() {
                    text.as_bytes().to_vec()
                } else {
                    serde_json::from_value::<Vec<u8>>(a["bytes"].clone())
                        .map_err(|e| e.to_string())?
                };
                profile_core::plot_files::write(path, text(a, "format")?, &bytes)?;
                Ok(json!({"path":path,"bytes":bytes.len()}))
            }
            "source_map" => {
                let c = self.capture(a)?;
                let batch = profile_core::source_files::map_directory(
                    &profile_core::source_files::recorded_paths(&c),
                    text(a, "prefix")?,
                    Path::new(text(a, "directory")?),
                )?;
                self.control.follow(
                    json!({"captureId":c.id,"view":"hotspots","sourceMappings":batch.files}),
                );
                Ok(json!(batch))
            }
            "source_read" => {
                let start = a["startLine"].as_u64().unwrap_or(1) as usize;
                let count = a["count"].as_u64().unwrap_or(200).min(2000) as usize;
                if a.get("captureId").is_some() {
                    let c = self.capture(a)?;
                    Ok(json!(profile_core::source_files::read_snapshot_page(
                        &c,
                        text(a, "path")?,
                        start,
                        count
                    )?))
                } else {
                    Ok(json!(profile_core::source_files::read_page(
                        Path::new(text(a, "path")?),
                        start,
                        count
                    )?))
                }
            }
            "capture_export" => {
                let c = self.capture(a)?;
                let p = Path::new(text(a, "path")?);
                profile_core::capture_archive::write_new(p, &c, &flag)?;
                Ok(json!({"path":p}))
            }
            "att_discover" => Ok(json!(profile_core::att_runs::discover(Path::new(text(
                a, "path"
            )?))?)),
            "att_manifest" => {
                let m = serde_json::from_value(a["manifest"].clone()).map_err(|e| e.to_string())?;
                let p = Path::new(text(a, "path")?);
                new_target(p)?;
                profile_core::att_manifest::write(p, m)?;
                Ok(json!({"path":p}))
            }
            "spm_attach" => {
                let c = self.capture(a)?;
                let spm =
                    profile_core::project::load_input(Path::new(text(a, "path")?), flag.clone())?;
                let combined = profile_core::spm_prepare::attach(
                    &c,
                    &spm,
                    a["se"].as_u64().ok_or("se required")? as u32,
                )?;
                let p = Path::new(text(a, "output")?);
                profile_core::capture_archive::write_new(p, &combined, &flag)?;
                self.insert(profile_core::project::load_input(p, flag.clone())?, &flag)
            }
            "code_object_extract" => {
                let c = self.capture(a)?;
                let object = c
                    .code_objects
                    .iter()
                    .find(|o| Some(o.id.as_str()) == a["objectId"].as_str())
                    .ok_or("Code object not found")?;
                let p = Path::new(text(a, "output")?);
                new_target(p)?;
                Ok(json!(profile_core::code_object_files::extract_from(
                    object.uri.as_deref().ok_or("Code object has no URI")?,
                    a["sourcePath"].as_str().map(Path::new),
                    p,
                    &flag
                )?))
            }
            "project_open" => {
                let mut project = profile_core::project::read(Path::new(text(a, "path")?))?;
                let mut captures = profile_core::project::stage(&project, flag.clone())?;
                profile_core::project::reconcile(&mut project, &mut captures);
                Control::check(&flag)?;
                let mut staged = BTreeMap::new();
                let mut values = Vec::new();
                for mut c in captures {
                    c.id = self.store.next.fetch_add(1, Ordering::Relaxed).to_string();
                    values.push(summary(&c));
                    staged.insert(c.id.clone(), Arc::new(c));
                }
                Control::check(&flag)?;
                *self.store.captures.lock().unwrap() = staged;
                let ids: Vec<_> = values.iter().map(|v| v["id"].clone()).collect();
                self.control.follow(
                    json!({"captureIds":ids,"captureId":ids.get(project.active),"project":project}),
                );
                Ok(json!({"project":project,"captures":values}))
            }
            "project_save" => {
                let document = if let Some(project) = a.get("project") {
                    project.clone()
                } else {
                    let workspace = self.control.workspace.lock().unwrap().clone();
                    let captures = self.store.captures.lock().unwrap();
                    let active = captures
                        .keys()
                        .position(|id| Some(id.as_str()) == workspace["captureId"].as_str())
                        .unwrap_or(0);
                    let entries:Vec<_>=captures.values().map(|c|json!({"input":c.input_path,"fingerprint":c.input_fingerprint,"sources":{},"state":{"location":{"waveId":"","instruction":0,"iteration":0,"range":[c.start,c.end.max(c.start+1.)]},"view":"trace","mode":"capture","timeline":"wave","nonhidden":false,"includeIdle":false,"history":[],"historyIndex":-1}})).collect();
                    json!({"schemaVersion":5,"active":active,"entries":entries})
                };
                let project: profile_core::project::Project =
                    serde_json::from_value(document).map_err(|e| e.to_string())?;
                let p = Path::new(text(a, "path")?);
                new_target(p)?;
                profile_core::project::write(p, &project)?;
                Ok(json!({"path":p}))
            }
            "reference_query" => {
                self.control.follow(json!({"view":"reference","isaQuery":a["query"],"isaArchitecture":a["architecture"]}));
                reference(a)
            }
            "capture_analyze" => {
                let c = self.capture(a)?;
                let folder = job_directory()?;
                fs::write(
                    folder.join("analysis.cjs"),
                    include_str!(concat!(env!("OUT_DIR"), "/mcp-analysis.cjs")),
                )
                .map_err(|e| e.to_string())?;
                let input = serde_json::to_vec(&json!({"capture":c,"args":a}))
                    .map_err(|e| e.to_string())?;
                if input.len() > 256 * 1024 * 1024 {
                    return Err("Analysis input exceeds 256 MiB; use paged queries".into());
                }
                fs::write(folder.join("input.json"), input).map_err(|e| e.to_string())?;
                let analysis = text(a, "analysis")?;
                let view = match analysis {
                    "timeline" => "trace",
                    "diagnostics" => "evidence",
                    "hardware" => "summary",
                    "activity" => "summary",
                    "kernels" => "dispatch",
                    "source" => "flame",
                    "scalar" | "tensor" => "counters",
                    other => other,
                };
                self.control.follow(json!({"captureId":c.id,"view":view}));
                let run = run_process(
                    vec![
                        std::env::var("ROCPROF_MCP_NODE").unwrap_or("node".into()),
                        "--max-old-space-size=1024".into(),
                        folder.join("analysis.cjs").to_string_lossy().into(),
                        folder.join("input.json").to_string_lossy().into(),
                    ],
                    &folder,
                    &flag,
                    120,
                );
                // The input is a copy of the open capture, up to 256 MiB per call; keep it
                // only as long as the worker needs it. A failed run keeps its logs.
                let _ = fs::remove_file(folder.join("input.json"));
                run?;
                let stdout = fs::read(folder.join("stdout.log")).map_err(|e| e.to_string());
                let _ = fs::remove_dir_all(&folder);
                let data: Value = serde_json::from_slice(&stdout?).map_err(|e| e.to_string())?;
                if let Some(output) = a["output"].as_str() {
                    Control::check(&flag)?;
                    let path = Path::new(output);
                    new_target(path)?;
                    match analysis {
                        "timeline" => profile_core::plot_files::write(
                            path,
                            "html",
                            text(&data, "html")?.as_bytes(),
                        )?,
                        "diagnostics" => profile_core::diagnostics::write(
                            path,
                            &serde_json::to_vec_pretty(&data).unwrap(),
                        )?,
                        _ => {
                            let mut f = fs::OpenOptions::new()
                                .write(true)
                                .create_new(true)
                                .open(path)
                                .map_err(|e| e.to_string())?;
                            serde_json::to_writer_pretty(&mut f, &data)
                                .map_err(|e| e.to_string())?;
                        }
                    }
                    return Ok(json!({"path":path}));
                }
                if data.is_array() {
                    page(data, a, &flag)
                } else {
                    Ok(data)
                }
            }
            "profile_start" => profile_start_core(&self.control, a),
            "profile_trigger" => profile_trigger_core(&self.control, a, &flag),
            "profile_status" => profile_status_core(&self.control, a),
            "profile_stop" => profile_stop_core(&self.control, a, &flag),
            "profile_run" => {
                if self.control.live_profile.lock().unwrap().is_some() {
                    return Err("A live profile is already running; use profile_stop first".into());
                }
                let folder = job_directory()?;
                let argv: Vec<String> =
                    serde_json::from_value(a["argv"].clone()).map_err(|e| e.to_string())?;
                if argv.is_empty() {
                    return Err("Application argv cannot be empty".into());
                }
                let env = profile_env(a)?;
                let seconds = a["timeoutSeconds"].as_u64().unwrap_or(600);
                if !(1..=3600).contains(&seconds) {
                    return Err("timeoutSeconds must be between 1 and 3600".into());
                }
                let cwd = Path::new(text(a, "cwd")?);
                // Recorded so a tuning session opened from this trace reruns the same application.
                fs::write(
                    folder.join("profile.json"),
                    serde_json::to_vec_pretty(&json!({"tracer":a["tracer"].as_str().unwrap_or("rocprofv3"),"argv":argv,"cwd":cwd,"env":a.get("env").cloned().unwrap_or(json!({}))}))
                        .map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
                if a["tracer"] == "radv" {
                    if rocprofv3_only_run_keys().iter().any(|key| a.get(*key).is_some()) {
                        return Err(format!("{} apply to tracer rocprofv3 only", rocprofv3_only_run_keys().join(", ")));
                    }
                    let started = std::time::SystemTime::now();
                    let mut vars = vec![
                        ("MESA_VK_TRACE".to_owned(), "rgp".to_owned()),
                        ("MESA_VK_TRACE_PER_SUBMIT".to_owned(), "1".to_owned()),
                    ];
                    vars.extend(env);
                    let run = run_process_env(argv.clone(), cwd, &folder, &flag, seconds, &vars);
                    let report = radv_artifacts(&argv[0], started, &folder, if run.is_ok() { "completed" } else { "failed" })
                        .map_err(|e| format!("{e}. Artifacts remain in {}", folder.display()))?;
                    return run.map(|_| report.clone()).map_err(|e| format!("{e}. Artifacts: {report}"));
                }
                let mut command = vec![
                    a["profiler"]
                        .as_str()
                        .unwrap_or("/opt/rocm/bin/rocprofv3")
                        .into(),
                    "--output-format".into(),
                    "csv".into(),
                    "--output-directory".into(),
                    folder.to_string_lossy().into(),
                ];
                rocprof_streams(a, &mut command)?;
                kernel_filters(a, &mut command)?;
                command.push("--".into());
                command.extend(argv);
                let run = run_process_env(command, cwd, &folder, &flag, seconds, &env);
                let mut paths = Vec::new();
                find_outputs(&folder, &mut paths)?;
                let report = json!({"directory":folder,"files":paths,"stdout":folder.join("stdout.log"),"stderr":folder.join("stderr.log"),"next":"Use capture_open with a returned trace or counter CSV path; open a ui_output directory or a single .att file for SQTT waves"});
                run.map(|_| report.clone()).map_err(|e| format!("{e}. Artifacts: {report}"))
            }
            "tuning_open" => {
                let id = tuning::create(
                    &self.store,
                    &self.tuning,
                    text(a, "captureId")?.into(),
                    text(a, "dispatchId")?.into(),
                )?;
                self.control.follow(json!({"captureId":a["captureId"],"view":"dispatch","dispatchId":a["dispatchId"],"tuningId":id}));
                if let Some(app) = self.control.app.lock().unwrap().clone() {
                    tuning::show_window(&app, &id)?;
                }
                tuning::status(&self.tuning, &id)
            }
            "tuning_status" => tuning::status(&self.tuning, text(a, "id")?),
            "tuning_close" => {
                let id = text(a, "id")?;
                tuning::close(&self.tuning, id)?;
                if let Some(app) = self.control.app.lock().unwrap().clone() {
                    if let Some(w) = app.get_webview_window(&format!("tuning-{id}")) {
                        let _ = w.close();
                    }
                }
                Ok(json!({"closed":id}))
            }
            "tuning_action" => {
                let id = text(a, "id")?;
                let request = a.get("request").ok_or("request required")?.clone();
                if let Some(app) = self.control.app.lock().unwrap().clone() {
                    tuning::show_window(&app, id)?;
                }
                self.control.follow(json!({"tuningId":id}));
                tuning::start(&self.tuning, id, request)?;
                loop {
                    if flag.load(Ordering::SeqCst) {
                        tuning::cancel(&self.tuning, id)?;
                        return Err("Cancelled".into());
                    }
                    let status = tuning::status(&self.tuning, id)?;
                    if status["running"] == false {
                        if let Some(e) = status["error"].as_str() {
                            return Err(e.into());
                        }
                        if let Some(e) = status["session"]["error"].as_str() {
                            return Err(e.into());
                        }
                        self.control.follow(json!({"tuningId":id}));
                        return Ok(status);
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
            _ => Err("Tool not implemented".into()),
        }
    }
}
/// One request key that applies only to the rocprofv3 tracer, tagged with the
/// contexts it is valid in. The RADV denylists (profile_run's and the
/// profile_trigger window gate) and the end+collection guard all derive from this
/// one table, so they cannot drift or fail open on a one-sided edit; a contract
/// test cross-checks the table against the tool schemas. `run` covers profile_run
/// (and, for `profiler`, profile_start handled separately); `window` covers a
/// profile_trigger collection window. `end` is a command, not a collection
/// option, so it is excluded here and rejected by its own dedicated check.
struct Rocprofv3OnlyKey {
    key: &'static str,
    run: bool,
    window: bool,
}
const ROCPROFV3_ONLY_KEYS: [Rocprofv3OnlyKey; 9] = [
    Rocprofv3OnlyKey { key: "counters", run: true, window: true },
    Rocprofv3OnlyKey { key: "traces", run: true, window: true },
    Rocprofv3OnlyKey { key: "att", run: true, window: true },
    Rocprofv3OnlyKey { key: "kernelIncludeRegex", run: true, window: true },
    Rocprofv3OnlyKey { key: "kernelExcludeRegex", run: true, window: true },
    Rocprofv3OnlyKey { key: "kernelIterationRange", run: true, window: true },
    Rocprofv3OnlyKey { key: "profiler", run: true, window: false },
    Rocprofv3OnlyKey { key: "windowMs", run: false, window: true },
    Rocprofv3OnlyKey { key: "triggered", run: false, window: true },
];
/// rocprofv3-only keys valid on profile_run (the RADV reject list there).
fn rocprofv3_only_run_keys() -> Vec<&'static str> {
    ROCPROFV3_ONLY_KEYS.iter().filter(|k| k.run).map(|k| k.key).collect()
}
/// rocprofv3-only collection/window keys for a profile_trigger window (the RADV
/// gate reject list, and the keys `end` may not be combined with).
fn rocprofv3_only_window_keys() -> Vec<&'static str> {
    ROCPROFV3_ONLY_KEYS.iter().filter(|k| k.window).map(|k| k.key).collect()
}

/// Starts the long-lived application for gated live profiling. Shared by the MCP
/// `profile_start` arm and the `live_profile_start` desktop command.
fn profile_start_core(control: &Control, a: &Value) -> Result<Value, String> {
    let argv: Vec<String> = serde_json::from_value(a["argv"].clone()).map_err(|e| e.to_string())?;
    if argv.is_empty() {
        return Err("Application argv cannot be empty".into());
    }
    let tracer = a["tracer"].as_str().unwrap_or("radv");
    if !["radv", "rocprofv3"].contains(&tracer) {
        return Err("tracer must be radv or rocprofv3".into());
    }
    if tracer != "rocprofv3" && a.get("profiler").is_some() {
        return Err("profiler applies to tracer rocprofv3 only".into());
    }
    let env = profile_env(a)?;
    if tracer == "radv" && env.iter().any(|(key, _)| ["MESA_VK_TRACE", "MESA_VK_TRACE_PER_SUBMIT", "MESA_VK_TRACE_TRIGGER", "MESA_VK_TRACE_FRAME"].contains(&key.as_str())) {
        return Err("RADV trace control variables are managed by profile_start".into());
    }
    if tracer == "rocprofv3" && env.iter().any(|(key, _)| key == "ROCP_TOOL_ATTACH") {
        return Err("ROCP_TOOL_ATTACH is managed by profile_start".into());
    }
    let cwd = Path::new(text(a, "cwd")?);
    let mut live = control.live_profile.lock().unwrap();
    if live.is_some() {
        return Err("A live profile is already running; use profile_stop first".into());
    }
    let folder = job_directory()?;
    let id = folder.file_name().unwrap().to_string_lossy().into_owned();
    let mut vars = env;
    let (gate, capture_mode) = if tracer == "radv" {
        let trigger = folder.join("radv-trigger");
        vars.extend([
            ("MESA_VK_TRACE".to_owned(), "rgp".to_owned()),
            ("MESA_VK_TRACE_PER_SUBMIT".to_owned(), "0".to_owned()),
            ("MESA_VK_TRACE_TRIGGER".to_owned(), trigger.to_string_lossy().into_owned()),
        ]);
        (Gate::Radv { trigger }, "file-trigger")
    } else {
        vars.push(("ROCP_TOOL_ATTACH".to_owned(), "1".to_owned()));
        let profiler = a["profiler"].as_str().unwrap_or("/opt/rocm/bin/rocprofv3").to_owned();
        (Gate::Rocprofv3 { profiler, window: None, collection: None }, "attach-window")
    };
    fs::write(folder.join("profile.json"), serde_json::to_vec_pretty(&json!({"tracer":tracer,"captureMode":capture_mode,"argv":argv,"cwd":cwd,"env":a.get("env").cloned().unwrap_or(json!({}))})).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let started = SystemTime::now();
    let child = spawn_process_env(&argv, cwd, &folder, &vars)?;
    let pid = child.id();
    let trigger_file = match &gate {
        Gate::Radv { trigger } => Some(trigger.clone()),
        Gate::Rocprofv3 { .. } => None,
    };
    *live = Some(LiveProfile { id: id.clone(), child, folder: folder.clone(), program: argv[0].clone(), started, triggers: 0, gate });
    Ok(match trigger_file {
        Some(trigger) => json!({"id":id,"pid":pid,"directory":folder,"triggerFile":trigger,"stdout":folder.join("stdout.log"),"stderr":folder.join("stderr.log"),"driverRequirement":"Headless compute file triggers require a RADV build with docs/radv-compute-trigger.patch; stock Mesa checks file triggers only during swapchain presentation.","next":"Wait for the server to become ready, call profile_trigger, send one inference request, wait for savedMessages, then call profile_stop to collect captures"}),
        None => json!({"id":id,"pid":pid,"directory":folder,"stdout":folder.join("stdout.log"),"stderr":folder.join("stderr.log"),"armRequirement":"The application is started with ROCP_TOOL_ATTACH=1 so the ROCm runtime keeps an attach thread ready; rocprofv3 can attach only to processes armed this way.","next":"Wait for the application to become ready, then call profile_trigger. Give the window its collection (counters, traces, att) and, optionally, kernelIncludeRegex/kernelExcludeRegex; choose a timed window (windowMs) or a triggered window (triggered:true, which stays open until profile_trigger end:true or profile_stop). Every window of this process must request the same streams, filters and mode. Collect with profile_stop."}),
    })
}

/// Appends `--kernel-include-regex` / `--kernel-exclude-regex` /
/// `--kernel-iteration-range` when the request supplies them. All three are
/// honoured in attach mode and, on reattachment, are among the options
/// rocprofv3 requires to be identical across windows (SDK filter
/// `^kernel_(include_regex|exclude_regex|iteration_range)$`), so they are built
/// into the same `collection_args` the same-collection guard compares. A name
/// regex cannot discriminate different grids under one kernel name; the
/// iteration range selects a later occurrence instead (campaign finding).
fn kernel_filters(a: &Value, command: &mut Vec<String>) -> Result<(), String> {
    for (key, option) in [
        ("kernelIncludeRegex", "--kernel-include-regex"),
        ("kernelExcludeRegex", "--kernel-exclude-regex"),
    ] {
        if let Some(value) = a.get(key) {
            let regex = value
                .as_str()
                .filter(|s| !s.is_empty() && !s.contains('\0'))
                .ok_or_else(|| format!("{key} must be a nonempty string"))?;
            if regex.len() > 4096 {
                return Err(format!("{key} is too long"));
            }
            command.push(option.into());
            command.push(regex.into());
        }
    }
    if let Some(value) = a.get("kernelIterationRange") {
        let range = value
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("kernelIterationRange must be a nonempty string")?;
        if !range.bytes().all(|b| b.is_ascii_digit() || b == b',' || b == b'-') {
            return Err("kernelIterationRange must contain only ASCII digits, commas and hyphens (e.g. \"64\" or \"0-3\")".into());
        }
        command.push("--kernel-iteration-range".into());
        command.push(range.into());
    }
    Ok(())
}

/// Fires one capture in a live profile. Shared by the MCP `profile_trigger` arm
/// and the `live_profile_trigger` desktop command. For the rocprofv3 tracer this
/// opens a timed window (`windowMs`) or a triggered window (`triggered:true`, no
/// `--attach-duration-msec`), ends an open triggered window (`end:true`), and
/// enforces that every window of one process requests the same collection,
/// filters and mode — the options rocprofv3 pickles and reattaches with.
fn profile_trigger_core(control: &Control, a: &Value, flag: &AtomicBool) -> Result<Value, String> {
    let mut live = control.live_profile.lock().unwrap();
    let profile = live.as_mut().ok_or("No live profile; use profile_start first")?;
    if profile.id != text(a, "id")? {
        return Err("Live profile ID does not match".into());
    }
    let end = a["end"].as_bool().unwrap_or(false);
    if profile.child.try_wait().map_err(|e| e.to_string())?.is_some() {
        return Err("Profiled process has exited; use profile_stop to collect artifacts".into());
    }
    let pid = profile.child.id();
    let id = profile.id.clone();
    let folder = profile.folder.clone();
    let mut new_triggers = profile.triggers;
    let response = match &mut profile.gate {
        Gate::Radv { trigger } => {
            if end {
                return Err("end applies to tracer rocprofv3 only".into());
            }
            if rocprofv3_only_window_keys().iter().any(|key| a.get(*key).is_some()) {
                return Err(format!("{} apply to tracer rocprofv3 only", rocprofv3_only_window_keys().join(", ")));
            }
            fs::OpenOptions::new().write(true).create_new(true).open(trigger.as_path())
                .map_err(|e| format!("Cannot create RADV trigger (a previous trigger may still be pending): {e}"))?;
            new_triggers += 1;
            json!({"id":id,"triggerFile":trigger,"triggerCount":new_triggers,"next":"Send the inference request now; RADV captures the next eligible frame or submit"})
        }
        Gate::Rocprofv3 { profiler, window, collection } => {
            if end {
                // end only closes the open window; combining it with collection
                // options silently dropped them before, so reject that up front.
                if let Some(key) = rocprofv3_only_window_keys().iter().find(|key| a.get(**key).is_some()) {
                    return Err(format!("end only closes the open window and cannot be combined with collection options ({key} given); open a new window to change the collection"));
                }
                let mut open = window.take().ok_or("No collection window is open to end")?;
                let count = new_triggers;
                if !open.triggered {
                    // A timed window detaches on its own timer, not on end:true. If
                    // it has already exited there is nothing left to end, so report
                    // it detached rather than tell the client to keep waiting for
                    // something that already happened.
                    if open.child.try_wait().map_err(|e| e.to_string())?.is_some() {
                        json!({"id":id,"ended":count,"detached":true,"directory":folder.join(format!("window-{count}")),"warnings":[],"next":"Collect with profile_stop, or trigger again for another window with the same streams, filters and mode"})
                    } else {
                        *window = Some(open);
                        return Err("This window ends on its own timer (windowMs); wait for profile_status to report windowRunning=false".into());
                    }
                } else {
                    let (detached, warn) = detach(&mut open.child, true, flag)?;
                    let warnings: Vec<String> = warn.into_iter().collect();
                    json!({"id":id,"ended":count,"detached":detached,"directory":folder.join(format!("window-{count}")),"warnings":warnings,"next":"Collect with profile_stop, or trigger again for another window with the same streams, filters and mode"})
                }
            } else {
                if let Some(open) = window {
                    if open.child.try_wait().map_err(|e| e.to_string())?.is_none() {
                        return Err(if open.triggered {
                            "A collection window is still open; end it with profile_trigger end:true, or call profile_stop".into()
                        } else {
                            "A collection window is still open; wait for profile_status to report windowRunning=false".into()
                        });
                    }
                    *window = None;
                }
                let triggered = a["triggered"].as_bool().unwrap_or(false);
                if triggered && a.get("windowMs").is_some() {
                    return Err("windowMs and triggered are mutually exclusive; a triggered window has no fixed duration".into());
                }
                let ms = if triggered {
                    None
                } else {
                    let ms = a["windowMs"].as_u64().unwrap_or(5000);
                    if !(1..=600_000).contains(&ms) {
                        return Err("windowMs must be between 1 and 600000".into());
                    }
                    Some(ms)
                };
                let mut collection_args = Vec::new();
                rocprof_streams(a, &mut collection_args)?;
                kernel_filters(a, &mut collection_args)?;
                // Whether this call is the window that fixes the per-process
                // collection, so a failed launch can roll it back (see below).
                let first_window = collection.is_none();
                match collection {
                    Some(prev) => {
                        if prev.triggered != triggered {
                            return Err(format!("rocprofv3 reattaches to a PID only with the options it pickled on the first window: this process started in {} mode, so every window must stay {}. Use profile_stop and a fresh profile_start to change mode.",
                                if prev.triggered { "triggered (no fixed duration)" } else { "timed (windowMs)" },
                                if prev.triggered { "triggered" } else { "timed" }));
                        }
                        if prev.args != collection_args {
                            return Err(format!("rocprofv3 requires every window of one process to request the same streams, counters, att and kernel filters (it reattaches with the first window's pickled options). First window: [{}]; this window: [{}]. Match them, or profile_stop and start a fresh run.", prev.args.join(" "), collection_args.join(" ")));
                        }
                    }
                    None => *collection = Some(Collection { args: collection_args.clone(), triggered }),
                }
                let count = new_triggers + 1;
                let directory = folder.join(format!("window-{count}"));
                fs::create_dir(&directory).map_err(|e| e.to_string())?;
                let mut command = vec![
                    profiler.clone(),
                    "--output-format".into(),
                    "csv".into(),
                    "--output-directory".into(),
                    directory.to_string_lossy().into(),
                ];
                command.extend(collection_args);
                command.push("--attach".into());
                command.push(pid.to_string());
                if let Some(ms) = ms {
                    command.push("--attach-duration-msec".into());
                    command.push(ms.to_string());
                }
                command.push("--attach-sync-output".into());
                // If the launch fails, remove the window directory and roll back the
                // collection this call just committed; otherwise the directory and a
                // committed collection would poison every later trigger with a stale
                // "File exists" while triggers stayed at its old value.
                let attach = match spawn_attach_window(&command, &directory, triggered, pid, flag) {
                    Ok(attach) => attach,
                    Err(e) => {
                        let _ = fs::remove_dir_all(&directory);
                        if first_window {
                            *collection = None;
                        }
                        return Err(e);
                    }
                };
                let warnings: Vec<String> = attach.diagnostic.into_iter().collect();
                *window = Some(AttachWindow { child: attach.child, directory: directory.clone(), triggered });
                new_triggers = count;
                let next = if triggered {
                    "Send the workload now; the window stays attached. End it with profile_trigger end:true (writes a newline so rocprofv3 detaches) or profile_stop when the work you want captured is done."
                } else {
                    "Send the workload now; rocprofv3 collects the selected streams for everything dispatched during the window, then detaches (detachment can lag the window end, 1-2 minutes under ROCm). Wait for profile_status windowRunning=false, then trigger again (same streams, filters and mode) or profile_stop."
                };
                json!({"id":id,"window":count,"mode":if triggered { "triggered" } else { "timed" },"windowMs":ms,"directory":directory,"attachAttempts":attach.attempts,"warnings":warnings,"next":next})
            }
        }
    };
    profile.triggers = new_triggers;
    Ok(response)
}

/// Reports whether a live profiled process and its current window are running.
/// Shared by the MCP `profile_status` arm and `live_profile_status`.
fn profile_status_core(control: &Control, a: &Value) -> Result<Value, String> {
    let mut live = control.live_profile.lock().unwrap();
    let profile = live.as_mut().ok_or("No live profile; use profile_stop to collect a finished run")?;
    if profile.id != text(a, "id")? {
        return Err("Live profile ID does not match".into());
    }
    let status = profile.child.try_wait().map_err(|e| e.to_string())?;
    let id = profile.id.clone();
    let folder = profile.folder.clone();
    let triggers = profile.triggers;
    match &mut profile.gate {
        Gate::Radv { trigger } => {
            let (saved, unique) = rgp_save_counts(&folder);
            Ok(json!({"id":id,"running":status.is_none(),"exitCode":status.and_then(|s| s.code()),"triggerPending":trigger.exists(),"triggerCount":triggers,"savedMessages":saved,"uniqueSavePaths":unique,"directory":folder,"stdout":folder.join("stdout.log"),"stderr":folder.join("stderr.log")}))
        }
        Gate::Rocprofv3 { window, .. } => {
            let (window_running, window_triggered) = match window {
                Some(open) => (open.child.try_wait().map_err(|e| e.to_string())?.is_none(), open.triggered),
                None => (false, false),
            };
            Ok(json!({"id":id,"running":status.is_none(),"exitCode":status.and_then(|s| s.code()),"windowRunning":window_running,"windowTriggered":window_triggered,"triggerCount":triggers,"directory":folder,"stdout":folder.join("stdout.log"),"stderr":folder.join("stderr.log")}))
        }
    }
}

/// Stops a live profiled application and collects its artifacts. Shared by the
/// MCP `profile_stop` arm and `live_profile_stop`.
fn profile_stop_core(control: &Control, a: &Value, flag: &AtomicBool) -> Result<Value, String> {
    let mut live = control.live_profile.lock().unwrap();
    {
        let profile = live.as_ref().ok_or("No live profile")?;
        if profile.id != text(a, "id")? {
            return Err("Live profile ID does not match".into());
        }
    }
    let LiveProfile { mut child, folder, program, started, triggers, gate, .. } = live.take().unwrap();
    // Release the live_profile mutex before the up-to-180 s detach wait below.
    // Holding it across the wait would block Control::status()'s try_lock on the
    // GUI poll and (with a blocking lock) any concurrent status call.
    drop(live);
    let status = child.try_wait().map_err(|e| e.to_string())?;
    match gate {
        Gate::Radv { trigger } => {
            tuning::stop(&mut child);
            let _ = fs::remove_file(&trigger);
            let mut report = radv_artifacts(&program, started, &folder, if status.is_some() { "exited" } else { "stopped" })
                .map_err(|e| format!("{e}. Artifacts remain in {}", folder.display()))?;
            report["triggerCount"] = json!(triggers);
            report["exitCode"] = json!(status.and_then(|code| code.code()));
            if triggers > 0 && report["files"].as_array().is_some_and(|files| files.is_empty()) {
                report["warnings"].as_array_mut().unwrap().push(json!("No RGP file was saved after the trigger. Check stderr.log; RADV may not have seen an eligible traceable submit before the process stopped."));
            }
            Ok(report)
        }
        Gate::Rocprofv3 { window, .. } => {
            let mut warnings = Vec::new();
            if let Some(mut open) = window {
                // A triggered window is detached by a newline (then SIGINT/SIGKILL);
                // a timed window detaches on its own, so detach only escalates if it
                // has not finished. One primitive handles both.
                if let (_, Some(warn)) = detach(&mut open.child, open.triggered, flag)? {
                    warnings.push(warn);
                }
            }
            tuning::stop(&mut child);
            // rocprofv3 pickles each PID's attach options in this file and never
            // removes it; a later session (or a manual rocprofv3 --attach) on a
            // reused PID would load these and fatal-error on any mismatch. Remove
            // it best-effort now that the target is stopped.
            let _ = fs::remove_file(format!("/tmp/rocprofv3_attach_{}.pkl", child.id()));
            let mut files = Vec::new();
            find_outputs(&folder, &mut files)?;
            for count in 1..=triggers {
                let window = folder.join(format!("window-{count}"));
                if !files.iter().any(|f| f.starts_with(&window)) {
                    let log = fs::read_to_string(window.join("stderr.log")).unwrap_or_default();
                    let detail: String = log.lines().rev().find(|l| l.contains("Error") || l.contains("error"))
                        .or_else(|| log.lines().rev().find(|l| !l.trim().is_empty()))
                        .unwrap_or("stderr.log is empty").chars().take(300).collect();
                    if attach_set_agent_failure(&log) {
                        warnings.push(format!("window-{count} produced no output files: rocprofv3 could not set the agent to be profiled (the ROCm set_profiler_active_on_queue PM4 handshake timed out while the target queue was busy — a known ROCm attach limitation, not fixed here; retry when the GPU queue is idle). Its stderr: {detail}"));
                    } else {
                        warnings.push(format!("window-{count} produced no output files (rocprofv3 requires every window of one process to request the same streams, counters, att and kernel filters; the attach may also have failed or no GPU work ran). Its stderr: {detail}"));
                    }
                }
            }
            Ok(json!({"status":if status.is_some() { "exited" } else { "stopped" },"directory":folder,"files":files,"triggerCount":triggers,"exitCode":status.and_then(|code| code.code()),"stdout":folder.join("stdout.log"),"stderr":folder.join("stderr.log"),"warnings":warnings,"next":"Use capture_open with a returned trace or counter CSV; open a ui_output directory or a single .att file for SQTT waves"}))
        }
    }
}

/// Whether a process id is still alive (used to bound attach retries).
fn process_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// The last ~400 characters of a window's rocprofv3 stderr log.
fn stderr_tail(dir: &Path) -> String {
    let log = fs::read_to_string(dir.join("stderr.log")).unwrap_or_default();
    let trimmed = log.trim_end();
    trimmed
        .chars()
        .rev()
        .take(400)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

/// Whether rocprofv3's stderr shows the "could not set the agent to be profiled"
/// attach failure (the `set_profiler_active_on_queue` timeout in queue.cpp).
fn attach_set_agent_failure(log: &str) -> bool {
    log.contains("Could not set agent to be profiled")
        || log.contains("set_profiler_active_on_queue")
        || log.contains("set_profiler_active")
}

enum AttachOutcome {
    /// Still running after the confirmation window — attached.
    Running,
    /// Exited during confirmation for a reason other than the attach failure.
    ExitedOther,
    /// Exited (or is exiting) with the set-agent attach failure; carries the tail.
    AttachFailure(String),
}

/// Watches a freshly spawned rocprofv3 attach process for up to `confirm` to tell
/// an established attach from the `set_profiler_active_on_queue` failure.
fn confirm_attach(child: &mut Child, dir: &Path, confirm: Duration, flag: &AtomicBool) -> Result<AttachOutcome, String> {
    let start = Instant::now();
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            let tail = stderr_tail(dir);
            return Ok(if attach_set_agent_failure(&tail) {
                AttachOutcome::AttachFailure(tail)
            } else {
                AttachOutcome::ExitedOther
            });
        }
        if attach_set_agent_failure(&stderr_tail(dir)) {
            // The failure is logged and rocprofv3 is about to abort; stop it so the
            // caller can retry without waiting out the rest of the confirm window.
            let tail = stderr_tail(dir);
            tuning::stop(child);
            return Ok(AttachOutcome::AttachFailure(tail));
        }
        if flag.load(Ordering::SeqCst) || start.elapsed() > confirm {
            return Ok(AttachOutcome::Running);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

struct AttachResult {
    child: Child,
    attempts: u32,
    diagnostic: Option<String>,
}

/// Spawns a rocprofv3 attach window and, when the attach fails because the target
/// queue was busy (`set_profiler_active_on_queue` timeout), retries with backoff
/// while the target is alive. The retry mitigates but cannot fix the ROCm
/// limitation; a final failure is returned as a diagnostic, not an error, so the
/// client can decide whether to trigger again.
fn spawn_attach_window(command: &[String], dir: &Path, triggered: bool, pid: u32, flag: &AtomicBool) -> Result<AttachResult, String> {
    const MAX_RETRIES: u32 = 2;
    const CONFIRM: Duration = Duration::from_secs(4);
    let mut backoff = Duration::from_millis(500);
    let mut attempt = 0;
    loop {
        let mut child = spawn_process_env_stdin(command, dir, dir, &[], triggered)?;
        match confirm_attach(&mut child, dir, CONFIRM, flag)? {
            AttachOutcome::Running => {
                return Ok(AttachResult { child, attempts: attempt + 1, diagnostic: None });
            }
            AttachOutcome::ExitedOther => {
                // rocprofv3 exited during confirmation for a reason other than the
                // set-agent failure. A short timed window that completed with output
                // is fine; an early exit that produced nothing is not (e.g. a
                // pickled-options mismatch from an earlier session on this PID), so
                // surface it as a diagnostic instead of reporting a silent success.
                let mut produced = Vec::new();
                let _ = find_outputs(dir, &mut produced);
                let diagnostic = produced.is_empty().then(|| {
                    format!("rocprofv3 exited during attach confirmation with no output files and without the set-agent failure; the attach may not have taken (for example a pickled-options mismatch from an earlier session on this PID, cleared on profile_stop). rocprofv3 stderr tail: {}", stderr_tail(dir))
                });
                return Ok(AttachResult { child, attempts: attempt + 1, diagnostic });
            }
            AttachOutcome::AttachFailure(tail) => {
                if attempt >= MAX_RETRIES || flag.load(Ordering::SeqCst) || !process_alive(pid) {
                    return Ok(AttachResult {
                        child,
                        attempts: attempt + 1,
                        diagnostic: Some(format!("rocprofv3 could not set the agent to be profiled after {} attempt(s): the ROCm set_profiler_active_on_queue PM4 handshake timed out while the target queue was busy. This is a known ROCm attach limitation; retry when the GPU queue is idle. rocprofv3 stderr tail: {tail}", attempt + 1)),
                    });
                }
                attempt += 1;
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(Duration::from_secs(2));
            }
        }
    }
}

/// Sends SIGINT to a window's process group. The window is its own group
/// (`process_group(0)`), so this reaches rocprofv3 and anything it spawned.
fn send_sigint(child: &Child) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGINT);
    }
}

/// Detaches a rocprofv3 attach window and waits for it to exit so
/// `--attach-sync-output` can finish writing. For a triggered window
/// (`write_newline`), first writes a newline to rocprofv3's stdin (its `input()`
/// returns → detach) and closes the pipe (EOF, a second detach signal). If the
/// window has not exited after ~30 s, sends SIGINT — rocprofv3's documented
/// alternative detach signal (docs/using-rocprofv3-process-attachment.rst:93) —
/// which still flushes the sync-output files; only past 180 s does it SIGKILL,
/// which can truncate them and leave a surviving target in an unknown attach
/// state. Returns (detached_cleanly, optional warning). Shared by the `end:true`
/// trigger path and the profile_stop timed/triggered window collection.
fn detach(child: &mut Child, write_newline: bool, flag: &AtomicBool) -> Result<(bool, Option<String>), String> {
    const SIGINT_AFTER: Duration = Duration::from_secs(30);
    const DEADLINE: Duration = Duration::from_secs(180);
    if write_newline {
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(b"\n");
            let _ = stdin.flush();
            // Dropping stdin here also closes the pipe (EOF), a second detach signal.
        }
    }
    let waited = Instant::now();
    let mut interrupted = false;
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            return Ok((true, None));
        }
        if flag.load(Ordering::SeqCst) || waited.elapsed() > DEADLINE {
            tuning::stop(child);
            return Ok((false, Some("The collection window was aborted before rocprofv3 detached (detachment can take 1-2 minutes under ROCm); its files may be missing or incomplete.".to_owned())));
        }
        if !interrupted && waited.elapsed() > SIGINT_AFTER {
            send_sigint(child);
            interrupted = true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn text<'a>(a: &'a Value, key: &str) -> Result<&'a str, String> {
    a[key]
        .as_str()
        .filter(|s| !s.is_empty() && !s.contains('\0'))
        .ok_or_else(|| format!("{key} must be a nonempty string"))
}
fn range(v: &Value) -> Result<[f64; 2], String> {
    let r: [f64; 2] = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
    if !r.iter().all(|n| n.is_finite()) || r[1] <= r[0] {
        return Err("Invalid range".into());
    }
    Ok(r)
}
fn new_target(p: &Path) -> Result<(), String> {
    if p.exists() {
        Err("Output already exists; choose a new path".into())
    } else {
        Ok(())
    }
}
fn summary(c: &Capture) -> Value {
    json!({"id":c.id,"name":c.name,"path":c.input_path,"format":c.format,"architecture":c.architecture,"unit":c.unit,"start":c.start,"end":c.end,"warnings":c.warnings,"dispatches":c.dispatches.len(),"instructions":c.instructions.len(),"waves":c.waves.len(),"counters":c.counters.len(),"compileReports":c.compile_reports.len()})
}
/// Builds the queryable `compileReports` rows: one row per imported Loom compile
/// report, each carrying its report-level static facts and a nested per-entry
/// array. These are compile-time static facts (unit "static", version-zero,
/// same-compiler-horizon), never measurements; the labelling travels with every
/// row so a client reading the dataset cannot mistake it for runtime data.
fn compile_report_rows(c: &Capture) -> Vec<Value> {
    c.compile_reports.iter().map(compile_report_row).collect()
}
fn compile_report_row(r: &profile_core::loom_report::LoomCompileReport) -> Value {
    let mut o = serde_json::Map::new();
    o.insert("stability".into(), json!(r.stability));
    o.insert("unit".into(), json!("static"));
    o.insert("facts".into(), json!("Compile-time static facts from the Loom compiler; not measurements, dispatches or runtime counts. Absent fields are unavailable, not zero. Version-zero, same-compiler-horizon: not comparable across targets or compiler horizons."));
    o.insert("containerKind".into(), json!(r.container_kind));
    o.insert("mode".into(), json!(r.mode));
    o.insert("schemaVersion".into(), json!(r.schema_version));
    o.insert(
        "status".into(),
        json!({"code": r.status_code, "name": r.status_name}),
    );
    if !r.identity.is_empty() {
        o.insert("identity".into(), json!(r.identity));
    }
    if let Some(flags) = r.detail_flags {
        o.insert("detailFlags".into(), json!(flags));
    }
    if !r.detail_flag_names.is_empty() {
        o.insert("detailFlagNames".into(), json!(r.detail_flag_names));
    }
    if !r.envelope_context.is_empty() {
        o.insert("envelopeContext".into(), json!(r.envelope_context));
    }
    if let Some(size) = r.artifact_size {
        o.insert("artifactSize".into(), json!(size));
    }
    if !r.config_bindings.is_empty() {
        o.insert("configBindings".into(), json!(r.config_bindings));
    }
    o.insert("provenance".into(), json!(r.provenance));
    if let Some(tr) = target_resource_facts(r.sections.get("target_resources")) {
        o.insert("targetResources".into(), tr);
    }
    if let Some(mix) = instruction_mix_facts(&r.sections) {
        o.insert("instructionMix".into(), mix);
    }
    if let Some(wp) = r.sections.get("wait_plan") {
        o.insert("waitPlan".into(), wp.clone());
    }
    if let Some(wl) = r.sections.get("workload") {
        o.insert("workload".into(), wl.clone());
    }
    if !r.residency_constraints.is_empty() {
        o.insert("residencyConstraints".into(), json!(r.residency_constraints));
    }
    o.insert(
        "entries".into(),
        json!(r.entries.iter().map(compile_entry_row).collect::<Vec<_>>()),
    );
    Value::Object(o)
}
fn compile_entry_row(e: &profile_core::loom_report::LoomCompileEntry) -> Value {
    let mut o = serde_json::Map::new();
    o.insert("index".into(), json!(e.index));
    o.insert("displayName".into(), json!(e.display_name));
    if !e.identity.is_empty() {
        o.insert("identity".into(), json!(e.identity));
    }
    if let Some(flags) = e.detail_flags {
        o.insert("detailFlags".into(), json!(flags));
    }
    if !e.detail_flag_names.is_empty() {
        o.insert("detailFlagNames".into(), json!(e.detail_flag_names));
    }
    // Flat integer counters keep their producer names: allocation/spill counts,
    // emitted instruction counts, code byte counts, local/private memory bytes.
    if !e.counters.is_empty() {
        o.insert("counters".into(), json!(e.counters));
    }
    if let Some(tr) = target_resource_facts(e.sections.get("target_resources")) {
        o.insert("targetResources".into(), tr);
    }
    if let Some(mix) = instruction_mix_facts(&e.sections) {
        o.insert("instructionMix".into(), mix);
    }
    if let Some(wp) = e.sections.get("wait_plan") {
        o.insert("waitPlan".into(), wp.clone());
    }
    if let Some(wl) = e.sections.get("workload") {
        o.insert("workload".into(), wl.clone());
    }
    Value::Object(o)
}
/// Pulls the called-out target-resource facts out of the verbatim section:
/// SGPR/VGPR final counts plus scheduled pressure, subgroup geometry, the static
/// occupancy percent, the limiting resource and the residency summary. Absent
/// sub-fields stay absent rather than defaulting to zero.
fn target_resource_facts(tr: Option<&Value>) -> Option<Value> {
    let tr = tr?.as_object()?;
    let mut out = serde_json::Map::new();
    let registers = |class: &str| -> Option<Value> {
        let r = tr.get(class)?.as_object()?;
        let mut m = serde_json::Map::new();
        if let Some(v) = r.get("register_class") {
            m.insert("registerClass".into(), v.clone());
        }
        if let Some(f) = r.get("final").and_then(Value::as_object) {
            if let Some(v) = f.get("register_count") {
                m.insert("finalRegisterCount".into(), v.clone());
            }
            if let Some(v) = f.get("overhead_units") {
                m.insert("overheadUnits".into(), v.clone());
            }
        }
        if let Some(p) = r.get("scheduled_pressure").and_then(Value::as_object) {
            if let Some(v) = p.get("peak_live_units") {
                m.insert("scheduledPeakLiveUnits".into(), v.clone());
            }
        }
        (!m.is_empty()).then(|| Value::Object(m))
    };
    if let Some(v) = registers("scalar") {
        out.insert("sgpr".into(), v);
    }
    if let Some(v) = registers("vector") {
        out.insert("vgpr".into(), v);
    }
    for (src, dst) in [
        ("subgroup_size", "subgroupSize"),
        ("max_subgroups_per_simd", "maxSubgroupsPerSimd"),
        ("resident_subgroups_per_simd", "residentSubgroupsPerSimd"),
        ("occupancy_percent", "occupancyPercent"),
        ("limiting_resource", "limitingResource"),
    ] {
        if let Some(v) = tr.get(src) {
            out.insert(dst.into(), v.clone());
        }
    }
    if let Some(v) = tr.get("residency") {
        out.insert("residency".into(), v.clone());
    }
    (!out.is_empty()).then(|| Value::Object(out))
}
/// Surfaces the static (and dynamic, when present) instruction-mix sections
/// verbatim under camelCase keys, or nothing when neither is present.
fn instruction_mix_facts(sections: &BTreeMap<String, Value>) -> Option<Value> {
    let mut m = serde_json::Map::new();
    if let Some(v) = sections.get("static_instruction_mix") {
        m.insert("static".into(), v.clone());
    }
    if let Some(v) = sections.get("dynamic_instruction_mix") {
        m.insert("dynamic".into(), v.clone());
    }
    (!m.is_empty()).then(|| Value::Object(m))
}
fn page(v: Value, a: &Value, flag: &AtomicBool) -> Result<Value, String> {
    let rows = v.as_array().ok_or("Dataset is not a list")?;
    let query = a["query"].as_str().unwrap_or("").to_lowercase();
    let offset = a["offset"].as_u64().unwrap_or(0) as usize;
    let limit = a["limit"].as_u64().unwrap_or(100).min(1000) as usize;
    let mut matches = Vec::new();
    let mut total = 0;
    for row in rows {
        Control::check(flag)?;
        if query.is_empty() || row.to_string().to_lowercase().contains(&query) {
            if total >= offset && matches.len() < limit {
                matches.push(row.clone());
            }
            total += 1;
        }
    }
    Ok(
        json!({"rows":matches,"total":total,"offset":offset,"nextOffset":if offset+limit<total{Some(offset+limit)}else{None}}),
    )
}
fn reference(a: &Value) -> Result<Value, String> {
    let architecture = a["architecture"].as_str().unwrap_or("manifest");
    let raw = match architecture {
        "manifest" => include_str!("../../public/isa/manifest.json"),
        "cdna1" => include_str!("../../public/isa/cdna1.json"),
        "cdna2" => include_str!("../../public/isa/cdna2.json"),
        "cdna3" => include_str!("../../public/isa/cdna3.json"),
        "cdna4" => include_str!("../../public/isa/cdna4.json"),
        "rdna1" => include_str!("../../public/isa/rdna1.json"),
        "rdna2" => include_str!("../../public/isa/rdna2.json"),
        "rdna3" => include_str!("../../public/isa/rdna3.json"),
        "rdna3_5" => include_str!("../../public/isa/rdna3_5.json"),
        "rdna4" => include_str!("../../public/isa/rdna4.json"),
        _ => return Err("Unknown architecture".into()),
    };
    let mut v: Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if architecture == "manifest" {
        return Ok(v);
    }
    let instructions = v
        .as_object_mut()
        .unwrap()
        .remove("instructions")
        .unwrap_or(json!([]));
    v.as_object_mut().unwrap().remove("encodings");
    Ok(json!({"architecture":v,"instructions":page(instructions,a,&AtomicBool::new(false))?}))
}
fn tools() -> Vec<Value> {
    let str_ = json!({"type":"string"});
    let num = json!({"type":"integer","minimum":0});
    let obj = json!({"type":"object"});
    let traces = json!({"type":"array","items":{"enum":["kernel","hip","marker","memoryCopy","memoryAllocation","scratchMemory","hsa","rccl","runtime","sys"]},"minItems":1});
    let att = json!({"type":"object","properties":{"targetCu":num,"simdSelect":num,"bufferSize":num,"shaderEngineMask":num},"additionalProperties":false});
    let entries=vec![
 ("workspace","List open captures, tuning sessions, current view and MCP status.",json!({}),vec![]),
 ("capture_open","Import a trace, counters, ATT run, sampling data or portable capture from an explicit path.",json!({"path":str_,"samplingMethod":str_,"loadWaveStates":{"type":"boolean"}}),vec!["path"]),
 ("capture_analyze","Run the same analysis functions as the GUI: kernels, occupancy, activity, hardware, hotspots, source, markers, scalar, tensor. Scalar takes name/formula (C0 etc). Tensor takes definitions (NAME := expression) and raw named {shape:[XCC,SE,CU,Time],values:[...]}. Timeline renders dispatch or global residency HTML with the same paged exporter as the GUI. Diagnostics produces its diagnostic manifest. Optional output writes a new HTML, diagnostics JSON, or analysis JSON file. Returns paged rows otherwise. Requires Node.js.",json!({"captureId":str_,"analysis":{"enum":["kernels","occupancy","activity","hardware","hotspots","source","markers","scalar","tensor","timeline","diagnostics"]},"metric":{"enum":["latency","pcSamples","pcIssued","pcStalled"]},"se":num,"kernel":str_,"range":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2},"name":str_,"formula":str_,"definitions":str_,"raw":obj,"output":str_,"timeline":{"enum":["dispatch","global"]},"timePages":num,"lanesPerPage":num,"workspace":obj,"query":str_,"offset":num,"limit":num}),vec!["captureId","analysis"]),
 ("profile_start","Start a long-lived application for gated live profiling. tracer radv (default) runs a Vulkan server under RADV RGP file-trigger capture: headless compute needs a RADV build with docs/radv-compute-trigger.patch; stock Mesa checks file triggers only on swapchain presentation, and the capture gets the next eligible submit after each trigger. tracer rocprofv3 starts a HIP/ROCm application armed with ROCP_TOOL_ATTACH=1 so each profile_trigger attaches rocprofv3 for one bounded, time-based collection window; profiler overrides the rocprofv3 path. Returns an ID and log paths immediately. Only one live profile can run at a time; the process is stopped when rocprof-studio shuts down. Does not execute a shell.",json!({"argv":{"type":"array","items":str_,"minItems":1},"cwd":str_,"env":obj,"tracer":{"enum":["radv","rocprofv3"]},"profiler":str_}),vec!["argv","cwd"]),
 ("profile_trigger","Fire one capture in a live profile. tracer radv: creates the file that arms one RADV RGP capture; send the workload request after this call; a pending trigger cannot be armed again until RADV consumes it. tracer rocprofv3: attaches rocprofv3 to the application, collecting the selected streams (counters, traces, att; kernel trace if none given, as in profile_run) plus optional kernelIncludeRegex/kernelExcludeRegex (mapped to --kernel-include-regex/--kernel-exclude-regex, honoured in attach mode including for att data) and optional kernelIterationRange (mapped to --kernel-iteration-range, e.g. \"64\" or \"0-3\": ATT otherwise samples the FIRST occurrence of each kernel name and a name regex cannot discriminate grids that share a name, so an iteration range is the only way to target a later dispatch) into a window-N directory. Two window modes: a TIMED window (windowMs milliseconds, default 5000) that detaches on its own, or a TRIGGERED window (triggered:true, no windowMs) that stays attached until you end it with profile_trigger end:true (writes a newline to rocprofv3's stdin so it detaches) or profile_stop — use it to capture exactly one request without guessing a duration. A window is time- or trigger-based and cannot isolate a single dispatch; only one window is open at a time; detachment can lag by 1-2 minutes (ROCm); and because rocprofv3 pickles each PID's attach options and reattaches only with identical ones, every window of one process must request the same streams, counters, att, kernel filters AND mode (timed vs triggered) — a mismatch is rejected up front, and an empty window still warns at profile_stop. Attach is retried with backoff when the ROCm set_profiler_active handshake times out on a busy queue; attachAttempts and any attachWarning are reported.",json!({"id":str_,"windowMs":{"type":"integer","minimum":1,"maximum":600000},"triggered":{"type":"boolean"},"end":{"type":"boolean"},"counters":{"type":"array","items":str_},"traces":traces,"att":att,"kernelIncludeRegex":str_,"kernelExcludeRegex":str_,"kernelIterationRange":str_}),vec!["id"]),
 ("profile_status","Check whether a live profiled process is running. tracer radv also reports whether its trigger is pending and how many RGP saves RADV has announced; tracer rocprofv3 reports whether the current attach window is still open (windowRunning) and whether it is a triggered window (windowTriggered). Call profile_stop to collect files.",json!({"id":str_}),vec!["id"]),
 ("profile_stop","Stop a live profiled application and collect its artifacts: RADV .rgp files (tracer radv), or every attach window's trace/counter CSVs, .att files and decoded ui_output directories (tracer rocprofv3). An open triggered window is detached first by writing a newline to rocprofv3's stdin; a timed window detaches on its own. If a window has not detached after ~30 s it is sent SIGINT (rocprofv3's documented alternative detach, which still flushes --attach-sync-output), and only past 180 s is it SIGKILLed with a warning (SIGKILL can truncate the sync-output files). Warnings also surface the rocprofv3 stderr tail for any window that produced no files, and name the ROCm set_profiler_active attach failure explicitly when it occurred. Reports log paths and warnings.",json!({"id":str_}),vec!["id"]),
 ("profile_run","Run an application argv under a profiler and return output files to import with capture_open. tracer rocprofv3 (default) collects, in one combinable run: counters as --pmc hardware counters, traces as rocprofv3 trace streams (kernel, hip, marker, memoryCopy, memoryAllocation, scratchMemory, hsa, rccl, runtime, sys), and att as SQTT thread trace (--att) with options targetCu, simdSelect, bufferSize (bytes), shaderEngineMask; with none of the three, a kernel trace is collected. ATT writes *_shader_engine_SE_RUN.att files plus decoded ui_output directories when the rocprof trace decoder is installed; open one with capture_open for waves. kernelIncludeRegex/kernelExcludeRegex restrict collection to matching kernels (--kernel-include-regex/--kernel-exclude-regex); kernelIterationRange (--kernel-iteration-range, e.g. \"64\" or \"0-3\") selects which occurrences are sampled, the only way to target a dispatch other than the first under a given kernel name. tracer radv uses per-submit capture (experimental), which can greatly slow startup and overwrite same-second filenames; prefer profile_start/profile_trigger/profile_stop for long-lived servers. env adds environment variables such as VK_DRIVER_FILES or RADV_THREAD_TRACE_ROCPROF_COMPAT. Cancellable; timeoutSeconds defaults to 600, at most 3600. Does not execute a shell.",json!({"argv":{"type":"array","items":str_,"minItems":1},"cwd":str_,"profiler":str_,"counters":{"type":"array","items":str_},"traces":traces,"att":att,"kernelIncludeRegex":str_,"kernelExcludeRegex":str_,"kernelIterationRange":str_,"tracer":{"enum":["rocprofv3","radv"]},"env":obj,"timeoutSeconds":{"type":"integer","minimum":1,"maximum":3600}}),vec!["argv","cwd"]),
 ("capture_close","Close an imported capture.",json!({"captureId":str_}),vec!["captureId"]),
 ("capture_query","Page/search capture datasets. Dataset: summary, dispatches, instructions, waves, counters, markers, hostCalls, codeObjects, intervals, events, eventCounters, monitorIntervals, otherSimd, metadata, sources, compileReports. Limit <=1000. Counter rows include recorded samples. compileReports returns Loom compile-time STATIC facts (unit static, version-zero/same-compiler-horizon), one row per report with report- and per-entry identity, detail flags, target resources (SGPR/VGPR, subgroup geometry, static occupancy percent, limiting resource, residency), instruction mix, allocation/spill and emitted-instruction counters, wait plan and workload geometry; these are compiler evidence, never measurements, dispatches or runtime counts.",json!({"captureId":str_,"dataset":str_,"query":str_,"offset":num,"limit":num}),vec!["captureId","dataset"]),
 ("workspace_select","Follow a capture, view, range, wave, instruction or dispatch in the GUI. Views: reference, trace, hardware, summary, sampling, occupancy, dispatch, hotspots, flame, markers, counters, evidence.",json!({"captureId":str_,"view":str_,"range":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2},"waveId":str_,"dispatchId":str_,"instruction":num,"timeline":str_,"se":num}),vec!["captureId"]),
 ("wave_read","Read a detailed ATT wave including instruction execution and wait tokens.",json!({"captureId":str_,"waveId":str_}),vec!["captureId","waveId"]),
 ("wave_overview","Load bounded wave/timeline overview bins, optionally architecture-aware SIMD utilization with separate LDS attribution.",json!({"captureId":str_,"indices":{"type":"array","items":num},"range":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2},"utilization":{"type":"boolean"},"separateLds":{"type":"boolean"}}),vec!["captureId","indices","range"]),
 ("wave_analyze","Aggregate dynamic instruction statistics for selected wave indices and time range.",json!({"captureId":str_,"indices":{"type":"array","items":num},"range":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2}}),vec!["captureId","indices","range"]),
 ("hidden_latency","Analyze hidden/nonhidden latency for detailed ATT waves.",json!({"captureId":str_}),vec!["captureId"]),
 ("source_map","Map recorded source paths from a prefix to a local directory; follows in the GUI.",json!({"captureId":str_,"prefix":str_,"directory":str_}),vec!["captureId","prefix","directory"]),
 ("definition_file","Read or write a counter-definition file using GUI validation. Writes require a new path.",json!({"op":{"enum":["read","write"]},"path":str_,"text":str_}),vec!["op","path"]),
 ("plot_export","Save validated SVG/HTML text or PNG bytes to a new path. Use capture_analyze with timeline and output to generate printable timelines directly.",json!({"path":str_,"format":{"enum":["svg","png","html"]},"text":str_,"bytes":{"type":"array","items":num}}),vec!["path","format"]),
 ("source_read","Read paged source; captureId selects recorded capture source, otherwise read explicit local path.",json!({"captureId":str_,"path":str_,"startLine":num,"count":num}),vec!["path"]),
 ("capture_export","Export a portable capture to a new output path.",json!({"captureId":str_,"path":str_}),vec!["captureId","path"]),
 ("att_discover","Discover ATT runs in a directory.",json!({"path":str_}),vec!["path"]),
 ("att_manifest","Write an ATT manifest to a new path, using the same manifest schema as the GUI.",json!({"path":str_,"manifest":obj}),vec!["path","manifest"]),
 ("spm_attach","Attach SPM data from the same profiling run to a trace using its shader-engine clock anchors, save and open the combined capture.",json!({"captureId":str_,"path":str_,"output":str_,"se":num}),vec!["captureId","path","output","se"]),
 ("code_object_extract","Extract a recorded code object into a new file. Optional sourcePath relocates its container.",json!({"captureId":str_,"objectId":str_,"output":str_,"sourcePath":str_}),vec!["captureId","objectId","output"]),
 ("project_open","Open an existing GUI project and restore its captures/workspace.",json!({"path":str_}),vec!["path"]),
 ("project_save","Write a GUI project document to a new path. Omit project to save the open captures with default view settings, or supply a project object from project_open to retain full workspace settings.",json!({"path":str_,"project":obj}),vec!["path"]),
 ("reference_query","Search/page the embedded ISA and matrix-type documentation. architecture: manifest, cdna1..4, rdna1..4, rdna3_5.",json!({"architecture":str_,"query":str_,"offset":num,"limit":num}),vec![]),
 ("tuning_open","Select one traced dispatch for tuning; opens the native editor when a GUI is attached. A dispatch from a RADV capture opens a Vulkan single-shader session (experimental), anything else a HIP kernel session.",json!({"captureId":str_,"dispatchId":str_}),vec!["captureId","dispatchId"]),
 ("tuning_status","Read a session: mode (vulkan or HIP), source files, ISA, captured arguments or dispatch, calibration, run metrics/deltas, output validation and job log.",json!({"id":str_}),vec!["id"]),
 ("tuning_action","Execute and await a tuning operation. request.op: search (config.sourceTree), select (index), capture (config.cwd/argv; config.captureScope 'reachable', the default, snapshots only the allocations the kernel's arguments reach, following pointer tables, while 'all' snapshots every allocation in the process for kernels that reach memory another way), calibrate, run (variant, code, iterations, validate, autoCounters, autoCalibrate, optional work/counters), save (code). Config merges with session config. Capture preserves launch arguments and inputs; save replaces the selected source definition with original-file conflict checks. Long calls can be cancelled. Vulkan sessions: capture (config.cwd/argv/env, occurrence, optional name/groups) first, then search (matches the captured SPIR-V against the ggml shader build under config.sourceTree), select (index), run (variant, edits {path: full text}, iterations, validate, trace, autoCounters, conditions), save (edits); no calibrate. A Vulkan run replays under conditions {mode: stateless (default) or sustained (back to back, after one restore), memory: device (default), app, gtt or auto (system memory on integrated GPUs), flushMb (evict caches first), repeats 1-10 (replays with fresh allocations; reports their spread)}; the default is the isolated best case. autoCounters (default true) adds GPU cycles, the clock the dispatch ran at, and L0/L1/L2 hit rates. Deltas compare with the latest run under the same conditions and work.",json!({"id":str_,"request":{"type":"object","properties":{"op":{"enum":["search","select","capture","calibrate","run","save"]},"config":obj,"index":num,"variant":{"type":"boolean"},"code":str_,"iterations":num,"validate":{"type":"boolean"},"autoCounters":{"type":"boolean"},"autoCalibrate":{"type":"boolean"},"work":obj,"counters":{"type":"array","items":obj},"edits":obj,"trace":{"type":"boolean"},"conditions":{"type":"object","properties":{"mode":{"enum":["stateless","sustained"]},"memory":{"enum":["device","app","gtt","auto"]},"flushMb":num,"repeats":num}}},"required":["op"]}}),vec!["id","request"]),
 ("tuning_close","Stop and close a tuning session. Its captured inputs (a snapshot of every allocation the kernel reads, often the whole model) and validation dumps are deleted; results, logs and source backups remain on disk.",json!({"id":str_}),vec!["id"]),
 ];
    entries.into_iter().map(|(name,description,properties,required)|json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})).collect()
}

pub fn block_local_invoke(invoke: &tauri::ipc::Invoke<tauri::Wry>) -> bool {
    let command = invoke.message.command();
    let allowed = command.starts_with("mcp_")
        || command.starts_with("read_")
        || [
            "tuning_status",
            "live_profile_status",
            "diagnostic_context",
            "begin_wave_query",
            "cancel_wave_query",
            "analyze_hidden",
        ]
        .contains(&command);
    !allowed
        && invoke
            .message
            .webview()
            .state::<Arc<Control>>()
            .active
            .lock()
            .unwrap()
            .is_some()
}
fn job_directory() -> Result<PathBuf, String> {
    let root = crate::appdir::subdir("mcp/jobs")?.join(random()?);
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    Ok(root)
}
fn run_process(
    argv: Vec<String>,
    folder: &Path,
    flag: &AtomicBool,
    seconds: u64,
) -> Result<(), String> {
    run_process_at(argv, folder, folder, flag, seconds)
}
fn run_process_at(
    argv: Vec<String>,
    cwd: &Path,
    folder: &Path,
    flag: &AtomicBool,
    seconds: u64,
) -> Result<(), String> {
    run_process_env(argv, cwd, folder, flag, seconds, &[])
}
/// Variables the runner removes from the inherited environment, or sets itself.
const RESERVED_ENV: [&str; 4] = ["LD_PRELOAD", "HSA_TOOLS_LIB", "HSA_TOOLS_REPORT_LOAD_FAILURE", "TMPDIR"];
fn profile_env(a: &Value) -> Result<Vec<(String, String)>, String> {
    let Some(env) = a.get("env") else { return Ok(vec![]) };
    let env = env.as_object().ok_or("env must be an object of strings")?;
    if env.len() > 64 {
        return Err("env has more than 64 variables".into());
    }
    env.iter()
        .map(|(k, v)| {
            let name = k.bytes().enumerate().all(|(i, b)| b == b'_' || b.is_ascii_uppercase() || (i > 0 && b.is_ascii_digit()));
            if k.is_empty() || !name || RESERVED_ENV.contains(&k.as_str()) {
                return Err(format!("env variable {k} is not allowed"));
            }
            let value = v.as_str().filter(|s| !s.contains('\0')).ok_or(format!("env {k} must be a string"))?;
            Ok((k.clone(), value.to_owned()))
        })
        .collect()
}
/// Trace streams a client can request, with the rocprofv3 flag each enables.
const TRACE_STREAMS: [(&str, &str); 10] = [
    ("kernel", "--kernel-trace"),
    ("hip", "--hip-trace"),
    ("marker", "--marker-trace"),
    ("memoryCopy", "--memory-copy-trace"),
    ("memoryAllocation", "--memory-allocation-trace"),
    ("scratchMemory", "--scratch-memory-trace"),
    ("hsa", "--hsa-trace"),
    ("rccl", "--rccl-trace"),
    ("runtime", "--runtime-trace"),
    ("sys", "--sys-trace"),
];
/// ATT (SQTT thread trace) options, with the rocprofv3 flag each sets.
const ATT_OPTIONS: [(&str, &str); 4] = [
    ("targetCu", "--att-target-cu"),
    ("simdSelect", "--att-simd-select"),
    ("bufferSize", "--att-buffer-size"),
    ("shaderEngineMask", "--att-shader-engine-mask"),
];
/// Appends the rocprofv3 collection flags selected by `counters`, `traces` and
/// `att`. The selections combine in one run; without any, a kernel trace is
/// collected, matching the historical default.
fn rocprof_streams(a: &Value, command: &mut Vec<String>) -> Result<(), String> {
    let mut selected = false;
    if let Some(counters) = a.get("counters") {
        let names: Vec<String> =
            serde_json::from_value(counters.clone()).map_err(|e| e.to_string())?;
        if names.is_empty()
            || names.iter().any(|n| {
                n.is_empty() || !n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            })
        {
            return Err("Invalid counter names".into());
        }
        command.push("--pmc".into());
        command.extend(names);
        selected = true;
    }
    if let Some(traces) = a.get("traces") {
        let names: Vec<String> =
            serde_json::from_value(traces.clone()).map_err(|e| e.to_string())?;
        if names.is_empty() {
            return Err("traces cannot be empty".into());
        }
        if let Some(unknown) = names
            .iter()
            .find(|name| !TRACE_STREAMS.iter().any(|(stream, _)| stream == name))
        {
            return Err(format!("Unknown trace stream {unknown}"));
        }
        for (stream, option) in TRACE_STREAMS {
            if names.iter().any(|name| name == stream) {
                command.push(option.into());
            }
        }
        selected = true;
    }
    if let Some(att) = a.get("att") {
        let att = att.as_object().ok_or("att must be an object")?;
        if let Some(unknown) = att
            .keys()
            .find(|key| !ATT_OPTIONS.iter().any(|(name, _)| name == key))
        {
            return Err(format!("Unknown att option {unknown}"));
        }
        command.push("--att".into());
        for (name, option) in ATT_OPTIONS {
            if let Some(value) = att.get(name) {
                let value = value
                    .as_u64()
                    .ok_or_else(|| format!("att {name} must be a nonnegative integer"))?;
                command.push(option.into());
                command.push(value.to_string());
            }
        }
        selected = true;
    }
    if !selected {
        command.push("--kernel-trace".into());
    }
    Ok(())
}
fn spawn_process_env(
    argv: &[String],
    cwd: &Path,
    folder: &Path,
    env: &[(String, String)],
) -> Result<Child, String> {
    spawn_process_env_stdin(argv, cwd, folder, env, false)
}
/// As `spawn_process_env`, but `pipe_stdin` opens a writable stdin pipe instead of
/// `/dev/null`. A triggered rocprofv3 attach window needs this: it has no
/// `--attach-duration-msec`, so it blocks on `input()` and detaches when a newline
/// is written to that pipe (or the pipe closes). A null stdin would hit EOF and
/// detach immediately.
fn spawn_process_env_stdin(
    argv: &[String],
    cwd: &Path,
    folder: &Path,
    env: &[(String, String)],
    pipe_stdin: bool,
) -> Result<Child, String> {
    use std::process::{Command, Stdio};
    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .current_dir(cwd)
        .envs(env.iter().map(|(k, v)| (k, v)))
        .env("TMPDIR", folder)
        .stdin(if pipe_stdin { Stdio::piped() } else { Stdio::null() })
        .stdout(fs::File::create(folder.join("stdout.log")).map_err(|e| e.to_string())?)
        .stderr(fs::File::create(folder.join("stderr.log")).map_err(|e| e.to_string())?);
    for key in [
        "LD_PRELOAD",
        "HSA_TOOLS_LIB",
        "HSA_TOOLS_REPORT_LOAD_FAILURE",
    ] {
        command.env_remove(key);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command.spawn().map_err(|e| e.to_string())
}
fn run_process_env(
    argv: Vec<String>,
    cwd: &Path,
    folder: &Path,
    flag: &AtomicBool,
    seconds: u64,
    env: &[(String, String)],
) -> Result<(), String> {
    Control::check(flag)?;
    let mut child = spawn_process_env(&argv, cwd, folder, env)?;
    let start = Instant::now();
    loop {
        let status = child.try_wait().map_err(|e| e.to_string())?;
        let exceeded = ["stdout.log", "stderr.log"]
            .iter()
            .any(|p| fs::metadata(folder.join(p)).is_ok_and(|m| m.len() > 64 * 1024 * 1024));
        if status.is_some()
            || flag.load(Ordering::SeqCst)
            || start.elapsed().as_secs() > seconds
            || exceeded
        {
            tuning::stop(&mut child);
            Control::check(flag)?;
            if exceeded || status.is_none() {
                return Err("Process exceeded its time or output budget".into());
            }
            if !status.unwrap().success() {
                let log = fs::read_to_string(folder.join("stderr.log")).unwrap_or_default();
                return Err(format!(
                    "Process failed: {}. Artifacts: {}",
                    log.chars().take(4000).collect::<String>(),
                    folder.display()
                ));
            }
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
/// Mesa writes RGP captures to `/tmp/<process>_<timestamp>[_submit<N>].rgp`, a
/// directory it does not let callers change. The files this run wrote are moved
/// into the job folder, ordered by submit.
fn collect_rgp(program: &str, since: std::time::SystemTime, folder: &Path) -> Result<Vec<PathBuf>, String> {
    let name = Path::new(program).file_name().ok_or("Application path has no file name")?.to_string_lossy();
    let prefix = format!("{name}_");
    let since = since - Duration::from_secs(1);
    let mut found = vec![];
    // /tmp is shared: entries can vanish while it is listed, so only this run's
    // candidates are examined and one that disappears is skipped.
    for item in fs::read_dir("/tmp").map_err(|e| e.to_string())?.flatten() {
        let file = item.file_name().to_string_lossy().into_owned();
        if !file.starts_with(&prefix) || !file.ends_with(".rgp") {
            continue;
        }
        let Ok(meta) = item.metadata() else { continue };
        if meta.is_file() && meta.modified().is_ok_and(|m| m >= since) {
            found.push((item.path(), meta.len()));
        }
    }
    let needed: u64 = found.iter().map(|(_, len)| len).sum();
    let available = free_bytes(folder)?;
    if needed > available {
        return Err(format!(
            "{} RGP captures ({needed} bytes) remain in /tmp: {} has only {available} bytes free",
            found.len(),
            folder.display()
        ));
    }
    let target = folder.join("rgp");
    fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    let submit = |p: &Path| -> u64 {
        let stem = p.file_stem().unwrap_or_default().to_string_lossy();
        stem.rsplit_once("_submit").and_then(|(_, n)| n.parse().ok()).unwrap_or(0)
    };
    found.sort_by_key(|(p, _)| submit(p));
    let mut out = Vec::with_capacity(found.len());
    for (path, _) in found {
        let to = target.join(path.file_name().unwrap());
        if fs::rename(&path, &to).is_err() {
            fs::copy(&path, &to).map_err(|e| format!("Cannot move {}: {e}", path.display()))?;
            fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
        out.push(to);
    }
    Ok(out)
}
fn rgp_save_counts(folder: &Path) -> (usize, usize) {
    let log = fs::read_to_string(folder.join("stderr.log")).unwrap_or_default();
    let paths: std::collections::BTreeSet<_> = log.lines()
        .filter_map(|line| line.split("RGP capture saved to '").nth(1)?.split_once('\'').map(|(path, _)| path))
        .collect();
    let messages = log.lines().filter(|line| line.contains("RGP capture saved to '")).count();
    (messages, paths.len())
}
fn radv_artifacts(program: &str, started: SystemTime, folder: &Path, status: &str) -> Result<Value, String> {
    let paths = collect_rgp(program, started, folder)?;
    let (saved, unique) = rgp_save_counts(folder);
    let mut warnings = Vec::new();
    if saved > unique {
        warnings.push(format!("RADV reported {saved} saves to only {unique} unique filenames; repeated filenames overwrote earlier captures."));
    }
    if unique > paths.len() {
        warnings.push(format!("RADV reported {unique} unique capture paths but only {} files were retained. Check stderr.log for missing or failed captures.", paths.len()));
    }
    Ok(json!({"status":status,"directory":folder,"files":paths,"stdout":folder.join("stdout.log"),"stderr":folder.join("stderr.log"),"savedMessages":saved,"uniqueSavePaths":unique,"warnings":warnings,"next":"Use capture_open with a returned .rgp path"}))
}
fn free_bytes(path: &Path) -> Result<u64, String> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut stat) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(stat.f_bavail as u64 * stat.f_frsize as u64)
}
fn find_outputs(root: &Path, output: &mut Vec<PathBuf>) -> Result<(), String> {
    for item in fs::read_dir(root).map_err(|e| e.to_string())? {
        let item = item.map_err(|e| e.to_string())?;
        if item.file_type().map_err(|e| e.to_string())?.is_dir() {
            // A decoded SQTT directory imports as one capture; list it instead
            // of its hundreds of per-wave files.
            if item.path().join("filenames.json").is_file() {
                output.push(item.path());
            } else {
                find_outputs(&item.path(), output)?
            }
        } else if item
            .path()
            .extension()
            .is_some_and(|e| e == "csv" || e == "json" || e == "att" || e == "out")
        {
            output.push(item.path());
        }
    }
    Ok(())
}

pub fn guard<F: Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static>(
    handler: F,
) -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
    move |invoke| {
        if block_local_invoke(&invoke) {
            invoke
                .resolver
                .reject("Currently driven by MCP; cancel MCP execution first");
            true
        } else {
            handler(invoke)
        }
    }
}

fn validate(value: &Value, schema: &Value) -> Result<(), String> {
    if let Some(choices) = schema["enum"].as_array() {
        if !choices.contains(value) {
            return Err("Argument is outside its supported values".into());
        }
    }
    let valid = match schema["type"].as_str() {
        Some("string") => value.is_string(),
        Some("boolean") => value.is_boolean(),
        Some("number") => value.is_number(),
        Some("integer") => value.is_u64(),
        Some("object") => value.is_object(),
        Some("array") => value.is_array(),
        _ => true,
    };
    if !valid {
        return Err(format!(
            "Argument type mismatch: expected {}",
            schema["type"]
        ));
    }
    if let Some(o) = value.as_object() {
        if let Some(required) = schema["required"].as_array() {
            for k in required {
                if !o.contains_key(k.as_str().unwrap()) {
                    return Err(format!("Missing argument {k}"));
                }
            }
        }
        for (k, v) in o {
            if let Some(s) = schema["properties"].get(k) {
                validate(v, s)?
            } else if schema["additionalProperties"] == false {
                return Err(format!("Unknown argument {k}"));
            }
        }
    }
    if let Some(items) = value.as_array() {
        if schema["minItems"]
            .as_u64()
            .is_some_and(|n| items.len() < (n as usize))
            || schema["maxItems"]
                .as_u64()
                .is_some_and(|n| items.len() > (n as usize))
        {
            return Err("Argument array length is out of bounds".into());
        }
        for item in items {
            validate(item, &schema["items"])?
        }
    }
    Ok(())
}

static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn stop_signal(_: i32) {
    STOP.store(true, Ordering::SeqCst);
}
pub fn shutdown(control: &Control, tuning: &tuning::Tuning) {
    control.shutting_down.store(true, Ordering::SeqCst);
    control.cancel(None, None);
    if let Some(mut profile) = control.live_profile.lock().unwrap().take() {
        let pid = profile.child.id();
        if let Gate::Rocprofv3 { window: Some(open), .. } = &mut profile.gate {
            tuning::stop(&mut open.child);
        }
        tuning::stop(&mut profile.child);
        match &profile.gate {
            Gate::Radv { trigger } => {
                // Match profile_stop_core: leave no per-job trigger/pickle debris.
                let _ = fs::remove_file(trigger);
                let _ = radv_artifacts(&profile.program, profile.started, &profile.folder, "stopped");
            }
            Gate::Rocprofv3 { .. } => {
                let _ = fs::remove_file(format!("/tmp/rocprofv3_attach_{pid}.pkl"));
            }
        }
    }
    tuning::stop_all(tuning);
    let start = Instant::now();
    while control.active.lock().unwrap().is_some() && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn schemas_and_argument_validation() {
        let catalog = tools();
        let mut names = std::collections::BTreeSet::new();
        for tool in &catalog {
            assert!(names.insert(tool["name"].as_str().unwrap()));
            assert!(tool["inputSchema"]["properties"].is_object());
        }
        let query = &catalog
            .iter()
            .find(|t| t["name"] == "capture_query")
            .unwrap()["inputSchema"];
        assert!(validate(
            &json!({"captureId":"x","dataset":"waves","limit":10}),
            query
        )
        .is_ok());
        for bad in [
            json!({"captureId":"x"}),
            json!({"captureId":"x","dataset":"waves","limit":-1}),
            json!({"captureId":"x","dataset":"waves","unexpected":true}),
        ] {
            assert!(validate(&bad, query).is_err());
        }
        let run = &catalog
            .iter()
            .find(|t| t["name"] == "tuning_action")
            .unwrap()["inputSchema"];
        assert!(validate(&json!({"id":"x","request":{"op":"shell"}}), run).is_err());
    }
    #[test]
    fn rocprofv3_option_vocabulary_matches_the_schema() {
        use std::collections::BTreeSet;
        let catalog = tools();
        let schema = |name: &str| {
            catalog.iter().find(|t| t["name"] == name).unwrap()["inputSchema"]["properties"].clone()
        };
        let set = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<BTreeSet<_>>();
        // The hand-written traces enum and att property set in the tool schemas must
        // equal the TRACE_STREAMS / ATT_OPTIONS consts that build the rocprofv3
        // command line, for both tools — a one-sided edit would otherwise accept or
        // reject streams the other side disagrees with.
        let trace_names = set(&TRACE_STREAMS.map(|(s, _)| s));
        let att_names = set(&ATT_OPTIONS.map(|(n, _)| n));
        for tool in ["profile_run", "profile_trigger"] {
            let props = schema(tool);
            let traces_enum: BTreeSet<String> = props["traces"]["items"]["enum"]
                .as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
            assert_eq!(traces_enum, trace_names, "{tool} traces enum vs TRACE_STREAMS");
            let att_props: BTreeSet<String> = props["att"]["properties"]
                .as_object().unwrap().keys().cloned().collect();
            assert_eq!(att_props, att_names, "{tool} att properties vs ATT_OPTIONS");
        }
        // Every schema key is EITHER a universal key OR a rocprofv3-only key in the
        // single ROCPROFV3_ONLY_KEYS table — so adding a rocprofv3-only option to a
        // schema without adding it to the table (which drives the RADV denylists)
        // fails here, closing the fail-open gap.
        let run_keys = ROCPROFV3_ONLY_KEYS.iter().filter(|k| k.run).map(|k| k.key).collect::<BTreeSet<_>>();
        let window_keys = ROCPROFV3_ONLY_KEYS.iter().filter(|k| k.window).map(|k| k.key).collect::<BTreeSet<_>>();
        let run_universal = set(&["argv", "cwd", "tracer", "env", "timeoutSeconds"]);
        for key in schema("profile_run").as_object().unwrap().keys() {
            assert_eq!(run_universal.contains(key), !run_keys.contains(key.as_str()),
                "profile_run key {key}: must be either a universal key or in ROCPROFV3_ONLY_KEYS(run)");
        }
        // `end` is a command (its own check), `id` is universal; all else is a window key.
        let trigger_universal = set(&["id", "end"]);
        for key in schema("profile_trigger").as_object().unwrap().keys() {
            assert_eq!(trigger_universal.contains(key), !window_keys.contains(key.as_str()),
                "profile_trigger key {key}: must be id/end or in ROCPROFV3_ONLY_KEYS(window)");
        }
    }
    #[test]
    fn rocprof_streams_combine_and_validate() {
        let build = |a: Value| {
            let mut command = Vec::new();
            rocprof_streams(&a, &mut command).map(|_| command)
        };
        assert_eq!(build(json!({})).unwrap(), ["--kernel-trace"]);
        assert_eq!(build(json!({"counters":["SQ_WAVES"]})).unwrap(), ["--pmc", "SQ_WAVES"]);
        assert_eq!(build(json!({"att":{}})).unwrap(), ["--att"]);
        assert_eq!(
            build(json!({"counters":["SQ_WAVES","GRBM_COUNT"],"traces":["marker","kernel","hip"],"att":{"targetCu":1,"simdSelect":15,"bufferSize":100000000_u64,"shaderEngineMask":1}})).unwrap(),
            ["--pmc","SQ_WAVES","GRBM_COUNT","--kernel-trace","--hip-trace","--marker-trace","--att","--att-target-cu","1","--att-simd-select","15","--att-buffer-size","100000000","--att-shader-engine-mask","1"]
        );
        for bad in [
            json!({"counters":[]}),
            json!({"counters":["bad name"]}),
            json!({"traces":[]}),
            json!({"traces":["frame"]}),
            json!({"att":{"targetCu":-1}}),
            json!({"att":{"waveSize":4}}),
            json!({"att":[]}),
        ] {
            assert!(build(bad.clone()).is_err(), "{bad}");
        }
        let catalog = tools();
        for tool in ["profile_run", "profile_trigger"] {
            let schema = &catalog.iter().find(|t| t["name"] == tool).unwrap()["inputSchema"];
            let base = if tool == "profile_run" { json!({"argv":["app"],"cwd":"/"}) } else { json!({"id":"x"}) };
            let with = |extra: Value| {
                let mut v = base.clone();
                for (k, value) in extra.as_object().unwrap() {
                    v[k] = value.clone();
                }
                v
            };
            assert!(validate(&with(json!({"counters":["SQ_WAVES"],"traces":["kernel","hip"],"att":{"targetCu":1}})), schema).is_ok(), "{tool}");
            assert!(validate(&with(json!({"traces":["frame"]})), schema).is_err(), "{tool}");
            assert!(validate(&with(json!({"att":{"waveSize":4}})), schema).is_err(), "{tool}");
        }
        let start = &catalog.iter().find(|t| t["name"] == "profile_start").unwrap()["inputSchema"];
        assert!(validate(&json!({"argv":["app"],"cwd":"/","tracer":"rocprofv3","profiler":"/opt/rocm/bin/rocprofv3"}), start).is_ok());
        assert!(validate(&json!({"argv":["app"],"cwd":"/","tracer":"rgp"}), start).is_err());
    }
    #[test]
    fn kernel_filters_pass_through_and_validate() {
        let build = |a: Value| {
            let mut command = Vec::new();
            kernel_filters(&a, &mut command).map(|_| command)
        };
        assert!(build(json!({})).unwrap().is_empty());
        // Iteration range passes through verbatim after light validation; the
        // SDK reattach set treats it like the name regexes, so it joins the
        // same collection_args the same-collection guard compares.
        assert_eq!(
            build(json!({"kernelIterationRange":"64"})).unwrap(),
            ["--kernel-iteration-range", "64"]
        );
        assert_eq!(
            build(json!({"kernelIterationRange":"0-3"})).unwrap(),
            ["--kernel-iteration-range", "0-3"]
        );
        assert_eq!(
            build(json!({"kernelIncludeRegex":"mul_mat","kernelIterationRange":"2,5-7"})).unwrap(),
            ["--kernel-include-regex", "mul_mat", "--kernel-iteration-range", "2,5-7"]
        );
        for bad in [
            json!({"kernelIterationRange":""}),
            json!({"kernelIterationRange":"64 "}),
            json!({"kernelIterationRange":"first"}),
            json!({"kernelIterationRange":"0..3"}),
            json!({"kernelIterationRange":64}),
            json!({"kernelIterationRange":["64"]}),
        ] {
            assert!(build(bad.clone()).is_err(), "{bad}");
        }
    }
    #[test]
    fn attach_requires_a_display_and_only_expected_environment() {
        assert!(validate_attach_environment(&json!({"environment":{"DISPLAY":":99","GDK_BACKEND":"x11"}})).is_ok());
        for bad in [
            json!({"environment":{}}),
            json!({"environment":{"DISPLAY":""}}),
            json!({"environment":{"DISPLAY":":99","LD_PRELOAD":"/tmp/x.so"}}),
            json!({"environment":{"DISPLAY":":99\u{0000}"}}),
            json!({"environment":{"DISPLAY":42}}),
        ] {
            assert!(validate_attach_environment(&bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn cancellation_is_owned_and_request_scoped() {
        let c = Control::default();
        let flag = Arc::new(AtomicBool::new(false));
        *c.active.lock().unwrap() = Some(Active {
            id: json!(7),
            owner: "one".into(),
            name: "run".into(),
            key: "run\n{}".into(),
            cancel: flag.clone(),
            started: Instant::now(),
            outcome: Arc::new(Outcome::default()),
        });
        c.cancel(Some("two"), Some(&json!(7)));
        assert!(!flag.load(Ordering::SeqCst));
        c.cancel(Some("one"), Some(&json!(8)));
        assert!(!flag.load(Ordering::SeqCst));
        c.cancel(Some("one"), Some(&json!(7)));
        assert!(flag.load(Ordering::SeqCst));
    }
    #[test]
    fn status_poll_never_blocks_on_a_held_live_profile_lock() {
        // The GUI polls mcp_status on the main thread every 150 ms; the
        // live_profile mutex can be held across multi-second child waits. If
        // status() took that lock with `.lock()` instead of `try_lock`, this test
        // would deadlock (std Mutex is not reentrant) — so it pins the invariant
        // that a status poll is nonblocking regardless of the lock being held.
        let c = Control::default();
        let held = c.live_profile.lock().unwrap();
        assert_eq!(c.status()["liveProfile"], json!("unavailable"));
        drop(held);
        assert!(c.status()["liveProfile"].is_null());
    }
    #[test]
    fn desktop_error_maps_mcp_tool_vocabulary() {
        // The panel's buttons are "Start profiling"/"Stop and collect"; it must
        // never surface the MCP tool names profile_start/profile_stop.
        for mcp in [
            "A live profile is already running; use profile_stop first",
            "No live profile; use profile_start first",
            "No live profile; use profile_stop to collect a finished run",
            "No live profile",
            "Profiled process has exited; use profile_stop to collect artifacts",
        ] {
            let mapped = desktop_error(mcp.into());
            assert!(!mapped.contains("profile_start") && !mapped.contains("profile_stop"), "{mcp} -> {mapped}");
        }
        // Errors with no MCP vocabulary pass through unchanged.
        assert_eq!(desktop_error("windowMs must be between 1 and 600000".into()), "windowMs must be between 1 and 600000");
    }
    #[test]
    fn mcp_cancel_reaches_a_desktop_live_profile_op() {
        // The desktop live_profile_* commands share Control::live_cancel (they run
        // outside `active`), so cancel()/shutdown can abort an in-flight detach.
        let c = Control::default();
        assert!(!c.live_cancel.load(Ordering::SeqCst));
        c.cancel(None, None);
        assert!(c.live_cancel.load(Ordering::SeqCst));
    }
    #[test]
    fn undelivered_results_go_once_to_an_identical_retry() {
        let c = Control::default();
        let response = json!({"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"done"}],"isError":false}});
        c.remember("dropped", &json!(1), "tuning_action\n{}".into(), response.clone());
        assert!(c.reclaim("tuning_action\n{\"id\":\"x\"}", &json!(5)).is_none());
        let retry = c.reclaim("tuning_action\n{}", &json!(5)).unwrap();
        assert_eq!(retry["id"], 5);
        assert_eq!(retry["result"], response["result"]);
        assert!(c.reclaim("tuning_action\n{}", &json!(6)).is_none());
        // A response that reached its client is not kept.
        c.remember("connected", &json!(2), "profile_run\n{}".into(), response);
        c.delivered("connected", &json!(2));
        assert!(c.reclaim("profile_run\n{}", &json!(3)).is_none());
    }
    #[test]
    fn a_joining_call_receives_the_running_calls_response() {
        let outcome = Arc::new(Outcome::default());
        let joiner = {
            let outcome = outcome.clone();
            std::thread::spawn(move || outcome.wait())
        };
        std::thread::sleep(Duration::from_millis(20));
        outcome.set(json!({"jsonrpc":"2.0","id":1,"result":42}));
        assert_eq!(joiner.join().unwrap()["result"], 42);
    }
    #[test]
    fn pagination_filter_and_cancel() {
        let flag = AtomicBool::new(false);
        let v = page(
            json!(["a", "b", "ab", "c"]),
            &json!({"query":"a","offset":1,"limit":1}),
            &flag,
        )
        .unwrap();
        assert_eq!(v["rows"], json!(["ab"]));
        assert_eq!(v["total"], 2);
        assert!(v["nextOffset"].is_null());
        flag.store(true, Ordering::SeqCst);
        assert!(page(json!([1]), &json!({}), &flag).is_err());
    }
    #[test]
    fn profile_env_accepts_plain_variables_and_refuses_reserved_ones() {
        let ok = profile_env(&json!({"env":{"VK_DRIVER_FILES":"/x.json","RADV_THREAD_TRACE_ROCPROF_COMPAT":"true"}})).unwrap();
        assert_eq!(ok.len(), 2);
        assert!(profile_env(&json!({})).unwrap().is_empty());
        for bad in [
            json!({"env":{"LD_PRELOAD":"/evil.so"}}),
            json!({"env":{"TMPDIR":"/tmp"}}),
            json!({"env":{"HSA_TOOLS_LIB":"x"}}),
            json!({"env":{"lower":"x"}}),
            json!({"env":{"1ABC":"x"}}),
            json!({"env":{"A":1}}),
            json!({"env":"A=1"}),
        ] {
            assert!(profile_env(&bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn rgp_captures_are_collected_by_process_name_and_submit_order() {
        // Removes what the test put in /tmp even when an assertion fails.
        struct Cleanup(Vec<PathBuf>);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                for p in &self.0 {
                    let _ = if p.is_dir() { fs::remove_dir_all(p) } else { fs::remove_file(p) };
                }
            }
        }
        let job = tempfile_dir();
        let tag = format!("rpgtest{}", std::process::id());
        let mut cleanup = Cleanup(vec![job.clone(), PathBuf::from(format!("/tmp/other{tag}_submit0.rgp"))]);
        let started = std::time::SystemTime::now();
        for n in [10, 2, 1] {
            let path = PathBuf::from(format!("/tmp/{tag}_2026.01.01_00.00.00_submit{n}.rgp"));
            fs::write(&path, b"x").unwrap();
            cleanup.0.push(path);
        }
        fs::write(format!("/tmp/other{tag}_submit0.rgp"), b"x").unwrap();
        let files = collect_rgp(&format!("/usr/bin/{tag}"), started, &job).unwrap();
        let names: Vec<String> = files.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names.len(), 3);
        assert!(names[0].ends_with("submit1.rgp") && names[2].ends_with("submit10.rgp"), "{names:?}");
        assert!(files.iter().all(|p| p.starts_with(job.join("rgp"))));
        assert!(!Path::new(&format!("/tmp/{tag}_2026.01.01_00.00.00_submit1.rgp")).exists());
    }
    fn tempfile_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rpg-mcp-test-{}-{}", std::process::id(), random().unwrap()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
    #[test]
    fn rgp_save_report_detects_reused_filenames() {
        let job = tempfile_dir();
        fs::write(job.join("stderr.log"), "RGP capture saved to '/tmp/a.rgp'\nRGP capture saved to '/tmp/a.rgp'\nRGP capture saved to '/tmp/b.rgp'\n").unwrap();
        assert_eq!(rgp_save_counts(&job), (3, 2));
        fs::remove_dir_all(job).unwrap();
    }
    #[test]
    fn following_another_capture_clears_stale_selections() {
        let c = Control::default();
        c.follow(json!({"captureId":"1","range":[0,2],"dispatchId":"a"}));
        c.follow(json!({"captureId":"2","view":"summary"}));
        let w = c.workspace.lock().unwrap();
        assert!(w.get("range").is_none());
        assert!(w.get("dispatchId").is_none());
        assert_eq!(w["view"], "summary");
    }
    #[test]
    fn compile_reports_dataset_surfaces_labelled_static_facts() {
        // A synthetic version-zero report (no GPU, no compiler), shaped like the
        // real ones, driven through the same dataset builder capture_query calls.
        let report_json = json!({
            "kind": "loom.compile_report",
            "schema_version": 0,
            "mode": "details",
            "status": {"code": 0, "name": "OK"},
            "function": "k",
            "backend": "amdgpu-hal",
            "target_family": "amdgpu",
            "target_key": "gfx1151",
            "detail_flags": 1u64 << 13, // TARGET_RESOURCES
            "static_instruction_mix": {"vector_alu_count": 7, "branch_count": 1},
            "wait_plan": {"action_count": 5},
            "workload": {"dispatch_workitem_count": 64},
            "target_resources": {
                "scalar": {"register_class": "amdgpu.sgpr", "final": {"register_count": 98, "overhead_units": 2}, "scheduled_pressure": {"peak_live_units": 80}},
                "vector": {"register_class": "amdgpu.vgpr", "final": {"register_count": 63, "overhead_units": 4}},
                "subgroup_size": 32,
                "occupancy_percent": 100,
                "limiting_resource": "max_waves",
                "residency": {"best_tier": 16, "current_tier": 16}
            },
            "entries": {"count": 1, "rows": [{
                "index": 0, "function": "k", "source_function": "k",
                "allocation_spill_count": 0, "code_byte_count": 1794, "instruction_count": 1794,
                "static_instruction_mix": {"vector_alu_count": 7},
                "target_resources": {
                    "scalar": {"register_class": "amdgpu.sgpr", "final": {"register_count": 98}},
                    "vector": {"register_class": "amdgpu.vgpr", "final": {"register_count": 63}},
                    "occupancy_percent": 100,
                    "limiting_resource": "max_waves"
                }
            }]}
        });
        let report = profile_core::loom_report::parse(&report_json, "<test>").unwrap();
        let mut capture = Capture::empty(
            "k.compile-report.json".into(),
            "Loom compile report (version-zero static facts)",
            "static",
        );
        capture.compile_reports.push(report);

        let rows = compile_report_rows(&capture);
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        // Epistemic labelling travels with every row.
        assert_eq!(r["unit"], json!("static"));
        assert_eq!(r["stability"], json!(profile_core::loom_report::STABILITY));
        assert!(r["facts"].as_str().unwrap().contains("not measurements"));
        assert_eq!(r["schemaVersion"], json!(0));
        assert_eq!(r["status"]["code"], json!(0));
        assert_eq!(r["identity"]["target_key"], json!("gfx1151"));
        assert!(r["detailFlagNames"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n == "TARGET_RESOURCES"));
        // Report-level target resources are broken out as called for.
        assert_eq!(r["targetResources"]["sgpr"]["finalRegisterCount"], json!(98));
        assert_eq!(r["targetResources"]["sgpr"]["scheduledPeakLiveUnits"], json!(80));
        assert_eq!(r["targetResources"]["vgpr"]["finalRegisterCount"], json!(63));
        assert_eq!(r["targetResources"]["subgroupSize"], json!(32));
        assert_eq!(r["targetResources"]["occupancyPercent"], json!(100));
        assert_eq!(r["targetResources"]["limitingResource"], json!("max_waves"));
        assert_eq!(r["targetResources"]["residency"]["best_tier"], json!(16));
        assert_eq!(r["instructionMix"]["static"]["vector_alu_count"], json!(7));
        assert_eq!(r["waitPlan"]["action_count"], json!(5));
        assert_eq!(r["workload"]["dispatch_workitem_count"], json!(64));
        // Per-entry facts: identity, allocation/spill and emitted-instruction
        // counters, and broken-out target resources.
        let e = &r["entries"][0];
        assert_eq!(e["displayName"], json!("k"));
        assert_eq!(e["counters"]["code_byte_count"], json!(1794));
        assert_eq!(e["counters"]["instruction_count"], json!(1794));
        assert_eq!(e["counters"]["allocation_spill_count"], json!(0));
        assert_eq!(e["targetResources"]["vgpr"]["finalRegisterCount"], json!(63));
        assert_eq!(e["targetResources"]["occupancyPercent"], json!(100));
    }
}
