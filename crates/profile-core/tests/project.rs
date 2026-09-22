use profile_core::{load_wave, project::*};
use std::{
    collections::BTreeMap,
    fs,
    sync::{atomic::AtomicBool, Arc},
};
fn state() -> WorkspaceState {
    let location = Location {
        wave_id: "missing-wave".into(),
        instruction: 999999,
        iteration: 0,
        range: [-100., 1e12],
        timeline: Some("wave".into()),
    };
    WorkspaceState {
        location: location.clone(),
        view: "trace".into(),
        mode: "capture".into(),
        timeline: "wave".into(),
        independent_ranges: true,
        dock_layout: None,
        analysis_preferences: BTreeMap::new(),
        isa_state: None,
        timeline_preferences: BTreeMap::from([(
            "wave".into(),
            TimelinePreferences {
                zoom_mode: "mip".into(),
                detail_pixels: -1,
                stacked: false,
                lane_height: 56,
            },
        )]),
        timeline_ranges: BTreeMap::from([("global".into(), [1., 2.])]),
        scalar_counters: vec![ScalarDefinition {
            name: "ratio".into(),
            formula: "C0 / 2".into(),
            bindings: vec![CounterBinding {
                index: 0,
                name: "cycles".into(),
                scope: "SE0".into(),
                unit: "cycles".into(),
            }],
        }],
        histogram_bins: Some(32),
        cu_event_groups: Some(16),
        cu_events: Some(false),
        global_layers: Some(GlobalLayers {
            residency: false,
            dispatches: true,
            markers: false,
            events: true,
            event_groups: 3,
        }),
        plot_ranges: BTreeMap::from([("occupancy".into(), [1000., 2000.])]),
        plot_axes: BTreeMap::from([("occupancy".into(), [-1., 2.])]),
        nonhidden: false,
        include_idle: false,
        history: vec![location],
        history_index: 0,
    }
}
fn fixture() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/transpose-ui/filenames.json")
        .canonicalize()
        .unwrap()
}
fn project() -> Project {
    Project {
        schema_version: 1,
        definition_library: Some(DefinitionLibrary {
            version: 1,
            selected: "file1".into(),
            files: vec![DefinitionDraft {
                id: "file1".into(),
                name: "custom.def".into(),
                text: "X := 2".into(),
            }],
        }),
        active: 0,
        entries: vec![ProjectEntry {
            load_wave_states: None,
            pc_sampling_method: None,
            input: Some(fixture()),
            fingerprint: None,
            state: state(),
            sources: BTreeMap::from([(
                "/kernel.hip".into(),
                SourceReplacement {
                    name: "replacement.hip".into(),
                    text: "custom source\n".into(),
                    reference: None,
                },
            )]),
        }],
    }
}

