use std::{path::PathBuf, sync::atomic::AtomicBool};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/radv-rgp").join(name)
}

fn default_mask_capture() -> PathBuf {
    fixture("compute-spm.rgp")
}

fn occupancy_capture() -> PathBuf {
    fixture("compute-occupancy.rgp")
}

/// Captured with RADV_THREAD_TRACE_ROCPROF_COMPAT=true (docs/radv-rocprof-compat.patch).
fn compat_capture() -> PathBuf {
    fixture("compute-rocprof-compat.rgp")
}

/// One llama.cpp prompt-processing submit, with GGML_VK_DEBUG_MARKERS=1.
fn llama_capture() -> PathBuf {
    fixture("llama-ggml-submit.rgp")
}

/// A llama.cpp submit holding one host-to-device upload, which RADV implements with
/// its own compute shader.
fn llama_copy_capture() -> PathBuf {
    fixture("llama-ggml-copy.rgp")
}

/// The eight dispatches of the llama.cpp fixture: shader label and workgroup counts.
const LLAMA_DISPATCHES: [(&str, [u32; 3]); 8] = [
    ("rope_norm_f32", [1280, 1, 1]),
    ("set_rows_f32_f16_i64", [64, 1, 1]),
    ("rope_norm_f32_f16", [256, 1, 1]),
    ("flash_attn_f32_f16_aligned_cm1", [4, 20, 1]),
    ("contig_cpy_f32_f16", [1, 320, 1]),
    ("matmul_quant_f16_f16acc_aligned_1", [40, 1, 1]),
    ("add_f32_f32_f32_norepeat", [1, 320, 1]),
    ("rms_norm_mul_f32", [64, 1, 1]),
];

/// Imports counters and identity only, independent of whether the ROCm decoder exists.
fn counters_only(path: &std::path::Path) -> Result<profile_core::Capture, String> {
    profile_core::rgp::load_options(path, &AtomicBool::new(false), false)
}

fn sections(path: &std::path::Path) -> Vec<(u32, Vec<u8>)> {
    let d = std::fs::read(path).unwrap();
    let table = profile_core::rgp::chunks(&d).unwrap();
    profile_core::rgp::sqtt_sections(&d, &table).unwrap()
}

#[test]
fn radv_capture_imports_spm_counters_with_units_and_topology() {
    let c = counters_only(&default_mask_capture()).unwrap();
    assert_eq!(c.format, "RADV RGP");
    assert_eq!(c.unit, "ns");
    assert_eq!(c.architecture, "STRIX_HALO");
    assert_eq!(c.metadata["deviceId"], "0x1586");
    assert_eq!(c.metadata["topology"], "2 shader engines × 20 CUs");
    assert_eq!(c.metadata["spmSamples"], "329");
    assert_eq!(c.metadata["spmSampleInterval"], "4096 clocks");
    assert_eq!(c.counters.len(), 14);
    for s in &c.counters {
        assert_eq!(s.points.len(), 329, "{} lost samples", s.name);
        assert!(s.points.windows(2).all(|w| w[0][0] <= w[1][0]), "{} is not ordered in time", s.name);
        assert!(s.points.iter().all(|p| p[1].is_finite()), "{} has non-finite values", s.name);
    }
    assert_eq!(c.start, 0.);
}

/// SPM timestamps are the 100 MHz GPU timestamp, not shader clocks. The ASIC_INFO
/// clock (931 MHz) is not the clock during capture and must not size the window.
#[test]
fn counter_axis_is_gpu_timestamp_nanoseconds() {
    let c = counters_only(&default_mask_capture()).unwrap();
    assert_eq!(c.metadata["spmWindow"], "0.488 ms of GPU timestamp");
    let x = &c.counters[0].points;
    assert_eq!(x.first().unwrap()[0], 0.);
    assert_eq!(x.last().unwrap()[0], 48_836. * 10.);
    assert!(c.metadata["asicReportedShaderClock"].contains("can differ"));
}

