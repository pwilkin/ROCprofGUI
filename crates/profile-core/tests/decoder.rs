#![cfg(unix)]
use profile_core::{
    decoder::{decode, DecoderConfig},
    load_wave,
};
use std::{fs, os::unix::fs::PermissionsExt, sync::atomic::AtomicBool, time::Duration};
static MOCK_PROCESS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn program(root: &std::path::Path, body: &str) -> DecoderConfig {
    let file = root.join("python");
    fs::write(&file, format!("#!/usr/bin/python3\n{body}\n")).unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o700)).unwrap();
    DecoderConfig {
        python: file.into_os_string(),
        library: None,
        python_package: None,
        timeout: Duration::from_secs(2),
    }
}
#[test]
fn explicit_python_package_is_forwarded_without_shell_interpretation() {
    let _guard = MOCK_PROCESS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let package = dir.path().join("decoder package ; literal.zip");
    fs::write(&package, b"mock package").unwrap();
    let mut config = program(
        dir.path(),
        r#"import sys
from pathlib import Path
assert '--python-package' in sys.argv
package=Path(sys.argv[sys.argv.index('--python-package')+1])
assert package.name == 'decoder package ; literal.zip'
assert package.read_bytes() == b'mock package'
print('package argument verified')
sys.exit(3)"#,
    );
    config.python_package = Some(package.clone());
    let error = decode(dir.path(), &config, &AtomicBool::new(false)).unwrap_err();
    assert!(error.contains("package argument verified"), "{error}");
    fs::remove_file(package).unwrap();
    assert!(decode(dir.path(), &config, &AtomicBool::new(false))
        .unwrap_err()
        .contains("Cannot open decoder Python package"));
}

#[test]
fn incomplete_python_package_fails_before_loading_capture_input() {
    let dir = tempfile::tempdir().unwrap();
    let package = dir.path().join("invalid.zip");
    fs::write(&package, b"not a zip").unwrap();
    let config = DecoderConfig {
        python: "python3".into(),
        library: None,
        python_package: Some(package),
        timeout: Duration::from_secs(5),
    };
    let error = decode(dir.path(), &config, &AtomicBool::new(false)).unwrap_err();
    assert!(error.contains("BadZipFile"), "{error}");
}