#[test]
fn wave_state_import_choice_requires_schema_four_and_survives_reopen() {
    let mut p = project();
    p.entries[0].load_wave_states = Some(false);
    assert!(p.validate().unwrap_err().contains("schema 4"));
    p.schema_version = 4;
    p.validate().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("choice.rocprof-project");
    write(&path, &p).unwrap();
    let restored = read(&path).unwrap();
    assert_eq!(restored.entries[0].load_wave_states, Some(false));
    let captures = stage(&restored, Arc::new(AtomicBool::new(false))).unwrap();
    assert_eq!(captures[0].metadata["loadWaveStates"], "false");
}
#[test]
fn project_roundtrip_reopens_inputs_and_preserves_source_replacements() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("workspace.rocprof-project");
    let p = project();
    write(&path, &p).unwrap();
    let mut restored = read(&path).unwrap();
    let mut captures = stage(&restored, Arc::new(AtomicBool::new(false))).unwrap();
    assert_eq!(captures[0].waves.len(), 24);
    assert_eq!(restored.entries[0].state.histogram_bins, Some(32));
    assert_eq!(
        restored.entries[0].state.timeline_preferences,
        p.entries[0].state.timeline_preferences
    );
    assert_eq!(restored.entries[0].state.cu_event_groups, Some(16));
    assert_eq!(restored.entries[0].state.cu_events, Some(false));
    assert_eq!(
        restored.entries[0].state.global_layers,
        p.entries[0].state.global_layers
    );
    assert_eq!(
        restored.entries[0].state.plot_ranges["occupancy"],
        [1000., 2000.]
    );
    assert_eq!(
        restored.definition_library.as_ref().unwrap().files[0].text,
        "X := 2"
    );
    assert_eq!(
        restored.entries[0].state.scalar_counters[0].formula,
        "C0 / 2"
    );
    assert_eq!(restored.entries[0].state.plot_axes["occupancy"], [-1., 2.]);
    assert_eq!(
        restored.entries[0].sources["/kernel.hip"].text,
        "custom source\n"
    );
    reconcile(&mut restored, &mut captures);
    assert_ne!(restored.entries[0].state.location.wave_id, "missing-wave");
    assert!(captures[0]
        .warnings
        .iter()
        .any(|w| w.contains("Project selections")));
    assert!(load_wave(&captures[0], &restored.entries[0].state.location.wave_id).is_ok());
    let old = fs::read(&path).unwrap();
    restored.schema_version = 99;
    assert!(write(&path, &restored).is_err());
    assert_eq!(fs::read(&path).unwrap(), old);
}
#[test]
fn invalid_state_missing_inputs_and_cancellation_fail_before_replacement() {
    let p = project();
    let old = stage(&p, Arc::new(AtomicBool::new(false))).unwrap();
    let mut invalid_library = p.clone();
    invalid_library
        .definition_library
        .as_mut()
        .unwrap()
        .selected = "missing".into();
    assert!(invalid_library.validate().is_err());
    let mut invalid_bins = p.clone();
    invalid_bins.entries[0].state.histogram_bins = Some(255);
    assert!(invalid_bins.validate().is_err());
    let mut invalid_axes = p.clone();
    invalid_axes.entries[0]
        .state
        .plot_axes
        .insert("bad".into(), [2., 1.]);
    assert!(invalid_axes.validate().is_err());
    let mut invalid = p.clone();
    invalid.entries[0].state.view = "invalid".into();
    assert!(invalid.validate().is_err());
    let mut missing = p.clone();
    missing.entries.push(ProjectEntry {
        load_wave_states: None,
        pc_sampling_method: None,
        input: Some(fixture().join("missing")),
        ..p.entries[0].clone()
    });
    assert!(stage(&missing, Arc::new(AtomicBool::new(false))).is_err());
    assert!(load_wave(&old[0], &old[0].waves[0].id).is_ok());
    assert!(stage(&p, Arc::new(AtomicBool::new(true)))
        .unwrap_err()
        .contains("cancelled"));
    let mut invalid = p.clone();
    invalid.entries[0]
        .sources
        .get_mut("/kernel.hip")
        .unwrap()
        .text = "\0".into();
    assert!(invalid.validate().is_err());
}
#[test]
fn relative_input_paths_and_example_are_supported() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.rocprof-project");
    let mut p = project();
    p.entries[0].input = Some("capture".into());
    write(&path, &p).unwrap();
    assert_eq!(
        read(&path).unwrap().entries[0].input.as_ref().unwrap(),
        &dir.path().join("capture")
    );
    p.entries[0].input = None;
    let captures = stage(&p, Arc::new(AtomicBool::new(false))).unwrap();
    assert!(captures[0].metadata.contains_key("demo"));
    assert!(!captures[0].waves.is_empty());
}

#[test]
fn project_retains_instruction_cost_modes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("costs.rocprof-project");
    for mode in ["capture-mean", "max", "iteration"] {
        let mut p = project();
        p.entries[0].state.mode = mode.into();
        p.entries[0].state.location.iteration = 3;
        write(&path, &p).unwrap();
        let restored = read(&path).unwrap();
        assert_eq!(restored.entries[0].state.mode, mode);
        assert_eq!(restored.entries[0].state.location.iteration, 3);
    }
}

