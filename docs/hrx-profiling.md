# HRX profiling: trace paths and the IRPF importer

This note records how rocprof studio opens profiles produced by **HRX**, which
trace paths actually work on this hardware, and the exact scope and limits of the
native IRPF importer. Everything here is grounded in the retained hrx-b99 beta
artifacts; nothing is inferred from a guessed schema (see
[blocked-inputs.md](blocked-inputs.md) for that rule).

## What HRX and Loom are

**HRX = "Hip Runtime Extended"** — a lighter, client-focused alternative
implementation of HIP maintained by AMD under the ROCm org
([ROCm/hrx-system](https://github.com/ROCm/hrx-system)). It is **not** a Radeon
GPU architecture and **not** a new gfx target; on this host it drives the same
gfx1151 device through native `amdgpu`, exposed to llama.cpp as an `HRX0` device.

**Loom** is the source-first kernel *compiler* substrate inside hrx-system
(`loom/`, JIT to HSACO). It is a **compiler, not a trace format**: there is no
"Loom trace" to import. Loom emits *compile reports* (telemetry about a compiled
kernel), but see the gap at the end of this document — that tool does not ship in
the pinned beta, so no compile-report importer exists.

## Pinned beta

| Item | Value |
|---|---|
| Release | [`hrx-b99`](https://github.com/ROCm/ggml-staging-automation/releases/tag/hrx-b99) (`llama-hrx-b99-bin-manylinux-hrx-x64.tar.gz`) |
| Archive SHA-256 | `02c88d96d39c607aefbb982e8b4f630785ec43c4416583834912ceaa63fd7624` |
| llama.cpp | `2ba2ddc76a8a2d977ce94a65c07e20cc0896e384` (`hrx-graph-develop-v2`) |
| HRX System | `32b86d42d96a8e3098d9a56bea201e5f83d44f85` |
| rocprofv3 | SDK 1.3.5 / ROCm 10.0 |
| GPU | gfx1151 (Radeon 8060S) |
| Model | Qwen3-4B-Q4_K_M, `--device HRX0 -ngl 99` |

The pinned schema headers and the retained bundles live under the campaign's
`phase2/` directory; the IRPF importer is written directly against
`iree/hal/profile_file.h` and `iree/hal/profile_schema.h` from that source tree.

## Trace paths that work on gfx1151

An HRX run can be profiled two ways: with **rocprofv3** (attached or as a direct
launcher against the HRX process), and with HRX's **native** profiler
(`HRX_PROFILE_FILE` / `HRX_PROFILE_MODE`). Both feed existing or new importer
arms.

| Path | Produces | Imports through | Evidence (hrx-b99, gfx1151) |
|---|---|---|---|
| rocprofv3 attach, kernel trace | `*_kernel_trace.csv` (XYZ grid/workgroup) | dispatch-CSV arm | 30,797 kernel rows (attach), 17,354 (direct launch) |
| rocprofv3 counter collection | `*_counter_collection.csv` (scalar `Grid_Size`/`Workgroup_Size`) | dispatch-counter-CSV arm | 252 `SQ_WAVES` counter rows + 5,834 kernel rows |
| rocprofv3 ATT (triggered attach) | raw `*.att` + companion ELF code objects | raw ATT decoder | dispatch 405 `ggml_mul_mat_swiglu_q4_q8_1_x4_lowtoken_dot_q8_output`, grid 155648×1×1, workgroup 512×1×1 → 30 waves, 374 ISA rows with positive hits |
| HRX native, `HRX_PROFILE_MODE=dispatch` | finalized IRPF 2.0 bundle (1,768,721 B) | **IRPF arm (new)** | 46 functions, 4,540 dispatch events, 46 code objects |
| HRX native, `HRX_PROFILE_MODE=queue` | finalized IRPF 2.0 bundle (517,089 B) | **IRPF arm (new)** | 46 functions, 1,708 command-operation dispatches, 0 code objects |
| HRX native, `executable` / `all` | 24-byte unfinalized header only | *(rejected)* | exits 1: "AMDGPU executable trace profiling requires a capture filter"; the stock libhrx frontend supplies none |

Notes that bit, kept so they are not rediscovered:

- **Counter CSV grid/workgroup.** A rocprofv3 counter-collection CSV carries a
  single scalar `Grid_Size`/`Workgroup_Size` column, not the kernel trace's
  `*_X/_Y/_Z` triple. The importer now reads whichever the header provides, so
  counter CSVs keep their grid/workgroup instead of rendering an empty `× ×`.
- **Native HRX bypasses the HIP passthrough layer**, so a native-HRX run produces
  **no HIP API trace CSV**. The absence of a HIP API CSV is not evidence of no
  GPU work.
- Native `HRX0` uses `amdgpu`, not RADV/Vulkan; forcing it produces no `.rgp`.
  The archive's separate Vulkan backend does not make HRX a Vulkan runtime.

## The IRPF importer (`crates/profile-core/src/irpf.rs`)

IRPF ("IREE HAL profile bundle") is the little-endian binary container HRX's
`libhrx` writes natively. The importer is a bounds-validated parser of the pinned
producer contract — a 24-byte file header, 104-byte record headers, and typed
chunks whose payloads are packed arrays of self-sized sub-records tagged with
`application/vnd.iree.hal.profile.<family>` content types.

### What it reconstructs

- **Dispatches.** From the `dispatch-events` chunk: event identity, resolved
  function name (by `executable_id` + `function_ordinal`), grid, workgroup and
  start/end ticks. A **queue-mode** bundle has no `dispatch-events`; the importer
  then falls back to the static `command-operations` dispatch program (named,
  gridded, but with no device timing), clearly labelled as such.
- **Kernel names.** Resolved from the `executable-functions` chunk (46 functions
  in both retained bundles, all dispatch names resolved).
- **Code-object identity.** From `executable-code-objects` +
  `executable-code-object-loads`: the embedded-ELF flag, image byte length, and
  per-device load delta / load size (46 code objects in the dispatch bundle).
- **Clock correlations.** Read and counted; their flags are inspected for
  `DEVICE_TICK_UNALIGNED` (see below).
- **Grid convention.** IRPF records workgroup **counts**; the global grid is
  reported as `count × size` — the exact value rocprofv3 reports as `Grid_Size`,
  verified on the shared kernels (e.g. `ggml_rope_f32` 32×32 = 1024;
  `ggml_mul_mat_swiglu…` 304×512 = 155648, matching the ATT capture above).

### Time domain and the unaligned-tick caveat

Dispatch events carry **raw device ticks**. The device record advertises a
timestamp frequency (99,810,000 Hz, `FREQ` flag set), so the importer converts
**elapsed** ticks to nanoseconds relative to the first tick — honest for
durations and for within-capture ordering. It **does not place events on a host
timeline**: both clock-correlation records in the dispatch bundle are flagged
`DEVICE_TICK_UNALIGNED`, which the producer defines to mean "keep raw device
ordering and tick durations, but do not fit this device clock to a host
timeline." The capture surfaces this as a warning and in `metadata.coverage`;
the full elapsed span of the dispatch bundle is 552.986 ms.

### Caps, validation, provenance

- Honours the configurable `ROCPROF_MAX_*` caps: per-file size
  (`ROCPROF_MAX_FILE_MIB`) up front, and record counts (`ROCPROF_MAX_RECORDS`)
  during the sub-record walk — it rejects explicitly past the bound, never
  samples.
- Rejects malformed input with an explicit message and never panics: bad magic,
  an unsupported major version, a declared file length past the actual bytes, a
  truncated record, an over-long sub-record, and an **unfinalized** bundle
  (`file_length == 0`, which is exactly what the `executable`/`all` modes
  produce).
- Labels provenance: `format = "HRX native profile · IRPF 2.0"`, with
  `metadata.hrxProfileMode`, `hrxFunctionCount`, `hrxDispatchCount`,
  `hrxCodeObjectCount` and `hrxTimestampFrequencyHz`.

### What it is not

These bundles carry dispatch metadata and timing, not microarchitecture: there
are **no wave occupancy, ISA hotspots, PC samples or hardware counters** in the
queue/dispatch IRPF streams. For waves and ISA on HRX, use the rocprofv3 ATT path
above. Register/LDS/scratch are not part of the IRPF dispatch or function
records, so those fields read "Not collected" rather than a guessed zero.

### End-to-end acceptance (CPU-only, headless MCP service)

Importing the retained bundles through the built release binary's headless MCP
service (`capture_open` → `capture_query` → `capture_analyze`):

| Bundle | `capture_open` dispatches | functions | code objects | `capture_analyze` kernels |
|---|---:|---:|---:|---:|
| `hook-dispatch/trace.irpf` | 4,540 | 46 | 46 | 19 kernel rows (top by time: `ggml_mul_mat_swiglu_q4_q8_1_x4_lowtoken_dot_q8_output`, 289 dispatches) |
| `hook-queue/trace.irpf` | 1,708 | 46 | 0 | 19 kernel rows (no device timing) |

## HRX kernel-name families

The read-side classifier (`src/kernelName.ts`) recognises two HRX/loom families
by their producer-observed name prefixes:

- `ggml_*` → **hrx-ggml**: compute kernels from the HRX/loom ggml backend
  (`ggml_rope_f32`, `ggml_mul_mat_swiglu_q4_q8_1_x4_lowtoken_dot_q8_output`,
  `ggml_flash_attention_decode_split_f32_f16_wmma_next_q8`, …).
- `iree_hal_amdgpu_device_*` → **hrx-runtime**: IREE HAL AMDGPU device-library
  runtime kernels (`iree_hal_amdgpu_device_buffer_copy_block_x16`), marked
  internal so the dispatch table hides their Tune button, like RADV's internal
  shaders.

Model-specific fused kernels (`qwen*`, `llm_attention_*`) are intentionally left
as plain names: they carry no stable backend prefix to key a family on, and
classifying by one workload's model name would over-fit. See
[kernel-name-layer.md](kernel-name-layer.md).

## ROCm-side limitations observed during collection

These are collection constraints on the ROCm side, not importer limits. They are
recorded so a future capture run does not relearn them the expensive way:

- **A second ATT window on one live target yields no new raw ATT files.** The SDK
  default empty iteration filter samples the *first* occurrence of each kernel
  name; that sampling state survives detach/reattach within the process, so later
  windows emit kernel CSV records but no new `.att`.
- **Timed counter windows can hang the queue.** A timed counter window on this
  workload failed with "Timeout while waiting for queue sync: 1 kernels still
  active" and sustained GPU work; a newline-terminated (triggered) counter window
  in a fresh target completed instead. This is a bounded remedy, not proof the
  SDK race is gone.
- **A 768 MiB ATT buffer aborts the target** (`aqlprofile_att_create_packets():
  SQTT Buffer size too high`); 384–512 MiB succeed. The exact maximum was not
  established.
- A name regex alone cannot pick a specific grid under one kernel name; record
  the desired occurrence from a representative CSV, or use a fresh
  target/capture filter.

## Gap: no Loom compile-report importer

The hrx-b99 archive ships `libhrx`/`libloomc` and a separate Vulkan backend, but
**does not ship** `hrx-info`, `loom-compile`, `loom-compile-report`,
`ggml-hrx-analyze-graph`, `libhip_logging.so` or `libhip_buffer_tracer.so`. There
is therefore **no Loom compile-report or HIP-interceptor artifact** to write an
importer against. Per the project's producer-contract rule, no speculative
compile-report importer is written: it would be guessing a layout nobody has
observed. Those tools exist in other/source distributions; if a representative
compile-report artifact arrives, that is the point at which the importer and its
identity mechanism get decided.

## Where this is wired

- Importer: `crates/profile-core/src/irpf.rs`, reached from both the headless
  dispatcher (`import::load_capture_options`) and the desktop/MCP dispatcher
  (`project::load_input_checked`) by the `.irpf` extension and by an IRPF magic
  sniff, so an oddly-named bundle still opens.
- Tests: synthetic round-trip, malformed/unfinalized rejection and cap behaviour
  run by default; the exact-count round-trips of the two retained bundles are
  `#[ignore]` and gated on `ROCPROF_IRPF_DISPATCH_BUNDLE` /
  `ROCPROF_IRPF_QUEUE_BUNDLE` (README, "Tests that do not run by default").
