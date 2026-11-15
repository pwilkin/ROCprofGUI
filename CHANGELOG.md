# Changelog

All notable changes to rocprof studio are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to follow [Semantic Versioning](https://semver.org/spec/v2.0.0/).

Releases are recorded here without git tags. 0.4.0 is the current state of `main`.

## [Unreleased]

## [0.4.0] - 2026-11-15

A feature release for HRX/Loom applications on the native AMD driver, which dispatch PM4 on their own KFD user queues and are invisible to rocprofv3. A new tracer, `hrx`, has the application record its own IREE HAL profile bundle, on the MCP profile tools and in the desktop Live profiling panel; the IRPF importer attributes device groups and, from a Loom tool built with the driver's thread-trace and counter producers, attaches sampled waves, ISA rows and per-dispatch hardware counters. Everything is additive except the stricter `live_profile_start` validation and the rejection of dispatch-less bundles noted below.

### Added

- Tracer `hrx` for `profile_run`, `profile_start` and `profile_stop`: collects the native IREE HAL profile of an HRX/Loom application that dispatches PM4 on its own KFD user queues (the native AMD driver used by `loom-serve`, `loom-run` and `loom-benchmark`), which rocprofv3 cannot observe in any trace, counter or ATT mode. The `hrx` object selects `families`, `flushIntervalMs`, `filterFunction` and `allowPartial`. `profile_stop` interrupts the application and waits for the bundle to be finalized; the report carries a bundle summary and a warning for each way it can fall short.
- `docs/hrx-device-profiling-allow-partial.patch`, a hrx-system tooling patch that lets each device of a GPU + NPU device group keep the profiling families it implements. Without it a hybrid Loom deployment aborts when asked for dispatch timing.
- Kernel family `loom-internal` for the `__command_launch_counts_N` kernels the Loom compiler synthesises.
- Tracer `hrx` in the desktop Live profiling panel, with fields for the data families (the default set or a custom one), the flush interval, the function filter and partial families per device. The panel has no trigger for it, states that the bundle is dispatch-level by default (waves and ISA need the thread-trace option, hardware counters the counters option) and that an XDNA device may record no dispatches, and on stop shows the bundle summary and every warning.
- The bundle summary of an `hrx` run reports dispatch and queue event counts per device producer (`perProducer`), and the report warns when one device of a group recorded no dispatch while another did.
- `tests/live-profiling-hrx-real.spec.ts`, a real-hardware click-through of the panel's `hrx` branch against a private headless instance. It runs only when `ROCPROF_HRX_PANEL_E2E` names its configuration.
- Waves and ISA rows for HRX/Loom kernels on the native AMD driver. The IRPF importer reads `executable-traces` chunks (SQTT thread traces the driver takes on its own PM4 queue) and their `executable-trace-captures` records: each trace is staged with the bundle's embedded ELF code objects as a placed manifest, decoded by the worker the RADV arm uses, and attached to the dispatch capture. Waves are placed on the dispatch axis through the trace's realtime anchors, which carry the same GPU timestamp counter as the dispatch events. This needs a Loom tool built from hrx-system branch `profiling/executable-traces` (not upstream) on a gfx11.5 GPU.
- The capture states what a thread trace covers: the traced workgroup processor, SIMD and shader engines, the buffer size, bytes written, dropped tokens, whether a stable power state was held, each trace's covered window and share of its traced operation, and that wave and instruction counts are a sample of about 2% of waves, not a census (`metadata.coverage`, the `hrxTrace*` keys and warnings).
- `hrx.trace` for the `hrx` tracer (`targetCu`, `simdSelect`, `shaderEngineMask`, `bufferSize`, `maxTraces`), valid only with `executable-traces` in `hrx.families`, and the same fields behind a "Thread trace (waves and ISA)" option in the Live profiling panel.
- The `hrx` report warns when a requested family produced nothing (`executable-traces` with no trace partition, counter families with no sample), names a truncated thread trace, and counts trace partitions and their bytes in the bundle summary (`executableTraces`, `executableTraceBytes`, `counterSamples`).
- Hardware counters for HRX/Loom kernels on the native AMD driver. The IRPF importer reads the `counter-sets`, `counters` and `counter-samples` families and the driver's `counter-captures` record: every sample becomes one event-counter row per counter, joined to its dispatch by dispatch event id and carrying the kernel, grid and workgroup size, and every counter becomes a per-dispatch series. Names and units come from the bundle; a counter's value is the sum of its value slots (one per shader engine or per workgroup processor). This needs a Loom tool built from the hrx-system branch with the PM4 counter producer (not upstream) on a gfx11.5 GPU.
- The capture states what a counter value is and is not: a window sum over the sampled dispatches, exact for event counts only among the producing device's queues and only when the driver reports an exclusive sample window, an upper bound when the bundle cannot tell, and never per-dispatch for cycle counters. It also states the sampled share of the dispatches, the bound, operations that ran unsampled, dropped and invalid samples, and another collector's perfmon state (`metadata.coverage`, the `hrxCounter*` keys and warnings).
- `hrx.counters` for the `hrx` tracer (`names`: the sets `sqg` and `instructions` or single counter names; `maxSamples`), valid only with `counters` in `hrx.families` and combinable with `hrx.trace`, and the same choice behind a "Hardware counters (per sampled dispatch)" option in the Live profiling panel. The tracer always names the counter set, so no device of a GPU + NPU group starts a set of its own.
- The bundle summary of an `hrx` run reports `counterSets`, `counterNames` and `counterCapture`, and the report warns when counters were produced but the bundle cannot tell what a sample window held, when another collector reprogrammed the counters, when the sample bound was reached and when operations ran unsampled.

### Changed

- The `hrx` report no longer says the native AMD GPU driver has no counter family; a `counters` request that produced no sample names the driver build, GPU generation and queue protocol it needs.
- The IRPF importer attributes every chunk to its own session, physical device and queue, resolves function names inside the dispatch's session, keeps each device's clock domain apart, and names a device that recorded no dispatch. Agents now read `HRX device N (producer)` when the producer names itself, and multi-queue bundles read `HRX queue N`.
- An IRPF bundle with no dispatch record is rejected with what it holds (sessions, producers, functions, queue events) instead of opening as a capture with 0 dispatches.
- The desktop `live_profile_start` command validates its request against the MCP `profile_start` input schema before starting anything, for every tracer. Unknown keys and unsupported values that the desktop command used to ignore or reject later are now rejected up front with the schema's messages.
- `decode.py`'s placed-manifest branch accepts an optional `start` and a `loads` list per code object and keeps a kernel's own ELF symbol name when an entry has no `name`. RADV manifests carry neither and decode as before.
- The capture model gains `waveTimeMaps`, serialized to the desktop frontend when present: piecewise-linear maps from a decoded wave file's own clock onto the capture axis, applied wherever wave files are read, so the wave tools return tokens already on the capture axis. `capture_query` has no dataset for the maps themselves. Captures without them are unchanged.

### Fixed

- `profile_run` and `profile_stop` no longer return a silent empty file list when rocprofv3 writes nothing for an application that opens `/dev/kfd` without loading the ROCm runtime; the report names the cause and points at tracer `hrx`.

### Upgrade notes

- An installed binary, including one run as a systemd service, must be rebuilt and reinstalled to pick up 0.4.0. Bumping the source version does not change an installed service.
- Tracer `hrx` needs an IREE HAL tooling binary that defines `--device_profiling_mode` and `--device_profiling_output` (`loom-serve`, `loom-run`, `iree-run-module`). The tracer appends and manages the `--device_profiling_*` flags, so an argv that already carries one is rejected.
- Waves and ISA rows need a Loom tool built from hrx-system branch `profiling/executable-traces`, and hardware counters one built from `profiling/counters`, which contains the former. Both branches are local and not upstream. A stock tool rejects the `executable-traces` and `counters` families, and the `hrx` report says so.
- A GPU + NPU device group needs a Loom tool built with `docs/hrx-device-profiling-allow-partial.patch` and `hrx.allowPartial: true` to record GPU dispatch timing. Without the patch the group aborts when asked for `dispatch-events`.
- The desktop `live_profile_start` command now rejects a malformed request for any tracer (an unknown key, an unsupported value, a tracer-only key on the wrong tracer) that it used to ignore or reject later. A caller of the Tauri command that sent such keys must drop them. The MCP `profile_start` tool already validated its input and is unchanged.
- An IRPF bundle with no dispatch record no longer opens as a capture with 0 dispatches; `capture_open` rejects it and states what the bundle holds.

### Limits and compatibility

- Thread traces and counters are implemented by the native AMD GPU driver for gfx11.5 and USER-published PM4 queues only. Other GPU generations, AQL queues and KERNEL-published queues return unimplemented. The collection and the numbers quoted here were measured on gfx1151 only.
- A thread trace is a sample of about 2% of waves, never a census: one SIMD of one workgroup processor per shader engine. Wave counts, instruction hit counts and hotspots describe the sampled waves only; per-kernel time still comes from every dispatch event. Measured on gfx1151 on a hybrid Loom run: 12,295 dispatches, 3,464 waves, 38,964 ISA rows (10,428 with hits), stable power state held.
- A recorded command buffer is traced whole or not at all, and a full buffer truncates the trace. The driver default of 16 MiB per shader engine covered 31 ms of a 577 ms recorded execution in the measured run. `capture_open` decodes every trace partition, at about 1.5 s per MiB of trace in that run.
- Counter values are window sums over the sampled dispatches. Event counts (waves, work items, instructions) are the dispatch's own only among the producing device's queues and only when the capture reports an exclusive sample window; otherwise they are upper bounds. Cycle counters (`SQG_BUSY_CYCLES`, `SQG_WAVE_CYCLES`, `SQG_CYCLES`) are never per-dispatch.
- Only the first dispatches matching the filter are sampled, up to `hrx.counters.maxSamples` (driver default 4,096; the tracer accepts 1 to 1,000,000). Later dispatches carry no counter values, and totals cover the sampled dispatches only. Measured on gfx1151: 7,489 dispatches, 4,096 samples, `SQG_WAVES` equal to the dispatch geometry in 4,064 of the 4,064 samples with a recorded grid.
- Sampling stretches the run: about 44 µs per dispatch for the `sqg` set and about 210 µs with the `instructions` set added, measured on gfx1151 inside one recorded execution. Dispatch intervals moved by about 2%.
- Loom kernels on gfx1151 run as 32-lane waves, so the expected `SQG_WAVES` of a dispatch is its workgroup count times `ceil(threads per workgroup / 32)`. The workgroup count is the capture's grid divided by the workgroup size per dimension.
- The per-workgroup-processor `SQ_*` counters are 32 bits wide. The XDNA driver's own counter set is not collected through this tracer, the `counter-ranges` family is not implemented by the native GPU driver, and kernels placed on the NPU never appear as dispatches.
- Tracer `hrx` records from process start to exit. There is no trigger or window and no attach to an already running server, and the Live profiling panel reports the `.irpf` path without opening it.
- Existing inputs are unaffected: bundles without trace or counter chunks import as before, RADV placed manifests decode as before, and the import caps (`ROCPROF_MAX_*`) are unchanged and apply to the new trace and counter data.

## [0.3.0] - 2026-10-14

### Changed

- MCP queries select typed rows before JSON serialization, preserving row shapes, serialized-JSON search and `nextOffset`. Additive `effectiveLimit` exposes the 1000-row clamp; zero remains count-only. Explicit guards bound rows to 4 MiB and selected arrays/unpaged responses to 16 MiB.
- Private worker transport v2 carries counted dependency projections. Kernel aggregation streams name/start/end records through the GUI's shared incremental reducer, preserving exact arithmetic, interval unions and stable ordering; paging applies after aggregation.
- Timeline analysis projects the requested range, retains bounded windows and streams HTML with glyph, overlay and output budgets. Inline output permits 16 MiB / 100,000 glyphs; new-file output permits 512 MiB / two million glyphs. `maxOutputBytes` can lower the byte ceiling. Responses add clock, coverage and effective-budget context.

### Fixed

- Timeline points at lower and split boundaries appear exactly once. Output publication validates the complete document and commits atomically without replacing an existing path; late cancellation preserves a completed publication.

### Limits and compatibility

- Import memory is unchanged. Exact kernel aggregation still scans all records, O(N), with bounded group/name/interval state. Other analyses still materialize relevant projections under a 256 MiB encoded-input cap; diagnostics retains full-capture compatibility. This release does not make all analysis out-of-core or establish an RSS ceiling. Import overrides, the 1024 MiB Node heap and the 120-second deadline are unchanged.

## [0.2.1] - 2026-10-02

A review-hardening release that applies the findings of an adversarial code review of the 0.2.0 remediation arc. There are no new features; the changes below make the live-profiling, import and tuning paths safer and more honest.

### Fixed

- The desktop GUI no longer freezes for up to 180 seconds while a live profile is stopped or triggered. Status polls never block on the live-profile lock, and the detach wait runs without holding it.
- Live-profile detach escalates instead of killing outright: it writes a newline, then sends SIGINT after about 30 seconds, then SIGKILL only past 180 seconds. The previous SIGKILL-only path could truncate rocprofv3's synchronized output files and leave a surviving target in an unknown attach state.
- A failed attach-window launch no longer poisons the session with a "File exists" error; its directory and the collection it would have committed are rolled back. The rocprofv3 attach-argument cache under `/tmp` (`rocprofv3_attach_<pid>.pkl`) is removed when the target stops and again at shutdown, so a reused PID does not inherit a stale attach spec. Ending an already-exited timed window reports that it detached instead of a misleading error, and ending a window rejects stray collection options.
- The IRPF importer honors the producer's truncation contract. A truncated dispatch-events chunk is no longer silently relabelled as queue mode; truncated or dropped records and a failed session produce loud warnings; and a dynamic (all-zero) workgroup size renders as "Not recorded" instead of a fabricated grid.
- A Loom compile report within the per-file size cap can no longer exhaust memory before the record-cap check: the importer rejects an over-cap collection before allocating for it. The Loom importer is also cancellable now, and validates wait-reason inventories like the strict version-zero loader.
- `ROCPROF_MAX_FILE_MIB` now applies to `.rgp` imports. It was hard-coded to 256 MiB, so the variable was silently ineffective for RADV captures.
- `kernelIterationRange` rejects degenerate values such as `-`, `,` and `0-3,,` that passed the old character-set check, and a blank or degenerate kernel name no longer crashes the tuning worker.
- The tuning and shader workers no longer hang on an uninterruptible descendant after a kill: a timed-out process tree is reaped with a bounded post-kill wait.
- Untracked bytecode and local scratch that had been swept into earlier commits are removed from version control.

### Changed

- "Stop and collect" is cancellable from the desktop, through a shared cancellation flag on the live-profile control.
- The headless CLI uses the same cancellable, input-fingerprinted import router as the GUI and MCP, and accepts `.rocprof-att` manifests. The earlier CLI/desktop import divergence is gone.
- The desktop Live profiling panel validates the window duration against the 1 to 600000 ms bound the MCP schema enforces, exposes the ATT sub-options (`targetCu`, `simdSelect`, `bufferSize`, `shaderEngineMask`) and the kernel iteration range, and reports errors in the panel's own vocabulary rather than raw MCP tool names.
- Wave-budget and row-cap rejection messages interpolate their effective constants, so the message a user sees cannot drift from the enforced bound.
- The rocprofv3 option vocabulary lives in one key table, and a contract test fails on a one-sided edit instead of letting a rocprofv3-only option be silently accepted on a RADV run.

### Security

- The tuning and shader workers no longer inherit `LD_PRELOAD`, `HSA_TOOLS_LIB` or `HSA_TOOLS_REPORT_LOAD_FAILURE` from the launching environment, and reject them if passed back in `env`, so a profiler library the app itself was launched under cannot perturb or instrument the kernel being measured.

### Upgrade notes

- An installed binary, including one run as a systemd service, must be rebuilt and reinstalled to pick up 0.2.1.
- The headless CLI now computes the same input fingerprint as the desktop and MCP. A capture opened through the CLI participates in the same input-change detection: a file that changes during import is rejected, and the capture-archive overwrite guard refuses to overwrite an archive whose source file has since changed.

## [0.2.0] - 2026-10-02

A remediation release driven by user feedback, grouped by theme: live-decode profiling, input caps, HRX/loom support, the source flamegraph, and kerncap parity.

### Added

- Live-decode profiling: a desktop **Live profiling** sidebar panel for launching an application and collecting gated windows without a human at the MCP keyboard. It drives the same core as the MCP tools through four `live_profile_start`/`trigger`/`status`/`stop` Tauri commands.
- Kernel filters on `profile_run` and `profile_trigger`: `kernelIncludeRegex` and `kernelExcludeRegex`, mapping to rocprofv3's `--kernel-include-regex` and `--kernel-exclude-regex`, so a window can trace one expensive dispatch instead of the first small kernels that share its name.
- `kernelIterationRange` on `profile_run` and `profile_trigger`, mapping to rocprofv3's `--kernel-iteration-range` (a value like `64` or `0-3`). It is the only way to reach a dispatch other than the first occurrence of a kernel name, which a default per-dispatch ATT trace samples once.
- Triggered profiling windows: `profile_trigger` with `triggered: true` and no `windowMs` stays attached until you end it with `{"end": true}` (which writes a newline to rocprofv3's stdin so it detaches) or `profile_stop`, so you can capture exactly one request without guessing a duration. `profile_status` reports `windowTriggered` for such a window.
- Environment-configurable import and analysis caps: `ROCPROF_MAX_RECORDS` (default 2,000,000), `ROCPROF_MAX_FILE_MIB` (256), `ROCPROF_MAX_HIDDEN_TOKENS` (4,000,000), `ROCPROF_MAX_ATT_TRACES` (256) and `ROCPROF_MAX_ATT_CODE_OBJECTS` (256), resolved in the new `crates/profile-core/src/caps.rs` and applied across desktop, headless CLI and MCP.
- HRX/loom support: a new IRPF v2.0 importer (`crates/profile-core/src/irpf.rs`) for HRX native profile bundles produced with `HRX_PROFILE_MODE=queue` or `dispatch`. It reconstructs resolved dispatch and function names, grids, code-object identity and clock correlations. The `.irpf` extension is wired into the import dispatcher and the desktop file picker, and bundles are also recognized by content when the filename differs.
- Loom compile-report import: a new importer (`crates/profile-core/src/loom_report.rs`) for Loom's version-zero compile reports from `loom-compile` (`*.compile-report.json`) and from an `iree-benchmark-loom` benchmark envelope. It mirrors the strict version-zero loader (`loom/py/loom/reporting/compile_report.py`), rejecting any other schema version, an unknown mode, a non-object or failed status, entry count or index mismatches, duplicate entry identity and the residency-constraint invariant violations, and it refuses the pre-schema public sample. Accepted reports are surfaced as static compile facts (identity, static and dynamic instruction mix, target resources, allocation and spill facts, wait plan, workload geometry, residency and constraints, move causes, detail rows) on a `compileReports` capture surface kept separate from dispatches and measured data. Every field is optional and absent stays absent; the importer does not infer cycles, dynamic counts, hardware register counts, occupancy as throughput or bank-conflict freedom. Provenance is read out of band from `tool-provenance.json` and `commands.json` sidecars. The report is recognized by the `.compile-report.json` suffix or a `loom.compile_report` content sniff. The imported facts are queryable over MCP with `capture_query` and dataset `compileReports`, which returns one labelled row per report (unit `static`, version-zero/same-compiler-horizon) with report- and per-entry identity, detail flags, target resources (SGPR/VGPR, subgroup geometry, static occupancy percent, limiting resource, residency), instruction mix, allocation/spill and emitted-instruction counters, wait plan and workload geometry, never presented as measurements. A schema-mismatched or pre-schema report is rejected with regeneration guidance under both filenames — naming the expected `kind`/`schema_version 0` and the regenerate-with-this-compiler instruction — while unrelated JSON keeps its existing message. The importer is verified end-to-end against a real `iree-benchmark-loom` envelope produced on gfx1151 by the provider-enabled tool (pinned hrx-system `32b86d42`), vendored with provenance at `fixtures/loom/`. See [HRX profiling](docs/hrx-profiling.md).
- HRX kernel-name families: `ggml_*` names are classified as hrx-ggml, and `iree_hal_amdgpu_device_*` names as internal runtime kernels (hidden from the Tune button like RADV's internal shaders).
- A shared read-side kernel-name classifier, `src/kernelName.ts` (`classifyKernelName`, `kernelBase`, `isInternalKernel`, `registerKernelFamily`), as the single plug-in point for backend families.
- A "Group by" control in the Source flamegraph viewport, grouping by source location (the default) or by code object (a pipeline/kernel label for RADV captures, the ELF code-object id for HIP/ATT captures). The grouping is saved per capture, each grouping keeps its own viewport, and the SVG export caption records which produced it.
- HIP kernel-tuning replay conditions `sustained` (Kerncap's `--no-recopy` path) and `repeats`, with a mode echo-verify: a session refuses a result whose reported mode does not match the request, so an un-rebuilt runtime cannot silently ignore a condition.
- New documentation: `docs/hrx-profiling.md`, `docs/kernel-name-layer.md` and `docs/kernel-tuning-parity.md`, plus a documentation site changelog page.

### Changed

- Import and analysis caps are now configurable rather than fixed, but they remain explicit rejection boundaries: defaults are unchanged, nothing is silently sampled, and the rejection message interpolates the effective value. The wave-viewport budgets (128 waves / 500,000 tokens), the 4,096-wave statistics budget and the six-open-capture limit were deliberately left fixed, because those paths are not yet paged.
- The same-collection-per-process rule for rocprofv3 attach windows now also covers kernel filters, iteration range and window mode (timed versus triggered), in addition to streams, counters and ATT options. rocprofv3 pickles each process's attach options and reattaches only with identical ones, so a mismatch is rejected up front.
- `profile_trigger` now retries a busy-queue attach abort (the ROCm `set_profiler_active_on_queue` handshake) with backoff while the target is alive, reporting `attachAttempts`. A persistent failure is surfaced as an `attachWarning`, with a per-window warning at `profile_stop` carrying rocprofv3's stderr tail. This is a known ROCm limitation, not a fix for it.
- `docs/profiling-caveats.md` records that a default per-dispatch ATT trace samples the first occurrence of each kernel name, and that `kernelIterationRange` is how to target a later dispatch.
- `docs/kernel-tuning-parity.md` records what was deliberately not done for HIP tuning (`--memory` and `--flush-mb`) and why Kerncap's fixed-address restore on a unified-memory APU makes them meaningless there.

### Fixed

- rocprofv3 counter-collection CSVs keep their grid and workgroup. The importer now reads whichever columns the header provides: the kernel trace's `_X`/`_Y`/`_Z` triple, or the single scalar `Grid_Size`/`Workgroup_Size` column a counter CSV carries. Counter CSVs previously rendered an empty `× ×` because the importer always formatted the triple.

### Upgrade notes

- The import and analysis caps are environment-configurable now. No action is required, because the defaults equal the previously shipped values; set a `ROCPROF_MAX_*` variable before launch only if a large capture is being rejected.
- An installed binary, including one run as a systemd service, must be rebuilt and reinstalled to pick up 0.2.0 and the new behavior.
