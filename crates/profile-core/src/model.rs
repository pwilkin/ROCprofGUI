use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capture {
    #[serde(default)]
    pub pc_sampling_observations: Option<std::sync::Arc<PcSamplingObservations>>,
    pub id: String,
    pub name: String,
    pub format: String,
    pub architecture: String,
    pub unit: String,
    pub origin: String,
    pub start: f64,
    pub end: f64,
    pub warnings: Vec<String>,
    pub waves: Vec<WaveIndex>,
    pub intervals: Vec<Interval>,
    pub instructions: Vec<Instruction>,
    pub counters: Vec<Series>,
    #[serde(default)]
    pub event_counters: Vec<EventCounter>,
    #[serde(default)]
    pub monitor_intervals: Vec<MonitorInterval>,
    #[serde(default)]
    pub host_calls: Vec<HostCall>,
    pub dispatches: Vec<Dispatch>,
    #[serde(default)]
    pub code_objects: Vec<CodeObjectRecord>,
    #[serde(default)]
    pub realtime: Option<RealtimeCalibration>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub se_time_offsets: BTreeMap<u32, i64>,
    #[serde(default)]
    pub decoder_events: Vec<DecoderEvent>,
    #[serde(default)]
    pub other_simd: Vec<OtherSimd>,
    pub sources: BTreeMap<String, String>,
    #[serde(default)]
    pub source_files: Vec<String>,
    #[serde(skip)]
    pub source_paths: BTreeMap<String, PathBuf>,
    pub metadata: BTreeMap<String, String>,
    #[serde(default)]
    pub markers: Vec<crate::markers::Marker>,
    /// Version-zero Loom compile reports: static, compile-time kernel facts kept
    /// separate from dispatches, counters and waves so they cannot be read as
    /// measured data. Empty for every other input format.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compile_reports: Vec<crate::loom_report::LoomCompileReport>,
    #[serde(skip)]
    pub root: PathBuf,
    #[serde(skip)]
    pub input_path: Option<PathBuf>,
    #[serde(skip)]
    pub input_fingerprint: Option<crate::input_fingerprint::InputFingerprint>,
    #[serde(skip)]
    pub workspace: Option<std::sync::Arc<tempfile::TempDir>>,
}
impl Capture {
    pub(crate) fn clone_without_instructions(&self) -> Self {
        Self {
            pc_sampling_observations: self.pc_sampling_observations.clone(),
            id: self.id.clone(),
            name: self.name.clone(),
            format: self.format.clone(),
            architecture: self.architecture.clone(),
            unit: self.unit.clone(),
            origin: self.origin.clone(),
            start: self.start,
            end: self.end,
            warnings: self.warnings.clone(),
            waves: self.waves.clone(),
            intervals: self.intervals.clone(),
            instructions: Vec::new(),
            counters: self.counters.clone(),
            event_counters: self.event_counters.clone(),
            monitor_intervals: self.monitor_intervals.clone(),
            host_calls: self.host_calls.clone(),
            dispatches: self.dispatches.clone(),
            code_objects: self.code_objects.clone(),
            realtime: self.realtime.clone(),
            se_time_offsets: self.se_time_offsets.clone(),
            decoder_events: self.decoder_events.clone(),
            other_simd: self.other_simd.clone(),
            sources: self.sources.clone(),
            source_files: self.source_files.clone(),
            source_paths: self.source_paths.clone(),
            metadata: self.metadata.clone(),
            markers: self.markers.clone(),
            compile_reports: self.compile_reports.clone(),
            root: self.root.clone(),
            input_path: self.input_path.clone(),
            input_fingerprint: self.input_fingerprint.clone(),
            workspace: self.workspace.clone(),
        }
    }
    pub fn empty(name: String, format: &str, unit: &str) -> Self {
        Self {
            pc_sampling_observations: None,
            id: String::new(),
            name,
            format: format.into(),
            architecture: "Unknown".into(),
            unit: unit.into(),
            origin: "0".into(),
            start: 0.,
            end: 1.,
            warnings: vec![],
            waves: vec![],
            intervals: vec![],
            instructions: vec![],
            counters: vec![],
            event_counters: vec![],
            monitor_intervals: vec![],
            host_calls: vec![],
            dispatches: vec![],
            code_objects: vec![],
            realtime: None,
            se_time_offsets: BTreeMap::new(),
            decoder_events: vec![],
            other_simd: vec![],
            sources: BTreeMap::new(),
            source_files: vec![],
            source_paths: BTreeMap::new(),
            metadata: BTreeMap::new(),
            markers: vec![],
            compile_reports: vec![],
            root: PathBuf::new(),
            input_path: None,
            input_fingerprint: None,
            workspace: None,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RealtimeCalibration {
    pub frequency_hz: Option<String>,
    pub anchors: Vec<RealtimeAnchor>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RealtimeAnchor {
    pub se: u32,
    pub shader_clock: String,
    pub realtime_clock: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WaveIndex {
    pub id: String,
    pub se: u32,
    pub simd: u32,
    pub slot: u32,
    pub wid: u32,
    pub start: f64,
    pub end: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Interval {
    pub start: f64,
    pub end: f64,
    pub se: u32,
    pub cu: u32,
    pub simd: u32,
    pub slot: u32,
    pub kernel: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Instruction {
    pub index: i64,
    pub text: String,
    pub source: String,
    pub address: String,
    pub code_object: String,
    pub hits: u64,
    pub latency: f64,
    pub stall: f64,
    pub idle: f64,
    pub pc_samples: u64,
    #[serde(default)]
    pub pc_issued: Option<u64>,
    #[serde(default)]
    pub pc_stalled: Option<u64>,
    #[serde(default)]
    pub stall_reasons: Option<Vec<u64>>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostCall {
    pub id: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub link_basis: Option<String>,
    pub pid: String,
    pub tid: String,
    pub name: String,
    pub arguments: Option<String>,
    pub start: f64,
    pub end: f64,
    pub operations: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorInterval {
    pub id: String,
    pub device_type: String,
    pub device_id: String,
    pub name: String,
    pub start: f64,
    pub end: f64,
    pub value: Option<String>,
    pub unit: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventCounter {
    pub id: String,
    pub counter_id: String,
    pub event: Option<String>,
    pub dispatches: Vec<String>,
    pub name: String,
    pub unit: String,
    pub scope: String,
    pub value: Option<f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttCounterSelection {
    pub counter: String,
    pub simd_mask: u8,
    pub simds: Vec<u8>,
    pub explicit: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CounterDefinition {
    pub id: String,
    pub description: String,
    pub block: String,
    pub expression: String,
    pub is_derived: Option<bool>,
    pub is_constant: Option<bool>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Series {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spm_clock: Option<SpmSampleClock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition: Option<CounterDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub att: Option<AttCounterSelection>,
    pub name: String,
    pub unit: String,
    pub scope: String,
    pub points: Vec<[f64; 2]>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpmSampleClock {
    pub anchor_se: u32,
    pub realtime_timestamps: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dispatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shader_engine: Option<u32>,
    pub id: String,
    pub kernel: String,
    pub start: f64,
    pub end: f64,
    pub agent: String,
    pub queue: String,
    pub vgpr: Option<u64>,
    pub sgpr: Option<u64>,
    pub lds: Option<u64>,
    pub scratch: Option<u64>,
    pub grid: String,
    pub workgroup: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Token {
    pub start: f64,
    pub duration: f64,
    pub stall: f64,
    pub category: u32,
    pub instruction: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Wave {
    pub id: String,
    pub start: f64,
    pub end: f64,
    pub cu: Option<u32>,
    pub tokens: Vec<Token>,
    pub states: Vec<[f64; 3]>,
    pub waits: serde_json::Value,
    pub info: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecoderEvent {
    pub id: String,
    pub se: u32,
    pub time: f64,
    pub kind: String,
    pub fields: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OtherSimd {
    pub se: u32,
    pub wgp: Option<u32>,
    pub source: String,
    pub start: f64,
    pub duration: f64,
    pub category: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeObjectRecord {
    pub id: String,
    pub object_id: String,
    pub scope: String,
    pub agent: Option<String>,
    pub uri: Option<String>,
    pub storage: Option<String>,
    pub load_base: Option<String>,
    pub load_size: Option<String>,
    pub load_delta: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PcSamplingObservations {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}