#[test]
fn project_global_layers_are_optional_but_strictly_typed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("layers.rocprof-project");
    let mut value = serde_json::to_value(project()).unwrap();
    value["entries"][0]["state"]
        .as_object_mut()
        .unwrap()
        .remove("globalLayers");
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(read(&path).unwrap().entries[0]
        .state
        .global_layers
        .is_none());
    value["entries"][0]["state"]["globalLayers"] =
        serde_json::json!({"residency": false, "markers": true, "dispatches": true});
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(
        read(&path).unwrap().entries[0]
            .state
            .global_layers
            .as_ref()
            .unwrap()
            .events
    );
    assert_eq!(
        read(&path).unwrap().entries[0]
            .state
            .global_layers
            .as_ref()
            .unwrap()
            .event_groups,
        47
    );
    for mask in [0, 63, 64] {
        value["entries"][0]["state"]["globalLayers"]["eventGroups"] = serde_json::json!(mask);
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(read(&path).is_ok(), mask <= 63);
    }
    value["entries"][0]["state"]["globalLayers"] =
        serde_json::json!({"residency": "false", "markers": true, "dispatches": true});
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(read(&path).is_err());
}

#[test]
fn referenced_sources_require_v2_and_unambiguous_valid_fingerprints() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("kernel.hip");
    fs::write(&source, "abc").unwrap();
    let reference = profile_core::source_index::SourceCache::default()
        .reference(&source)
        .unwrap();
    let mut p = project();
    let replacement = p.entries[0].sources.get_mut("/kernel.hip").unwrap();
    replacement.text.clear();
    replacement.reference = Some(reference);
    assert!(p.validate().is_err());
    p.schema_version = 2;
    let path = dir.path().join("v2.rocprof-project");
    write(&path, &p).unwrap();
    let restored = read(&path).unwrap();
    let reference = restored.entries[0].sources["/kernel.hip"]
        .reference
        .as_ref()
        .unwrap();
    assert_eq!(
        reference.sha256,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(reference.path, source.to_str().unwrap());
    p.entries[0].sources.get_mut("/kernel.hip").unwrap().text = "ambiguous".into();
    assert!(p.validate().is_err());
    p.entries[0]
        .sources
        .get_mut("/kernel.hip")
        .unwrap()
        .text
        .clear();
    p.entries[0]
        .sources
        .get_mut("/kernel.hip")
        .unwrap()
        .reference
        .as_mut()
        .unwrap()
        .sha256 = "bad".into();
    assert!(p.validate().is_err());
}

#[test]
fn timeline_preferences_are_optional_but_validate_each_view_and_option() {
    let mut value = serde_json::to_value(project()).unwrap();
    value["entries"][0]["state"]
        .as_object_mut()
        .unwrap()
        .remove("timelinePreferences");
    let old: Project = serde_json::from_value(value).unwrap();
    old.validate().unwrap();
    assert!(old.entries[0].state.timeline_preferences.is_empty());
    for (field, invalid) in [
        ("zoomMode", serde_json::json!("invalid")),
        ("laneHeight", serde_json::json!(21)),
        ("detailPixels", serde_json::json!(3)),
        ("stacked", serde_json::json!("false")),
    ] {
        let mut value = serde_json::to_value(project()).unwrap();
        value["entries"][0]["state"]["timelinePreferences"]["wave"][field] = invalid;
        assert!(serde_json::from_value::<Project>(value)
            .map(|p| p.validate().is_err())
            .unwrap_or(true));
    }
    let mut p = project();
    let preferences = p.entries[0]
        .state
        .timeline_preferences
        .remove("wave")
        .unwrap();
    p.entries[0]
        .state
        .timeline_preferences
        .insert("unknown".into(), preferences);
    assert!(p.validate().is_err());
}

#[test]
fn dock_layout_roundtrips_and_rejects_invalid_panels_and_geometry() {
    let mut p = project();
    p.entries[0].state.dock_layout = Some(DockLayout {
        pinned: vec!["trace-cu".into(), "trace-global".into(), "occupancy".into()],
        arrangement: "columns".into(),
        split: 65,
        tree: None,
    });
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("docked.rocprof-project");
    write(&path, &p).unwrap();
    let restored = read(&path).unwrap();
    let layout = restored.entries[0].state.dock_layout.as_ref().unwrap();
    assert_eq!(layout.pinned, ["trace-cu", "trace-global", "occupancy"]);
    assert_eq!(layout.split, 65);
    for invalid in [
        serde_json::json!({"pinned":["trace","trace"],"arrangement":"columns","split":50}),
        serde_json::json!({"pinned":["reference"],"arrangement":"columns","split":50}),
        serde_json::json!({"pinned":["trace"],"arrangement":"floating","split":50}),
        serde_json::json!({"pinned":["trace"],"arrangement":"columns","split":99}),
    ] {
        let mut value = serde_json::to_value(&p).unwrap();
        value["entries"][0]["state"]["dockLayout"] = invalid;
        assert!(serde_json::from_value::<Project>(value)
            .unwrap()
            .validate()
            .is_err());
    }
}

