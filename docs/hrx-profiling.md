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
"Loom trace" to import. Loom emits compile reports, which are telemetry about a
compiled kernel. rocprof studio imports those reports as static compile facts;
see the Loom compile reports section below.

## Two HRX drivers, two visibilities

Whether rocprofv3 can see an HRX or Loom run depends on which GPU driver the
binary was built with. This decides everything else on this page.

| Driver | How it dispatches | rocprofv3 | Native IRPF profile |
|---|---|---|---|
| **Legacy ROCr driver** (`legacy-amdgpu`, what hrx-b99 `libhrx` uses) | loads `libhsa-runtime64` and submits AQL packets to ROCr queues | visible: kernel trace, counters, ATT | `HRX_PROFILE_FILE` / `HRX_PROFILE_MODE` |
| **Native AMD driver** (`amdgpu://`, `rdna://`; what current `loom-serve`, `loom-run` and `loom-benchmark` use) | opens `/dev/kfd` itself, creates its own KFD user queues and writes PM4 into them | **invisible in every mode** | `--device_profiling_mode` / `--device_profiling_output` |

The second row is the current Loom tooling in hrx-system (verified at
`23e9b32232`, branch `loom-working`, gfx1151). See
[Native AMD driver: PM4 direct dispatch](#native-amd-driver-pm4-direct-dispatch)
below for the mechanism, the reproduction and the collection path that works.

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
  gridded, but with no device timing), clearly labelled as such. A
  `dispatch-events` chunk flagged `TRUNCATED` with **zero payload** is *not*
  relabelled queue-mode — that would silently change the timing semantics — it is
  rejected (the producer is expected to fail the session rather than truncate a
  dispatch/device timeline). An all-zero workgroup size (a dynamic/indirect
  dispatch per the schema) renders grid and workgroup as `Not recorded`, never as
  a fabricated `count × 1`.
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
  truncated record, an over-long sub-record, an **unfinalized** bundle
  (`file_length == 0`, which is exactly what the `executable`/`all` modes
  produce), and a nonzero value in either reserved "must be zero" `flags` field
  (file header `@12`, record header `@102`).
- Honours the producer's **chunk-truncation contract**. The record header carries
  `chunk_flags` (`TRUNCATED`), `dropped_record_count` and `session_status_code`.
  Any `TRUNCATED` chunk imports with a loud warning naming the family and the
  dropped-record count, so partial data is never presented as complete; a
  `SESSION_END` record with a nonzero status code is surfaced as a warning that
  the run may have failed.
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

## Native AMD driver: PM4 direct dispatch

Verified on gfx1151 against hrx-system `23e9b32232` (`loom-working`), the tree
that holds the Qwen3.8-27B hybrid project (`loom/models/qwen3.8-27b-hybrid`).

### Why rocprofv3 returns nothing

The Loom tools are built with the native AMD driver only (`-DLIBHRX_BUILD=OFF
-DIREE_HAL_DRIVER_AMD_GPU=ON -DIREE_HAL_DRIVER_AMD_XDNA=ON`). `loom-serve` is a
static binary: `ldd` shows libc, libm, libstdc++ and libgcc, and its strings
contain `/dev/kfd` but neither `libhsa-runtime64` nor `libamdhip64`.

- `libamdf` opens `/dev/kfd` (`libamdf/src/gpu/umd/kfd/file.h:24`) and creates
  the compute queue with `AMDKFD_IOC_CREATE_QUEUE`
  (`libamdf/src/gpu/umd/kfd/user_queue_native.c:46`) as
  `KFD_IOC_QUEUE_TYPE_COMPUTE` — a PM4 user queue, not `COMPUTE_AQL`
  (`libamdf/src/gpu/umd/kfd/target/pm4_queue.c:99`).