#[test]
fn counter_identity_scope_and_units_come_from_the_capture() {
    let c = counters_only(&default_mask_capture()).unwrap();
    let by = |n: &str| c.counters.iter().find(|s| s.name == n).unwrap_or_else(|| panic!("missing {n}"));
    assert_eq!(by("L2 cache hit").unit, "%");
    assert_eq!(by("L2 cache hit").scope, "GPU · Cache");
    assert_eq!(by("Fetch size").unit, "bytes");
    assert_eq!(by("Fetch size").scope, "GPU · Memory (bytes)");
    assert_eq!(by("Ray-box tests").unit, "count");
    assert_eq!(by("Ray-box tests").scope, "GPU · Ray tracing");
    assert_eq!(by("LDS Bank Conflict").scope, "GPU · LDS");
    for s in &c.counters {
        let d = s.definition.as_ref().unwrap();
        assert_eq!(d.id, s.name);
        assert_eq!(d.is_derived, Some(true), "{} is one of RADV's derived counters", s.name);
        assert!(!d.description.is_empty(), "{} lost RADV's description", s.name);
    }
    assert!(by("L2 cache hit").definition.as_ref().unwrap().description.starts_with("The percentage of"));
}

/// Captured with RADV_SPM_COUNTERS_CONFIG=compute-raw-counters.conf on a dispatch of
/// 64 wave64 workgroups, each issuing one buffer load and one buffer store, so the true
/// TA totals are known: 128 vec32 packets of each.
#[test]
fn raw_counter_config_imports_as_hardware_counters_with_coverage_caveats() {
    let c = counters_only(&fixture("compute-raw-counters.rgp")).unwrap();
    let names: Vec<_> = c.counters.iter().map(|s| (s.name.as_str(), s.scope.as_str())).collect();
    assert_eq!(names.len(), 10);
    assert_eq!(names[0], ("SQ_WAVES", "GPU · Waves"));
    assert_eq!(names[5], ("TA_TA_BUSY", "GPU · Activity"));
    assert_eq!(names[9], ("GL2C_MISS", "GPU · L2"));
    for s in &c.counters {
        assert_eq!(s.definition.as_ref().unwrap().is_derived, Some(false), "{}", s.name);
        assert_eq!(s.unit, "count", "{}", s.name);
        assert_eq!(s.points.len(), 76, "{}", s.name);
    }
    let total = |n: &str| c.counters.iter().find(|s| s.name == n).unwrap().points.iter().map(|p| p[1]).sum::<f64>();
    // Below the true 128: RADV's instance expansion does not cover every CU.
    assert_eq!(total("TA_BUFFER_LOAD_WAVEFRONTS"), 80.);
    assert_eq!(total("TA_BUFFER_STORE_WAVEFRONTS"), 80.);
    assert!(c.warnings.iter().any(|w| w.starts_with("10 SPM counters are raw hardware events") && w.contains("not GPU totals")));
    assert!(c
        .warnings
        .iter()
        .any(|w| w.starts_with("SQ_WAVES, SQ_WAVE_CYCLES, SQ_WAIT_INST_ANY, SQ_WAIT_ANY, SQ_BUSY_CYCLES read zero in every sample")));
    assert!(!c.warnings.iter().any(|w| w.contains("device-wide")), "raw counters are not device-wide");
}

/// The workload uses no LDS, issues no raytracing and runs on a unified-memory
/// APU, so these must stay zero; a non-zero reading means the value mapping drifted.
#[test]
fn counters_the_workload_cannot_exercise_stay_zero() {
    let c = counters_only(&default_mask_capture()).unwrap();
    for name in ["LDS Bank Conflict", "Ray-box tests", "Ray-triangle tests", "PCIe bytes"] {
        let s = c.counters.iter().find(|s| s.name == name).unwrap();
        assert!(s.points.iter().all(|p| p[1] == 0.), "{name} reported activity the workload cannot produce");
    }
    let fetch = c.counters.iter().find(|s| s.name == "Fetch size").unwrap();
    assert!(fetch.points.iter().any(|p| p[1] > 0.), "Fetch size never moved; the value run is misaligned");
}

#[test]
fn counters_only_import_carries_no_waves_and_no_rejection() {
    let c = counters_only(&default_mask_capture()).unwrap();
    assert!(c.waves.is_empty());
    assert!(c.intervals.is_empty());
    assert!(!c.warnings.iter().any(|w| w.contains("Thread trace not imported")));
    assert!(c.warnings.iter().any(|w| w.contains("device-wide")));
}