#[test]
fn isa_view_state_roundtrips_and_rejects_invalid_column_sizes() {
    let mut p = project();
    p.entries[0].state.isa_state = Some(IsaState {
        search: "v_add".into(),
        fold: false,
        collapsed: vec![10, 30],
        widths: vec![48., 85., 410., 70., 85., 75., 95.],
        hidden_columns: None,
        sort: true,
        page: 3,
    });
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("isa.rocprof-project");
    write(&path, &p).unwrap();
    let restored = read(&path).unwrap();
    let isa = restored.entries[0].state.isa_state.as_ref().unwrap();
    assert_eq!(isa.search, "v_add");
    assert_eq!(isa.collapsed, [10, 30]);
    assert_eq!(isa.page, 3);
    assert!(isa.hidden_columns.is_none());
    {
        let isa = p.entries[0].state.isa_state.as_mut().unwrap();
        isa.widths.extend([85., 95., 95., 120., 240.]);
        isa.hidden_columns = Some(vec!["address".into(), "hits".into()]);
    }
    write(&path, &p).unwrap();
    let restored = read(&path).unwrap();
    assert_eq!(
        restored.entries[0]
            .state
            .isa_state
            .as_ref()
            .unwrap()
            .hidden_columns,
        Some(vec!["address".into(), "hits".into()])
    );
    for keys in [vec!["instruction"], vec!["unknown"], vec!["hits", "hits"]] {
        p.entries[0]
            .state
            .isa_state
            .as_mut()
            .unwrap()
            .hidden_columns = Some(keys.into_iter().map(str::to_string).collect());
        assert!(p.validate().is_err());
    }
    p.entries[0]
        .state
        .isa_state
        .as_mut()
        .unwrap()
        .hidden_columns = Some(vec![]);
    assert!(p.validate().is_ok());
    for width in [0., 39., 1001., f64::INFINITY] {
        p.entries[0].state.isa_state.as_mut().unwrap().widths[2] = width;
        assert!(p.validate().is_err());
    }
}

#[test]
fn staged_payload_budget_counts_all_captures_and_honors_cancellation() {
    let mut p = project();
    p.entries[0].input = None;
    p.entries.truncate(1);
    let cancel = Arc::new(AtomicBool::new(false));
    let capture = stage(&p, cancel.clone()).unwrap().remove(0);
    let exact = serde_json::to_vec(&p).unwrap().len() + serde_json::to_vec(&capture).unwrap().len();
    assert!(stage_with_limit(&p, cancel.clone(), exact).is_ok());
    let error = stage_with_limit(&p, cancel.clone(), exact - 1).unwrap_err();
    assert!(error.contains("Capture 1"));
    assert!(error.contains("staged JSON payload"));
    let mut second = p.entries[0].clone();
    second.input = Some(fixture());
    p.entries.push(second);
    let budget =
        serde_json::to_vec(&p).unwrap().len() + serde_json::to_vec(&capture).unwrap().len();
    assert!(stage_with_limit(&p, cancel.clone(), budget)
        .unwrap_err()
        .contains("Capture 2"));
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(stage_with_limit(&p, cancel, usize::MAX)
        .unwrap_err()
        .contains("cancelled"));
}