- The HAL driver prefers PM4 on every non-CDNA GPU
  (`runtime/src/iree/hal/driver/amd/gpu/device.c:1091-1095`; its README: "The
  default prefers PM4 on RDNA and AQL on CDNA").
- Dispatches are `DISPATCH_DIRECT` packets the driver encodes itself
  (`iree_hal_amd_gpu_pm4_emit_dispatch_direct`,
  `runtime/src/iree/hal/driver/amd/gpu/queue/pm4/commands.h:49`) and publishes by
  storing the write index and ringing the doorbell
  (`runtime/src/iree/hal/driver/amd/gpu/queue/pm4/publication.c:162-168`).

rocprofiler-sdk observes dispatches by hooking ROCr's AQL queues inside the
target. A process that never loads ROCr gives it nothing to hook, so this is a
"producer cannot emit it through that interface" limit, not a rocprofv3 option
that was missed. Every mode was tried against 192,000 real dispatches of
`loom-benchmark` on `amdgpu://0` (rocprofv3 1.4.1, ROCm 10.1):

| Attempt | Result |
|---|---|
| `--kernel-trace` (direct launch) | exit 0, **no output file** |
| `--kernel-trace --hip-trace --hsa-trace --marker-trace` | exit 0, no output file |
| `--kernel-trace --pmc SQ_WAVES` | exit 0, no output file |
| `--kernel-trace --att --att-target-cu 0 --att-simd-select 0xF --att-buffer-size 0x6000000 --att-shader-engine-mask 0x1` | exit 0, no output file (kernel logs `failed to disable PTL`) |
| `--sys-trace --kfd-trace --stats --summary` | exit 0, no output file |
| `--attach PID` on a target started with `ROCP_TOOL_ATTACH=1` | exit 1: `Calling attach in librocprofiler-sdk-rocattach.so.1 returned non-zero status 1`; no output file; target unaffected |

`profile_run` and `profile_stop` now name this case instead of returning an empty
file list: when rocprofv3 leaves no output and the target is an ELF that opens
`/dev/kfd` without referencing `libhsa-runtime64` or `libamdhip64`, the report
carries a warning that points at tracer `hrx`.

### What the native driver can record

`iree_hal_amd_gpu_profile_begin` accepts exactly three data families
(`runtime/src/iree/hal/driver/amd/gpu/profile.c:626-628`): `queue-events`,
`dispatch-events` and `executable-metadata`. Anything else fails with
`unsupported GPU profiling data families`; hardware counters fail with `GPU queue
profiling does not support hardware counters`. There is **no ATT/SQTT, wave or
counter path in the native driver**, so waves and ISA hotspots cannot be collected
for these kernels by any tool today.

`dispatch-events` gives one record per dispatch with the function, workgroup
counts and sizes, and a start/end pair of raw device ticks at 99,810,000 Hz. The
samples "bracket dispatch issue and completion joins"
(`runtime/src/iree/hal/driver/amd/gpu/profile.h`), and on the real capture below
they never overlap.

Two producer behaviours shape collection:

- **A bounded ring.** The driver keeps 4,096 records per queue between flushes
  (`IREE_HAL_AMD_GPU_PROFILE_CAPACITY`, `gpu/profile.c:335`) and drops the rest,
  marking the chunk `TRUNCATED`. Two 48-token requests on the hybrid model, with
  no periodic flush, kept 4,096 of 16,312 queue events on the busy queue and
  dropped 12,216. With a periodic flush (20 or 25 ms) the same workload lost none.
- **One session per device, one request for the whole group.** The tooling begins
  profiling on every device of a device group with the same family set
  (`runtime/src/iree/tooling/device_profiling.c`). The XDNA NPU driver implements
  `queue-events`, `host-execution`, `counters` and `executable-metadata`
  (`runtime/src/iree/hal/driver/amd/xdna/profile.c:59-64`) but not
  `dispatch-events`, so a GPU + NPU group asked for dispatch timing fails with
  `xdna/profile.c:85: UNIMPLEMENTED; unsupported XDNA profiling data families 0x8`
  and the server exits 1. The only family set both accept is `queue-events` +
  `executable-metadata`, which carries no dispatch record at all.

### Collecting: tracer `hrx`

`profile_run`, `profile_start` and `profile_stop` take `tracer: "hrx"`. The target
must be an IREE HAL tooling binary that defines `--device_profiling_mode` and
`--device_profiling_output`; those flags are appended to the argv you pass, so an
argv that already carries one is rejected.

| `hrx` option | Flag | Default |
|---|---|---|
| `families` | `--device_profiling_mode` | `queue-events`, `dispatch-events`, `executable-metadata` |
| `flushIntervalMs` | `--device_profiling_flush_interval_ms` | 25 (0 disables) |
| `filterFunction` | `--device_profiling_filter_function` | none |
| `allowPartial` | `--device_profiling_allow_partial=true` | off |

The target records for its whole lifetime, so there is no `profile_trigger`
window. `profile_stop` sends SIGINT, waits up to 90 s for the bundle to be
finalized (the header's file length is written with the last session-end record),
and kills the target only after that. The report carries the `.irpf` path, a
structural summary (sessions, producers, functions, dispatch events, queue events,
truncated families) and a warning for each way the bundle can fall short: not
written, not finalized, truncated, no dispatch events, a device rejecting a
family, or an unknown flag.

```jsonc
{"name":"profile_start","arguments":{
  "tracer":"hrx","cwd":"/path/to/run",
  "argv":["/path/to/loom-serve","--device=amdgpu://0","--model=/path/to/model","--listen=127.0.0.1:8093"]}}
// ... wait for /v1/models to report ready, send the requests to profile ...
{"name":"profile_stop","arguments":{"id":"<id>"}}
{"name":"capture_open","arguments":{"path":"<job>/hrx-profile.irpf"}}
```

### A GPU + NPU device group needs a patched tool

For a hybrid deployment, stock tooling cannot give GPU dispatch timing: the NPU
aborts the group. [`hrx-device-profiling-allow-partial.patch`](hrx-device-profiling-allow-partial.patch)
adds `--device_profiling_allow_partial` to `runtime/src/iree/tooling/device_profiling.c`
(84 added lines, against `23e9b32232`): when a device reports a family as
unimplemented, it is retried with the largest subset of the requested families it
accepts, and each reduction is printed on stderr. Drivers validate families before
they touch the sink, so a rejected attempt leaves no session behind. The patch is
not applied to hrx-system; build a patched `loom-serve` from a copy of the tree and
pass `hrx.allowPartial: true`.

With it, the hybrid server prints `device_profiling: device 1 keeps data families
0x21 of requested 0x29` and the GPU records dispatch events while the NPU records
queue events and executable metadata.

### The real capture

Qwen3.8-27B hybrid (`amdgpu://0` + `xdna://0?columns=8`), patched `loom-serve`,
one 60-token prompt with 48 generated tokens and one 5,556-token prompt with 24,
collected with `profile_start`/`profile_stop` (tracer `hrx`, `allowPartial`) and
opened through a private headless instance of the release binary:

| Fact | Value |
|---|---:|
| Bundle | 7,662,769 bytes, finalized, 2 sessions (`amd-gpu`, `xdna`), nothing truncated |
| Dispatches | 47,875, all on `HRX device 0 (amd-gpu)`, `HRX queue 0` |
| Kernel names resolved | 101 distinct, none unresolved |
| Elapsed span | 11.991 s of device time |
| Queue events | 16,590 (counted, not imported) |
| Waves / ISA rows / counters | 0 / 0 / 0 |

Top kernels by total device time (`capture_analyze`, `kernels`):

| Kernel | Dispatches | Total |
|---|---:|---:|
| `g_gemm_q4_k_store_m256` | 962 | 3,229.5 ms |
| `g_gemm_q4_k_swiglu_m256_rt` | 188 | 1,024.4 ms |
| `g_gemm_q6_k_store_m256` | 412 | 940.7 ms |
| `g_gemm_q4_k_swiglu_m256` | 94 | 763.8 ms |
| `g_mv_q4_k_k5120_swiglu_t8_q8` | 1,196 | 569.9 ms |
| `g_gdn_recurrence` | 2,640 | 476.1 ms |

A dispatch row reads, for example, `g_gemm_q4_k_store_m256_rt`, grid `18432 × 4 ×
1`, workgroup `256 × 1 × 1`, 3.17 ms.

### What the importer does with these bundles

- **Each chunk is attributed to its own session, device and queue.** The native
  driver splits a family across many chunks (one per queue and per flush) and
  states the session, physical device ordinal, queue ordinal and stream only in
  the chunk header. The importer walks chunks individually: a dispatch's agent is
  `HRX device N (producer)`, its queue `HRX queue N`, and its function name is
  resolved inside its own session, because executable identifiers are
  session-local and a GPU and an NPU session can reuse one.
- **Device clock domains stay apart.** Every device that records ticks has its own
  origin and frequency. Ticks of two devices are never compared.
- **Dispatch ids stay unique.** `event_id` is unique only within one dispatch
  stream; with more than one stream the id becomes `dispatch-events:s<stream>:<id>`.
- **A device that recorded no dispatch is named.** For the hybrid capture the
  warning reads that `HRX device 1 (xdna)` recorded no dispatch records, so NPU
  work is absent from the dispatch table and from every per-kernel total.
- **A bundle with no dispatch record is rejected, not opened empty.** The
  `queue-events` + `executable-metadata` bundle of a stock hybrid server, and the
  bundle an aborted session leaves, used to open as a capture with 0 dispatches
  labelled "queue mode". They now fail with what the bundle does hold (sessions,
  producers, functions, queue events) and how to get dispatches.
- **Queue events are counted, not imported.** They are host-timestamped admission
  records with no kernel identity; the count is in `metadata.hrxQueueEventCount`.

### What still does not work

- **rocprofv3 on the native driver**, in any mode. Nothing to collect.
- **Waves, ISA hotspots and hardware counters** for native-driver kernels. The
  driver implements no executable-trace or counter family.
- **NPU work.** The XDNA driver records queue events and executable metadata only.
  Kernels placed on the NPU never appear as dispatches.
- **Hybrid dispatch timing without the patch.** A stock `loom-serve` on a GPU +
  NPU group either aborts or records no dispatch.
- **`loom-run` with `dispatch-events`** at this checkout: the single direct
  dispatch fails with `pm4/queue.c:2077: FAILED_PRECONDITION; dispatch capture
  requires command buffer profile metadata`. `loom-serve` works; `loom-benchmark`
  has its own `--profile-final-batch` / `--profile-artifacts-dir` flags and writes
  `.irpf` files that open directly.
- **Host timeline.** Ticks are device-relative, as for hrx-b99 bundles.
- **Published model cards from another compiler horizon.** The `loom-fun` cards in
  the Hugging Face cache (Qwen2.5-0.5B, Qwen3-4B) do not compile with this tree
  (`PARSE/006: unknown op 'vector.constant'`), so they could not be used as a
  smaller workload.

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

Loom programs add one more family:

- `__command_launch_counts_N` → **loom-internal**: the launch-count realization
  kernel the Loom command-program lowering synthesises
  (`loom/src/loom/target/arch/cmd/lower/launch_count_realization.c`), marked
  internal.

Authored Loom kernels stay plain. A Loom kernel is named by its author's
`kernel.def export("…")`, so `g_mv_q4_k_k5120_t8_q8`, `g_gdn_recurrence` or
`native_*` follow one model generator's convention, not a compiler-wide one.

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

## Loom compile reports

Loom can emit a compile report alongside the compiled artifact. It is compiler
telemetry about one compiled kernel: schedule, allocation, register pressure,
emitted code size, residency, the static and dynamic instruction mix, wait plan
and workload geometry the compiler modelled. It is not a trace, not a dispatch
and not a runtime measurement. There are no cycle or time counts anywhere in a
compile report.

Produce one with loom-compile:

```shell
loom-compile kernel.loom \
  --format=amdgpu-hsaco \
  --target=amdgpu:gfx1151 \
  --output=kernel.hsaco \
  --compile-report=summary \
  --compile-report-output=kernel.summary.json
```

Use `--compile-report=details` for the detail-mode collections (pressure rows,
schedule bands, spill rows, source-low rows and the rest). iree-benchmark-loom
embeds the same report in a benchmark envelope under a `compile_report` key with
run and candidate context; rocprof studio reads that envelope too.

The stability contract from the producer is explicit, and rocprof studio honours
it: "Compile reports are version-zero, same-compiler-horizon diagnostics.
Regenerate them with the current checkout instead of treating them as durable
records or adding compatibility paths to the consumer." The importer accepts only
schema version zero and rejects anything else, telling the reader to regenerate
with the current compiler checkout.

### What the importer validates

Validation mirrors the strict loader `loom/py/loom/reporting/compile_report.py`
at the pinned checkout `32b86d42d96a8e3098d9a56bea201e5f83d44f85`. A report is
rejected, with an explicit message and never a panic, when any of these hold:

- the envelope kind is not `loom.compile_report` and the document is not an
  explicit `type=compile_report` benchmark envelope;
- `schema_version` is not zero;
- `mode` is neither `summary` nor `details`;
- `status` is not an object, or its code is not zero (the compiler status name is
  surfaced in the message, because a failed compile has no meaningful facts);
- an entries, config-bindings or residency-constraints collection has a count
  that disagrees with its row array, or a row whose index is out of order;
- an entry has no source or export identity, or two entries share one identity;
- a residency summary reports a negative tier, or marks itself unavailable while
  still claiming a current or next tier;
- a residency constraint pairs `units` without `rounded_units`, gives a fixed
  limit a reducible footprint, or claims a reduction without a known limiting
  footprint;
- a `wait_reason_summary_rows` inventory (validation ported from the loader's
  `compile_report_wait_reasons.py` at the same checkout) breaks a stable
  counter/reason name↔id mapping, duplicates a `(function, counter_id, reason_id)`,
  violates a summary count invariant, or references a function no entry defines.

The importer honours the same `ROCPROF_MAX_*` caps and cancellation as every
other arm. It reads each collection's declared row count and rejects an over-cap
report **before** allocating for it, not after materialising every row, so a
report that is within the per-file size cap can no longer exhaust memory ahead of
the record-cap check; the rejection interpolates the effective
`ROCPROF_MAX_RECORDS` value. The load is cancellable: it checks the shared
cancellation flag before and during the walk, like the IRPF and CSV arms.

The pre-schema public sample, a benchmark result with an embedded older-shaped
report and no version-zero envelope, is rejected on the same rule. The rejection
names the expected `kind='loom.compile_report'` and `schema_version 0` and
carries the regenerate-with-this-compiler instruction, under both filenames: a
`.compile-report.json` copy reaches the Loom validator directly, and the sample
under its original plain `.json` name — which misses the suffix and the
content sniff because it has no version-zero discriminator — is still steered to
the same regeneration guidance because it is recognisably compile-report-shaped.
Genuinely unrelated JSON keeps the ordinary "unrecognized JSON" message.

### What it surfaces

Accepted reports are mapped into a capture as static compile facts on a dedicated
`compileReports` surface, kept apart from dispatches, counters and waves so they
cannot be read as measured data. The capture format is labelled "Loom compile
report (version-zero static facts)" and its unit is "static". For each report it
records the report-level identity (function, lowered symbol, backend, target
family, target key, target bundle and config), the decoded 30-bit detail-flag
mask, and the version-zero stability label. For each entry it records the
identity and display name, the flat counters under their producer names, the
nested analysis objects preserved verbatim (target resources with SGPR and VGPR
final counts and scheduled pressure, subgroup size, occupancy percent and
limiting resource; the static and dynamic instruction mix; economics; wait plan;
workload geometry; move causes; planning), the residency constraints, and in
detail mode the row collections. Every field is optional. A field the compiler
did not establish stays absent; it is never defaulted to zero.

Over MCP these facts are queryable with `capture_query` and dataset
`compileReports`: one row per report, each carrying the report- and per-entry
identity, detail flags, the broken-out target resources (SGPR/VGPR final counts
and scheduled pressure, subgroup geometry, static occupancy percent, limiting
resource, residency summary), the instruction mix, the allocation/spill and
emitted-instruction counters, the wait plan, the workload geometry and the
residency constraints. Every row carries `unit: "static"` and the version-zero
stability label, so a client cannot read it as a measurement.

### What it refuses to infer

The importer never synthesises evidence the report does not carry. It does not
invent cycles or time. It does not derive dynamic execution counts from static
counts, hardware register counts from scheduled peak-live pressure, or a
throughput prediction from occupancy percent. An absent bank-service summary is
not a claim that there are no bank conflicts. Reports are not comparable across
targets or compiler horizons, and an AMDGPU report and a SPIR-V report are not
comparable even at schema version zero. These caveats are attached to every
imported capture as warnings.

### Provenance

A compile report carries no compiler identity, so provenance is recorded out of
band. When a `tool-provenance.json` or `commands.json` sidecar sits beside the
report or at the bundle root, the importer records the producing tool
(loom-compile for a direct report, iree-benchmark-loom for a benchmark envelope),
the hrx-system commit and checkout, the tool build date and SHA-256, and the
exact invocation with its target and config. When no sidecar is present the
importer marks provenance unavailable and says so in a warning, rather than
guessing. The report file itself is covered by the existing SHA-256 input
fingerprint on the desktop and MCP import paths.

### A real benchmark envelope

The importer is tested against a real `iree-benchmark-loom` envelope, not only
synthetic fixtures. `fixtures/loom/shared_memory_vector_tile.benchmark-envelope.compile-report.json`
(116,031 bytes, sha256 `0390e3e2083054e4eaac6f18263c7af27b78395be423c773e786baa17c7b5747`)
is the sidecar the benchmark tool wrote while compiling and running one kernel
once on a gfx1151 GPU. It is an envelope: `type=compile_report`, `state=ok`, with
`run_id`/`candidate_id`/`candidate_index`/`benchmark`/`case` context wrapping a
nested `kind=loom.compile_report`, `schema_version=0`, `mode=details` report with
one entry, `backend=amdgpu-hal`, `target_key=gfx1151`.

It was produced with the provider-enabled tool, which stock public Loom builds do
not include — the AMDGPU HAL driver must be compiled in. The tool was built from
pinned hrx-system `32b86d42d96a8e3098d9a56bea201e5f83d44f85` with:

```shell
python3 dev.py bazel build //loom/src/loom/tools/iree-benchmark-loom:iree-benchmark-loom \
  --//runtime/config/hal:drivers=amdgpu,task \
  --//runtime/src/iree/hal/drivers/amdgpu:targets=gfx1151
```

and run once against the hrx-b99 runtime on gfx1151:

```shell
iree-benchmark-loom shared_memory_vector_tile.loom \
  --device=amdgpu --target=amdgpu:gfx1151 \
  --artifact-bundle-dir=.../bundle --artifact-bundle-policy=debug \
  --compile-report=details --output-format=jsonl \
  --iterations=1 --warmup-iterations=0 --batch-size=1 \
  --min-time-ms=0 --max-batches=1 --stable-p90-to-p50-ppm=0 --input-ring-count=1
```

`fixtures/loom/PROVENANCE.md` records the tool and source hashes and the full
discovery. An ungated test (`imports_real_benchmark_envelope_fixture`) imports
this vendored envelope through the real path and asserts the outer envelope
context and the nested report's facts; the env-gated real-report test
additionally sweeps the original `benchmark-live` bundle directory when present.

## Where this is wired

- Importer: `crates/profile-core/src/irpf.rs`, reached through the shared import
  router (`project::load_input` → `import::load_capture_options`, the one
  cancellable, fingerprinted path the GUI, headless CLI and MCP all use) by the
  `.irpf` extension and by an IRPF magic sniff, so an oddly-named bundle still
  opens.
- Tests: synthetic round-trip, malformed/unfinalized rejection, cap behaviour, the
  device-group attribution and the two real native-driver bundles under
  `fixtures/irpf/` run by default; the exact-count round-trips of the retained
  hrx-b99 bundles and of the Loom hybrid serving bundle are `#[ignore]` and gated
  on `ROCPROF_IRPF_DISPATCH_BUNDLE` / `ROCPROF_IRPF_QUEUE_BUNDLE` /
  `ROCPROF_IRPF_LOOM_HYBRID_BUNDLE` (README, "Tests that do not run by default").
- Collection: tracer `hrx` in `src-tauri/src/mcp.rs` (`hrx_profiling_flags`,
  `hrx_report`, `Gate::Hrx`) for `profile_run`, `profile_start` and
  `profile_stop`, plus `native_kfd_hint` for an empty rocprofv3 result.
- Loom compile-report importer: `crates/profile-core/src/loom_report.rs`, reached
  through the same router by the `.compile-report.json` suffix or a
  `loom.compile_report` content sniff, so a report named `.summary.json` or
  `.details.json` still opens. Tests in `crates/profile-core/tests/loom.rs`:
  synthetic validation and rejection, detail-flag gating, absent-stays-absent, the
  benchmark-envelope path, the binary-level routing and regeneration-wording
  checks (the pre-schema gist under both filenames, and unrelated JSON keeping its
  message), and the import of the real vendored `fixtures/loom/` envelope all run
  by default; the validation against the retained real reports is `#[ignore]` and
  gated on `ROCPROF_LOOM_REPORTS_DIR` (default `/devel/tools/loom-reports`), and
  now also sweeps the `benchmark-live` bundle directory when present.
- MCP queryability: `src-tauri/src/mcp.rs` adds `compileReports` to the
  `capture_query` dataset dispatch (helpers `compile_report_rows` /
  `target_resource_facts` / `instruction_mix_facts`), tested by
  `mcp::tests::compile_reports_dataset_surfaces_labelled_static_facts`.
