use crate::{tuning, Store};
use profile_core::Capture;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
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
                    println!("rocprof-studio [--headless] [--mcp-port 8765] [--mcp-token-file PATH] [--no-mcp]\nThe token file defaults to ~/.rocprofgui/mcp/token; set ROCPROFGUI_HOME to relocate the whole data directory.\nMCP: http://127.0.0.1:PORT/mcp (Streamable HTTP/SSE), /sse (legacy SSE). Authorization: Bearer <token file contents>.");
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
struct Active {
    id: Value,
    owner: String,
    name: String,
    cancel: Arc<AtomicBool>,
    started: Instant,
}
#[derive(Default)]
pub struct Control {
    shutting_down: AtomicBool,
    active: Mutex<Option<Active>>,
    workspace: Mutex<Value>,
    revision: AtomicU64,
    endpoint: Mutex<Value>,
}
impl Control {
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
    }
    fn status(&self) -> Value {
        let a = self.active.lock().unwrap();
        json!({"active":a.as_ref().map(|a|json!({"requestId":a.id,"tool":a.name,"cancelling":a.cancel.load(Ordering::SeqCst),"elapsedSeconds":a.started.elapsed().as_secs()})),"revision":self.revision.load(Ordering::SeqCst),"workspace":*self.workspace.lock().unwrap(),"server":*self.endpoint.lock().unwrap()})
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
    app: Option<tauri::AppHandle>,
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
) -> Result<(), String> {
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
    while !STOP.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(100));
    }
    shutdown(&control, &tuning);
    Ok(())
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
    let server = Arc::new(Server {
        store,
        tuning,
        control,
        app,
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
                let _ = tx.try_send(format!("event: message\ndata: {result}\n\n"));
            }
        } else {
            let mut writer = r.into_writer();
            let _=write!(writer,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nMcp-Session-Id: {id}\r\nConnection: close\r\n\r\n").and_then(|_|writer.flush());
            let (tx, rx) = mpsc::sync_channel(1);
            let server = self.clone();
            std::thread::spawn(move || {
                let _ = tx.send(server.rpc(&id, &v));
            });
            loop {
                match rx.recv_timeout(Duration::from_secs(2)) {
                    Ok(result) => {
                        let _ = write!(writer, "event: message\ndata: {result}\n\n")
                            .and_then(|_| writer.flush());
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
                json!({"protocolVersion":version,"capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"rocprof-studio","version":env!("CARGO_PKG_VERSION")},"instructions":"Local profiling and kernel tuning. Tool calls are serialized. Cancel with notifications/cancelled or the desktop Cancel MCP execution button. Read tool schemas before mutation; artifacts default to ~/.rocprofgui (override with ROCPROFGUI_HOME)."})
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
                {
                    let mut active = self.control.active.lock().unwrap();
                    if active.is_some() {
                        return error(id, -32001, "Another MCP action is running");
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
                        cancel: flag.clone(),
                        started: Instant::now(),
                    });
                }
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    self.tool(name, &args, flag.clone())
                }))
                .unwrap_or_else(|_| Err("Internal MCP execution failure".into()));
                let cancelled = {
                    let mut active = self.control.active.lock().unwrap();
                    let cancelled = flag.load(Ordering::SeqCst);
                    active.take();
                    cancelled
                };
                if cancelled {
                    return error(id,-32800,"MCP execution cancelled; worker stopped. Completed artifacts are retained.");
                }
                match outcome {
                    Ok(data) => {
                        json!({"content":[{"type":"text","text":data.to_string()}],"isError":false})
                    }
                    Err(e) => json!({"content":[{"type":"text","text":e}],"isError":true}),
                }
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
            "profile_run" => {
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
                if a["tracer"] == "radv" {
                    if a.get("counters").is_some() || a.get("profiler").is_some() {
                        return Err("counters and profiler apply to rocprofv3 only".into());
                    }
                    // Recorded so a shader-tuning session can rerun the same application.
                    fs::write(
                        folder.join("profile.json"),
                        serde_json::to_vec_pretty(&json!({"tracer":"radv","argv":argv,"cwd":cwd,"env":a.get("env").cloned().unwrap_or(json!({}))}))
                            .map_err(|e| e.to_string())?,
                    )
                    .map_err(|e| e.to_string())?;
                    let started = std::time::SystemTime::now();
                    let mut vars = vec![
                        ("MESA_VK_TRACE".to_owned(), "rgp".to_owned()),
                        ("MESA_VK_TRACE_PER_SUBMIT".to_owned(), "1".to_owned()),
                    ];
                    vars.extend(env);
                    let run = run_process_env(argv.clone(), cwd, &folder, &flag, seconds, &vars);
                    let paths = collect_rgp(&argv[0], started, &folder)?;
                    run?;
                    return Ok(json!({"directory":folder,"files":paths,"next":"Use capture_open with a returned .rgp path; each file holds one queue submit"}));
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
                if let Some(counters) = a.get("counters") {
                    let names: Vec<String> =
                        serde_json::from_value(counters.clone()).map_err(|e| e.to_string())?;
                    if names.is_empty()
                        || names.iter().any(|n| {
                            n.is_empty()
                                || !n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                        })
                    {
                        return Err("Invalid counter names".into());
                    }
                    command.push("--pmc".into());
                    command.extend(names)
                } else {
                    command.push("--kernel-trace".into());
                }
                command.push("--".into());
                command.extend(argv);
                run_process_env(command, cwd, &folder, &flag, seconds, &env)?;
                let mut paths = Vec::new();
                find_outputs(&folder, &mut paths)?;
                Ok(
                    json!({"directory":folder,"files":paths,"next":"Use capture_open with a returned trace or counter CSV path"}),
                )
            }
            "tuning_open" => {
                let id = tuning::create(
                    &self.store,
                    &self.tuning,
                    text(a, "captureId")?.into(),
                    text(a, "dispatchId")?.into(),
                )?;
                self.control.follow(json!({"captureId":a["captureId"],"view":"dispatch","dispatchId":a["dispatchId"],"tuningId":id}));
                if let Some(app) = &self.app {
                    tuning::show_window(app, &id)?;
                }
                tuning::status(&self.tuning, &id)
            }
            "tuning_status" => tuning::status(&self.tuning, text(a, "id")?),
            "tuning_close" => {
                let id = text(a, "id")?;
                tuning::close(&self.tuning, id)?;
                if let Some(app) = &self.app {
                    if let Some(w) = app.get_webview_window(&format!("tuning-{id}")) {
                        let _ = w.close();
                    }
                }
                Ok(json!({"closed":id}))
            }
            "tuning_action" => {
                let id = text(a, "id")?;
                let request = a.get("request").ok_or("request required")?.clone();
                if let Some(app) = &self.app {
                    tuning::show_window(app, id)?;
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
    json!({"id":c.id,"name":c.name,"path":c.input_path,"format":c.format,"architecture":c.architecture,"unit":c.unit,"start":c.start,"end":c.end,"warnings":c.warnings,"dispatches":c.dispatches.len(),"instructions":c.instructions.len(),"waves":c.waves.len(),"counters":c.counters.len()})
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
    let entries=vec![
 ("workspace","List open captures, tuning sessions, current view and MCP status.",json!({}),vec![]),
 ("capture_open","Import a trace, counters, ATT run, sampling data or portable capture from an explicit path.",json!({"path":str_,"samplingMethod":str_,"loadWaveStates":{"type":"boolean"}}),vec!["path"]),
 ("capture_analyze","Run the same analysis functions as the GUI: kernels, occupancy, activity, hardware, hotspots, source, markers, scalar, tensor. Scalar takes name/formula (C0 etc). Tensor takes definitions (NAME := expression) and raw named {shape:[XCC,SE,CU,Time],values:[...]}. Timeline renders dispatch or global residency HTML with the same paged exporter as the GUI. Diagnostics produces its diagnostic manifest. Optional output writes a new HTML, diagnostics JSON, or analysis JSON file. Returns paged rows otherwise. Requires Node.js.",json!({"captureId":str_,"analysis":{"enum":["kernels","occupancy","activity","hardware","hotspots","source","markers","scalar","tensor","timeline","diagnostics"]},"metric":{"enum":["latency","pcSamples","pcIssued","pcStalled"]},"se":num,"kernel":str_,"range":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2},"name":str_,"formula":str_,"definitions":str_,"raw":obj,"output":str_,"timeline":{"enum":["dispatch","global"]},"timePages":num,"lanesPerPage":num,"workspace":obj,"query":str_,"offset":num,"limit":num}),vec!["captureId","analysis"]),
 ("profile_run","Run an application argv under a profiler and return output files to import with capture_open. tracer rocprofv3 (default) collects a kernel trace, or the given counters. tracer radv sets MESA_VK_TRACE=rgp with per-submit capture and returns the .rgp files the application's RADV Vulkan driver wrote, one per queue submit (experimental; see docs/radv-profiling.md). env adds environment variables such as VK_DRIVER_FILES or RADV_THREAD_TRACE_ROCPROF_COMPAT. Cancellable; timeoutSeconds defaults to 600, at most 3600. Does not execute a shell.",json!({"argv":{"type":"array","items":str_,"minItems":1},"cwd":str_,"profiler":str_,"counters":{"type":"array","items":str_},"tracer":{"enum":["rocprofv3","radv"]},"env":obj,"timeoutSeconds":{"type":"integer","minimum":1,"maximum":3600}}),vec!["argv","cwd"]),
 ("capture_close","Close an imported capture.",json!({"captureId":str_}),vec!["captureId"]),
 ("capture_query","Page/search capture datasets. Dataset: summary, dispatches, instructions, waves, counters, markers, hostCalls, codeObjects, intervals, events, eventCounters, monitorIntervals, otherSimd, metadata, sources. Limit <=1000. Counter rows include recorded samples.",json!({"captureId":str_,"dataset":str_,"query":str_,"offset":num,"limit":num}),vec!["captureId","dataset"]),
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
 ("tuning_action","Execute and await a tuning operation. request.op: search (config.sourceTree), select (index), capture (config.cwd/argv), calibrate, run (variant, code, iterations, validate, autoCounters, autoCalibrate, optional work/counters), save (code). Config merges with session config. Capture preserves launch arguments and inputs; save replaces the selected source definition with original-file conflict checks. Long calls can be cancelled. Vulkan sessions: capture (config.cwd/argv/env, occurrence, optional name/groups) first, then search (matches the captured SPIR-V against the ggml shader build under config.sourceTree), select (index), run (variant, edits {path: full text}, iterations, validate, trace), save (edits); no calibrate.",json!({"id":str_,"request":{"type":"object","properties":{"op":{"enum":["search","select","capture","calibrate","run","save"]},"config":obj,"index":num,"variant":{"type":"boolean"},"code":str_,"iterations":num,"validate":{"type":"boolean"},"autoCounters":{"type":"boolean"},"autoCalibrate":{"type":"boolean"},"work":obj,"counters":{"type":"array","items":obj}},"required":["op"]}}),vec!["id","request"]),
 ("tuning_close","Stop and close a tuning session; completed artifacts remain on disk.",json!({"id":str_}),vec!["id"]),
 ];
    entries.into_iter().map(|(name,description,properties,required)|json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})).collect()
}

pub fn block_local_invoke(invoke: &tauri::ipc::Invoke<tauri::Wry>) -> bool {
    let command = invoke.message.command();
    let allowed = command.starts_with("mcp_")
        || command.starts_with("read_")
        || [
            "tuning_status",
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
fn run_process_env(
    argv: Vec<String>,
    cwd: &Path,
    folder: &Path,
    flag: &AtomicBool,
    seconds: u64,
    env: &[(String, String)],
) -> Result<(), String> {
    use std::process::{Command, Stdio};
    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .current_dir(cwd)
        .envs(env.iter().map(|(k, v)| (k, v)))
        .env("TMPDIR", folder)
        .stdin(Stdio::null())
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
    Control::check(flag)?;
    let mut child = command.spawn().map_err(|e| e.to_string())?;
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
            find_outputs(&item.path(), output)?
        } else if item
            .path()
            .extension()
            .is_some_and(|e| e == "csv" || e == "json")
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
    fn cancellation_is_owned_and_request_scoped() {
        let c = Control::default();
        let flag = Arc::new(AtomicBool::new(false));
        *c.active.lock().unwrap() = Some(Active {
            id: json!(7),
            owner: "one".into(),
            name: "run".into(),
            cancel: flag.clone(),
            started: Instant::now(),
        });
        c.cancel(Some("two"), Some(&json!(7)));
        assert!(!flag.load(Ordering::SeqCst));
        c.cancel(Some("one"), Some(&json!(8)));
        assert!(!flag.load(Ordering::SeqCst));
        c.cancel(Some("one"), Some(&json!(7)));
        assert!(flag.load(Ordering::SeqCst));
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
        let job = tempfile_dir();
        let tag = format!("rpgtest{}", std::process::id());
        let started = std::time::SystemTime::now();
        for n in [10, 2, 1] {
            fs::write(format!("/tmp/{tag}_2026.01.01_00.00.00_submit{n}.rgp"), b"x").unwrap();
        }
        fs::write(format!("/tmp/other{tag}_submit0.rgp"), b"x").unwrap();
        let files = collect_rgp(&format!("/usr/bin/{tag}"), started, &job).unwrap();
        let names: Vec<String> = files.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names.len(), 3);
        assert!(names[0].ends_with("submit1.rgp") && names[2].ends_with("submit10.rgp"), "{names:?}");
        assert!(files.iter().all(|p| p.starts_with(job.join("rgp"))));
        assert!(!Path::new(&format!("/tmp/{tag}_2026.01.01_00.00.00_submit1.rgp")).exists());
        let _ = fs::remove_file(format!("/tmp/other{tag}_submit0.rgp"));
        let _ = fs::remove_dir_all(&job);
    }
    fn tempfile_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rpg-mcp-test-{}-{}", std::process::id(), random().unwrap()));
        fs::create_dir_all(&dir).unwrap();
        dir
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
}