#[test]
fn nested_dock_tree_requires_exact_unique_pinned_leaves_and_valid_splits() {
    let mut value = serde_json::to_value(project()).unwrap();
    let tree = serde_json::json!({"kind":"split","axis":"horizontal","ratio":60,
        "first":{"kind":"panel","view":"trace-cu"},
        "second":{"kind":"split","axis":"vertical","ratio":40,
            "first":{"kind":"panel","view":"occupancy"},
            "second":{"kind":"panel","view":"counters"}}});
    value["entries"][0]["state"]["dockLayout"] = serde_json::json!({
        "pinned":["trace-cu","occupancy","counters"],"arrangement":"custom","split":50,"tree":tree});
    let valid: Project = serde_json::from_value(value.clone()).unwrap();
    valid.validate().unwrap();
    assert_eq!(
        serde_json::to_value(&valid).unwrap()["entries"][0]["state"]["dockLayout"]["tree"],
        tree
    );
    for invalid in [
        serde_json::json!(null),
        serde_json::json!({"kind":"panel","view":"trace-cu"}),
        serde_json::json!({"kind":"split","axis":"horizontal","ratio":50,"first":{"kind":"panel","view":"trace-cu"},"second":{"kind":"panel","view":"trace-cu"}}),
    ] {
        let mut broken = value.clone();
        broken["entries"][0]["state"]["dockLayout"]["tree"] = invalid;
        assert!(serde_json::from_value::<Project>(broken)
            .unwrap()
            .validate()
            .is_err());
    }
    for (key, invalid) in [
        ("axis", serde_json::json!("diagonal")),
        ("ratio", serde_json::json!(90)),
    ] {
        let mut broken = value.clone();
        broken["entries"][0]["state"]["dockLayout"]["tree"][key] = invalid;
        assert!(serde_json::from_value::<Project>(broken)
            .unwrap()
            .validate()
            .is_err());
    }
}

#[test]
fn fingerprinted_projects_reject_changed_inputs_without_replacing_old_capture() {
    let dir = tempfile::tempdir().unwrap();
    let source = fixture().parent().unwrap().to_path_buf();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        if entry.path().is_file() {
            fs::copy(entry.path(), dir.path().join(entry.file_name())).unwrap();
        }
    }
    let path = dir.path().join("filenames.json");
    let mut p = project();
    p.entries[0].input = Some(path.clone());
    p.schema_version = 3;
    let capture = load_input(&path, Arc::new(AtomicBool::new(false))).unwrap();
    p.entries[0].fingerprint = capture.input_fingerprint.clone();
    let saved = dir.path().join("verified.rocprof-project");
    write(&saved, &p).unwrap();
    let restored = read(&saved).unwrap();
    stage(&restored, Arc::new(AtomicBool::new(false))).unwrap();
    let wave = dir.path().join(&capture.waves[0].id);
    let mut bytes = fs::read(&wave).unwrap();
    bytes.push(b' ');
    fs::write(wave, bytes).unwrap();
    assert!(stage(&restored, Arc::new(AtomicBool::new(false)))
        .unwrap_err()
        .contains("input changed"));
    assert_eq!(capture.waves.len(), 24);
    let mut legacy = restored.clone();
    legacy.schema_version = 2;
    assert!(legacy.validate().is_err());
    legacy.entries[0].fingerprint = None;
    assert!(stage(&legacy, Arc::new(AtomicBool::new(false))).is_ok());
}