/// Instruction-timing streams carry 24-bit ALU/VMEM execution tokens the decoder
/// cannot size. Every one must be found, and nothing else may be unrecognised.
#[test]
fn sanitizer_blanks_every_execution_token_and_leaves_no_unknown_headers() {
    for (se, mut body) in sections(&default_mask_capture()) {
        let original = body.clone();
        let report = profile_core::rgp::sanitize_sqtt(&mut body);
        assert_eq!(report.blanked, 330, "SE {se}");
        assert_eq!(report.unknown, 0, "SE {se}");
        assert_eq!(body.len(), original.len(), "SE {se}: sanitizing must not change the stream length");
        for (i, (a, b)) in original.iter().zip(&body).enumerate() {
            for shift in [0, 4] {
                let (before, after) = ((a >> shift) & 0xF, (b >> shift) & 0xF);
                assert!(after == before || after == 0, "SE {se} byte {i}: a nibble changed to something other than NOP");
            }
        }
        assert_eq!(profile_core::rgp::sanitize_sqtt(&mut body), Default::default(), "SE {se}: not idempotent");
    }
}

#[test]
fn occupancy_and_rocprof_compatible_streams_need_no_sanitizing() {
    for path in [occupancy_capture(), compat_capture()] {
        let found = sections(&path);
        assert_eq!(found.len(), 2, "{}", path.display());
        for (se, mut body) in found {
            assert_eq!(profile_core::rgp::sanitize_sqtt(&mut body), Default::default(), "{} SE {se}", path.display());
        }
    }
}

/// RADV traces carry no in-trace load markers; the loader event's hash must match
/// the ELF's `.internal_pipeline_hash` so each shader lands at the right address.
#[test]
fn code_objects_are_placed_at_their_loader_event_address() {
    let d = std::fs::read(default_mask_capture()).unwrap();
    let table = profile_core::rgp::chunks(&d).unwrap();
    let objects = profile_core::rgp::placed_code_objects(&d, &table).unwrap();
    assert_eq!(objects.len(), 1);
    assert_eq!(objects[0].base, 0x8000_0000_0000);
    assert_eq!(objects[0].text_size, 256);
    assert_eq!(objects[0].pipeline_hash, Some([0xb213_a084_03f3_01e3; 2]));
    assert_eq!(&objects[0].elf[..4], b"\x7fELF");
}

#[test]
fn realtime_validation_flags_shader_clock_jumps_only() {
    let steady: Vec<[u64; 2]> = (0..50).map(|i| [i * 2048, 1000 + i * 148]).collect();
    let v = serde_json::json!({ "SE0": steady, "metadata": {} });
    assert_eq!(profile_core::rgp::realtime_anomalies(&v), vec![("SE0".into(), 0, 50)]);
    let mut jumped = steady.clone();
    for p in &mut jumped[30..] {
        p[0] += 179_707_065_489;
    }
    let v = serde_json::json!({ "SE0": jumped });
    assert_eq!(profile_core::rgp::realtime_anomalies(&v), vec![("SE0".into(), 1, 50)]);

    // Realtime tokens pause while the GPU idles, as between most llama.cpp dispatches:
    // a long step whose shader-clock ratio matches its neighbours is legitimate.
    let mut idle = steady.clone();
    for p in &mut idle[25..] {
        p[0] += 2048 * 15;
        p[1] += 148 * 15;
    }
    let v = serde_json::json!({ "SE0": idle });
    assert_eq!(profile_core::rgp::realtime_anomalies(&v), vec![("SE0".into(), 0, 50)]);

    // A misparsed forward jump in the final value leaves no backward step to catch;
    // across it the shader clock would have had to nearly stop.
    let mut late = steady.clone();
    late[49][1] += 62_000_000_000;
    let v = serde_json::json!({ "SE0": late });
    assert_eq!(profile_core::rgp::realtime_anomalies(&v), vec![("SE0".into(), 1, 50)]);

    // Observed in a real wave32 capture: one misparsed realtime value makes the step
    // into it negative and the step out of it huge, so the shader-clock jump that
    // follows looks proportionate unless the step out of it is checked too.
    let base = 62_189_000_000;
    let mut corrupted: Vec<[u64; 2]> = (0..50).map(|i| [i * 2048, base + i * 148]).collect();
    corrupted[20][1] = 63_965_188;
    for p in &mut corrupted[21..] {
        p[0] += 86_304_566;
    }
    let v = serde_json::json!({ "SE1": corrupted });
    let found = profile_core::rgp::realtime_anomalies(&v);
    assert!(found[0].1 >= 2, "a corrupted realtime token was not flagged: {found:?}");
}

