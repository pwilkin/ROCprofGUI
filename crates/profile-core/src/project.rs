use crate::Capture;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
pub const MAX_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_STAGED_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_PAGED_STAGED_BYTES: usize = 6 * MAX_STAGED_BYTES;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    pub wave_id: String,
    pub instruction: i64,
    pub iteration: u64,
    pub range: [f64; 2],
    #[serde(default)]
    pub timeline: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CounterBinding {
    pub index: usize,
    pub name: String,
    pub scope: String,
    pub unit: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScalarDefinition {
    pub name: String,
    pub formula: String,
    pub bindings: Vec<CounterBinding>,
}
fn default_events_layer() -> bool {
    true
}
fn default_event_groups() -> u8 {
    47
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GlobalLayers {
    pub residency: bool,
    pub dispatches: bool,
    pub markers: bool,
    #[serde(default = "default_events_layer")]
    pub events: bool,
    #[serde(default = "default_event_groups")]
    pub event_groups: u8,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TimelinePreferences {
    pub zoom_mode: String,
    pub detail_pixels: i8,
    pub stacked: bool,
    pub lane_height: u8,
}
impl TimelinePreferences {
    fn valid(&self) -> bool {
        ["continuous", "mip"].contains(&self.zoom_mode.as_str())
            && [-1, 0, 2, 4, 8].contains(&self.detail_pixels)
            && [20, 28, 42, 56].contains(&self.lane_height)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IsaState {
    pub search: String,
    pub fold: bool,
    pub collapsed: Vec<i64>,
    pub widths: Vec<f64>,
    #[serde(
        default,
        rename = "hiddenColumns",
        skip_serializing_if = "Option::is_none"
    )]
    pub hidden_columns: Option<Vec<String>>,
    pub sort: bool,
    pub page: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DockLayout {
    pub pinned: Vec<String>,
    pub arrangement: String,
    pub split: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree: Option<DockTree>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum DockTree {
    Panel {
        view: String,
    },
    Split {
        axis: String,
        ratio: u8,
        first: Box<DockTree>,
        second: Box<DockTree>,
    },
}
impl DockTree {
    fn validate<'a>(
        &'a self,
        depth: usize,
        leaves: &mut std::collections::BTreeSet<&'a str>,
    ) -> bool {
        if depth > 16 {
            return false;
        }
        match self {
            Self::Panel { view } => leaves.insert(view),
            Self::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                ["horizontal", "vertical"].contains(&axis.as_str())
                    && (20..=80).contains(ratio)
                    && first.validate(depth + 1, leaves)
                    && second.validate(depth + 1, leaves)
            }
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceState {
    pub location: Location,
    pub view: String,
    pub mode: String,
    pub timeline: String,
    #[serde(default)]
    pub independent_ranges: bool,
    #[serde(default)]
    pub timeline_ranges: BTreeMap<String, [f64; 2]>,
    #[serde(default)]
    pub timeline_preferences: BTreeMap<String, TimelinePreferences>,
    #[serde(default)]
    pub dock_layout: Option<DockLayout>,
    #[serde(default)]
    pub analysis_preferences: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub isa_state: Option<IsaState>,
    #[serde(default)]
    pub plot_axes: BTreeMap<String, [f64; 2]>,
    #[serde(default)]
    pub plot_ranges: BTreeMap<String, [f64; 2]>,
    #[serde(default)]
    pub histogram_bins: Option<u8>,
    #[serde(default)]
    pub cu_event_groups: Option<u8>,
    #[serde(default)]
    pub cu_events: Option<bool>,
    #[serde(default)]
    pub global_layers: Option<GlobalLayers>,
    #[serde(default)]
    pub scalar_counters: Vec<ScalarDefinition>,
    pub nonhidden: bool,
    pub include_idle: bool,
    pub history: Vec<Location>,
    pub history_index: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceReplacement {
    pub name: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<crate::source_files::SourceReference>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEntry {
    pub input: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_wave_states: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pc_sampling_method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<crate::input_fingerprint::InputFingerprint>,
    pub state: WorkspaceState,
    pub sources: BTreeMap<String, SourceReplacement>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DefinitionDraft {
    pub id: String,
    pub name: String,
    pub text: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DefinitionLibrary {
    pub version: u32,
    pub selected: String,
    pub files: Vec<DefinitionDraft>,
}
impl DefinitionLibrary {
    fn validate(&self) -> Result<(), String> {
        let mut ids = std::collections::BTreeSet::new();
        let mut names = std::collections::BTreeSet::new();
        if self.version != 1 || self.files.is_empty() || self.files.len() > 32 {
            return Err("Invalid project definition library".into());
        }
        let mut total = 0;
        for file in &self.files {
            if file.id.is_empty()
                || file.id.len() > 8192
                || !ids.insert(&file.id)
                || file.name.trim().is_empty()
                || file.name.encode_utf16().count() > 120
                || file
                    .name
                    .chars()
                    .any(|c| c == '/' || c == '\\' || (c as u32) < 32)
                || !names.insert(&file.name)
                || file.text.contains('\0')
                || file.text.len() > 256 * 1024
            {
                return Err("Invalid project definition file".into());
            }
            total += file.text.len();
        }
        if total > 2 * 1024 * 1024 || !ids.contains(&self.selected) {
            return Err(
                "Project definition library exceeds limits or has missing selection".into(),
            );
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub schema_version: u32,
    #[serde(default)]
    pub definition_library: Option<DefinitionLibrary>,
    pub active: usize,
    pub entries: Vec<ProjectEntry>,
}
const TIMELINES: &[&str] = &["wave", "cu", "util", "global", "dispatch"];
fn location_valid(l: &Location) -> bool {
    l.wave_id.len() <= 8192
        && l.range.iter().all(|v| v.is_finite())
        && l.range[1] > l.range[0]
        && l.iteration <= 1_000_000
        && l.timeline
            .as_ref()
            .is_none_or(|t| TIMELINES.contains(&t.as_str()))
}
fn valid_analysis_preference(key: &str, value: &serde_json::Value) -> bool {
    let text = || {
        value
            .as_str()
            .is_some_and(|s| s.len() <= 8192 && !s.contains('\0'))
    };
    match key {
        "att.associations" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Association {
                path: String,
                #[serde(default)]
                relocation: String,
                mode: String,
                dispatch: String,
            }
            let Some(text) = value.as_str().filter(|text| text.len() <= 1024 * 1024) else {
                return false;
            };
            let Ok(rows) = serde_json::from_str::<Vec<Association>>(text) else {
                return false;
            };
            let mut paths = std::collections::HashSet::new();
            rows.len() <= 64
                && rows.iter().all(|row| {
                    [&row.path, &row.dispatch]
                        .iter()
                        .all(|text| text.len() <= 8192 && !text.contains('\0'))
                        && row.path.starts_with('/')
                        && !row.path.starts_with("//")
                        && row.path.to_ascii_lowercase().ends_with(".rocprof-att")
                        && ["unknown", "single-dispatch", "region"].contains(&row.mode.as_str())
                        && (row.mode == "single-dispatch") == !row.dispatch.is_empty()
                        && (row.relocation.is_empty()
                            || (row.relocation.len() <= 8192
                                && !row.relocation.contains('\0')
                                && row.relocation.starts_with('/')
                                && !row.relocation.starts_with("//")
                                && row
                                    .relocation
                                    .to_ascii_lowercase()
                                    .ends_with(".rocprof-att")))
                        && paths.insert(&row.path)
                })
        }
        "objects.relocations" => {
            let Some(text) = value.as_str().filter(|text| text.len() <= 1024 * 1024) else {
                return false;
            };
            let Ok(rows) = serde_json::from_str::<Vec<[String; 3]>>(text) else {
                return false;
            };
            let mut keys = std::collections::HashSet::new();
            rows.len() <= 256
                && rows.iter().all(|row| {
                    row.iter()
                        .all(|item| !item.is_empty() && item.len() <= 8192 && !item.contains('\0'))
                        && row[1].starts_with("file://")
                        && row[2].starts_with('/')
                        && !row[2].starts_with("//")
                        && keys.insert((&row[0], &row[1]))
                })
        }
        "plot.lodBias" => value.as_i64().is_some_and(|n| (-20..=20).contains(&n)),
        "hardware.selected" | "sampling.search" | "markers.scope" | "dispatch.search"
        | "host.search" | "host.selected" | "objects.search" | "counters.search"
        | "counters.formula" | "counters.name" | "events.search" | "monitor.search"
        | "tensor.scope" | "source.location" => text(),
        "objects.page" | "sampling.page" | "markers.page" | "summary.page" | "host.page"
        | "host.linkPage" | "events.page" | "monitor.page" => {
            value.as_u64().is_some_and(|v| v <= 1_000_000)
        }
        "sampling.selected" | "markers.selected" => {
            value.is_null()
                || value
                    .as_i64()
                    .is_some_and(|v| v.unsigned_abs() <= 9_007_199_254_740_991)
        }
        "sampling.flame"
        | "summary.counts"
        | "flame.hierarchy"
        | "source.lineNumbers"
        | "source.includeHidden"
        | "counters.waveStates" => value.is_boolean(),
        "source.costWidth" => value.as_u64().is_some_and(|n| n <= 400),
        "source.costMetric" => value
            .as_str()
            .is_some_and(|s| ["latency", "pcSamples", "pcIssued", "pcStalled"].contains(&s)),
        "hardware.cursor" => value
            .as_f64()
            .is_some_and(|v| v.is_finite() && (0.0..=1.0).contains(&v)),
        "sampling.metric" => value
            .as_str()
            .is_some_and(|s| ["pcSamples", "pcIssued", "pcStalled"].contains(&s)),
        "sampling.order" => value
            .as_str()
            .is_some_and(|s| ["samples", "issued", "stalled"].contains(&s)),
        "dispatch.sort" => value
            .as_str()
            .is_some_and(|s| ["total", "count", "name"].contains(&s)),
        "flame.latency" | "flame.pcSamples" | "flame.pcIssued" | "flame.pcStalled" => {
            value.is_null() || text()
        }
        _ => false,
    }
}
impl Project {
    pub fn validate(&self) -> Result<(), String> {
        if ![1, 2, 3, 4, 5].contains(&self.schema_version) {
            return Err("Unsupported project schema version".into());
        }
        if self.entries.is_empty()
            || self.entries.len() > 7
            || self.active >= self.entries.len()
            || self.entries.iter().filter(|e| e.input.is_none()).count() > 1
            || self.entries.iter().filter(|e| e.input.is_some()).count() > 6
        {
            return Err("Invalid project capture count or active tab".into());
        }
        if let Some(library) = &self.definition_library {
            library.validate()?;
        }
        for e in &self.entries {
            if let Some(method) = &e.pc_sampling_method {
                if self.schema_version < 5
                    || !["host-trap", "stochastic"].contains(&method.as_str())
                    || !e
                        .input
                        .as_ref()
                        .is_some_and(|p| p.extension().is_some_and(|e| e == "json"))
                {
                    return Err("PC sampling selection requires schema 5, a supported method and a JSON input".into());
                }
            }
            if e.load_wave_states.is_some() && (self.schema_version < 4 || e.input.is_none()) {
                return Err(
                    "Wave-state import preferences require schema 4 and a capture path".into(),
                );
            }
            if let Some(fingerprint) = &e.fingerprint {
                if self.schema_version < 3 || e.input.is_none() {
                    return Err("Input fingerprints require schema 3 and a capture path".into());
                }
                fingerprint.validate()?;
            }
            let s = &e.state;
            if ![
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
            ]
            .contains(&s.view.as_str())
                || ![
                    "capture",
                    "capture-mean",
                    "wave",
                    "all",
                    "all-mean",
                    "mean",
                    "max",
                    "iteration",
                ]
                .contains(&s.mode.as_str())
                || !TIMELINES.contains(&s.timeline.as_str())
                || !location_valid(&s.location)
                || s.history.len() > 4096
                || !s.history.iter().all(location_valid)
                || s.history_index < -1
                || s.history_index >= s.history.len() as i64
                || s.scalar_counters.len() > 64
                || s.scalar_counters.iter().any(|d| {
                    d.name.trim().is_empty()
                        || d.name.len() > 1024
                        || d.formula.len() > 16384
                        || d.name.contains('\0')
                        || d.formula.contains('\0')
                        || d.bindings.is_empty()
                        || d.bindings.len() > 1024
                        || d.bindings.iter().any(|b| {
                            b.index > 2_000_000
                                || b.name.len() > 8192
                                || b.scope.len() > 8192
                                || b.unit.len() > 8192
                        })
                })
                || s.plot_axes.len() > 4096
                || s.global_layers
                    .as_ref()
                    .is_some_and(|layers| layers.event_groups > 63)
                || s.cu_event_groups.is_some_and(|mask| mask > 63)
                || s.histogram_bins
                    .is_some_and(|n| ![8, 16, 32, 64].contains(&n))
                || s.plot_ranges.len() > 4096
                || s.plot_axes
                    .iter()
                    .chain(s.plot_ranges.iter())
                    .any(|(key, r)| {
                        key.is_empty()
                            || key.len() > 8192
                            || key.contains('\0')
                            || !r.iter().all(|v| v.is_finite())
                            || r[1] <= r[0]
                            || !(r[1] - r[0]).is_finite()
                    })
                || s.timeline_ranges.iter().any(|(k, r)| {
                    !TIMELINES.contains(&k.as_str())
                        || !r.iter().all(|v| v.is_finite())
                        || r[1] <= r[0]
                })
            {
                return Err("Invalid project navigation state".into());
            }
            if e.input
                .as_ref()
                .is_some_and(|p| p.as_os_str().is_empty() || p.as_os_str().len() > 8192)
            {
                return Err("Invalid project input path".into());
            }
            if e.state
                .timeline_preferences
                .iter()
                .any(|(view, preferences)| {
                    (!TIMELINES.contains(&view.as_str())
                        && !["markers", "host-calls"].contains(&view.as_str()))
                        || !preferences.valid()
                })
            {
                return Err("Invalid project timeline preferences".into());
            }
            if e.state
                .analysis_preferences
                .iter()
                .any(|(key, value)| !valid_analysis_preference(key, value))
            {
                return Err("Invalid project analysis preferences".into());
            }
            if let Some(layout) = &e.state.dock_layout {
                let allowed = [
                    "trace-wave",
                    "trace-cu",
                    "trace-util",
                    "trace-global",
                    "trace-dispatch",
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
                let unique: std::collections::BTreeSet<_> = layout.pinned.iter().collect();
                if layout.pinned.len() > allowed.len()
                    || unique.len() != layout.pinned.len()
                    || layout.pinned.iter().any(|v| !allowed.contains(&v.as_str()))
                    || !["stacked", "columns", "custom"].contains(&layout.arrangement.as_str())
                    || !(25..=75).contains(&layout.split)
                {
                    return Err("Invalid project panel layout".into());
                }
                if let Some(tree) = &layout.tree {
                    let mut leaves = std::collections::BTreeSet::new();
                    if !tree.validate(0, &mut leaves)
                        || leaves != layout.pinned.iter().map(String::as_str).collect()
                    {
                        return Err("Invalid project panel split tree".into());
                    }
                } else if layout.arrangement == "custom" && !layout.pinned.is_empty() {
                    return Err("Nested panel layout requires a split tree".into());
                }
            }
            if let Some(isa) = &e.state.isa_state {
                if isa.search.len() > 8192
                    || isa.search.contains('\0')
                    || isa.collapsed.len() > 100000
                    || ![7, 12].contains(&isa.widths.len())
                    || isa.hidden_columns.as_ref().is_some_and(|columns| {
                        let allowed = [
                            "line",
                            "address",
                            "hits",
                            "cycles",
                            "stall",
                            "samples",
                            "idle",
                            "issued",
                            "stalled",
                            "codeObject",
                            "source",
                        ];
                        columns.len() > allowed.len()
                            || columns.iter().any(|key| !allowed.contains(&key.as_str()))
                            || columns
                                .iter()
                                .collect::<std::collections::BTreeSet<_>>()
                                .len()
                                != columns.len()
                    })
                    || isa
                        .widths
                        .iter()
                        .any(|n| !n.is_finite() || !(40.0..=1000.0).contains(n))
                    || isa.page > 1000000
                {
                    return Err("Invalid project ISA view state".into());
                }
            }
            if e.sources.len() > 10000 {
                return Err("Too many project source mappings".into());
            }
            let mut bytes = 0;
            for (path, file) in &e.sources {
                if let Some(reference) = &file.reference {
                    if self.schema_version < 2 || !file.text.is_empty() {
                        return Err(
                            "Source references require project schema 2 and no embedded text"
                                .into(),
                        );
                    }
                    reference.validate()?;
                }
                if path.is_empty()
                    || path.len() > 8192
                    || path.contains('\0')
                    || file.name.len() > 8192
                    || file.text.contains('\0')
                    || file.text.len() > 2 * 1024 * 1024
                {
                    return Err("Invalid project source replacement".into());
                }
                bytes += file.text.len();
            }
            if bytes > 16 * 1024 * 1024 {
                return Err("Project source mappings exceed 16 MiB per capture".into());
            }
        }
        Ok(())
    }
}
pub fn read(path: &Path) -> Result<Project, String> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Project must be a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > MAX_BYTES {
        return Err("Project exceeds 128 MiB".into());
    }
    let mut project: Project =
        serde_json::from_slice(&bytes).map_err(|e| format!("Invalid project: {e}"))?;
    project.validate()?;
    for entry in &mut project.entries {
        if let Some(input) = &mut entry.input {
            if input.is_relative() {
                *input = path
                    .parent()
                    .ok_or("Missing project directory")?
                    .join(&input);
            }
        }
    }
    Ok(project)
}
pub fn write(path: &Path, project: &Project) -> Result<(), String> {
    project.validate()?;
    let bytes = serde_json::to_vec(project).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_BYTES {
        return Err("Project exceeds 128 MiB".into());
    }
    let mut file =
        tempfile::NamedTempFile::new_in(path.parent().ok_or("Missing project directory")?)
            .map_err(|e| e.to_string())?;
    file.write_all(&bytes).map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
pub fn load_input(path: &Path, cancelled: Arc<AtomicBool>) -> Result<Capture, String> {
    load_input_checked(path, cancelled, None, true, None)
}
pub fn load_input_with_wave_states(
    path: &Path,
    cancelled: Arc<AtomicBool>,
    load_wave_states: bool,
) -> Result<Capture, String> {
    load_input_checked(path, cancelled, None, load_wave_states, None)
}
pub fn load_input_options(
    path: &Path,
    cancelled: Arc<AtomicBool>,
    load_wave_states: bool,
    sampling_method: Option<&str>,
) -> Result<Capture, String> {
    load_input_checked(path, cancelled, None, load_wave_states, sampling_method)
}
fn load_input_checked(
    path: &Path,
    cancelled: Arc<AtomicBool>,
    expected: Option<&crate::input_fingerprint::InputFingerprint>,
    load_wave_states: bool,
    sampling_method: Option<&str>,
) -> Result<Capture, String> {
    if cancelled.load(Ordering::Relaxed) {
        return Err("Import cancelled".into());
    }
    let path =
        fs::canonicalize(path).map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
    if sampling_method.is_some() && !path.extension().is_some_and(|e| e == "json") {
        return Err("PC sampling selection requires raw SDK JSON".into());
    }
    let fingerprint = crate::input_fingerprint::compute(&path, &cancelled)?;
    if expected.is_some_and(|expected| expected != &fingerprint) {
        return Err("Project capture input changed since import; open the capture separately to inspect the new data".into());
    }
    let mut capture = if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("rocprof-capture"))
    {
        crate::capture_archive::read(&path, &cancelled)
    } else if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("rgp"))
    {
        crate::rgp::load_cancellable(&path, &cancelled)
    } else if (path.is_dir() && !path.join("filenames.json").exists())
        || path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("att") || e.eq_ignore_ascii_case("rocprof-att"))
    {
        crate::decoder::decode_with_wave_states(
            &path,
            &Default::default(),
            &cancelled,
            load_wave_states,
        )
    } else if path
        .extension()
        .is_some_and(|e| e == "rpd" || e == "rocpd" || e == "db" || e == "sqlite")
    {
        crate::rocpd::load_cancellable(&path, cancelled.clone())
    } else if path.extension().is_some_and(|e| e == "csv") {
        crate::import::load_csv_cancellable(&path, &cancelled)
    } else {
        crate::load_capture_options(&path, load_wave_states, sampling_method)
    }?;
    if fingerprint != crate::input_fingerprint::compute(&path, &cancelled)? {
        return Err("Capture inputs changed during import".into());
    }
    capture.metadata.insert(
        "inputIdentity".into(),
        "SHA-256 file-set fingerprint verified around import; later file changes are not monitored"
            .into(),
    );
    capture.input_fingerprint = Some(fingerprint);
    capture.input_path = Some(path);
    Ok(capture)
}
pub fn stage(project: &Project, cancelled: Arc<AtomicBool>) -> Result<Vec<Capture>, String> {
    stage_with_limit(project, cancelled, MAX_STAGED_BYTES)
}

pub fn stage_with_limit(
    project: &Project,
    cancelled: Arc<AtomicBool>,
    max_bytes: usize,
) -> Result<Vec<Capture>, String> {
    project.validate()?;
    let mut budget = PayloadBudget::new(max_bytes, &cancelled);
    budget.add(project)?;
    let mut captures = Vec::new();
    for (index, entry) in project.entries.iter().enumerate() {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Project import cancelled".into());
        }
        let capture = match &entry.input {
            Some(path) => load_input_checked(
                path,
                cancelled.clone(),
                entry.fingerprint.as_ref(),
                entry.load_wave_states.unwrap_or(true),
                entry.pc_sampling_method.as_deref(),
            )?,
            None => {
                let bundle: serde_json::Value =
                    serde_json::from_str(include_str!("../../../public/demo.json"))
                        .map_err(|e| e.to_string())?;
                serde_json::from_value(bundle["capture"].clone()).map_err(|e| e.to_string())?
            }
        };
        budget
            .add(&capture)
            .map_err(|e| format!("Capture {}: {e}", index + 1))?;
        captures.push(capture);
    }
    Ok(captures)
}

struct PayloadBudget<'a> {
    remaining: usize,
    limit: usize,
    cancelled: &'a AtomicBool,
}
impl<'a> PayloadBudget<'a> {
    fn new(limit: usize, cancelled: &'a AtomicBool) -> Self {
        Self {
            remaining: limit,
            limit,
            cancelled,
        }
    }
    fn add(&mut self, value: &impl Serialize) -> Result<(), String> {
        serde_json::to_writer(self, value).map_err(|e| e.to_string())
    }
}
impl Write for PayloadBudget<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(std::io::Error::other("Project import cancelled"));
        }
        if bytes.len() > self.remaining {
            return Err(std::io::Error::other(format!(
                "Project staged JSON payload exceeds {} bytes; open fewer captures separately",
                self.limit
            )));
        }
        self.remaining -= bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub fn check_staged_payload<T: Serialize>(
    payload: &T,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    PayloadBudget::new(MAX_STAGED_BYTES, cancelled).add(payload)
}

pub fn reconcile(project: &mut Project, captures: &mut [Capture]) {
    for (entry, capture) in project.entries.iter_mut().zip(captures) {
        let mut changed = false;
        let clamp = |r: &mut [f64; 2]| {
            let old = *r;
            r[0] = r[0].max(capture.start);
            r[1] = r[1].min(capture.end);
            if r[1] <= r[0] {
                *r = [capture.start, capture.end];
            }
            old != *r
        };
        let mut location = |l: &mut Location| {
            changed |= clamp(&mut l.range);
            if !capture.waves.iter().any(|w| w.id == l.wave_id)
                && (!l.wave_id.is_empty() || !capture.waves.is_empty())
            {
                l.wave_id = capture
                    .waves
                    .first()
                    .map(|w| w.id.clone())
                    .unwrap_or_default();
                l.iteration = 0;
                changed = true;
            }
            if !capture
                .instructions
                .iter()
                .any(|i| i.index == l.instruction)
                && !capture.instructions.is_empty()
            {
                l.instruction = capture.instructions[0].index;
                l.iteration = 0;
                changed = true;
            }
        };
        location(&mut entry.state.location);
        for l in &mut entry.state.history {
            location(l);
        }
        for r in entry.state.timeline_ranges.values_mut() {
            changed |= clamp(r);
        }
        capture.metadata.insert("projectRestore".into(), if entry.fingerprint.is_some() {
            "Capture inputs match saved fingerprints; source replacements remain revision-unverified"
        } else { "Inputs re-imported from live references; source replacements remain revision-unverified" }.into());
        if changed {
            capture.warnings.push("Project selections or ranges no longer match the input; unavailable locations were reset".into());
        }
    }
}

#[cfg(test)]
mod payload_tests {
    use super::*;

    #[test]
    fn counting_writer_handles_json_escaping_and_midstream_cancellation() {
        let cancelled = AtomicBool::new(false);
        let text = "é\n\"\\";
        let bytes = serde_json::to_vec(text).unwrap();
        let mut budget = PayloadBudget::new(bytes.len(), &cancelled);
        budget.add(&text).unwrap();
        assert_eq!(budget.remaining, 0);
        assert!(budget.add(&0).unwrap_err().contains("exceeds"));
        let mut budget = PayloadBudget::new(100, &cancelled);
        budget.add(&text).unwrap();
        cancelled.store(true, Ordering::Relaxed);
        assert!(budget.add(&text).unwrap_err().contains("cancelled"));
    }
}