#[test]
fn analysis_preferences_roundtrip_and_reject_unknown_or_invalid_values() {
    let mut p = project();
    p.entries[0].state.analysis_preferences = BTreeMap::from([
        ("counters.waveStates".into(), serde_json::json!(false)),
        ("sampling.search".into(), serde_json::json!("v_mfma")),
        ("source.location".into(), serde_json::json!("inline.h:4")),
        ("objects.search".into(), serde_json::json!("kernel.so")),
        ("objects.page".into(), serde_json::json!(2)),
        (
            "att.associations".into(),
            serde_json::json!(
                r#"[{"path":"/trace.rocprof-att","mode":"single-dispatch","dispatch":"partition:guid:42","relocation":"/moved/trace.rocprof-att"}]"#
            ),
        ),
        (
            "objects.relocations".into(),
            serde_json::json!(
                r#"[["scope-1","file:///host#offset=32&size=173","/relocated/host"]]"#
            ),
        ),
        ("sampling.metric".into(), serde_json::json!("pcSamples")),
        ("sampling.order".into(), serde_json::json!("samples")),
        ("flame.pcSamples".into(), serde_json::json!("kernel.hip")),
        ("markers.selected".into(), serde_json::json!(null)),
        ("hardware.cursor".into(), serde_json::json!(0.75)),
        ("flame.latency".into(), serde_json::json!("kernel.hip")),
        ("flame.hierarchy".into(), serde_json::json!(true)),
        ("source.lineNumbers".into(), serde_json::json!(false)),
        ("source.includeHidden".into(), serde_json::json!(false)),
        ("source.costWidth".into(), serde_json::json!(240)),
        ("plot.lodBias".into(), serde_json::json!(-20)),
        ("source.costMetric".into(), serde_json::json!("pcIssued")),
        ("dispatch.sort".into(), serde_json::json!("count")),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("analysis.rocprof-project");
    write(&file, &p).unwrap();
    assert_eq!(
        read(&file).unwrap().entries[0].state.analysis_preferences,
        p.entries[0].state.analysis_preferences
    );
    for (key, value) in [
        ("unknown", serde_json::json!(true)),
        (
            "att.associations",
            serde_json::json!(r#"[{"path":"/trace.rocprof-att","mode":"region","dispatch":"42"}]"#),
        ),
        (
            "att.associations",
            serde_json::json!(
                r#"[{"path":"/trace.rocprof-att","mode":"single-dispatch","dispatch":""}]"#
            ),
        ),
        (
            "att.associations",
            serde_json::json!(
                r#"[{"path":"relative.rocprof-att","mode":"unknown","dispatch":""}]"#
            ),
        ),
        (
            "att.associations",
            serde_json::json!(r#"[{"path":"/trace.att","mode":"unknown","dispatch":""}]"#),
        ),
        (
            "att.associations",
            serde_json::json!(
                r#"[{"path":"/trace.rocprof-att","mode":"unknown","dispatch":"","relocation":"relative.rocprof-att"}]"#
            ),
        ),
        (
            "att.associations",
            serde_json::json!(
                r#"[{"path":"/trace.rocprof-att","mode":"unknown","dispatch":"","relocation":null}]"#
            ),
        ),
        ("objects.relocations", serde_json::json!("not JSON")),
        (
            "objects.relocations",
            serde_json::json!(r#"[["id","file:///host","relative"]]"#),
        ),
        (
            "objects.relocations",
            serde_json::json!(r#"[["id","memory://1","/host"]]"#),
        ),
        (
            "objects.relocations",
            serde_json::json!(r#"[["id","file:///host","/host"],["id","file:///host","/other"]]"#),
        ),
        (
            "objects.relocations",
            serde_json::json!(r#"[["id","file:///host","/host","extra"]]"#),
        ),
        ("plot.lodBias", serde_json::json!(-21)),
        ("plot.lodBias", serde_json::json!(21)),
        ("plot.lodBias", serde_json::json!(0.5)),
        ("sampling.order", serde_json::json!("latency")),
        ("source.lineNumbers", serde_json::json!(1)),
        ("source.costWidth", serde_json::json!(-1)),
        ("source.costWidth", serde_json::json!(401)),
        ("source.costWidth", serde_json::json!(1.5)),
        ("source.costMetric", serde_json::json!("unknown")),
        ("host.page", serde_json::json!(-1)),
        ("summary.counts", serde_json::json!("true")),
        ("hardware.cursor", serde_json::json!(1.01)),
        ("sampling.selected", serde_json::json!(9007199254740992u64)),
        ("counters.search", serde_json::json!("x".repeat(8193))),
    ] {
        let mut invalid = p.clone();
        invalid.entries[0]
            .state
            .analysis_preferences
            .insert(key.into(), value);
        assert!(invalid.validate().is_err());
    }
}

#[test]
fn project_reopens_combined_spm_snapshot_with_fingerprint() {
    let dir = tempfile::tempdir().unwrap();
    let mut trace = profile_core::Capture::empty("trace".into(), "rocprofv3 UI JSON", "cycles");
    trace.realtime = Some(profile_core::RealtimeCalibration {
        frequency_hz: None,
        anchors: vec![
            profile_core::RealtimeAnchor {
                se: 0,
                shader_clock: "0".into(),
                realtime_clock: "1000".into(),
            },
            profile_core::RealtimeAnchor {
                se: 0,
                shader_clock: "100".into(),
                realtime_clock: "1100".into(),
            },
        ],
    });
    let mut spm = profile_core::Capture::empty("spm".into(), "rocprofv3 SPM JSON", "ns");
    spm.origin = "1000".into();
    spm.counters.push(serde_json::from_value(serde_json::json!({"name":"BUSY","unit":"events","scope":"process 0 · agent 1 · dispatch 1 · counter 1 · []","points":[[0,1],[10,2]]})).unwrap());
    let combined = profile_core::spm_prepare::attach(&trace, &spm, 0).unwrap();
    let archive = dir.path().join("combined.rocprof-capture");
    profile_core::capture_archive::write_new(&archive, &combined, &AtomicBool::new(false)).unwrap();
    let reopened = load_input(&archive, Arc::new(AtomicBool::new(false))).unwrap();
    let saved = Project {
        schema_version: 3,
        active: 0,
        definition_library: None,
        entries: vec![ProjectEntry {
            load_wave_states: None,
            pc_sampling_method: None,
            input: Some(archive),
            fingerprint: reopened.input_fingerprint.clone(),
            state: state(),
            sources: BTreeMap::new(),
        }],
    };
    let path = dir.path().join("attached.rocprof-project");
    write(&path, &saved).unwrap();
    let restored = stage(&read(&path).unwrap(), Arc::new(AtomicBool::new(false))).unwrap();
    assert_eq!(
        serde_json::to_value(&restored[0].counters).unwrap(),
        serde_json::to_value(&combined.counters).unwrap()
    );
    assert_eq!(
        restored[0].metadata["spmAttachment"],
        combined.metadata["spmAttachment"]
    );
}

#[test]
fn project_restores_two_sampling_methods_from_one_fingerprinted_input() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("mixed.json");
    let host = serde_json::json!({"inst_index":0,"record":{"timestamp":100,"pc":{"code_object_id":4,"code_object_offset":32}}});
    let mut stochastic = host.clone();
    stochastic["record"]["wave_issued"] = 1.into();
    stochastic["record"]["snapshot"] = serde_json::json!({"lck_err":0});
    fs::write(
        &input,
        serde_json::json!({"rocprofiler-sdk-tool":[{
            "strings":{"pc_sample_instructions":["s_nop 0"],"pc_sample_comments":[""]},
            "buffer_records":{"pc_sample_host_trap":[host],"pc_sample_stochastic":[stochastic]}
        }]})
        .to_string(),
    )
    .unwrap();
    let capture = profile_core::project::load_input_options(
        &input,
        Arc::new(AtomicBool::new(false)),
        true,
        Some("host-trap"),
    )
    .unwrap();
    let mut p = project();
    p.schema_version = 5;
    p.entries[0].input = Some(input);
    p.entries[0].fingerprint = capture.input_fingerprint;
    p.entries[0].pc_sampling_method = Some("host-trap".into());
    let mut second = p.entries[0].clone();
    second.pc_sampling_method = Some("stochastic".into());
    p.entries.push(second);
    let file = root.path().join("methods.rocprof-project");
    write(&file, &p).unwrap();
    let restored = read(&file).unwrap();
    let captures = stage(&restored, Arc::new(AtomicBool::new(false))).unwrap();
    assert_eq!(captures.len(), 2);
    assert_eq!(captures[0].instructions[0].pc_issued, None);
    assert_eq!(captures[1].instructions[0].pc_issued, Some(1));
    assert_eq!(captures[0].metadata["pc_sampling_selection"], "host-trap");
    assert_eq!(captures[1].metadata["pc_sampling_selection"], "stochastic");
    p.schema_version = 4;
    assert!(p.validate().unwrap_err().contains("schema 5"));
    p.schema_version = 5;
    p.entries[0].pc_sampling_method = Some("unknown".into());
    assert!(p.validate().is_err());
    p.entries[0].pc_sampling_method = None;
    assert!(stage(&p, Arc::new(AtomicBool::new(false)))
        .unwrap_err()
        .contains("explicit method selection"));
}