/// A desync can leave the realtime pairs intact while a wave's span is absorbed into
/// one oversized state; per-wave invariants still hold, so the window is the check.
#[test]
fn waves_outside_their_trace_window_are_flagged() {
    let realtime = serde_json::json!({ "SE0": (0..50u64).map(|i| [10_000 + i * 2048, i * 148]).collect::<Vec<_>>() });
    let inside = serde_json::json!({ "wave_filenames": { "0": { "0": { "0": ["w0.json", 20_000, 40_000] } } } });
    let occupancy = serde_json::json!({ "0": [[15_000, 0, 0, 0, 1], [60_000, 0, 0, 0, 0]] });
    assert_eq!(
        profile_core::rgp::timestamps_outside_window(&realtime, &inside, &occupancy),
        vec![("SE0".into(), 0, 4)]
    );
    let stretched = serde_json::json!({ "wave_filenames": { "0": { "0": { "0": ["w0.json", 20_000, 86_459_770] } } } });
    assert_eq!(
        profile_core::rgp::timestamps_outside_window(&realtime, &stretched, &occupancy),
        vec![("SE0".into(), 1, 4)]
    );
}

#[test]
fn both_import_dispatchers_accept_rgp() {
    let cli = profile_core::load_capture(&default_mask_capture()).unwrap();
    let desktop =
        profile_core::project::load_input_options(&default_mask_capture(), Default::default(), true, None).unwrap();
    assert!(cli.format.starts_with("RADV RGP"));
    assert_eq!(cli.format, desktop.format);
    assert_eq!(cli.counters.len(), desktop.counters.len());
}

#[test]
fn malformed_captures_are_rejected_without_panicking() {
    let dir = tempfile::tempdir().unwrap();
    let good = std::fs::read(default_mask_capture()).unwrap();

    let empty = dir.path().join("empty.rgp");
    std::fs::write(&empty, b"").unwrap();
    assert!(counters_only(&empty).unwrap_err().contains("shorter than an RGP header"));

    let wrong = dir.path().join("wrong.rgp");
    let mut bad_magic = good.clone();
    bad_magic[..4].copy_from_slice(b"NOPE");
    std::fs::write(&wrong, &bad_magic).unwrap();
    assert!(counters_only(&wrong).unwrap_err().contains("B00P"));

    let truncated = dir.path().join("truncated.rgp");
    std::fs::write(&truncated, &good[..good.len() / 3]).unwrap();
    assert!(counters_only(&truncated).is_err());

    let header_only = dir.path().join("header.rgp");
    std::fs::write(&header_only, &good[..56]).unwrap();
    assert!(counters_only(&header_only).is_err());
}

fn decoded(path: &std::path::Path) -> profile_core::Capture {
    let c = profile_core::rgp::load(path).unwrap();
    assert_eq!(c.format, "RADV RGP · thread trace", "decoding failed: {:?}", c.warnings);
    c
}

fn assert_consistent_waves(c: &profile_core::Capture) {
    for i in 0..c.waves.len() {
        let w = profile_core::load_wave(c, &c.waves[i].id).unwrap();
        assert!(w.states.iter().all(|s| s[1] >= 0.), "{}: negative state", w.id);
        let total: f64 = w.states.iter().map(|s| s[1]).sum();
        assert_eq!(total, w.end - w.start, "{}: states do not cover the wave", w.id);
        assert!(w.tokens.windows(2).all(|t| t[0].start <= t[1].start), "{}: tokens out of order", w.id);
    }
}

