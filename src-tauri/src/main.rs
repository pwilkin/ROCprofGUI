#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod appdir;
mod tuning;
mod mcp;
use profile_core::{Capture, Wave};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
};
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;
#[derive(Default)]
struct Store {
    captures: Mutex<BTreeMap<String, Arc<Capture>>>,
    next: AtomicU64,
    timeline_export: Mutex<Option<(String, Option<profile_core::timeline_file::TimelineFile>)>>,
    timeline_receipts: Mutex<profile_core::timeline_file::CompletionReceipts>,
    catalog_request: Mutex<Option<String>>,
    project_transfer: Arc<Mutex<Option<ProjectStage>>>,
    catalog_transfer: Arc<Mutex<Option<(String, profile_core::catalog_transfer::CatalogTransfer)>>>,
    import: Mutex<Option<Arc<AtomicBool>>>,
    source_cache: Arc<Mutex<profile_core::source_index::SourceCache>>,
    wave_cache: Arc<Mutex<profile_core::wave_index::WaveWindowCache>>,
    token_cache: Arc<profile_core::token_cache::TokenCache>,
    token_directory: Arc<Mutex<Option<Arc<profile_core::token_directory::TokenDirectory>>>>,
    wave_requests: Mutex<profile_core::wave_requests::WaveRequests>,
}
enum ProjectStage {
    Reading {
        id: String,
        batch: profile_core::catalog_transfer::CatalogBatch,
    },
    Applied {
        id: String,
        previous: BTreeMap<String, Arc<Capture>>,
        ids: Vec<String>,
    },
}
impl ProjectStage {
    fn id(&self) -> &str {
        match self {
            Self::Reading { id, .. } | Self::Applied { id, .. } => id,
        }
    }
}
#[derive(serde::Serialize)]
struct PagedProject {
    #[serde(flatten)]
    catalogs: profile_core::catalog_transfer::CatalogBatchHeader,
    project: profile_core::project::Project,
}
#[derive(serde::Serialize)]
#[serde(untagged)]
enum ProjectResponse {
    Complete(OpenedProject),
    Paged(PagedProject),
}
enum PreparedProject {
    Complete(OpenedProject),
    Paged(PagedProject, profile_core::catalog_transfer::CatalogBatch),
}
#[tauri::command]
fn diagnostic_context(
    capture_id: String,
    state: tauri::State<'_, Arc<Store>>,
) -> Result<serde_json::Value, String> {
    let captures = state.captures.lock().map_err(|e| e.to_string())?;
    let (input, fingerprint) = if capture_id == "demo" {
        (None, None)
    } else {
        let capture = captures
            .get(&capture_id)
            .ok_or("Capture is no longer open")?;
        (
            capture.input_path.clone(),
            capture.input_fingerprint.clone(),
        )
    };
    let identity = if fingerprint.is_some() {
        "SHA-256 file-set identity recorded at import; not rechecked by diagnostic export"
    } else {
        "live reference, not fingerprinted"
    };
    let config = profile_core::decoder::DecoderConfig::default();
    Ok(
        serde_json::json!({"environment":"desktop", "backendVersion":env!("CARGO_PKG_VERSION"), "os":std::env::consts::OS, "cpuArchitecture":std::env::consts::ARCH, "inputPath":input, "inputHash":null, "inputIdentity":identity,"inputFingerprint":fingerprint, "decoder":{"python":config.python.to_string_lossy(),"library":config.library.as_ref().map(|s|s.to_string_lossy()),"pythonPackage":config.python_package.as_ref().map(|p|p.to_string_lossy()),"timeoutSeconds":config.timeout.as_secs(),"status":"configuration only; availability not probed by export"}}),
    )
}
#[tauri::command]
async fn export_diagnostics(
    app: tauri::AppHandle,
    bytes: Vec<u8>,
    state: tauri::State<'_, Arc<Store>>,
) -> Result<bool, String> {
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("Diagnostic manifest exceeds 16 MiB".into());
    }
    let inputs: Vec<_> = state
        .captures
        .lock()
        .map_err(|e| e.to_string())?
        .values()
        .filter_map(|c| c.input_path.clone())
        .collect();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = app
            .dialog()
            .file()
            .set_file_name("rocprof-diagnostics.json")
            .add_filter("Diagnostic manifest", &["json"])
            .blocking_save_file()
        else {
            return Ok(false);
        };
        let path = path.into_path().map_err(|e| e.to_string())?;
        if std::fs::canonicalize(&path).is_ok_and(|p| inputs.contains(&p)) {
            return Err("Cannot overwrite an open capture input with diagnostics".into());
        }
        profile_core::diagnostics::write(&path, &bytes)?;
        Ok(true)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn save_att_manifest(
    app: tauri::AppHandle,
    manifest: profile_core::att_manifest::AttManifest,
) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = app
            .dialog()
            .file()
            .set_file_name("capture.rocprof-att")
            .add_filter("ATT input manifest", &["rocprof-att"])
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let path = path.into_path().map_err(|e| e.to_string())?;
        profile_core::att_manifest::write(&path, manifest)?;
        Ok(Some(path.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn discover_att_runs(path: String) -> Result<Vec<profile_core::att_runs::AttRun>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        profile_core::att_runs::discover(std::path::Path::new(&path))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn attach_spm(
    app: tauri::AppHandle,
    capture_id: String,
    se: u32,
    instruction_pages: Option<bool>,
    transfer_id: Option<String>,
    state: tauri::State<'_, Arc<Store>>,
) -> Result<Option<CaptureResponse>, String> {
    let paged = instruction_pages.unwrap_or(false);
    let request_id = transfer_id.unwrap_or_else(|| {
        format!(
            "catalog-request-{}",
            state.next.fetch_add(1, Ordering::Relaxed)
        )
    });
    if paged && (request_id.is_empty() || request_id.len() > 128 || request_id.contains('\0')) {
        return Err("Invalid catalog transfer identity".into());
    }
    let trace = {
        let captures = state.captures.lock().map_err(|e| e.to_string())?;
        if captures.len() >= 6 {
            return Err("Close a capture before attaching SPM (six-capture limit)".into());
        }
        captures
            .get(&capture_id)
            .cloned()
            .ok_or("Open a native trace before attaching SPM")?
    };
    if !trace
        .realtime
        .as_ref()
        .is_some_and(|c| c.anchors.iter().any(|a| a.se == se))
    {
        return Err("No REALTIME anchors for the selected wave's shader engine".into());
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let mut pending = state.import.lock().map_err(|e| e.to_string())?;
        if pending.is_some() {
            return Err("Another import or export is running".into());
        }
        *pending = Some(cancelled.clone());
        if paged {
            *state.catalog_request.lock().map_err(|e| e.to_string())? = Some(request_id.clone());
        }
    }
    let worker_flag = cancelled.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let cancelled = worker_flag;
        let Some(input) = app
            .dialog()
            .file()
            .set_title("Select SPM from the same profiling run")
            .add_filter("SPM JSON", &["json"])
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        let spm = profile_core::project::load_input(
            &input.into_path().map_err(|e| e.to_string())?,
            cancelled.clone(),
        )?;
        let combined = profile_core::spm_prepare::attach(&trace, &spm, se)?;
        if cancelled.load(Ordering::Relaxed) {
            return Err("SPM attachment cancelled".into());
        }
        let Some(output) = app
            .dialog()
            .file()
            .set_title("Save combined capture to a new file")
            .set_file_name("trace-with-spm.rocprof-capture")
            .add_filter("Portable capture", &["rocprof-capture"])
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let output = output.into_path().map_err(|e| e.to_string())?;
        profile_core::capture_archive::write_new(&output, &combined, &cancelled)?;
        profile_core::project::load_input(&output, cancelled)
            .map(Some)
            .map_err(|e| {
                format!(
                    "Combined archive saved at {} but could not be opened: {e}",
                    output.display()
                )
            })
    })
    .await
    .map_err(|e| e.to_string());
    let mut pending = state.import.lock().map_err(|e| e.to_string())?;
    let outcome = (|| {
        if cancelled.load(Ordering::Relaxed) {
            return Err("SPM attachment cancelled".into());
        }
        let Some(mut capture) = result?? else {
            return Ok(None);
        };
        capture.id = state.next.fetch_add(1, Ordering::Relaxed).to_string();
        stage_capture_response(Arc::new(capture), paged, request_id, cancelled, &state).map(Some)
    })();
    if !matches!(&outcome, Ok(Some(CaptureResponse::Paged(_)))) {
        *pending = None;
        *state.catalog_request.lock().map_err(|e| e.to_string())? = None;
    }
    outcome
}
#[tauri::command]
async fn extract_code_object(
    app: tauri::AppHandle,
    capture_id: String,
    object_id: String,
    choose_source: Option<bool>,
    source_path: Option<String>,
    state: tauri::State<'_, Arc<Store>>,
) -> Result<Option<profile_core::code_object_files::Extraction>, String> {
    let uri = {
        let captures = state.captures.lock().map_err(|e| e.to_string())?;
        let capture = captures
            .get(&capture_id)
            .ok_or("Open a native capture before extracting a code object")?;
        capture
            .code_objects
            .iter()
            .find(|record| record.id == object_id)
            .ok_or("Code-object record is not in this capture")?
            .uri
            .clone()
            .ok_or("No URI was recorded for this code object")?
    };
    profile_core::code_object_files::parse(&uri)?;
    if let Some(path) = &source_path {
        if choose_source.unwrap_or(false)
            || !path.starts_with('/')
            || path.starts_with("//")
            || path.len() > 8192
            || path.contains('\0')
        {
            return Err("Invalid relocated container path or conflicting source selection".into());
        }
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let mut pending = state.import.lock().map_err(|e| e.to_string())?;
        if pending.is_some() {
            return Err("Another import or export is running".into());
        }
        *pending = Some(cancelled.clone());
    }
    let result = tauri::async_runtime::spawn_blocking(move || {
        let replacement = if choose_source.unwrap_or(false) {
            let Some(path) = app
                .dialog()
                .file()
                .set_title("Select relocated container — recorded offset and size will be used")
                .blocking_pick_file()
            else {
                return Ok(None);
            };
            Some(path.into_path().map_err(|e| e.to_string())?)
        } else {
            source_path.map(std::path::PathBuf::from)
        };
        if cancelled.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let Some(path) = app
            .dialog()
            .file()
            .set_file_name("code-object.co")
            .add_filter("AMDGPU code object", &["co", "hsaco"])
            .blocking_save_file()
        else {
            return Ok(None);
        };
        profile_core::code_object_files::extract_from(
            &uri,
            replacement.as_deref(),
            &path.into_path().map_err(|e| e.to_string())?,
            &cancelled,
        )
        .map(Some)
    })
    .await
    .map_err(|e| e.to_string());
    state.import.lock().map_err(|e| e.to_string())?.take();
    result?
}
#[tauri::command]
async fn export_capture_archive(
    app: tauri::AppHandle,
    capture_id: String,
    state: tauri::State<'_, Arc<Store>>,
) -> Result<bool, String> {
    let capture = state
        .captures
        .lock()
        .map_err(|e| e.to_string())?
        .get(&capture_id)
        .cloned()
        .ok_or("Open a native capture before exporting an archive")?;
    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let mut pending = state.import.lock().map_err(|e| e.to_string())?;
        if pending.is_some() {
            return Err("An import or archive export is already running".into());
        }
        *pending = Some(cancelled.clone());
    }
    let result = tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = app
            .dialog()
            .file()
            .set_file_name("capture.rocprof-capture")
            .add_filter("Portable capture", &["rocprof-capture"])
            .blocking_save_file()
        else {
            return Ok(false);
        };
        profile_core::capture_archive::write(
            &path.into_path().map_err(|e| e.to_string())?,
            &capture,
            &cancelled,
        )?;
        Ok(true)
    })
    .await
    .map_err(|e| e.to_string());
    let cancelled = state
        .import
        .lock()
        .map_err(|e| e.to_string())?
        .take()
        .is_some_and(|flag| flag.load(Ordering::Relaxed));
    match result? {
        Err(_) if cancelled => Ok(false),
        outcome => outcome,
    }
}
#[derive(serde::Serialize)]
#[serde(untagged)]
enum CaptureResponse {
    Complete(Arc<Capture>),
    Paged(profile_core::catalog_transfer::CatalogHeader),
}
fn stage_capture_response(
    capture: Arc<Capture>,
    paged: bool,
    request_id: String,
    cancelled: Arc<AtomicBool>,
    state: &Store,
) -> Result<CaptureResponse, String> {
    if state.captures.lock().map_err(|e| e.to_string())?.len() >= 6 {
        return Err("Close a capture before opening another (six-capture memory limit)".into());
    }
    if paged {
        let transfer = profile_core::catalog_transfer::CatalogTransfer::new(
            capture,
            request_id.clone(),
            cancelled,
        )?;
        let header = transfer.header()?;
        *state.catalog_transfer.lock().map_err(|e| e.to_string())? = Some((request_id, transfer));
        Ok(CaptureResponse::Paged(header))
    } else {
        state
            .captures
            .lock()
            .map_err(|e| e.to_string())?
            .insert(capture.id.clone(), capture.clone());
        Ok(CaptureResponse::Complete(capture))
    }
}
#[tauri::command]
async fn open_capture(
    path: String,
    sampling_method: Option<String>,
    load_wave_states: Option<bool>,
    instruction_pages: Option<bool>,
    transfer_id: Option<String>,
    state: tauri::State<'_, Arc<Store>>,
) -> Result<CaptureResponse, String> {
    let paged = instruction_pages.unwrap_or(false);
    let request_id = transfer_id.unwrap_or_else(|| {
        format!(
            "catalog-request-{}",
            state.next.fetch_add(1, Ordering::Relaxed)
        )
    });
    if paged && (request_id.is_empty() || request_id.len() > 128 || request_id.contains('\0')) {
        return Err("Invalid catalog transfer identity".into());
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let mut pending = state.import.lock().map_err(|e| e.to_string())?;
        if pending.is_some() {
            return Err("An import is already running".into());
        }
        if state.captures.lock().map_err(|e| e.to_string())?.len() >= 6 {
            return Err("Close a capture before opening another (six-capture memory limit)".into());
        }
        *pending = Some(cancelled.clone());
        if paged {
            *state.catalog_request.lock().map_err(|e| e.to_string())? = Some(request_id.clone());
        }
    }
    let worker_flag = cancelled.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        profile_core::project::load_input_options(
            std::path::Path::new(&path),
            worker_flag,
            load_wave_states.unwrap_or(true),
            sampling_method.as_deref(),
        )
    })
    .await
    .map_err(|e| e.to_string());
    let mut pending = state.import.lock().map_err(|e| e.to_string())?;
    let outcome = (|| {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Import cancelled".into());
        }
        let mut capture = result??;
        capture.id = state.next.fetch_add(1, Ordering::Relaxed).to_string();
        let capture = Arc::new(capture);
        stage_capture_response(capture, paged, request_id, cancelled, &state)
    })();
    if !paged || outcome.is_err() {
        *state.catalog_request.lock().map_err(|e| e.to_string())? = None;
        *pending = None;
    }
    outcome
}
#[tauri::command]
async fn read_catalog_page(
    transfer_id: String,
    offset: usize,
    rows: usize,
    state: tauri::State<'_, Arc<Store>>,
) -> Result<profile_core::catalog_transfer::InstructionPage, String> {
    let staged = state.catalog_transfer.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut staged = staged.lock().map_err(|e| e.to_string())?;
        let (id, transfer) = staged
            .as_mut()
            .ok_or("Catalog transfer is no longer active")?;
        if id != &transfer_id {
            return Err("Catalog transfer identity mismatch".into());
        }
        transfer.page(offset, rows)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn read_observation_page(
    transfer_id: String,
    offset: usize,
    rows: usize,
    state: tauri::State<'_, Arc<Store>>,
) -> Result<profile_core::catalog_transfer::ObservationPage, String> {
    let staged = state.catalog_transfer.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut staged = staged.lock().map_err(|e| e.to_string())?;
        let (id, transfer) = staged
            .as_mut()
            .ok_or("Catalog transfer is no longer active")?;
        if id != &transfer_id {
            return Err("Catalog transfer identity mismatch".into());
        }
        transfer.observation_page(offset, rows)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn finish_catalog_transfer(
    transfer_id: String,
    app: tauri::AppHandle,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Arc<Store>>();
        let result = (|| {
            let mut pending = state.import.lock().map_err(|e| e.to_string())?;
            let mut staged = state.catalog_transfer.lock().map_err(|e| e.to_string())?;
            if staged.as_ref().map(|(id, _)| id) != Some(&transfer_id) {
                return Err("Catalog transfer identity mismatch".into());
            }
            let (_, transfer) = staged.take().unwrap();
            let result = (|| {
                let capture = transfer.commit()?;
                let mut captures = state.captures.lock().map_err(|e| e.to_string())?;
                if captures.len() >= 6 {
                    return Err("Close a capture before committing another".into());
                }
                let id = capture.id.clone();
                captures.insert(id.clone(), capture);
                Ok(id)
            })();
            *pending = None;
            *state.catalog_request.lock().map_err(|e| e.to_string())? = None;
            result
        })();
        if result.is_err() {
            release_capture_data(());
        }
        result
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
fn abort_catalog_transfer(
    transfer_id: String,
    state: tauri::State<'_, Arc<Store>>,
) -> Result<(), String> {
    state.abort_catalog_request(Some(&transfer_id))
}
impl Store {
    fn abort_catalog_request(&self, expected: Option<&str>) -> Result<(), String> {
        let mut pending = self.import.lock().map_err(|e| e.to_string())?;
        let mut request = self.catalog_request.lock().map_err(|e| e.to_string())?;
        if request.is_none() || expected.is_some_and(|id| request.as_deref() != Some(id)) {
            return Ok(());
        }
        if let Some(flag) = pending.as_ref() {
            flag.store(true, Ordering::Relaxed);
        }
        let single = self
            .catalog_transfer
            .lock()
            .map_err(|e| e.to_string())?
            .take();
        let project = self
            .project_transfer
            .lock()
            .map_err(|e| e.to_string())?
            .take();
        let staged = single.is_some() || project.is_some();
        let retired = match project {
            Some(ProjectStage::Applied { previous, .. }) => {
                let replacement = std::mem::replace(
                    &mut *self.captures.lock().map_err(|e| e.to_string())?,
                    previous,
                );
                self.wave_requests.lock().map_err(|e| e.to_string())?.cancel_all();
                self.token_cache.clear()?;
                (single, None, Some(replacement))
            }
            project => (single, project, None),
        };
        if staged {
            *pending = None;
            *request = None;
        }
        drop(request);
        drop(pending);
        if staged {
            tauri::async_runtime::spawn_blocking(move || release_capture_data(retired));
        }
        Ok(())
    }
    fn abandon_catalog_request(&self) {
        let _ = self.abort_catalog_request(None);
    }
}
#[tauri::command]
async fn read_project_catalog_page(
    transfer_id: String,
    capture_index: usize,
    offset: usize,
    rows: usize,
    state: tauri::State<'_, Arc<Store>>,
) -> Result<profile_core::catalog_transfer::InstructionPage, String> {
    let staged = state.project_transfer.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut stage = staged.lock().map_err(|e| e.to_string())?;
        match stage.as_mut() {
            Some(ProjectStage::Reading { id, batch }) if id == &transfer_id => {
                batch.page(capture_index, offset, rows)
            }
            _ => Err("Project transfer is not readable or identity mismatches".into()),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn read_project_observation_page(
    transfer_id: String,
    capture_index: usize,
    offset: usize,
    rows: usize,
    state: tauri::State<'_, Arc<Store>>,
) -> Result<profile_core::catalog_transfer::ObservationPage, String> {
    let staged = state.project_transfer.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut stage = staged.lock().map_err(|e| e.to_string())?;
        match stage.as_mut() {
            Some(ProjectStage::Reading { id, batch }) if id == &transfer_id => {
                batch.observation_page(capture_index, offset, rows)
            }
            _ => Err("Project transfer is not readable or identity mismatches".into()),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn finish_project_transfer(
    transfer_id: String,
    app: tauri::AppHandle,
) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Arc<Store>>();
        let result = (|| {
            let mut pending = state.import.lock().map_err(|e| e.to_string())?;
            let mut request = state.catalog_request.lock().map_err(|e| e.to_string())?;
            let mut staged = state.project_transfer.lock().map_err(|e| e.to_string())?;
            if staged.as_ref().map(ProjectStage::id) != Some(transfer_id.as_str()) {
                return Err("Project transfer identity mismatch".into());
            }
            if pending
                .as_ref()
                .is_some_and(|flag| flag.load(Ordering::Relaxed))
            {
                return Err("Project transfer cancelled".into());
            }
            if let Some(ProjectStage::Applied { ids, .. }) = staged.as_ref() {
                return Ok(ids.clone());
            }
            let Some(ProjectStage::Reading { id, batch }) = staged.take() else {
                unreachable!()
            };
            let captures = match batch.commit() {
                Ok(captures) => captures,
                Err(error) => {
                    *pending = None;
                    *request = None;
                    return Err(error);
                }
            };
            let ids: Vec<_> = captures.iter().map(|c| c.id.clone()).collect();
            let replacement = captures
                .into_iter()
                .filter(|c| c.id != "demo")
                .map(|c| (c.id.clone(), c))
                .collect();
            let previous = std::mem::replace(
                &mut *state.captures.lock().map_err(|e| e.to_string())?,
                replacement,
            );
            state.wave_requests.lock().map_err(|e| e.to_string())?.cancel_all();
            state.token_cache.clear()?;
            *staged = Some(ProjectStage::Applied {
                id,
                previous,
                ids: ids.clone(),
            });
            Ok(ids)
        })();
        if result.is_err() {
            release_capture_data(());
        }
        result
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
fn acknowledge_project_transfer(
    transfer_id: String,
    state: tauri::State<'_, Arc<Store>>,
) -> Result<(), String> {
    let mut pending = state.import.lock().map_err(|e| e.to_string())?;
    let mut request = state.catalog_request.lock().map_err(|e| e.to_string())?;
    let mut staged = state.project_transfer.lock().map_err(|e| e.to_string())?;
    if staged.as_ref().map(ProjectStage::id) != Some(transfer_id.as_str()) {
        return Ok(());
    }
    if matches!(staged.as_ref(), Some(ProjectStage::Reading { .. })) {
        return Err("Project transfer is not committed".into());
    }
    let retired = staged.take();
    *pending = None;
    *request = None;
    drop(staged);
    drop(request);
    drop(pending);
    tauri::async_runtime::spawn_blocking(move || release_capture_data(retired));
    Ok(())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveProjectEntry {
    capture_id: String,
    state: profile_core::project::WorkspaceState,
    sources: BTreeMap<String, profile_core::project::SourceReplacement>,
}
#[derive(serde::Serialize)]
struct OpenedProject {
    captures: Vec<Arc<Capture>>,
    project: profile_core::project::Project,
}
#[tauri::command]
async fn save_project(
    app: tauri::AppHandle,
    entries: Vec<SaveProjectEntry>,
    active: usize,
    definition_library: Option<profile_core::project::DefinitionLibrary>,
    state: tauri::State<'_, Arc<Store>>,
) -> Result<bool, String> {
    let project = {
        let captures = state.captures.lock().map_err(|e| e.to_string())?;
        let entries = entries
            .into_iter()
            .map(|entry| {
                let input = if entry.capture_id == "demo" {
                    None
                } else {
                    Some(
                        captures
                            .get(&entry.capture_id)
                            .ok_or("Capture is closed")?
                            .input_path
                            .clone()
                            .ok_or("Capture has no reopenable input path")?,
                    )
                };
                let fingerprint = captures
                    .get(&entry.capture_id)
                    .and_then(|capture| capture.input_fingerprint.clone());
                Ok(profile_core::project::ProjectEntry {
                    load_wave_states: captures
                        .get(&entry.capture_id)
                        .and_then(|c| c.metadata.get("loadWaveStates"))
                        .and_then(|s| s.parse().ok()),
                    pc_sampling_method: input
                        .as_ref()
                        .filter(|p| p.extension().is_some_and(|e| e == "json"))
                        .and_then(|_| captures.get(&entry.capture_id))
                        .and_then(|c| c.metadata.get("pc_sampling_selection"))
                        .cloned(),
                    input,
                    fingerprint,
                    state: entry.state,
                    sources: entry.sources,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        profile_core::project::Project {
            schema_version: if entries.iter().any(|e| e.pc_sampling_method.is_some()) {
                5
            } else if entries.iter().any(|e| e.load_wave_states.is_some()) {
                4
            } else if entries.iter().any(|e| e.fingerprint.is_some()) {
                3
            } else if entries
                .iter()
                .any(|e| e.sources.values().any(|s| s.reference.is_some()))
            {
                2
            } else {
                1
            },
            definition_library,
            active,
            entries,
        }
    };
    project.validate()?;
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = app
            .dialog()
            .file()
            .set_file_name("workspace.rocprof-project")
            .add_filter("rocprof project", &["rocprof-project"])
            .blocking_save_file()
        else {
            return Ok(false);
        };
        profile_core::project::write(&path.into_path().map_err(|e| e.to_string())?, &project)?;
        Ok(true)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn open_project(
    app: tauri::AppHandle,
    instruction_pages: Option<bool>,
    transfer_id: Option<String>,
    state: tauri::State<'_, Arc<Store>>,
) -> Result<Option<ProjectResponse>, String> {
    let paged = instruction_pages.unwrap_or(false);
    let request_id = transfer_id
        .unwrap_or_else(|| format!("project-{}", state.next.fetch_add(1, Ordering::Relaxed)));
    if paged && (request_id.is_empty() || request_id.len() > 96 || request_id.contains('\0')) {
        return Err("Invalid project transfer identity".into());
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let mut pending = state.import.lock().map_err(|e| e.to_string())?;
        if pending.is_some() {
            return Err("An import is already running".into());
        }
        *pending = Some(cancelled.clone());
        if paged {
            *state.catalog_request.lock().map_err(|e| e.to_string())? = Some(request_id.clone());
        }
    }
    let worker_cancelled = cancelled.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = app
            .dialog()
            .file()
            .add_filter("rocprof project", &["rocprof-project"])
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        let path = path.into_path().map_err(|e| e.to_string())?;
        let mut project = profile_core::project::read(&path)?;
        let mut captures = profile_core::project::stage_with_limit(
            &project,
            worker_cancelled,
            if paged {
                profile_core::project::MAX_PAGED_STAGED_BYTES
            } else {
                profile_core::project::MAX_STAGED_BYTES
            },
        )?;
        profile_core::project::reconcile(&mut project, &mut captures);
        Ok::<_, String>(Some((project, captures)))
    })
    .await
    .map_err(|e| e.to_string());
    let outcome = async {
        let Some((project, captures)) = result?? else {
            return Ok::<_, String>(None);
        };
        let captures: Vec<_> = captures
            .into_iter()
            .zip(&project.entries)
            .map(|(mut c, e)| {
                c.id = if e.input.is_none() {
                    "demo".into()
                } else {
                    state.next.fetch_add(1, Ordering::Relaxed).to_string()
                };
                Arc::new(c)
            })
            .collect();
        let opened = OpenedProject { captures, project };
        let check_cancelled = cancelled.clone();
        let batch_id = request_id.clone();
        tauri::async_runtime::spawn_blocking(move || {
            if paged {
                let batch = profile_core::catalog_transfer::CatalogBatch::new(
                    opened.captures,
                    batch_id,
                    check_cancelled.clone(),
                )?;
                let header = PagedProject {
                    catalogs: batch.header()?,
                    project: opened.project,
                };
                profile_core::project::check_staged_payload(&header, &check_cancelled)?;
                Ok(Some(PreparedProject::Paged(header, batch)))
            } else {
                profile_core::project::check_staged_payload(&opened, &check_cancelled)?;
                Ok(Some(PreparedProject::Complete(opened)))
            }
        })
        .await
        .map_err(|e| e.to_string())?
    }
    .await;
    let mut pending = state.import.lock().map_err(|e| e.to_string())?;
    let result = (|| {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Project import cancelled".into());
        }
        let Some(prepared) = outcome? else {
            return Ok(None);
        };
        match prepared {
            PreparedProject::Paged(header, batch) => {
                *state.project_transfer.lock().map_err(|e| e.to_string())? =
                    Some(ProjectStage::Reading {
                        id: request_id,
                        batch,
                    });
                Ok(Some(ProjectResponse::Paged(header)))
            }
            PreparedProject::Complete(opened) => {
                let replacement = opened
                    .captures
                    .iter()
                    .filter(|c| c.id != "demo")
                    .map(|c| (c.id.clone(), c.clone()))
                    .collect();
                *state.captures.lock().map_err(|e| e.to_string())? = replacement;
                state.wave_requests.lock().map_err(|e| e.to_string())?.cancel_all();
                state.token_cache.clear()?;
                Ok(Some(ProjectResponse::Complete(opened)))
            }
        }
    })();
    if !matches!(&result, Ok(Some(ProjectResponse::Paged(_)))) {
        *pending = None;
        *state.catalog_request.lock().map_err(|e| e.to_string())? = None;
    }
    result
}
#[tauri::command]
fn cancel_import(state: tauri::State<'_, Arc<Store>>) -> Result<(), String> {
    if let Some(flag) = state.import.lock().map_err(|e| e.to_string())?.as_ref() {
        flag.store(true, Ordering::Relaxed);
    }
    Ok(())
}
#[tauri::command]
async fn read_wave(
    capture_id: String,
    wave_id: String,
    request_id: Option<String>,
    state: tauri::State<'_, Arc<Store>>,
) -> Result<Wave, String> {
    let capture = state
        .captures
        .lock()
        .map_err(|e| e.to_string())?
        .get(&capture_id)
        .cloned()
        .ok_or("Capture is closed")?;
    let cancelled = match request_id.as_deref() {
        Some(id) => state.wave_requests.lock().map_err(|e|e.to_string())?.claim(id, &capture_id)?,
        None => Arc::new(AtomicBool::new(false)),
    };
    let result = tauri::async_runtime::spawn_blocking(move || profile_core::load_wave_cancellable(&capture, &wave_id, &cancelled))
        .await
        .map_err(|e| e.to_string());
    if let Some(id) = request_id { state.wave_requests.lock().map_err(|e|e.to_string())?.cancel(&id); }
    result?
}
#[tauri::command]
fn begin_wave_query(capture_id: String, request_id: Option<String>, state: tauri::State<'_, Arc<Store>>) -> Result<String, String> {
    let captures = state.captures.lock().map_err(|e|e.to_string())?;
    if !captures.contains_key(&capture_id) { return Err("Capture is closed".into()); }
    let id = request_id.unwrap_or_else(|| format!("wave-{}", state.next.fetch_add(1, Ordering::Relaxed)));
    state.wave_requests.lock().map_err(|e|e.to_string())?.begin(id.clone(), capture_id)?;
    Ok(id)
}
#[tauri::command]
fn cancel_wave_query(request_id: String, state: tauri::State<'_, Arc<Store>>) -> Result<(), String> {
    state.wave_requests.lock().map_err(|e|e.to_string())?.cancel(&request_id);
    Ok(())
}
#[tauri::command]
async fn read_wave_stats(
    capture_id: String,
    se: u32,
    start: f64,
    end: f64,
    request_id: Option<String>,
    state: tauri::State<'_, Arc<Store>>,
) -> Result<Vec<profile_core::wave_stats::InstructionStats>, String> {
    let capture = state
        .captures
        .lock()
        .map_err(|e| e.to_string())?
        .get(&capture_id)
        .cloned()
        .ok_or("Capture is closed")?;
    let cancelled = match request_id.as_deref() {
        Some(id) => state.wave_requests.lock().map_err(|e|e.to_string())?.claim(id, &capture_id)?,
        None => Arc::new(AtomicBool::new(false)),
    };
    let cache = state.wave_cache.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        if cancelled.load(Ordering::Relaxed) { return Err("Wave query cancelled".into()); }
        let indices = cache
            .lock()
            .map_err(|e| e.to_string())?
            .select_aggregate(&capture, se, start, end)?;
        profile_core::wave_stats::load_cancellable(&capture, &indices, [start, end], &cancelled)
    })
    .await
    .map_err(|e| e.to_string());
    if let Some(id) = request_id { state.wave_requests.lock().map_err(|e|e.to_string())?.cancel(&id); }
    result?
}
#[tauri::command]
async fn read_wave_overview(
    capture_id: String,
    se: u32,
    start: f64,
    end: f64,
    utilization: Option<bool>,
    separate_lds: Option<bool>,
    request_id: Option<String>,
    state: tauri::State<'_, Arc<Store>>,
) -> Result<profile_core::wave_overview::Overview, String> {
    let capture = state
        .captures
        .lock()
        .map_err(|e| e.to_string())?
        .get(&capture_id)
        .cloned()
        .ok_or("Capture is closed")?;
    let cancelled = match request_id.as_deref() {
        Some(id) => state.wave_requests.lock().map_err(|e|e.to_string())?.claim(id, &capture_id)?,
        None => Arc::new(AtomicBool::new(false)),
    };
    let cache = state.wave_cache.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        if cancelled.load(Ordering::Relaxed) { return Err("Wave query cancelled".into()); }
        let indices = cache
            .lock()
            .map_err(|e| e.to_string())?
            .select_aggregate(&capture, se, start, end)?;
        profile_core::wave_overview::load_mode_cancellable(
            &capture,
            &indices,
            [start, end],
            utilization.unwrap_or(false),
            separate_lds.unwrap_or(false),
            &cancelled,
        )
    })
    .await
    .map_err(|e| e.to_string());
    if let Some(id) = request_id { state.wave_requests.lock().map_err(|e|e.to_string())?.cancel(&id); }
    result?
}
#[tauri::command]
async fn read_wave_window(
    app: tauri::AppHandle,
    capture_id: String,
    se: u32,
    start: f64,
    end: f64,
    request_id: Option<String>,
    state: tauri::State<'_, Arc<Store>>,
) -> Result<Vec<profile_core::wave_viewport::ViewportWave>, String> {
    let capture = state
        .captures
        .lock()
        .map_err(|e| e.to_string())?
        .get(&capture_id)
        .cloned()
        .ok_or("Capture is closed")?;
    let cancelled = match request_id.as_deref() {
        Some(id) => state.wave_requests.lock().map_err(|e|e.to_string())?.claim(id, &capture_id)?,
        None => Arc::new(AtomicBool::new(false)),
    };
    let cache = state.wave_cache.clone();
    let token_cache = state.token_cache.clone();
    let token_directory = state.token_directory.clone();
    let cache_parent = app.path().app_cache_dir().ok().map(|p| p.join("token-index"));
    let result = tauri::async_runtime::spawn_blocking(move || {
        if cancelled.load(Ordering::Relaxed) { return Err("Wave query cancelled".into()); }
        let indices = cache
            .lock()
            .map_err(|e| e.to_string())?
            .select(&capture, se, start, end)?;
        let directory = {
            let mut current = token_directory.lock().map_err(|e| e.to_string())?;
            if current.is_none() {
                if let Some(parent) = cache_parent {
                    *current = profile_core::token_directory::TokenDirectory::create(&parent).ok().map(Arc::new);
                }
            }
            current.clone()
        };
        let result = match directory {
            Some(directory) => token_cache.load_window(&capture, &indices, start, end, directory.path(), &cancelled),
            None => profile_core::wave_viewport::load_cancellable(&capture, &indices, start, end, &cancelled),
        };
        drop(capture);
        token_cache.stats()?;
        result
    })
    .await
    .map_err(|e| e.to_string());
    if let Some(id) = request_id { state.wave_requests.lock().map_err(|e|e.to_string())?.cancel(&id); }
    result?
}
#[tauri::command]
async fn analyze_hidden(
    capture_id: String,
    request_id: Option<String>,
    state: tauri::State<'_, Arc<Store>>,
) -> Result<profile_core::hidden::HiddenAnalysis, String> {
    let capture = state
        .captures
        .lock()
        .map_err(|e| e.to_string())?
        .get(&capture_id)
        .cloned()
        .ok_or("Capture is closed")?;
    let cancelled = match request_id.as_deref() {
        Some(id) => state.wave_requests.lock().map_err(|e|e.to_string())?.claim(id, &capture_id)?,
        None => Arc::new(AtomicBool::new(false)),
    };
    let result = tauri::async_runtime::spawn_blocking(move || profile_core::hidden::analyze_capture_cancellable(&capture, &cancelled))
        .await
        .map_err(|e| e.to_string());
    if let Some(id) = request_id { state.wave_requests.lock().map_err(|e|e.to_string())?.cancel(&id); }
    result?
}
#[tauri::command]
async fn close_capture(capture_id: String, state: tauri::State<'_, Arc<Store>>) -> Result<(), String> {
    let capture = state
        .captures
        .lock()
        .map_err(|e| e.to_string())?
        .remove(&capture_id);
    state.wave_requests.lock().map_err(|e|e.to_string())?.cancel_capture(&capture_id);
    if let Some(capture) = capture {
        state.token_cache.remove_capture(&capture)?;
        tauri::async_runtime::spawn_blocking(move || release_capture_data(capture))
        .await
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn release_capture_data<T>(data: T) {
    drop(data);
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        extern "C" {
            fn malloc_trim(pad: usize) -> std::os::raw::c_int;
        }
        // Large captures leave free pages in glibc arenas after destruction.
        malloc_trim(0);
    }
}
#[tauri::command]
async fn import_definition_files(
    app: tauri::AppHandle,
) -> Result<Vec<profile_core::definition_files::DefinitionFile>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(paths) = app
            .dialog()
            .file()
            .add_filter("Counter definitions", &["txt", "def"])
            .blocking_pick_files()
        else {
            return Ok(vec![]);
        };
        if paths.len() > 32 {
            return Err("Import at most 32 definition files at once".into());
        }
        let mut files = Vec::new();
        let mut total = 0;
        for path in paths {
            let path = path.into_path().map_err(|e| e.to_string())?;
            let file = profile_core::definition_files::read(&path)?;
            total += file.text.len();
            if total > 2 * 1024 * 1024 {
                return Err("Definition import exceeds 2 MiB".into());
            }
            files.push(file);
        }
        Ok(files)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn export_definition_file(
    app: tauri::AppHandle,
    name: String,
    text: String,
) -> Result<bool, String> {
    if text.len() > profile_core::definition_files::MAX_DEFINITION_BYTES {
        return Err("Definition file exceeds 256 KiB".into());
    }
    let name = std::path::Path::new(&name)
        .file_name()
        .ok_or("Invalid filename")?
        .to_string_lossy()
        .into_owned();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = app
            .dialog()
            .file()
            .set_file_name(name)
            .add_filter("Counter definitions", &["txt", "def"])
            .blocking_save_file()
        else {
            return Ok(false);
        };
        profile_core::definition_files::write(
            &path.into_path().map_err(|e| e.to_string())?,
            &text,
        )?;
        Ok(true)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn begin_timeline_export(
    app: tauri::AppHandle,
    id: String,
    name: String,
    state: tauri::State<'_, Arc<Store>>,
) -> Result<bool, String> {
    if id.is_empty() || id.len() > 128 {
        return Err("Invalid export request ID".into());
    }
    {
        let mut slot = state.timeline_export.lock().map_err(|e| e.to_string())?;
        if slot.is_some() {
            return Err("A timeline export is already running".into());
        }
        if state
            .timeline_receipts
            .lock()
            .map_err(|e| e.to_string())?
            .contains(&id)
        {
            return Err("Timeline export request ID was already completed".into());
        }
        *slot = Some((id.clone(), None));
    }
    let chooser = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_file_name(name)
            .add_filter("Timeline HTML", &["html"])
            .blocking_save_file()
    })
    .await;
    let result = (|| {
        let path = chooser.map_err(|e| e.to_string())?;
        let mut slot = state.timeline_export.lock().map_err(|e| e.to_string())?;
        if slot.as_ref().map(|s| &s.0) != Some(&id) {
            return Err("Timeline export cancelled".into());
        }
        let Some(path) = path else {
            *slot = None;
            return Ok(false);
        };
        let path = path.into_path().map_err(|e| e.to_string())?;
        protect_timeline_target(&path, &state)?;
        slot.as_mut().unwrap().1 = Some(profile_core::timeline_file::TimelineFile::new(&path)?);
        Ok(true)
    })();
    if result.is_err() {
        abort_timeline_export(id, state)?;
    }
    result
}
fn protect_timeline_target(path: &std::path::Path, state: &Store) -> Result<(), String> {
    if let Ok(target) = std::fs::canonicalize(path) {
        if state
            .captures
            .lock()
            .map_err(|e| e.to_string())?
            .values()
            .any(|c| {
                c.input_path
                    .as_ref()
                    .is_some_and(|p| std::fs::canonicalize(p).is_ok_and(|p| p == target))
            })
        {
            return Err("Cannot overwrite an open capture input with a timeline export".into());
        }
    }
    Ok(())
}
#[tauri::command]
fn append_timeline_export(
    id: String,
    sequence: u64,
    text: String,
    state: tauri::State<'_, Arc<Store>>,
) -> Result<(), String> {
    let mut slot = state.timeline_export.lock().map_err(|e| e.to_string())?;
    let (owner, file) = slot.as_mut().ok_or("No timeline export is running")?;
    if owner != &id {
        return Err("Timeline export request mismatch".into());
    }
    file.as_mut()
        .ok_or("Timeline export destination is pending")?
        .append(sequence, &text)
}
#[tauri::command]
fn finish_timeline_export(
    id: String,
    chunks: u64,
    state: tauri::State<'_, Arc<Store>>,
) -> Result<u64, String> {
    let mut slot = state.timeline_export.lock().map_err(|e| e.to_string())?;
    let mut receipts = state.timeline_receipts.lock().map_err(|e| e.to_string())?;
    if let Some(bytes) = receipts.lookup(&id, chunks)? {
        return Ok(bytes);
    }
    let (owner, file) = slot.as_ref().ok_or("No timeline export is running")?;
    if owner != &id {
        return Err("Timeline export request mismatch".into());
    }
    protect_timeline_target(
        file.as_ref()
            .ok_or("Timeline export destination is pending")?
            .target(),
        &state,
    )?;
    let (_, file) = slot.take().unwrap();
    let bytes = file.unwrap().finish(chunks)?;
    receipts.record(id, chunks, bytes);
    Ok(bytes)
}
#[tauri::command]
fn abort_timeline_export(id: String, state: tauri::State<'_, Arc<Store>>) -> Result<(), String> {
    let mut slot = state.timeline_export.lock().map_err(|e| e.to_string())?;
    if slot.as_ref().is_some_and(|s| s.0 == id) {
        *slot = None;
    }
    Ok(())
}
#[tauri::command]
async fn export_plot(
    app: tauri::AppHandle,
    name: String,
    format: String,
    bytes: Vec<u8>,
) -> Result<bool, String> {
    profile_core::plot_files::validate(&format, &bytes)?;
    let name = std::path::Path::new(&name)
        .file_name()
        .ok_or("Invalid filename")?
        .to_string_lossy()
        .into_owned();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = app
            .dialog()
            .file()
            .set_file_name(name)
            .add_filter("Plot image", &[format.as_str()])
            .blocking_save_file()
        else {
            return Ok(false);
        };
        profile_core::plot_files::write(
            &path.into_path().map_err(|e| e.to_string())?,
            &format,
            &bytes,
        )?;
        Ok(true)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn import_source_file(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<Store>>,
) -> Result<Option<profile_core::source_files::SourceFile>, String> {
    let cache = state.source_cache.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = app.dialog().file().blocking_pick_file() else {
            return Ok(None);
        };
        profile_core::source_files::import_replacement(
            &path.into_path().map_err(|e| e.to_string())?,
            &mut *cache.lock().map_err(|e| e.to_string())?,
        )
        .map(Some)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn import_source_directory(
    app: tauri::AppHandle,
    capture_id: String,
    prefix: String,
    state: tauri::State<'_, Arc<Store>>,
) -> Result<Option<profile_core::source_files::SourceBatch>, String> {
    let recorded = if capture_id == "demo" {
        profile_core::source_files::example_paths()?
    } else {
        let captures = state.captures.lock().map_err(|e| e.to_string())?;
        let capture = captures.get(&capture_id).ok_or("Capture is closed")?;
        profile_core::source_files::recorded_paths(capture)
    };
    let cache = state.source_cache.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(directory) = app.dialog().file().blocking_pick_folder() else {
            return Ok(None);
        };
        profile_core::source_files::map_directory_cached(
            &recorded,
            &prefix,
            &directory.into_path().map_err(|e| e.to_string())?,
            &mut *cache.lock().map_err(|e| e.to_string())?,
        )
        .map(Some)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn read_source(
    capture_id: String,
    original: String,
    state: tauri::State<'_, Arc<Store>>,
) -> Result<profile_core::source_files::SourceFile, String> {
    let capture = state
        .captures
        .lock()
        .map_err(|e| e.to_string())?
        .get(&capture_id)
        .cloned()
        .ok_or("Capture is closed")?;
    tauri::async_runtime::spawn_blocking(move || {
        profile_core::source_files::read_snapshot(&capture, &original)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn read_source_page(
    capture_id: String,
    original: String,
    start_line: usize,
    count: usize,
    state: tauri::State<'_, Arc<Store>>,
) -> Result<profile_core::source_files::SourcePage, String> {
    let capture = state
        .captures
        .lock()
        .map_err(|e| e.to_string())?
        .get(&capture_id)
        .cloned()
        .ok_or("Capture is closed")?;
    let cache = state.source_cache.clone();
    tauri::async_runtime::spawn_blocking(move || {
        cache
            .lock()
            .map_err(|e| e.to_string())?
            .read_snapshot_page(&capture, &original, start_line, count)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn read_source_reference_page(
    reference: profile_core::source_files::SourceReference,
    start_line: usize,
    count: usize,
    state: tauri::State<'_, Arc<Store>>,
) -> Result<profile_core::source_files::SourcePage, String> {
    let cache = state.source_cache.clone();
    tauri::async_runtime::spawn_blocking(move || {
        cache
            .lock()
            .map_err(|e| e.to_string())?
            .read_reference_page(&reference, start_line, count)
    })
    .await
    .map_err(|e| e.to_string())?
}
fn main() {
    let options = mcp::Options::parse().unwrap_or_else(|e| { eprintln!("{e}"); std::process::exit(2) });
    let store = Arc::new(Store::default());
    let tuning = Arc::new(tuning::Tuning::default());
    let control = Arc::new(mcp::Control::default());
    let was_headless = options.headless;
    if was_headless {
        let attached = mcp::headless(options.clone(), store.clone(), tuning.clone(), control.clone())
            .unwrap_or_else(|e| { eprintln!("{e}"); std::process::exit(1) });
        if !attached { return; }
    } else if mcp::try_attach(&options)
        .unwrap_or_else(|e| { eprintln!("{e}"); std::process::exit(1) }) {
        eprintln!("Opened the GUI in the running rocprof-studio service");
        return;
    } else {
        tuning::sweep_in_background();
    }
    let shutdown_control=control.clone();
    let shutdown_tuning=tuning.clone();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(store.clone())
        .manage(control.clone())
        .manage(tuning.clone())
        .on_page_load(|webview, payload| {
            if webview.label() == "main" && payload.event() == tauri::webview::PageLoadEvent::Started {
                webview.state::<Arc<Store>>().abandon_catalog_request();
                if let Ok(mut requests) = webview.state::<Arc<Store>>().wave_requests.lock() { requests.cancel_all(); }
                let _ = webview.state::<Arc<Store>>().token_cache.clear();
                if let Ok(mut export) = webview.state::<Arc<Store>>().timeline_export.lock() {
                    *export = None;
                }
            }
        })
        .setup(move |app| {
            if !profile_core::decoder::bundled_python_package_configured() {
                let package = app.path().resource_dir()?.join("decoder-python.zip");
                profile_core::decoder::set_bundled_python_package(package)?;
            }
            if let Ok(parent) = app.path().app_cache_dir() {
                tauri::async_runtime::spawn_blocking(move || {
                    let _ = profile_core::token_directory::TokenDirectory::reap(&parent.join("token-index"));
                });
            }
            if was_headless {
                mcp::attach_gui(&control, app.handle().clone());
            } else {
                mcp::start(options.clone(), store.clone(), tuning.clone(), control.clone(), Some(app.handle().clone()))?;
            }
            Ok(())
        })
        .invoke_handler(mcp::guard(tauri::generate_handler![
            mcp::mcp_status,
            mcp::mcp_cancel,
            mcp::mcp_capture,
            tuning::open_tuning,
            tuning::tuning_start,
            tuning::tuning_status,
            tuning::tuning_cancel,
            open_capture,
            read_catalog_page,
            read_observation_page,
            finish_catalog_transfer,
            abort_catalog_transfer,
            attach_spm,
            export_capture_archive,
            extract_code_object,
            save_project,
            open_project,
            read_project_catalog_page,
            read_project_observation_page,
            finish_project_transfer,
            acknowledge_project_transfer,
            discover_att_runs,
            diagnostic_context,
            export_diagnostics,
            save_att_manifest,
            cancel_import,
            read_wave,
            read_wave_window,
            read_wave_overview,
            begin_wave_query,
            cancel_wave_query,
            read_wave_stats,
            analyze_hidden,
            close_capture,
            import_definition_files,
            export_definition_file,
            export_plot,
            begin_timeline_export,
            append_timeline_export,
            finish_timeline_export,
            abort_timeline_export,
            import_source_file,
            import_source_directory,
            read_source,
            read_source_page,
            read_source_reference_page
        ]))
        .build(tauri::generate_context!())
        .expect("Tauri runtime failed")
        .run(move |app, event| {
            if was_headless {
                if let tauri::RunEvent::WindowEvent { label, event: tauri::WindowEvent::CloseRequested { api, .. }, .. } = &event {
                    if label == "main" {
                        api.prevent_close();
                        if let Some(window) = app.get_webview_window("main") { let _ = window.hide(); }
                    }
                }
            }
            if matches!(event, tauri::RunEvent::Exit) {
                mcp::shutdown(&shutdown_control, &shutdown_tuning);
            }
        });
}