#[test]
fn decoder_import_can_skip_generated_wave_state_sidecars() {
    let _guard = MOCK_PROCESS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let config = program(dir.path(), r#"import json,sys
from pathlib import Path
out=Path(sys.argv[3])
(out/'filenames.json').write_text(json.dumps({'wave_filenames':{'0':{'0':{'0':{'0':['wave.json',1,2]}}}}}))
(out/'wstates2.json').write_text('invalid optional sidecar')"#);
    assert!(decode(dir.path(), &config, &AtomicBool::new(false)).is_err());
    let capture = profile_core::decoder::decode_with_wave_states(
        dir.path(), &config, &AtomicBool::new(false), false,
    ).unwrap();
    assert_eq!(capture.metadata["loadWaveStates"], "false");
    assert_eq!(capture.waves.len(), 1);
}

#[test]
fn decoder_owns_temporary_outputs_until_last_capture_is_closed() {
    let _guard = MOCK_PROCESS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let config = program(
        dir.path(),
        r#"import json,sys
from pathlib import Path
out=Path(sys.argv[3])
(out/'filenames.json').write_text(json.dumps({'wave_filenames':{'0':{'0':{'0':{'0':['wave.json',1,2]}}}}}))
(out/'wave.json').write_text(json.dumps({'wave':{'begin':1,'end':2,'instructions':[[1,6,0,1,1]]}}))"#,
    );
    let capture = decode(dir.path(), &config, &AtomicBool::new(false)).unwrap();
    let root = capture.root.clone();
    let clone = capture.clone();
    drop(capture);
    assert!(root.exists());
    assert_eq!(load_wave(&clone, "wave.json").unwrap().tokens.len(), 1);
    drop(clone);
    assert!(!root.exists());
}
#[test]
fn decoder_reports_errors_timeout_and_cancellation() {
    let _guard = MOCK_PROCESS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let mut config = program(
        dir.path(),
        "import sys\nprint('broken decoder',file=sys.stderr)\nsys.exit(3)",
    );
    let error = decode(dir.path(), &config, &AtomicBool::new(false)).unwrap_err();
    assert!(error.contains("broken decoder"), "{error}");
    config = program(dir.path(), "import time\ntime.sleep(30)");
    config.timeout = Duration::from_millis(80);
    assert!(decode(dir.path(), &config, &AtomicBool::new(false))
        .unwrap_err()
        .contains("timed out"));
    assert!(decode(dir.path(), &config, &AtomicBool::new(true))
        .unwrap_err()
        .contains("cancelled"));
}
#[test]
#[ignore = "Requires decoder V2, Python package and ROCPROF_GUI_TEST_ATT fixture directory"]
fn real_att_decode_and_lazy_wave_read() {
    let path = std::env::var("ROCPROF_GUI_TEST_ATT").unwrap();
    let capture = decode(
        std::path::Path::new(&path),
        &Default::default(),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(!capture.waves.is_empty());
    assert!(!capture.intervals.is_empty());
    assert!(capture.instructions.iter().any(|i| i.hits > 0));
    let wave = load_wave(&capture, &capture.waves[0].id).unwrap();
    assert!(!wave.tokens.is_empty());
    for event in &capture.decoder_events {
        assert!(event.time >= capture.start && event.time <= capture.end);
    }
    if let Some(calibration) = &capture.realtime {
        let sidecar: serde_json::Value =
            serde_json::from_slice(&fs::read(capture.root.join("realtime.json")).unwrap()).unwrap();
        let mut expected = Vec::new();
        for (key, samples) in sidecar.as_object().unwrap() {
            if let Some(se) = key.strip_prefix("SE") {
                for sample in samples.as_array().unwrap() {
                    expected.push(profile_core::RealtimeAnchor {
                        se: se.parse::<u32>().unwrap(),
                        shader_clock: sample[0]
                            .as_i64()
                            .unwrap()
                            .checked_add(
                                *capture
                                    .se_time_offsets
                                    .get(&se.parse::<u32>().unwrap())
                                    .unwrap_or(&0),
                            )
                            .unwrap()
                            .to_string(),
                        realtime_clock: sample[1].as_u64().unwrap().to_string(),
                    });
                }
            }
        }
        assert_eq!(calibration.anchors, expected);
        let expected = expected.len();
        println!(
            "{expected} REALTIME anchors retained exactly; frequency {:?}",
            calibration.frequency_hz
        );
    } else {
        assert!(
            !capture.root.join("realtime.json").exists(),
            "Decoder emitted REALTIME data that import did not retain"
        );
        println!("Decoder emitted no REALTIME sidecar for this archived trace");
    }
    println!("{} decoder timeline events", capture.decoder_events.len());
    for activity in &capture.other_simd {
        assert!(activity.start >= capture.start);
        assert!(activity.start + activity.duration <= capture.end);
        assert!(activity.duration >= 0.);
        assert!(!activity.source.is_empty());
    }
    println!(
        "{} supplemental other-SIMD records",
        capture.other_simd.len()
    );
    let hidden = profile_core::hidden::analyze_capture(&capture).unwrap();
    assert_eq!(hidden.by_wave.len(), capture.waves.len());
    println!(
        "Hidden analysis: {} waves; warnings: {:?}",
        hidden.by_wave.len(),
        hidden.warnings
    );
    println!(
        "{} waves, {} ISA records, {} residency intervals, {} tokens in first wave",
        capture.waves.len(),
        capture.instructions.len(),
        capture.intervals.len(),
        wave.tokens.len()
    );
}

#[test]
fn selected_att_run_filters_prefix_normalizes_ids_and_keeps_all_engines() {
    let _guard = MOCK_PROCESS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    for name in [
        "p_shader_engine_0_3.att",
        "p_shader_engine_2_03.ATT",
        "p_shader_engine_0_4.att",
        "q_shader_engine_0_3.att",
        "unrelated.att",
    ] {
        fs::write(dir.path().join(name), b"trace").unwrap();
    }
    let config = program(
        dir.path(),
        r#"import sys,types,runpy,json
from pathlib import Path
package=types.ModuleType('rocprof_trace_decoder'); package.__path__=[]
sys.modules['rocprof_trace_decoder']=package
for name in ['att','codegen','code_index']:
    sys.modules['rocprof_trace_decoder.'+name]=types.ModuleType(name)
class Record:
    def __init__(self, **kw): self.__dict__.update(kw)
def generate(traces, **kwargs):
    assert sorted((t.shader_engine,t.run) for t in traces)==[(0,3),(2,3)]
    assert all(t.path.name.startswith('p_') for t in traces)
    out=Path(kwargs['output_dir'])
    (out/'filenames.json').write_text(json.dumps({'wave_filenames':{'0':{'0':{'0':{'0':['wave.json',1,2]}}}}}))
    (out/'wave.json').write_text(json.dumps({'wave':{'begin':1,'end':2,'instructions':[[1,6,0,1,1]]}}))
    return [out]
sys.modules['rocprof_trace_decoder.att'].AttTrace=Record
sys.modules['rocprof_trace_decoder.att'].generate_att_outputs=generate
sys.modules['rocprof_trace_decoder.codegen'].CodeObject=Record
sys.modules['rocprof_trace_decoder.code_index'].CodeIndex=Record
sys.argv=sys.argv[1:]
runpy.run_path(sys.argv[0],run_name='__main__')"#,
    );
    let capture = decode(
        &dir.path().join("p_shader_engine_0_3.att"),
        &config,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(capture.waves.len(), 1);
}

#[test]
fn explicit_att_manifest_passes_metadata_without_filename_inference() {
    let _guard = MOCK_PROCESS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("trace.bin"), b"trace").unwrap();
    fs::write(dir.path().join("kernel.bin"), b"object").unwrap();
    let manifest = dir.path().join("capture.rocprof-att");
    fs::write(&manifest, r#"{"version":1,"run":7,"traces":[{"path":"trace.bin","shaderEngine":2}],"codeObjects":[{"path":"kernel.bin","id":"18446744073709551615"}]}"#).unwrap();
    let config = program(
        dir.path(),
        r#"import sys,types,runpy,json
from pathlib import Path
package=types.ModuleType('rocprof_trace_decoder'); package.__path__=[]
sys.modules['rocprof_trace_decoder']=package
for name in ['att','codegen','code_index']:
    sys.modules['rocprof_trace_decoder.'+name]=types.ModuleType(name)
class Record:
    def __init__(self, **kw): self.__dict__.update(kw)
def generate(traces, **kwargs):
    assert [(t.shader_engine,t.run) for t in traces]==[(2,7)]
    assert traces[0].path.name=='trace.bin' and traces[0].path.is_absolute()
    assert kwargs['code_objects'][0].code_object_id==18446744073709551615
    assert kwargs['code_objects'][0].path.name=='kernel.bin'
    assert kwargs['code_index'] is None
    out=Path(kwargs['output_dir'])
    (out/'filenames.json').write_text(json.dumps({'wave_filenames':{'0':{'0':{'0':{'0':['wave.json',1,2]}}}}}))
    (out/'wave.json').write_text(json.dumps({'wave':{'begin':1,'end':2,'instructions':[[1,6,0,1,1]]}}))
    return [out]
sys.modules['rocprof_trace_decoder.att'].AttTrace=Record
sys.modules['rocprof_trace_decoder.att'].generate_att_outputs=generate
sys.modules['rocprof_trace_decoder.codegen'].CodeObject=Record
sys.modules['rocprof_trace_decoder.code_index'].CodeIndex=Record
sys.argv=sys.argv[1:]
runpy.run_path(sys.argv[0],run_name='__main__')"#,
    );
    let capture = decode(&manifest, &config, &AtomicBool::new(false)).unwrap();
    assert_eq!(capture.waves.len(), 1);
    assert_eq!(fs::read(dir.path().join("trace.bin")).unwrap(), b"trace");
}

#[test]
fn cancellation_stops_an_already_running_decoder_process() {
    let _guard = MOCK_PROCESS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let mut config = program(
        dir.path(),
        r#"import os,sys,time
from pathlib import Path
(Path(sys.argv[2])/'ready.pid').write_text(str(os.getpid()))
time.sleep(30)"#,
    );
    config.timeout = Duration::from_secs(5);
    let cancelled = std::sync::Arc::new(AtomicBool::new(false));
    let flag = cancelled.clone();
    let ready = dir.path().join("ready.pid");
    let observed = ready.clone();
    let signal = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while !observed.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let started = observed.exists();
        flag.store(true, std::sync::atomic::Ordering::Relaxed);
        started
    });
    let result = decode(dir.path(), &config, &cancelled);
    assert!(signal.join().unwrap(), "decoder did not signal readiness");
    assert!(result.unwrap_err().contains("cancelled"));
    #[cfg(target_os = "linux")]
    {
        let pid = fs::read_to_string(ready).unwrap();
        assert!(
            !std::path::Path::new("/proc").join(pid.trim()).exists(),
            "decoder child still exists after cancellation"
        );
    }
}