/// Every wave must satisfy the invariant ROCm traces do: no negative state, and state
/// durations summing exactly to the wave's span.
#[test]
#[ignore = "requires the ROCm trace decoder library and ROCPROF_GUI_DECODER_PACKAGE"]
fn default_mask_capture_decodes_to_consistent_waves() {
    let c = decoded(&default_mask_capture());
    assert_eq!(c.metadata["radvTraceMode"], "instruction timing, execution tokens removed");
    assert_eq!(c.waves.len(), 2);
    assert!(c.warnings.iter().any(|w| w.contains("must not be used for hotspot analysis")));
    assert_consistent_waves(&c);
    let spm: Vec<_> = c.counters.iter().filter(|s| s.scope.starts_with("GPU ·")).collect();
    assert_eq!(spm.len(), 14);
    assert!(spm.iter().all(|s| s.points.len() == 328), "SPM samples not placed on the trace clock");
}

/// 256 workgroups of one wave64 each: occupancy must account for exactly 256 waves.
#[test]
#[ignore = "requires the ROCm trace decoder library and ROCPROF_GUI_DECODER_PACKAGE"]
fn occupancy_capture_accounts_for_every_wave_in_the_dispatch() {
    let c = decoded(&occupancy_capture());
    assert_eq!(c.metadata["radvTraceMode"], "occupancy");
    assert_eq!(c.intervals.len(), 256);
    for se in 0..2 {
        assert_eq!(c.intervals.iter().filter(|i| i.se == se).count(), 128, "SE {se}");
    }
}

#[test]
#[ignore = "requires the ROCm trace decoder library and ROCPROF_GUI_DECODER_PACKAGE"]
fn gfx1201_occupancy_capture_keeps_dispatches_and_markers() {
    let c = decoded(&fixture("gfx1201-add-occupancy.rgp"));
    assert_eq!(c.metadata["asic"], "GFX1201");
    assert_eq!(c.metadata["radvTraceMode"], "occupancy");
    assert_eq!(c.metadata["rgpDispatchMarkers"], "6");
    assert_eq!(c.dispatches.len(), 24);
    assert_eq!(c.intervals.len(), 120);
    assert!(c.dispatches.iter().all(|d| d.kernel == "add_f32_f32_f32" && d.grid == "256 × 5 × 1"));
    assert!(c.instructions.iter().all(|i| i.hits == 0));
    assert!(c.warnings.iter().all(|w| !w.contains("Thread trace not imported")));
}

/// 64 workgroups of one wave64, each running a 64-iteration loop. With
/// rocprof-trace-decoder's token set every issued instruction on the traced CU is
/// attributed: each loop-body row exactly 64 times per wave, the loop test once more
/// for the entry check, straight-line code once, and never-issued hints not at all.
#[test]
#[ignore = "requires the ROCm trace decoder library and ROCPROF_GUI_DECODER_PACKAGE"]
fn rocprof_compatible_capture_attributes_every_instruction_exactly() {
    let c = decoded(&compat_capture());
    assert_eq!(c.metadata["radvTraceMode"], "instruction timing");
    assert!(!c.warnings.iter().any(|w| w.contains("hotspot") || w.contains("no per-instruction data")), "{:?}", c.warnings);
    assert_eq!(c.intervals.len(), 64);
    assert_eq!(c.waves.len(), 2);
    assert_consistent_waves(&c);
    for summary in &c.waves {
        let w = profile_core::load_wave(&c, &summary.id).unwrap();
        for row in c.instructions.iter().filter(|i| !i.text.starts_with(';') && !i.text.is_empty()) {
            let hits = w.tokens.iter().filter(|t| t.instruction == row.index).count();
            let op = row.text.split_whitespace().next().unwrap();
            let expected = match (row.address.as_str(), op) {
                (_, "s_delay_alu" | "s_nop") => 0,
                ("0x40" | "0x44" | "0x54" | "0x5c" | "0x64" | "0x6c", _) => 64,
                ("0x74" | "0x78", _) => 65,
                _ => 1,
            };
            assert_eq!(hits, expected, "{}: {} {}", w.id, row.address, row.text);
        }
    }
    let attributed: u64 = c.instructions.iter().map(|i| i.hits).sum();
    assert_eq!(attributed, 2 * (6 * 64 + 2 * 65 + 12));
}

