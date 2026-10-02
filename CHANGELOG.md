# Changelog

All notable changes to rocprof studio are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to follow [Semantic Versioning](https://semver.org/spec/v2.0.0/).

This project is not yet published or tagged. 0.2.0 is the current state of `main`.

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