/// Builds a thread-trace stream of REG tokens writing `dwords` to USERDATA_2/3 on
/// ME 1 pipe 2, two at a time, the way RADV emits RGP markers.
fn marker_stream(dwords: &[u32]) -> Vec<u8> {
    let mut out = vec![];
    for pair in dwords.chunks(2) {
        for (k, d) in pair.iter().enumerate() {
            let reg = 0xC342 + k as u64;
            let token = 0b1001 | 2 << 7 | 1 << 9 | reg << 16 | (*d as u64) << 32;
            out.extend(token.to_le_bytes());
        }
    }
    out
}

fn push(label: &str) -> Vec<u32> {
    let mut bytes = label.as_bytes().to_vec();
    bytes.resize(label.len().div_ceil(4) * 4, 0);
    let mut out = vec![5 | 2 << 12, bytes.len() as u32];
    out.extend(bytes.chunks(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])));
    out
}

#[test]
fn rgp_markers_attribute_dispatches_to_labels_and_pipelines() {
    let pop = vec![5 | 1 << 12];
    let bind = |hash: u64| vec![0xC | 1 << 7, hash as u32, (hash >> 32) as u32];
    let dispatch = |api: u32, dims: [u32; 3]| vec![0 | api << 7 | 1 << 31, 0, 0, dims[0], dims[1], dims[2]];
    let stream: Vec<u32> = [
        vec![1, 0, 0, 0],
        push("MUL_MAT ffn_up"),
        bind(0x1122_3344_5566_7788),
        push("mul_mat_vec (64,1,1)"),
        dispatch(6, [64, 1, 1]),
        pop.clone(),
        push("mul_mat_vec (128,2,1)"),
        dispatch(6, [128, 2, 1]),
        pop.clone(),
        pop.clone(),
        // A driver-internal fill: no application pipeline is bound for it.
        dispatch(14, [4, 1, 1]),
        push("(view)"),
        vec![0 | 7 << 7, 0, 0],
    ]
    .concat();
    let (markers, complete) = profile_core::rgp::dispatch_markers(&marker_stream(&stream));
    assert!(complete);
    assert_eq!(markers.len(), 4);
    assert_eq!((markers[0].me, markers[0].pipe, markers[0].api), (1, 2, 6));
    assert_eq!(markers[0].labels, ["MUL_MAT ffn_up", "mul_mat_vec (64,1,1)"]);
    assert_eq!(markers[1].dims, Some([128, 2, 1]));
    assert_eq!(markers[1].pipeline, Some(0x1122_3344_5566_7788));
    assert_eq!((markers[2].api, markers[2].pipeline), (14, None));
    assert_eq!((markers[3].api, markers[3].dims, markers[3].labels.as_slice()), (7, None, &["(view)".to_owned()][..]));
    assert_eq!(markers[3].pipeline, Some(0x1122_3344_5566_7788), "an indirect dispatch uses the bound pipeline");
    let names = profile_core::rgp::pipeline_names(&markers);
    assert_eq!(names.len(), 1);
    // Group counts are per dispatch, so both mul_mat_vec labels share one stem; a
    // parenthesised suffix that is not a count stays, and stems rank by use.
    assert_eq!(names[&0x1122_3344_5566_7788], "mul_mat_vec / (view)");

    let unsized_marker = [vec![1, 0, 0, 0], vec![8, 0]].concat();
    assert!(!profile_core::rgp::dispatch_markers(&marker_stream(&unsized_marker)).1);
}

/// RADV writes one event marker per hardware dispatch; with ggml's debug labels on,
/// each names its shader and carries its workgroup counts.
#[test]
fn llama_markers_name_every_pipeline_and_carry_dispatch_grids() {
    for (se, body) in sections(&llama_capture()) {
        let (markers, complete) = profile_core::rgp::dispatch_markers(&body);
        assert!(complete, "SE {se}");
        assert_eq!(markers.len(), 8, "SE {se}");
        for (m, (shader, groups)) in markers.iter().zip(LLAMA_DISPATCHES) {
            assert_eq!(m.api, 6, "SE {se}: {shader}");
            assert_eq!(m.dims, Some(groups), "SE {se}: {shader}");
            assert!(m.labels.last().unwrap().starts_with(shader), "SE {se}: {:?}", m.labels);
            assert_eq!(m.labels[0], "submit 7");
        }
        let names = profile_core::rgp::pipeline_names(&markers);
        let mut named: Vec<&str> = names.values().map(String::as_str).collect();
        named.sort();
        let mut expected: Vec<&str> = LLAMA_DISPATCHES.iter().map(|(s, _)| *s).collect();
        expected.sort();
        assert_eq!(named, expected, "SE {se}");
        assert_eq!(names[&0x927f_3750_fd47_fc8f], "rope_norm_f32");
    }
}

/// Every wave a dispatch launched is accounted for: occupancy per kernel equals the
/// marker's workgroup count times the waves per workgroup (wave64), and every
/// traced wave runs from its shader's entry to s_endpgm.
#[test]
#[ignore = "requires the ROCm trace decoder library and ROCPROF_GUI_DECODER_PACKAGE"]
fn llama_capture_occupancy_matches_marker_grids() {
    let c = decoded(&llama_capture());
    assert_eq!(c.metadata["radvTraceMode"], "instruction timing");
    assert_eq!(c.metadata["rgpDispatchMarkers"], "8");
    for se in 0..2 {
        let dispatches: Vec<_> = c.dispatches.iter().filter(|d| d.shader_engine == Some(se)).collect();
        assert_eq!(dispatches.len(), 8, "SE {se}");
        for (d, (shader, groups)) in dispatches.iter().zip(LLAMA_DISPATCHES) {
            assert_eq!(d.kernel, shader, "SE {se}");
            let threads: u32 = d.workgroup.split(" × ").map(|n| n.parse::<u32>().unwrap()).product();
            let waves = groups.iter().product::<u32>() * threads.div_ceil(64);
            let size: Vec<u32> = d.workgroup.split(" × ").map(|n| n.parse().unwrap()).collect();
            assert_eq!(d.grid, format!("{} × {} × {}", groups[0] * size[0], groups[1] * size[1], groups[2] * size[2]), "{shader}");
            assert_eq!(c.intervals.iter().filter(|i| i.kernel == shader).count() as u32, waves, "{shader}");
        }
    }
    assert_eq!(c.intervals.len(), 9488);
    assert_consistent_waves(&c);
    for summary in &c.waves {
        let w = profile_core::load_wave(&c, &summary.id).unwrap();
        let text = |t: &profile_core::Token| &c.instructions.iter().find(|i| i.index == t.instruction).unwrap().text;
        assert!(text(w.tokens.last().unwrap()).starts_with("s_endpgm"), "{}", w.id);
        let first = c.instructions.iter().find(|i| i.index == w.tokens[0].instruction).unwrap();
        assert_eq!(first.address, "0x0", "{}", w.id);
    }
}

/// RADV's copy shader has no bind marker, so it is named after the command its event
/// marker records. That marker's dimensions are thread counts for RADV's unaligned
/// dispatch (640 threads, so 10 wave64 workgroups), so no grid is claimed for it.
#[test]
#[ignore = "requires the ROCm trace decoder library and ROCPROF_GUI_DECODER_PACKAGE"]
fn driver_internal_dispatches_are_named_after_their_command() {
    let (markers, _) = profile_core::rgp::dispatch_markers(&sections(&llama_copy_capture())[0].1);
    assert_eq!((markers.len(), markers[0].api, markers[0].pipeline, markers[0].dims), (1, 8, None, Some([640, 1, 1])));
    let c = decoded(&llama_copy_capture());
    for d in &c.dispatches {
        assert_eq!(d.kernel, "vkCmdCopyBuffer (RADV internal)");
        assert_eq!(d.grid, "Not recorded");
        assert_eq!(d.workgroup, "64 × 1 × 1");
    }
    assert_eq!(c.dispatches.len(), 2);
    assert_eq!(c.intervals.len(), 10);
    assert!(c.intervals.iter().all(|i| i.kernel == "vkCmdCopyBuffer (RADV internal)"));
}
