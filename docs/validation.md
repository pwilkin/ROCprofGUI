# Validation record

Date: 2026-09-19. Workspace: `/devel/tools/rocprofgui`.

> **Read this as a chronological log, not a current status page.** Sections are appended per milestone and later ones supersede earlier ones without editing them — several passages describe as "unimplemented" behavior that a later section implements. The "Passed" table immediately below records the **2026-09-19** state only; its 35 Rust / 18 browser figures have been overtaken many times within this same file. For the latest measured counts see the regression checkpoint in [upstream-parity.md](upstream-parity.md), and treat any count here as dated to the section it appears in.

## Passed

| Check | Result |
|---|---|
| `cargo test -p profile-core` | 35 integration tests passed; optional real ATT test run separately |
| `python3 scripts/check-hidden-oracle.py` | Exact aggregate hidden-latency agreement with upstream C++ across 40 seeded generated captures |
| `node scripts/check-summary-oracle.mjs` | 640 aggregate peaks and 80 sampling intervals matched unchanged upstream C++ |
| `node scripts/check-tensor-oracle.mjs` | 570 expressions matched upstream C++ across 30 generated captures with validity gaps |
| `npm test` | 43 analysis/expression/reference/library/worker/summary/sampling/source/layout tests passed |
| `npm run build` | TypeScript and Vite production build passed |
| `cargo check -p rocprof-studio` | Native Tauri check passed |
| `npm run tauri build -- --debug --no-bundle` | Standalone Linux executable built with embedded assets |
| `npm run test:ui` with installed Chromium | 18 browser tests passed |
| Native application under isolated Xvfb + software rendering | Window remained running; rendered screenshot inspected |
| npm dependency audit after test-runner update | 0 reported vulnerabilities at install time |
| `npm ci --dry-run --offline` | Lockfile installation plan resolved |

Executable: `target/debug/rocprof-studio`. Native rendering evidence: [native-desktop.png](native-desktop.png). Browser screenshots: [workspace.png](workspace.png), [compute-fabric.png](compute-fabric.png).

The native smoke test used an isolated virtual display, launched only the new desktop executable, and terminated that process after inspection. It did not start rocprofv3, run a benchmark, or modify other projects.

## Parser test coverage

- Wave hierarchy, token/ISA links and source snapshots.
- Exact integer timestamp-origin subtraction above JavaScript's safe integer range.
- Dispatch deduplication preserving different process identities.
- Missing numeric metadata versus observed zero.
- Reversed dispatch timestamps and missing CSV headers.
- Paired occupancy with overlapping dispatcher identities.
- Unmatched occupancy boundaries reported and excluded.
- Sidecar traversal and symlink escape rejection.
- Malformed/negative wave token durations.
- Separate SPM instances with preserved timestamp origin.
- Bounded wave queries refusing more than 128 wave files.
- Malformed counter values rejected instead of converted to zero.

## Analysis and UI tests

Analysis tests cover interval union, bounded viewport ranges, occupancy balance, selected-wave/range statistics, expression precedence/functions, division by zero, unknown variables and rejection of JavaScript-like input.

Browser tests cover CU-wave aggregation, visible-wave cost scope, ISA search and linked source selection, loop-occurrence controls, CU selection, occupancy plotting, derived counter success/error, flamegraph drill-down, evidence warnings, timeline zoom, range reset, selection export and browser-only import guidance. Additional checks cover hidden versus total latency, full selection history, section folding, histogram range selection and ISA-based wait relationships across loop occurrences. They check the absence of page exceptions and horizontal page overflow at 1500 × 1050.

## External format checks

The headless Rust importer was run against sources outside the synthetic example:

| Input | Observed result |
|---|---|
| Upstream viewer `tests/pcsampling` | 1,635 ISA records; zero detailed wave files as expected for that fixture |
| Upstream viewer `tests/input/data/spm_results.json` | 6 scoped counter series, original timestamp origin `100` |
| Local rocprofiler-sdk `source/docs/data/kernel_trace.csv` | 9 dispatch records, original timestamp origin `1550151853029637` |

Viewer source was inspected at `ea1ce2b87562b28e3fb5c8819b5e12573cc14424`. These checks validate adapter compatibility with those examples; they do not constitute a full hardware/schema matrix.

## Explicitly not verified

- A newly collected real SQTT trace on this machine. An archived decoder-project gfx1101 fixture was decoded and imported; this is not a newly profiled workload.
- Exhaustive native dialog/IPC coverage. Definition file import/export and the SDK 3.0.4 ROCPD fixture were exercised through actual Linux native dialogs; raw ATT directory selection and cancellation still need native interaction coverage.
- Full behavior parity with upstream, especially complete decoder input workflows, summary hardware qualification, SPM clock alignment and complete counter schema coverage.
- Multi-gigabyte captures, import cancellation, long-running memory behavior, packaged installers, Windows or macOS.
- Any kernel performance improvement, replay correctness, tuning or LLM experiment.

The profiling handoffs are historical evidence used to design validity boundaries. Their workloads and old measurements were not rerun in this session.

## Hidden-latency compatibility

The oracle compiles the unchanged upstream C++ hidden-latency implementation against minimal data-model adapters, then compares instruction-level results with the Rust implementation for 40 deterministic generated multi-SE/SIMD captures, including overlapping execution, matrix instructions and other-SIMD activity. This verifies algorithm compatibility, not the physical accuracy of the upstream hiding model. The UI clamps hidden contributions separately to their matching idle/stall/issue components when displaying nonhidden values.

Wait sidecars use ISA line identities, not token array indexes. Parser and browser regressions cover this distinction; the diagram deliberately does not claim occurrence-specific dependencies.

## Dimension-aware counter analysis

The tensor oracle compiles unchanged upstream `derived_counter.cpp` and compares 19 expressions over 30 deterministic captures, including masked samples. Coverage includes axis reductions, broadcasting, comparisons, logical operations, slices, negative selection/removal, adjacent deltas and linear axes. Comparisons allow the expected float32-versus-JavaScript-float64 rounding difference (relative tolerance 1e-5). Non-finite values display as missing, including division by zero.

Local tests additionally cover cyclic definitions, built-in dimension sizes, invalid syntax and dimensions, allocation bounds, ATT bank separation, SPM agent separation, singleton instance broadcasting, duplicate-coordinate rejection and preservation of selected hardware/time identities. Browser checks exercise evaluation, result rendering, formula persistence and cyclic-definition errors.

ATT dimensions are recognized from imported SE/CU/bank scopes; SPM dimensions use XCC, shader engine and INSTANCE (upstream CU-axis convention). Unknown dimensions stay separate domains. This is not yet complete support for all producer schema variants. Dimensioned tensor construction and expression evaluation run in a cancellable worker with bounded input, raw tensors, cumulative allocations and returned results; see the worker limits below. Very-large-capture ingestion and rendering still require separate qualification. Derived units are user-defined, not inferred by dimensional analysis.

## Raw ATT integration

The local decoder project's archived `test/data/navi3_float.tar.gz` was extracted under `/tmp`, decoded by `rocprof-trace-decoder` Python 0.2.2 and `/opt/rocm/core-10.0/lib/librocprof-trace-decoder.so.0.2.0`, and imported through the Rust worker. The opt-in integration test passed: 8 detailed waves, 3,277 ISA records, 64 occupancy intervals and 96,020 tokens loaded lazily from the first wave. ISA hit data was nonzero. No GPU workload was launched.

Automated worker tests cover successful output ownership across cloned capture handles, cleanup after the last handle is dropped, decoder stderr/nonzero exit, timeout, and cancellation. An occupancy regression covers omitted dispatcher metadata on end records and refuses ambiguous fallback matches.

The decoder Python package was installed into the ignored project-local `.decoder-venv` for validation. This dependency is not yet bundled into application installers. Native cancellation IPC is wired but still needs automated native interaction coverage.

The same optional integration test also passed on archived `mi300_perf` (gfx942: 4 detailed waves, 1,146,178 ISA records, 40 occupancy intervals, 4,547 first-wave tokens) and `navi4_sdata` (gfx1200: 8 detailed waves, 3,564 ISA records, 256 intervals, 7,814 first-wave tokens). The MI300 import took about 40 seconds in this environment. These are parser/decode checks; million-record webview interactivity and memory use have not been qualified.

## ROCPD database ingestion

The Rust reader uses bundled SQLite in read-only/query-only mode with trusted-schema execution disabled. It reads physical UUID-partitioned tables directly, avoiding duplicate records from aggregate views. GUID-qualified symbol joins preserve process identities, and integer timestamp subtraction precedes conversion to frontend numbers. A SQLite progress callback supports cancellation and a five-minute query deadline.

Five ROCPD tests cover exact timestamps above JavaScript integer precision, input-file byte preservation, unknown versus zero resources, multiple GUIDs and UUID tables, unsupported/malformed schemas, missing kernel symbols, reversed intervals, early cancellation, and a fixture generated with the unchanged ROCm SDK 3.0.4 SQL schema and upstream synthetic data generator. The official fixture imports two kernel dispatches and two dimensioned SPM series; an SPM-only variant rebases its sample timestamps independently.

This is partial ROCPD support. Event-only PMC records are reported as not yet displayed. Embedded/external ATT extraction, legacy schemas, and merging datasets with different clocks remain open. No process environment table is read or copied into the GUI.

The ROCPD browser regression uses the Rust-generated JSON projection of the official database fixture. It verifies two dispatches aggregated into one kernel row and a dimension-aware SPM reduction of 32, with no detailed-wave assumptions or page exceptions. The SDK 3.0.4 fixture was subsequently imported through the actual native dialog/IPC path; see the native workflow checks below.

## Instrumentation markers

Four Rust regressions cover nesting/transitions/points, code-object-scoped identities, incomplete and unknown scopes, payload skipping, and missing context. Three frontend tests verify clipped inclusive flame widths, incomplete scopes, and repeated-path aggregation. The marker oracle compares 60 seeded streams against unchanged upstream C++ `marker_walker`; all scope IDs, times, depths and point/open flags matched. Browser coverage includes timeline/flame rendering, scope selection, range zoom and source navigation. Legacy timestamp encodings, ambiguous hardware-slot reuse and real instrumented hardware goldens remain unqualified.

## Offline ISA documentation

The generated catalogue covers all nine requested architecture generations, 10,628 instruction entries and 195 matrix instruction variants. Tests verify all instruction-to-encoding/operand-format references resolve, source hashes are present, older CDNA spellings resolve, RDNA 1/2 have no native WMMA entries, RDNA 3/3.5 have six each, and RDNA 4 has eleven dense and eleven sparse variants. The browser regression follows a selected trace instruction into the reference, handles an unavailable architecture, drills down from matrix compatibility into an exact FP8/BF8 variant, and switches to CDNA MFMA. A second reference regression closes every capture and blocks external network requests, then loads CDNA 4 scaled MFMA and the matrix compatibility table.

The XML archive SHA-256 is `82404f1126761b7877595b622afa7e1f311f2f41e89a3abe9aaf8ad045c082e2`. All nine XML documents declare release 2026-02-20. Local LLVM `llvm-mc --arch=amdgcn --mcpu=gfx1151 --show-encoding` confirms the three explicitly supported spelling aliases (DS_READ_B32, GLOBAL_LOAD_DWORD, S_LOAD_DWORDX4). Other compiler aliases are not claimed. Architecture choice is explicit; the reference does not infer device instruction support or throughput from the capture.

[Reference screen](isa-reference.png). The catalogue is documentation, not an assembler, disassembler or hardware validation result. PDF notes are checked against the linked AMD RDNA 3.5, RDNA 4 and CDNA 4 manuals; XML text and versioned provenance remain inspectable in the application.

## Definition library and native workflows

The definition editor now keeps up to 32 independent drafts (256 KiB each, 2 MiB total), remembers the active file, imports upstream text files without replacing drafts, and exports plain UTF-8 text. Unit tests cover collision handling, malformed stored libraries, UTF-8 size bounds, NUL/binary input, and selected-file edits. Two Rust tests verify byte-preserving reads/writes, replacement, rejected writes preserving the previous file, and bounded imports. The browser round trip verifies CRLF/comments survive import/export unchanged, draft selection survives reload, unevaluated edits survive view navigation, and failed imports preserve the current draft.

A real native Tauri session ran under isolated Xvfb/software rendering with temporary XDG data/config/cache directories. Through the actual GTK file dialogs and Tauri IPC, `input.def` was imported and exported to a separate `output.def`; all 32 bytes matched. [Native definition workflow](native-definition-files.png). No user definition file was touched.

The same native probe exposed a stale-wave request when switching from the demo trace to a wave-less ROCPD capture. The frontend now checks wave membership in the active capture before issuing the request. A browser IPC regression verifies no previous-capture wave ID is requested and that switching back restores the demo. The rebuilt native app was then retested with `fixtures/rocpd/sdk-3.0.4.db`: it opened without the error banner and displayed two dispatches grouped as one kernel, with summed and union durations of 200,000 ns. [Native ROCPD import](native-rocpd-import.png).

The virtual-display GTK run emitted widget-realization diagnostics while closing dialogs; the app remained running, completed the operations, and returned to the workspace. Native raw ATT selection, active decoder cancellation and platform variants remain separate qualification work. Definition execution was subsequently moved to a worker; see the checks below.

## Cancellable counter worker

Tensor domain discovery no longer allocates dense tensors on the UI thread. The selected domain is sent to a dedicated module worker, which builds its tensors and evaluates the definitions. Success, error, timeout, explicit cancellation, editing the draft, changing domains, capture changes and leaving the counter view release or terminate the worker. Generation checks discard stale responses. NaN validity gaps, axis identities and scalar versus time-series results survive structured cloning.

Current limits per evaluation: one million input samples, four million raw tensor cells across the selected domain, eight million cumulative newly allocated tensor cells, two million returned result cells, 256 KiB of definition text, and 30 seconds of elapsed time. The existing two-million-cell individual tensor limit also applies. These are explicit processing limits, not an asserted process-RSS ceiling; JavaScript objects, indexes and transport copies use additional memory. Output limits count named results even when several definitions alias the same tensor. UI pagination creates at most twelve series descriptors instead of expanding every hardware coordinate into an object.

Seven added unit tests cover structured-clone hydration with gaps and selected time indexes, sparse-domain discovery without dense allocation, input/raw/intermediate/output bounds, a 700,000-sample alias case, pagination of 100,001 result series, cancellation with late replies, timeout and startup failure. The browser regression substitutes a busy real worker and verifies cancellation, edit invalidation, unmount termination and successful subsequent evaluation with the normal worker. All existing counter browser tests now exercise the actual worker path. The unchanged upstream C++ oracle still agrees on 570 expressions across 30 captures.

The packaged module worker was also exercised in the native WebKitGTK Tauri application under isolated Xvfb: the demo's hardware-axis reduction returned a correctly labelled 50-sample time series with 50 valid values and rendered its plot. [Native worker result](native-counter-worker.png). No GPU workload ran. Multi-gigabyte captures, worst-case chart rendering, and process-wide peak memory remain unqualified.


## MI200 / MI300 activity summary

The Activity summary view separates whole-capture imported instruction costs (idle, stall/wait and issue) from ATT hardware activity. Issue is latency minus stall; invalid or zero totals are unavailable. The importer records incomplete cost coverage when latency/stall/idle fields are absent or null, so omitted idle data cannot appear as a measured zero. Hardware formulas are enabled only for explicit gfx90a/gfx940/gfx941/gfx942 identities. Missing busy baselines, zero denominators, nonfinite/negative samples, duplicate timestamps or counter identities, and mismatched CU coverage cannot silently become a global utilization value. Per-CU values require that CU's measured denominator. Counter arrays are not mutated.

Average utilization sums raw samples before applying upstream factors: LDS/VMEM/FLAT ×2, MFMA busy cycles ÷4, other activity ×1, divided by BUSY_CU_CYCLES. Peak ratios divide independently aggregated peaks and are labeled accordingly. Values above 100% are retained. Tables distinguish scaled accumulated counts from utilization and display at most 32 CUs per page. Range and hidden-latency controls are omitted because these summaries cover the entire capture.

The peak oracle compiles the unmodified upstream `datanode.cpp` / `datanode.h` at commit `ea1ce2b87562b28e3fb5c8819b5e12573cc14424`, using a no-op UI tracking macro. Eight counters over two banks, three SEs and four CUs, with gaps and skew, produce 640 peak comparisons and 80 sampling interval comparisons. These passed exactly for the generated integer samples. This verifies the upstream aggregation algorithm, not physical simultaneity of peaks or exhaustive float32 rounding behavior. Unit tests separately check average factors and unavailable states.

There is a deliberate total-aggregation difference: this implementation retains every valid raw sample. Upstream's derived-tensor end-bin calculation can exclude a last sample near a grid edge; the summary does not reproduce that truncation. Peak aggregation retains upstream's gap-zero insertion, four-clock coalescing and lack of a terminal zero.

An archived `mi300_perf` trace was decoded again without launching a workload. It produced 1,146,178 ISA entries and 7,852 counter samples in 36 series, but its decoder metadata identifies the architecture only as `vega` and provides no counter names (`Counter 0` etc.). The summary correctly remains unavailable: the filename's gfx942 hint is not promoted into measured counter semantics. Positive hardware summary qualification therefore still requires a capture with explicit architecture and counter metadata. The browser and native positive tests use synthetic inputs, never claimed as GPU measurements.

Browser checks cover unsupported RDNA gating, 200% VALU, 50% MFMA, CU pagination, accumulated-counter toggle, formulas, page errors and horizontal overflow. [activity-summary.png](activity-summary.png) records the synthetic browser fixture. Summary input is currently limited to 500,000 samples to bound synchronous work; larger-capture summary processing remains a follow-up.

The packaged Linux app imported a clearly named synthetic MI300 decoded fixture through the native file dialog and rendered LDS 100%, VALU 200% and MFMA 50% as expected. [native-activity-summary.png](native-activity-summary.png) records the inspected WebKitGTK view. The native probe was terminated afterward. GTK emitted the same nonfatal dialog widget-realization diagnostics seen in earlier native tests. The missing-cost metadata guard was checked in Rust and Chromium, followed by a fresh desktop build.


## PC-sampling details

Decoded `code.json` now preserves `PC_Issued`, `PC_Stalled` and `Stall_Reasons` separately using header positions. Absent/null values remain absent; measured zero stays zero. Unknown reason indices are retained instead of dropped or renamed to a known category. Negative, fractional, string-valued and individually inexact counts are rejected. Counts and aggregate sums that exceed JavaScript's exact integer range cannot appear as exact UI totals.

The dedicated PC sampling view shows whole-capture issued/stalled sample totals, row coverage, aggregate or selected-instruction reason bars, search, sorting, pagination and linked ISA/source navigation. It has no SQTT range or hidden-latency controls. Reason labels follow upstream `config.cpp` at the documented baseline commit. Reason sums are deliberately not forced to equal stalled samples. These observations are statistical samples, not cycle counts or GPU utilization.

`python3 scripts/check-pcs-fixture.py /path/to/upstream/tests/pcsampling` compares every imported PC field with the original decoded fixture. It passed for all 1,635 instructions: 1,306 issued and 3,384 stalled samples, with every reason array identical. Rust tests cover reordered headers, missing versus zero values, unknown categories and malformed counts. Frontend tests cover partial coverage and unsafe aggregate totals. The browser test exercises unknown categories, missing per-instruction reasons, search, absence of SQTT controls and linked ISA navigation without any wave records.

Raw event-format variants remain separate baseline work. Issued- and stalled-sample source flamegraphs reuse the existing source-to-ISA aggregation with explicit sample units, exclude missing weights and support linked ISA navigation. Browser checks verify the selected metric changes the total from 10 stalled to 4 issued samples in the partial-coverage fixture.

The packaged Linux app imported the upstream PC-sampling fixture through its actual native file dialog. Its WebKitGTK view showed 1,306 issued / 3,384 stalled samples and the expected reason distribution, including 777 pipe-stall and 761 barrier samples. The inspected screen is [native-pc-sampling.png](native-pc-sampling.png). The probe was then closed; no profiling workload was launched. The subsequently added sample-flamegraph controls are covered by Chromium and a fresh desktop build.


## Source remapping and bounded rendering

The source pane can associate an explicitly chosen local file with the selected instruction's recorded source path. Native selection happens inside the Rust command; the webview does not supply an arbitrary read path. Browser preview uses a file input. Reads require a regular UTF-8 text file, reject NUL content and enforce a 2 MiB file limit. Per-capture replacements have a 16 MiB combined UTF-8 budget. Rejected replacements preserve the prior mapping. Empty files are valid. Restoring a path reveals its captured snapshot again, when present.

Mappings are keyed by exact recorded path and capture identity, retained across view/tab changes and released on capture close. They are session-only; project persistence and directory-prefix mapping remain outstanding. Mapping does not rewrite source files or claim the replacement matches the profiled binary. The UI explicitly labels replacement revisions unverified. A picker result is discarded if its source pane has unmounted.

The source pane renders at most 200 lines at once. ISA selection opens the page containing the recorded source line; users can page and return to that line. Source-to-ISA selection uses original one-based line numbers. An out-of-range recorded line is reported rather than silently clamped to a different line. This bounded DOM rendering initially left eager snapshot import unchanged. The subsequent lazy-source implementation below removes that eager read.

Rust tests cover UTF-8, empty content, NUL, oversized input and non-file rejection. Frontend tests cover exact-path isolation, immutable restoration and per-file/combined byte limits. The browser test maps an 850-line replacement to recorded line 501, verifies only 200 line buttons render, pages to lines 601–800, returns to the selected line, switches views, rejects malformed replacement content without losing the mapping and restores the original missing-source state. The full 16-test browser suite passed with the existing ISA/source navigation tests.

The packaged Linux app mapped a local replacement through the native picker and rendered its text at recorded line 9 while retaining ISA selection. [native-source-remapping.png](native-source-remapping.png) records the inspected WebKitGTK view. The original source was restored afterward. The probe used an isolated virtual display and was terminated after inspection.

The capture-switch browser regression also maps a source through mocked native IPC, opens a separate ROCPD capture, switches back and verifies the original capture retains its mapping. Both focused source/capture-switch regressions passed after adding the closed-capture callback guard.


## Lazy captured-source reads

Capture import now indexes `snapshots.json` into recorded source names and private, capture-owned local paths. Source bodies are not included in the initial capture response. The source pane requests the selected file through `read_source`; the command only accepts a recorded path present in that capture. It re-resolves the path under the capture root before reading, rejects out-of-root symlinks, and applies the regular-file, UTF-8/NUL and 2 MiB per-file limits. The manifest has a 100,000-entry bound; ambiguous recorded paths are rejected. No source-body cache retains previously selected files.

The former 16 MiB aggregate snapshot-load limit no longer drops files when opening a capture. Rust tests index nine 2 MiB text files plus an invalid UTF-8 file without reading their bodies, then successfully read the last valid file on demand and surface the invalid encoding only when requested. Another test changes an indexed file into an out-of-root symlink after import and verifies rejection. Unlisted logical source paths are rejected.

Frontend requests are scoped by capture and recorded path; effect cleanup discards obsolete results. The browser regression resolves file B before a late response for A and verifies B remains selected, then checks visible read errors and retry recovery. The full 17-test browser suite passed before the retry addition; the focused lazy-source regression verifies the final retry path separately. Inline sources in the browser example remain supported.

The CLI `--bundle` explicitly reads and embeds sources for portability, with its existing 16 MiB output-source limit made explicit. A fixture export was checked for one embedded source and 24 waves. This export path is separate from lazy desktop capture import. Individual captured files above 2 MiB and directory-prefix remapping remain unimplemented; this is not yet streaming arbitrary-sized source files.

The native app imported `fixtures/transpose-ui/filenames.json` into a second capture tab, then displayed the captured source at the selected line through `read_source`. The inspected source pane is [native-lazy-source.png](native-lazy-source.png). The isolated native process was closed afterward. The final desktop build includes retry handling; the retry regression passed separately in Chromium.


## Timeline display slots and selection

Token display slots now follow upstream `WaveInstance::appendTokenWithSlotBump` at the baseline commit: simultaneous start clocks bump through slots 0–3 when the previous event in a slot extends past that clock. Additional collisions share slot 3. Slot assignment preserves input order, uses the larger duration/stall value as upstream does, and is separate from hardware wave-slot identity.

`node scripts/check-slot-oracle.mjs` compiles the unchanged upstream method body against a minimal token adapter. All 20,000 token slots matched across 200 generated streams with equal clocks, zero durations, stalls and overflow beyond four simultaneous tokens. The UI uses stacked bars by default and offers flat bars. Its CSS-pixel geometry has a two-pixel minimum height/width to keep small events selectable; it does not claim pixel identity with Qt's zoom-dependent mip renderer.

Drawing and pointer selection now use the same clipped rectangles, including actual vertical extent. This prevents the last token at a clock from intercepting clicks on another token's exposed lower portion, and allows selecting the visible two-pixel representation of a zero-duration event. Unit tests cover clipping, row exclusion, stacked heights and short-event hit boxes. The browser test clicks the three exposed portions of a same-clock stack and verifies distinct ISA selection, then selects a later occurrence of the same ISA and verifies occurrence 1. Flat-bar selection remains available. Selection highlighting now marks only the chosen occurrence instead of every execution of that ISA.

The complete 18-test browser suite and the desktop build passed. The subsequent issue-projection work below handles the architecture base clock for utilization; upstream zoom-dependent mip layout remains a separate baseline requirement.

The native WebKitGTK app imported a synthetic same-clock trace through the file dialog. Clicking the exposed middle and upper bar portions selected ISA 2 and ISA 3 respectively; the inspected state is [native-token-stacking.png](native-token-stacking.png). The native probe was terminated after inspection. A final unit regression also excludes a positive-duration event ending exactly at the viewport's left boundary, while preserving visible zero-duration marks.


## Architecture-specific issue projection

The utilization timeline now follows the issue interval and pipe tables in upstream `QUtilView::Add` and `QUtilization::AddTokens` / constructor (`src/wave/waveview.cpp`) at the baseline commit. RDNA uses a one-shader-clock base quantum; GCN/CDNA uses four. Exact supported gfx family identifiers, the decoder family labels `navi` / `vega`, or recognized integer `gfxip` metadata select the model. Unknown models show an explicit unavailable state. Raw trace timestamps and target-wave durations are not globally scaled.

Stall removal is clamped so a fully stalled token retains one issue quantum, or its entire duration when shorter. IMMED, TRAP and MSG retain at most one quantum at the end of their token. These are shared lanes. RDNA additionally uses a shared JUMP lane and separate LDS, RAY and MATRIX lanes; GCN/CDNA routes matrix instructions to VALU, LDS to VMEM by default, and offers a live separate-LDS option. Matrix mnemonics can refine the decoder category. Unknown categories are omitted. The chart describes recorded issue intervals, not measured utilization percentages or complete CU coverage.

Three new unit tests cover family recognition and unknown models, full stalls, sub-quantum and zero durations, duration/stall mismatch, immutable original tokens, every upstream category, shared lanes, matrix classification and LDS splitting. All 46 frontend tests passed. Browser tests select a shared IMMED event, inspect four-clock full-stall and IMMED hover durations, change LDS routing, and verify that an unknown architecture cannot silently inherit a clock model. All 20 browser tests and a fresh Tauri debug build passed. No GPU profiling workload was run. Supplemental other-SIMD tracks, hidden-overlap selection and upstream zoom-dependent mip rendering remain outstanding.

## Timeline vertical scaling and view preferences

All canvas timelines now offer four lane heights (20, 28, 42 and 56 CSS pixels), with W/S shortcuts while the canvas is focused. Lane backgrounds, labels, stacked token rectangles, hit testing and lane pagination use the selected height. Normal retains the previous geometry. Two-pixel minimum event heights remain selectable at compact density. The controls change display geometry only, not trace timestamps or lane identities.

Lane height and stacked/flat token layout are stored in versioned local preferences independently for target-wave, CU, utilization, global, dispatch and marker views. These preferences are shared across captures; they are not a project file. Switching views remounts the timeline with its own saved options. Invalid stored values fall back to defaults, and storage write failures show a notice while preserving the current view's settings. Full layout persistence, independent range locks, adjustable mip/LOD rendering and counter-axis scaling remain separate work.

Unit tests cover invalid preferences, independent storage keys, scaled lane positions and the exposed portions of stacked bars. Browser coverage selects distinct instructions after doubling lane height, exercises both keyboard shortcuts, saves flat layout, changes Global independently, and verifies both views after reload.

The first full browser run exposed an existing startup race: a sidebar selection made before the example response arrived was reset by capture initialization. Initialization now preserves that first view choice. A related fix adds a late example to the capture list without replacing an already imported capture or its active selection. Deterministic browser regressions hold the example response until after navigation or native import, then verify the resulting workspace.

Final validation: all 48 frontend unit tests and all 23 browser regressions passed; the production frontend and Tauri debug application built successfully. The two startup regressions ran with deliberately delayed responses. This milestone did not run a GPU workload or add a native screenshot qualification.


## Timeline PNG and line-chart SVG export

Timeline exports capture the rendered canvas at its device-pixel resolution, composite an opaque background, and add the plot title, exact visible range/unit and lane-page annotation. They include visible selection, cursor and measurement overlays. They export the current page, not all capture lanes. Line charts export a self-contained SVG with dimensions, background, title, visible range and explicit text styling. The exported paths are the same displayed samples, including existing decimation and gaps; export does not imply full-resolution raw-data output.

Desktop export sends bounded image bytes to a Rust command that opens its own native save dialog. The webview does not provide a destination path. The command checks the allowed format and content prefix, enforces a 16 MiB payload limit, writes and syncs a temporary file in the chosen directory, then persists it to the selected destination. These checks identify the expected export envelope, not a general-purpose image sanitizer. Cancellation and write errors are reported in the view. Browser preview uses a download.

The Rust regression verifies exact SVG bytes, rejected format/oversize inputs and preservation of an existing destination after validation failure. Browser tests verify PNG signature and dimensions, standalone SVG parsing and image decoding, embedded styling and range text, and native IPC cancellation/error recovery. All 25 browser regressions and the Tauri debug build passed.

The actual native save dialog exported [native-timeline-export.png](native-timeline-export.png), a 1219 by 279 pixel PNG. The saved artifact was inspected for title, range, all six visible lanes and opaque background. The isolated native process was then closed. No GPU workload ran. Histograms, flamegraphs and fabric diagrams still need export controls; whole-timeline multipage export and project export/reopen remain outstanding.


## Analysis-view SVG exports

Histogram export uses the same selected bin count, boundaries and accumulated instruction weights as the visible histogram. Bar height is normalized to the peak bin; captions record the exact mean-cost range, accumulated-cycle peak, latency scope and idle exclusion. Every bin includes a full SVG title with unrounded boundaries, total and ISA-row count. Empty bins have zero height rather than a fabricated minimum measurement.

Source flamegraph export retains the current source focus, source/ISA proportions and latency or issued/stalled PC-sample units. It includes full source and ISA labels and exact weights in SVG titles, while shortening only the visible labels to fit boxes. Captions identify the capture, scope, total and source-to-ISA aggregation semantics. Marker flamegraph export clones the displayed nesting, retains its range/location scope, and records the existing 64-level display bound and exclusion of open scopes and point markers. Marker timeline exports now also carry the capture unit and hardware scope.

Compute-fabric SVG export is a labeled grid of recorded SE:CU identities and active-wave counts at the inspection time, with the selected CU highlighted. It explicitly identifies untraced CUs as unknown and does not present counts as percent utilization. It does not yet export the separate SIMD-slot or resource-detail panels. Header text wraps to retain long scope descriptions; a full SVG description also preserves the unbroken metadata. Generated box exports reject more than 100,000 boxes or invalid geometry.

The full 26-test browser suite passed, including standalone SVG decoding for each new export, comparison of all eight exported histogram bins with their visible totals, focused source/ISA contents, marker labels and exclusion notes, and fabric architecture/count annotations. The production frontend and Tauri desktop builds passed. These controls reuse the native save path qualified in the preceding milestone; this milestone did not launch another native dialog or GPU workload.


## SIMD-slot and resource-detail export

The selected CU's SIMD panel exports its recorded active slots with original slot identifiers, kernel names and clock intervals. SIMD rows with no active interval remain explicitly empty; unrecorded hardware slots and capacity are not inferred. The resource panel exports VGPR/SGPR metadata, LDS allocation and scratch allocation with units, preserving zero separately from absent values. Captions include capture identity, CU/SE, inspection time, active-kernel context and interpretation limits, including unavailable bank-conflict measurements.

While implementing resource export, the existing UI match was corrected: it previously selected the first dispatch with the matching kernel name and shader-engine scope, regardless of the inspection time. It now requires exactly one active kernel and one active dispatch in that scope at the inspected clock. Multiple matching dispatches or active kernels produce unavailable resource values, with ambiguity explained. The export and on-screen panel share this result.

The browser regression constructs an expired matching dispatch with VGPR 999, an active dispatch with VGPR 64 / LDS zero / missing scratch, and two later overlapping matching dispatches. It checks slot identities, correct active metadata, preservation of zero and unknown values, rejection of ambiguous metadata, and empty SIMD activity exactly at the interval end. These synthetic controls exercise attribution semantics; they do not qualify hardware resource collection.

All 27 browser regressions passed, including the new resource/slot control. The production frontend and Tauri debug desktop builds passed. No native dialog or GPU workload was launched for this milestone; the exports reuse the previously qualified save command.


## Independent timeline ranges

The trace workspace now offers linked (default) or independent ranges for its five timeline modes. On a switch in independent mode, the departing timeline's current range is saved and the destination's saved range is restored. A first visit inherits the current range. Clicking an already selected mode is a no-op, so it cannot unexpectedly restore a stale snapshot. Reset, fit, pan, zoom, typed bounds and selection-driven recentering all operate on the current range; the next switch saves that resulting range. Relinking deliberately keeps the currently visible range and discards independent snapshots.

The policy and saved ranges are part of each capture tab's session state. They are isolated between captures and restored on tab switches; application restart/project persistence remains outstanding. Other sidebar analysis views continue to follow the currently selected timeline range, as the UI explains. This milestone does not claim independently docked counter/occupancy/detail panes.

Selection-history locations now include timeline identity. Global-to-wave navigation preserves the global range, opens the detailed wave at its own bounds, and records those bounds in history. Back/forward restores both the timeline and its range. Browser regressions exercise first-visit inheritance, independent range edits, repeated mode clicks, local reset, capture-tab isolation, relinking and global-to-wave/history round trips.

All 29 browser regressions passed, including both independent-range tests. The production frontend and Tauri debug desktop builds passed. No native dialog or GPU workload was needed for this state-management change.


## Line-chart vertical axes

Counter, derived-counter, occupancy and other shared line plots now offer per-plot minimum/maximum Y bounds and an Automatic reset. The controls are collapsed by default. Signed and fractional bounds are accepted; empty, nonfinite, reversed, equal or overflowing-span bounds are rejected without replacing the active scale. Custom bounds remain fixed as the visible X range changes. They are local component state and are not yet persisted across view unmounts or application restarts.

Every plotted path uses a unique SVG clip path confined to the data area, so out-of-axis values cannot paint over labels. Automatic and custom domains are named in SVG export captions, and exported SVG retains the clip definition and reference. Missing measurements remain path gaps. A stable ratio handles subtraction overflow for very large signed automatic domains. Samples whose coordinates cannot be represented at the current scale are omitted with an explicit on-screen and export annotation instead of creating invalid SVG coordinates.

Unit tests cover signed/fractional bounds, empty/invalid/reversed spans, large signed domains and tiny finite values. Browser coverage changes one occupancy plot to -1 through 1 while another remains automatic, checks unique clip identities, rejects a degenerate replacement without losing the valid scale, verifies custom bounds and clipping in the downloaded SVG, and resets to automatic scaling. No GPU workload or new native dialog probe was required.

Final validation passed: 50 frontend unit tests, all 30 browser regressions, production frontend build and Tauri debug desktop build. The complete browser suite was rerun after the numeric rendering guard and export annotation were finalized.


## Raw ATT run selection

Directory imports now perform Rust preflight discovery before starting Python. Discovery groups ATT files by trace prefix and numeric run ID, sorts shader engines, sums file sizes and returns a representative ATT path for each group. It bounds discovery to 10,000 ATT entries, rejects malformed identities, duplicate numeric SE/run identities, non-files and symlinks escaping the selected directory. Decoded directories with `filenames.json` retain their existing import path. A single discovered run opens directly; multiple runs open an explicit modal chooser. Cancellation at this stage does not invoke the decoder.

The decoder selects the chosen prefix/run while retaining every matching shader engine. It now normalizes numeric SE/run fields and supports case-insensitive `.ATT` extensions consistently with discovery. Explicit file selection ignores unrelated unrecognized ATT siblings; directory discovery still requires named identities. Code objects continue to be resolved from the containing directory, and ambiguous object IDs remain errors. This feature does not infer process-to-code-object mappings when files conflict.

Rust discovery tests cover multiple prefixes, multiple runs, multiple engines, uppercase extensions, zero padding, duplicate identities and decoded-versus-malformed inputs. An integration test executes the actual embedded decoder script against a stub decoder module and asserts that only the selected prefix/run's two engines reach decoding. All 39 ordinary Rust tests passed; the optional archived-data test was run separately and passed. The complete 31-test browser suite passed, including chooser details, cancellation with no decode call and exact selected-path dispatch. The production frontend and Tauri debug builds passed.

For archived qualification, the gfx1101 `navi3_float` trace and its code objects were copied to `/tmp/rocprofgui-multirun-fixture`; a second filename containing run 4 was made from the same trace bytes. Selecting the original run 3 through the real V2 decoder produced 8 waves, 3,277 ISA records, 64 residency intervals and 96,020 tokens in the first wave. The copied run is a selection control, not a distinct performance measurement. No GPU workload ran.

Native folder-picker smoke qualification remains open. In the isolated Xvfb session, GTK emitted `WIDGET_REALIZED_FOR_EVENT` assertions and did not submit the intended fixture directory. One wrong-directory selection reached the decoder and correctly reported no raw ATT files. The native test process was closed. This was not counted as a successful native chooser/import test; the browser IPC regression and real archived decoder result above are the accepted evidence for this milestone.


## Native ATT chooser qualification follow-up

The previously unqualified native folder-picker path was retested successfully. The isolated Xvfb server had no window manager: initial X input focus was PointerRoot, and opening the GTK folder picker did not transfer it from the main window. Explicit focus alone did not make the picker's Recent view navigate reliably. Switching the picker to Home produced a realized filesystem listing; setting the intended directory with Ctrl+L then submitted it successfully. This is the qualified test procedure, not a claim that the general GTK Recent-view issue has been repaired.

The actual Tauri application called Rust discovery and displayed the two runs in `/tmp/rocprofgui-multirun-fixture`, including prefix `1059_6871`, runs 3 and 4, shader engine 0 and 1,692,832 ATT bytes per run. [native-att-run-chooser.png](native-att-run-chooser.png) records the inspected chooser. Selecting run 3 invoked the real V2 decoder and opened a second capture tab named `1059_6871_shader_engine_0_3.att`, with 8 waves and 3,277 ISA records and a populated timeline. [native-att-selected-run.png](native-att-selected-run.png) records that result. This closes the native folder-to-chooser-to-decoder acceptance gap from the previous milestone.

No product code or binaries changed for this follow-up. No GPU workload ran; the inputs are the archived gfx1101 capture and a byte-identical second filename used as a run-selection control. The isolated native process was terminated after inspection. The test does not expand architecture coverage or qualify the copied run as a separate hardware measurement.


## Workspace project save and reopen

The desktop app now writes versioned `.rocprof-project` reference files. Saved state includes capture order, active tab, analysis view, scope and hidden/idle choices, selected wave/ISA/occurrence, navigation history, linked or independent timeline ranges, and embedded per-file source replacements. The Rust store owns the original canonical import path, including the selected ATT file for a decoded run; temporary decoder output paths are not exported as reopenable inputs. Relative references in hand-authored project files resolve against the project directory.

Open stages all inputs before replacing the capture store. Failed parsing, unsupported versions, invalid navigation/source state, missing inputs and cancellation preserve the existing workspace. New capture IDs are allocated only after successful staging. The frontend remounts the restored workspace even when the active capture remains the bundled example, and suppresses a late startup-example response after project installation. Save validates first and atomically replaces the selected destination. Limits are 128 MiB per project, six native captures plus one example, and existing 2 MiB per-file / 16 MiB per-capture replacement text bounds.

These are live-reference projects: input contents are re-imported without fingerprints, and source replacements remain revision-unverified. Reconciliation clamps ranges and resets missing wave/ISA selections with a warning. Per-widget choices such as chart Y bounds, counter libraries, and application presentation preferences are not packaged. Transactional staging temporarily retains both capture sets; byte-aware memory admission remains outstanding.

All 42 ordinary Rust tests passed (one optional real-decoder test remains ignored in the ordinary suite), including project round trips, preservation of an existing destination after invalid save, input failure/cancellation, source validation, relative paths and example staging. All 50 frontend unit tests passed. The full 32-test browser suite reported success; its project regression checks same-ID restoration of range linking, selection state and source replacements, followed by a failed reopen that preserves the modified workspace. Production frontend and Tauri debug desktop builds passed.

The native save/open dialogs were exercised in isolated Xvfb with the bundled example and a file-backed import of `fixtures/transpose-ui/filenames.json`. The saved JSON contained both entries, active index 1 and the second capture's range [5002, 7240]. After switching to the first capture, reopening restored the second capture and that range, recorded in [native-project-reopen.png](native-project-reopen.png). These are synthetic data, not hardware measurements. Source replacement round-trip evidence is from the Rust/browser tests, not this native probe.

The probe also exposed an existing range-input editing problem: intermediate keystrokes are immediately clamped, so typing 1200 into the start field produced 5002. The project correctly preserved the actual displayed value. Draft-until-commit range editing remains a UI follow-up and was not counted as fixed by project persistence.

A second native project with a deliberately missing file-backed input returned the expected filesystem error while preserving both current tabs, the active tab and [5002, 7240] range. This supplements the Rust staging and browser failure controls.
 The existing file-backed capture remained usable: Fit wave restored its populated timeline after the failure, as shown in [native-project-failure-recovery.png](native-project-failure-recovery.png). The isolated native process was closed after inspection. No GPU workload ran.


## Committed range editing

The native keyboard problem found during project qualification is fixed. Each visible-range endpoint now has a local draft. Typing does not change the plotted interval; Enter or blur validates and applies the draft, and Escape restores the current endpoint. Empty/nonfinite values and endpoints that would collapse or reverse the interval preserve the plot with an explanation. Valid values outside the capture clamp at commit, independently of the opposite endpoint. Fractional values are retained instead of rounded for display. Capture, project and timeline changes reset field identity; changes to the applied range synchronize the drafts.

All 33 browser regressions passed, including character-by-character input, unchanged duration while drafting, Enter and Tab commits, Escape, empty/reversed rejection, capture-bound clamping, fractional precision and reset. Existing independent-timeline, capture-tab and project round-trip regressions passed with blur commits. The production frontend and Tauri debug desktop builds passed. No Rust code changed in this milestone.

The actual Tauri/WebKitGTK app was tested in isolated Xvfb. Typing `1200.5` into the start field preserved that exact draft while the timeline remained [500, 7240], shown in [native-range-draft.png](native-range-draft.png). Pressing Tab committed [1200.5, 7240], updated the duration to 6039.5 cycles and moved the plotted interval accordingly, shown in [native-range-committed.png](native-range-committed.png). This directly replaces the previous 1200-to-5002 failure with a passing keyboard check. The input is synthetic; no GPU workload ran.


## Saved line-chart axes

Applied line-chart Y overrides now live in capture workspace state instead of component-local state. They survive analysis-view unmounts and capture-tab switches, and are serialized in project files. The Automatic action deletes the override. Occupancy identities separate SE and kernel plots; counter identities include category, name, scope and unit; tensor outputs include the evaluated scope, output/location label and clock-versus-index domain. This deliberately restores a named plot's chosen display scale, not a claim that changed input data or definitions retain the same measured meaning.

Project validation allows at most 4096 overrides per capture, with bounded identities and finite increasing bounds whose span is finite. The new field defaults to empty when absent, retaining schema-1 compatibility. Invalid drafts do not replace the saved scale. Draft form text itself is not persisted; reopening shows the applied values. Hidden or filtered-out plots retain their overrides, while derived counter results still require their existing definition/evaluation workflow. Source/counter data and metric units are unchanged.

The Rust round-trip test now checks exact saved Y values and rejects reversed axes. Browser controls check view persistence, capture isolation, deletion through Automatic and project restoration after an intervening reset. This closes the earlier saved-Y-bounds gap; general docking, complete per-view layout persistence and independent sidebar X ranges remain outstanding.

The first full browser run had 33 passes and one PNG export cancellation timeout. A traced repeat showed the button still in Exporting state, awaiting asynchronous encoding rather than losing its status. The regression now explicitly waits up to 15 seconds for the first native export request, then checks cancellation and write-error recovery. All three targeted repeats passed, including one taking 8.6 seconds. No production export code was changed in response to this timing observation.

Final validation passed: all 34 browser regressions, all 50 frontend unit tests, all 42 ordinary Rust tests, the production frontend build and the Tauri debug desktop build. No additional native dialog or GPU workload was launched for this state-persistence change; native project save/reopen was qualified in the preceding milestone, and the new field is covered by Rust serialization and browser restoration tests.


## Persistent scalar counter definitions

Scalar counters previously stored computed series in the Counters component and lost them on unmount. Definitions now belong to the capture workspace and are included in project files. Each definition stores its name, formula and referenced C-index/name/scope/unit bindings. Reopening recomputes exact-timestamp intersections against current samples; it does not deserialize cached values. Changed binding identities fail explicitly. Cross-scope arithmetic continues to require an explicit tensor reduction. Definitions that cannot be evaluated remain visible with a reason and a Remove action. Removal updates saved workspace state.

The frontend accepts up to 64 definitions, names of 256 characters and formulas of 4096 characters. Rust bounds serialized definitions and binding metadata; the optional project field defaults to empty for existing schema-1 files. Evaluation rejects more than 500,000 input samples per definition and shares a two-million input-sample-times-expression-length budget across the view's scalar definitions. This bounds the existing synchronous evaluator; large workloads should use the separate cancellable tensor worker. Scalar editor drafts and search filters remain local component state. Tensor definition libraries still use their existing application storage and are not included in projects.

Unit tests serialize definitions, recompute values after changing sample data, reject changed identities and cross-scope references, and exhaust a shared work budget. Rust project tests retain the scalar formula and bindings through disk round trips. Browser project coverage creates a scalar definition, saves it after leaving Counters, reopens the project, displays the result, removes it and confirms removal survives another view change. No native dialog or GPU workload was launched for this milestone.

Final verification passed: 52 frontend unit tests, all 34 browser regressions, all 42 ordinary Rust tests, production frontend build and Tauri debug desktop build. The ordinary Rust run leaves the optional real-decoder test ignored; no new hardware compatibility claim is made by this persistence change.


## Project tensor definition libraries

The tensor definition library is now owned by the workspace rather than the Counters component. Project save embeds all files and the selected file identity. Project reopen installs that library in memory and marks it project-local. Editing or evaluating a project-local library does not write either the machine library or its legacy single-definition preference. Save project persists those edits; individual file export continues to use the existing export path. This preserves unrelated machine drafts. Without a project library, normal edits retain their existing application-storage behavior. Schema-1 projects lacking this optional field retain the currently loaded library.

Rust validates library version, 1–32 files, unique IDs/names, valid selection, filename constraints, NUL-free text, 256 KiB per file and 2 MiB total. Project libraries contain definitions, not computed tensor results or an automatic instruction to execute them. Evaluation still requires selecting the appropriate current counter domain and invoking the existing bounded worker. Loaded project revisions and input contents remain unverified as described above.

The new browser regression saves `Captured := 2`, changes the machine draft to `Machine := 3`, reopens the project to recover the captured definition, edits it to `Project := 4`, navigates away/back, and saves again. It checks that the project payload contains the new edit while local storage and a subsequent reload retain the machine draft. Existing import/export, cancellation and draft-migration regressions also passed. Rust round trips retain definition text and reject a missing selected file.

Final verification passed: all 35 browser regressions, 52 frontend unit tests, all 42 ordinary Rust tests, production frontend build and Tauri debug desktop build. This milestone used Rust serialization and browser IPC tests; no additional native dialog probe or GPU workload was run.


## Directory-prefix source remapping

The desktop source pane now maps a recorded source-directory prefix to a selected local folder. Rust matches complete path components and supports `.` for relative recorded paths. The candidate set is the capture's registered source-file list plus legacy inline-source keys; it does not scan unrelated disk files or automatically read original host paths. Each matching relative path is resolved inside the selected folder. Missing files are counted and leave existing replacements intact; traversal, escaping symlinks, invalid text and read failures reject the batch.

The frontend validates the complete merged replacement set before installing it. The final-set budget matters when a batch adds one file while shrinking another: a transient intermediate size must not incorrectly reject an otherwise valid batch. Existing bounds remain 2 MiB per file, 16 MiB per capture and 10,000 mappings. This operation snapshots replacement text into the existing project-persisted mappings; it does not install a live prefix watcher. Individual Restore captured source actions remain available.

Core tests exercise nested files, exact prefix boundaries, missing paths, relative names, parent traversal, invalid text and symlink escape. Frontend tests check atomic failure and final-set budgeting. The browser regression verifies native command arguments, mapped source display, missing-file counts, invalid-batch preservation, cancellation and per-file restoration. The initial desktop build caught a missing direct serde_json dependency in the new command; moving example-path decoding into profile-core fixed it and also covered legacy inline sources absent from the newer sourceFiles manifest.

The first native folder probe identified a legacy alias mismatch: ISA source `transpose.hip` resolves to registered inline snapshot `/transpose.hip`. The source pane now initializes its directory prefix from the resolved registration and can display and restore replacements under that registration. A batch also supersedes an older relative-alias mapping when the relative name is not a separately registered source; distinct registered paths remain separate. Unit and browser controls cover that case.

The corrected Tauri app was tested through its real GTK folder picker with prefix `/` and `/tmp/rocprofgui-source-folder-native`. It mapped one file, reported zero missing files and displayed the 30-line replacement with the selected ISA source line highlighted. [native-source-directory.png](native-source-directory.png) records the result. The isolated process was closed afterward. No GPU workload ran.

Validation passed: 44 ordinary Rust tests, 54 frontend unit tests, the full 36-test browser suite, and production frontend/Tauri debug builds. After the final relative-alias supersession adjustment, the three affected source/project browser tests were rerun and passed, and the desktop was rebuilt. The native screenshot precedes only that final existing-alias edge-case adjustment; the picker and core directory reader were unchanged. Individual source files above 2 MiB remain a separate baseline gap.


## Hidden-analysis workspace lifetime

Hidden-latency jobs previously wrote their result by capture ID and cleared one shared busy flag unconditionally. A job finishing after project reopen could therefore populate a new same-ID example capture with obsolete results or clear the newer job's busy indicator. Requests now carry capture-object identity and a per-request token. Only a still-registered capture instance may receive the result; pending state is tracked per capture, and duplicate requests for that instance are suppressed.

Closing a capture removes its request registration and cached analysis. Project installation invalidates all previous request registrations and clears pending/cached state before new capture jobs start. Errors from obsolete jobs or inactive captures do not overwrite the current view's error message. Existing native computations are not forcibly cancelled by this change; they may finish in the backend, but their results cannot enter a replacement workspace.

The browser regression holds the initial example's hidden-analysis response, reopens a project with the same capture ID, then completes the old response first. It requires the newer job to remain busy and the nonhidden option to remain disabled until its own response arrives. It then closes a capture with another pending request, aborts that response and verifies that no stale error appears.

Validation: all 36 existing browser regressions passed. The new deferred-response test initially failed on Playwright's generic disabled-state matcher for an HTML option despite the option carrying `disabled`; it now checks that attribute directly. It also waits for response delivery and rendering before asserting that the obsolete result had no effect. The corrected regression passed separately. Production frontend and Tauri debug desktop builds passed. No Rust implementation, native dialog, hardware profiler or GPU workload changed or ran for this milestone.


## Independent line-plot X ranges

Occupancy, raw/derived-counter and tensor-result line plots now offer a Follow workspace range checkbox. Unlinking snapshots the current plot interval. Independent endpoints use the existing committed-input controls, and Full plot range restores the plot's domain without relinking. Relinking discards that override and follows the current workspace range. State is per capture and the same semantic plot identity used for Y bounds; it survives view changes and is serialized in project files.

All range-dependent sample selection, step carry-in, decimation, Y auto-scaling, X ticks and SVG export use the resolved plot interval. Exports annotate independent versus workspace scope. Clock-aligned plots use capture bounds; transformed tensor Time axes use sample-index bounds. A restored interval outside the current domain is clamped or reset to the full domain with a visible note. The stored choice is not silently rewritten solely because a different input revision was opened. Project validation bounds the number/size of range identities and requires finite increasing intervals with finite spans.

The new browser regression holds one occupancy plot at [1000, 2000] while the workspace and another plot move to [3000, 5000], checks the SVG interval, leaves/returns to the view, resets the plot domain and relinks. Project coverage saves an independent interval, clears it before reopening, then verifies restoration. Rust project round trips retain the range values. Histogram/flamegraph intervals and simultaneous docking remain separate baseline gaps.

Final verification passed: all 38 browser regressions, 54 frontend unit tests, all 44 ordinary Rust tests, production frontend build and Tauri debug desktop build. No additional native dialog or GPU workload was run; the change uses the already-qualified range input and SVG export paths with new per-plot state.

## ROCPD event-associated PMC values

The SDK 3.x adapter now reads PMC records with no sample ID into a separate event-counter collection. The schema contract follows the local ROCm SDK `pmc_events` view: definition and event joins use ID plus GUID, and dispatch associations use event identity within the same table partition. Counter, record and event identities retain their table namespace. Missing values remain null and measured zero remains zero. Records with multiple matching dispatches retain every match; records without a kernel association remain visible. Missing definitions/events and nonfinite values reject import. Import and association counts are bounded, and the existing read-only/cancellable database path is retained.

Counters now includes a searchable table with 100 records per page, units, process/agent scope, event identity and dispatch navigation. Ambiguous rows expand to explicit candidate dispatches (the first 20 are displayed). These observations have no sample timestamps: they are not interpolated, included in time-series expressions or filtered by the workspace interval. Clicking a recorded dispatch sets its interval and opens the dispatch view.

Two Rust regressions extend the official SDK 3.0.4 synthetic fixture and construct partitioned event-only PMC schemas. They cover mixed PMC/SPM separation, zero/null preservation, exact dispatch identities, unchanged database bytes, identical IDs across partitions, ambiguous associations, missing references and nonfinite values. The browser regression uses controlled IPC data to verify table values, filtering, ambiguity expansion, unchanged sampled plot count and dispatch interval navigation.

Validation passed: all 46 ordinary Rust tests, all 39 browser regressions, production frontend build and Tauri debug desktop build. One optional real-decoder Rust test remains ignored as before. No GPU workload or additional native UI probe ran for this change. Legacy database schemas and embedded/external trace artifact loading remain open baseline work; the supported event-counter contract still expects SDK dispatch/symbol tables to exist.

## Persistent light and dark appearance

The sidebar now offers a locally persisted Dark/Light appearance choice. Light presentation covers the application surfaces, controls, tables, ISA/source highlighting, diagram labels and analysis views. Canvas lane backgrounds, labels, grid and selection/cursor strokes read the same palette as the UI and repaint on a theme change. Instruction-category and resource-diagram data colors stay consistent. Appearance is a machine preference, independent of capture/project state. A storage failure leaves the session choice usable and displays an explicit persistence message.

PNG exports use the selected background and text palette. SVG exports preserve computed fill, stroke, text and opacity styles for attached plots, allowing CSS variables and theme-specific labels to render in a standalone image without application stylesheets. Detached resource box plots retain their explicit data colors and receive the selected export background/header palette.

The two new browser regressions cover light/dark selection, reload persistence, canvas background pixels in both themes, resolved SVG colors and storage-failure behavior. The first full run caught an unintended header surface tint in the light palette; after correction all 41 browser regressions passed. An additional targeted run also decoded the downloaded light PNG and verified its background pixels. The settled browser screenshot was visually inspected and saved as [light-theme.png](light-theme.png).

The final production frontend and Tauri debug builds passed. No Rust behavior changed and no GPU workload or native dialog probe ran for this milestone. Adjustable LOD, simultaneous docking and remaining per-view options are still open baseline work.

## Independent histogram bounds and saved bins

Instruction hotspots now retains its mean-latency interval through view changes, capture tab state and project save/reopen. The histogram uses a separate plot identity in the existing per-capture range map; its cycles-per-issue axis is independent of the workspace trace-time interval. Bin count is an optional project field, defaults to 16 for older projects and accepts only 8, 16, 32 or 64. Unsupported saved bin counts reject project validation.

The Min/Max controls use committed drafts: Enter or blur applies a finite increasing interval, Escape restores the applied value, and empty/reversed input leaves the plotted histogram intact. Automatic histogram range removes the override and follows the currently supplied instruction data. Custom saved bounds outside the supported nonnegative domain are adjusted with a visible note. SVG export reports automatic/custom scope and uses the same applied interval and bins as the on-screen chart.

Browser coverage exercises uncommitted drafts, fractional endpoints, empty/reversed input recovery, Escape, bin counts, view navigation, independence from trace-time changes and automatic reset. The project regression saves the histogram settings, changes them before reopening, then requires the saved settings to return. Rust project tests retain the bin count and reject unsupported values.

Validation: the full browser run passed 41 of 42 tests; the remaining older test used obsolete Histogram minimum/maximum labels. After updating it to the committed range controls, all five affected histogram/navigation/project/export tests passed, including added cross-capture isolation and custom-range SVG assertions. All 46 ordinary Rust tests passed, followed by the three project tests with the added invalid-bin assertion. Production frontend and Tauri debug desktop builds passed. No GPU workload ran. Flamegraph viewport controls and large-capture qualification remain separate baseline items.

## Source flamegraph weighted viewport

Source latency and issued/stalled PC-sample flamegraphs now support independently saved cumulative-weight intervals, committed numeric bounds, center zoom, half-span panning and full-range reset. Viewport identities include the metric and an explicit all-sources versus source-focus discriminator. Selecting a source uses its own viewport; resetting source focus returns to the saved all-sources viewport. These intervals measure accumulated cycles or samples and do not represent execution time.

A shared interval clipper supplies both displayed groups/leaves and exported boxes. Boundary entries retain their full recorded weight in labels while geometry represents only the intersecting portion. Fractional spans below one unit remain valid. Panning fits the interval to the available domain while preserving its span. Source grouping now appends to existing arrays instead of repeatedly copying a growing group. CSS no longer adds fixed inter-entry gaps or minimum horizontal padding that distort the relative widths of narrow entries.

Unit tests cover partial boundary leaves, excluded touching intervals, unmodified original weights, pan limits, fractional zoom and invalid/empty-domain recovery. The browser regression checks zoom/pan, per-source focus return, view persistence, sub-unit clipping, exported interval metadata and navigation from a clipped ISA leaf. Project tests save a viewport, clear it and verify restoration on reopen. A final browser geometry assertion measures every visible source/leaf width against its declared weight fraction within 0.1 pixel.

Validation passed: all 56 frontend unit tests, all 43 browser regressions, the targeted rendered-geometry check, production frontend build and Tauri debug desktop build. No Rust implementation changed or GPU workload ran. Marker-flamegraph viewport controls, richer source hierarchy and broader large-capture qualification remain on the baseline checklist.

## Marker flamegraph cumulative-duration viewport

Marker flamegraphs now expose committed cumulative-duration bounds, zoom in/out, half-span pan and full-range reset. The saved viewport is keyed by hardware scope in the existing per-capture/project range map. Workspace time continues to filter the marker tree's aggregated durations; this separate cumulative axis controls the displayed portion of that tree and does not represent execution time.

Nested rectangles use the shared weighted clipper. Parent and child intersections are projected into the same viewport, with full aggregated durations retained in accessible labels and tooltips. SVG export captures the clipped rectangles and records both the workspace time interval and cumulative-duration interval. Thin separator strokes replace geometry-shrinking gutters. Labels use the capture's unit instead of hard-coded cycles. Nonfinite totals display an error, and SVG depth calculation uses a reduction instead of spreading a potentially large rectangle list into a function call.

The new browser regression checks a concrete [500, 1000] viewport: the remaining Load tile and Transpose tile child widths are exactly 100 and 900 of the 1000-unit SVG width. It checks keyboard selection, exported intervals, hardware-scope isolation, zoom/pan, view persistence, full reset and changed workspace-time totals. Project coverage saves a marker viewport, resets it and requires restoration on reopen. A controlled fixture with unit ns verifies duration labels follow metadata.

Validation: the full browser run passed 43 of 44 tests. The remaining older test ambiguously matched the workspace and newly added marker range inputs; after making the workspace selector exact, all four affected marker/navigation/export/project tests passed, including the new unit-metadata check. Final production frontend and Tauri debug desktop builds passed. The shared clipping helper's 56-test frontend unit suite passed in the preceding source-flamegraph milestone and was unchanged here. No Rust implementation changed or GPU workload ran. Legacy marker clocks, ambiguous slot reuse, hardware goldens and the remaining broader baseline items stay open.

## Explicit raw ATT metadata and reusable manifests

ATT setup accepts arbitrary trace/code-object filenames and explicit shader-engine/run/code-object identities. Opening an unrecognized `.att` filename enters setup automatically; the toolbar entry also handles inputs with other extensions. The dialog supports multiple traces and code objects, removal, metadata validation and native save cancellation. It saves a `.rocprof-att` manifest before starting the existing cancellable decoder import. IDs are decimal strings in JSON to preserve all 64 bits across JavaScript/Rust/Python. Shader engines are not inferred from file order.

Rust resolves relative manifest paths against the manifest directory, canonicalizes selected inputs and requires nonempty regular files, unique shader engines, unique code-object IDs and at most 256 files of each kind. A trace cannot be assigned to multiple engines. Manifests are bounded to 1 MiB and saved atomically; saving over a referenced input is rejected. The decoder receives a validated canonical manifest in its owned temporary workspace, constructs the decoder API's AttTrace and CodeObject objects directly, and does not scan unrelated sibling files. Normal filename-based import remains available. Project input references retain the manifest path for subsequent decoding.

Two manifest tests cover relative-path resolution, exact u64 IDs, unchanged input bytes, duplicate/missing/invalid inputs and overwrite prevention. A decoder protocol test executes the actual Python bridge against controlled API records and verifies the explicit shader-engine/run and maximum u64 code-object ID. The browser regression covers manual/automatic setup entry, duplicate SE rejection, precise IDs, native save cancellation and save-before-decode ordering.

Real decoder qualification used copies of the archived gfx1101 fixture with names arbitrary-trace.bin, first-kernel.bin and second-kernel.bin. The manifest assigned SE 0, run 3 and code-object IDs 1/2. Decoder V2 produced 8 waves, 3,277 ISA records, 64 residency intervals and 96,020 tokens in the first wave, matching the earlier named-file result. All three copied inputs remained byte-identical to the archive files after decoding.

The final Tauri executable was also exercised in an isolated Xvfb session through actual GTK open/save dialogs. The setup UI accepted the three arbitrarily named inputs and explicit IDs, saved native.rocprof-att, and opened the decoded capture with 8 waves, 3,277 ISA records and a populated timeline. [native-att-setup.png](native-att-setup.png) and [native-att-manifest-import.png](native-att-manifest-import.png) record the inspected configuration and result. The saved manifest was inspected and the isolated process was closed afterward. No GPU workload ran.

Validation passed: 49 ordinary Rust tests, an additional real-decoder test, all 45 browser regressions, production frontend and Tauri debug desktop builds, plus the native setup/save/decode probe. Manifest/decoder tests were rerun after adding the regular-file bound on manifest reads. Additional architecture qualification and packaged decoder dependency setup remain open baseline work.

## Versioned diagnostic manifest export

Capture evidence now exports a separate versioned JSON diagnostic manifest. It records application/backend context, decoder configuration, original input reference, capture metadata/warnings, exact string clock origin, collection counts, sampled-counter identities/counts, event-counter missingness/association counts, marker completeness, resource/PC-field availability, workspace settings, definition drafts and source registry. Source replacements include UTF-8 byte/line counts and SHA-256 hashes with explicitly unverified revision status; source text and raw trace data are not embedded. Definition drafts are identified as potentially different from the last successful tensor evaluation.

Reports distinguish imported field presence from collection completeness and leave replay, correctness, native timing, runtime mapping and thermal/contention qualification unverified. Decoder configuration is reported without executing a probe. The original input is a live reference, not a file fingerprint. Export snapshots workspace/definition/source settings before asynchronous context collection. Count reductions avoid copying large record arrays. A 16 MiB UTF-8 limit applies on both sides of IPC, and native writes validate the report kind/version and use an atomic temporary file. The desktop command rejects overwriting an open capture input.

Two frontend tests cover zero versus missing values, exact large timestamp origins, event association counts, source hashing, unverified qualification, definition text and the UTF-8 byte limit. The Rust writer test checks malformed/unsupported/oversized rejection without replacing an existing report. Browser regressions inspect downloaded JSON and exercise native-command payloads, source hashes, cancellation, write errors and retry. A TypeScript fixture cast caught by the initial build was corrected; final builds passed.

The final native Tauri application imported the official SDK 3.0.4 ROCPD fixture, exported through the real GTK save dialog and displayed success. The saved 4,799-byte JSON was inspected: it retained the canonical database path, backend version 0.1.0, origin 1000000000, two dispatches and two sampled-counter series, with replay coverage still unverified. [native-diagnostic-export.png](native-diagnostic-export.png) shows the result and [native-diagnostics.json](native-diagnostics.json) preserves the checked report. The isolated app was closed afterward. No GPU workload ran.

Validation passed: all 50 ordinary Rust tests, all 58 frontend unit tests, all 47 browser regressions, final frontend/Tauri builds and the native export probe. The focused diagnostic unit tests were rerun after the final allocation reduction. Whole-timeline multipage export and the other baseline checklist items remain open.

## Adjustable timeline detail

Timeline views now offer exact events and 2/4/8-pixel aggregation, persisted per view alongside lane height and token layout. Short events share a group only when their lane, display slot, color and pixel bucket match. Selected occurrences remain individual and paint last in aggregated modes. Long events remain individual even when only a short tail intersects the viewport. Raw records are unchanged. Painting and hit testing use the same projected rectangles; group clicks zoom into the aggregate span without choosing its first instruction, with an Exact-events fallback when the viewport cannot narrow. PNG captions include detail and event/mark counts.

Three renderer unit tests cover count preservation, lane/slot/color separation, selection, clipping, exact order and zoom resolution. A 10,000-event fixture produces one 9,999-event aggregate and one selected event. The browser regression checks dense-event hover, group-to-zoom without changing ISA selection, independent view preferences, reload persistence and manual return to exact events. Two initially ambiguous test selectors were made exact/scoped; the existing 47 browser tests passed in the full run and the corrected new test passed separately. All 61 frontend unit tests and frontend/Tauri debug builds passed; the renderer unit tests were rerun after adding the clipped-long-event assertion. Rust was unchanged in this milestone.

The built native Tauri app imported the archived gfx1101 manifest through the actual GTK file chooser. Its first wave has 96,020 tokens; the visible six-lane page contained 86,783 events. Overview mode reduced that page to 412 drawn marks while retaining the event count. Clicking an aggregate narrowed the range to 97,507.5–107,665.5 cycles and showed 1,299 events as 219 marks. [native-timeline-detail.png](native-timeline-detail.png) and [native-timeline-detail-zoom.png](native-timeline-detail-zoom.png) preserve the inspected native states. The isolated app was closed afterward; no GPU workload ran.

This is pixel aggregation over a continuous range, not a port of upstream's discrete mip rounding and zoom-dependent slot-height geometry. It reduces drawing and hit-testing work but still scans the in-memory marks. Indexed large queries, upstream mip geometry and the remaining baseline checklist are still open.

## Remaining upstream ISA cost strategies

The ISA selector now includes whole-capture mean, target-wave maximum and selected-occurrence latency. The inspected upstream CyclesLabel::updateStrategy in src/code/asmcode.cpp selects a per-instruction cycle-vector element for ITERATION and the largest element for MAX; src/code/qcodelist.cpp lists these alongside capture/wave sums and means. The new maximum and occurrence modes likewise operate across the full target wave independently of the viewport. Occurrence numbering is zero-based per instruction, including executions before the viewport, and missing occurrences report zero. Means retain fractional precision rather than upstream integer rounding.

Both the raw-token fallback and the hidden-analysis path implement these scopes. Maximum compares costs after the selected hidden/idle options, retains stall/idle metadata from the winning token, and retains the full-wave hit count. Existing source/histogram accumulated-cost inputs remain sums. Project validation accepts all three added mode names and preserves the occurrence index without a format-version change. Occurrence bookkeeping runs only for the occurrence strategy.

A frontend test uses interleaved instruction occurrences outside the visible range, missing occurrences, different total/nonhidden maxima, matching stall values, and capture means to check both analysis paths. A Rust project roundtrip test verifies each added strategy and occurrence persistence. The browser regression checks selector costs, viewport independence, occurrence changes and view navigation. Its initial table-column assertion and invalid occurrence expectation were corrected: the UI intentionally clamps to the selected instruction's valid occurrence range, while the unit test covers missing occurrences on other instructions.

Validation passed: 62 frontend unit tests, all four Rust project tests, 48 existing browser tests in the full run and the corrected added browser test separately, plus final production frontend and Tauri debug builds. No native GUI probe or GPU workload ran in this milestone. Hardware range-semantics qualification and the other baseline parity work remain open.

## Global dispatch-duration and marker integration

Global now combines independently selectable residency, marker and dispatch-duration tracks on the imported capture clock. Residency retains its CU-verified detailed-wave navigation. Marker lanes include hardware location and stack depth; point records keep zero-duration geometry and open scopes extend to capture end with an explicit unclosed label. Dispatch labels retain ID, kernel, agent, queue, grid and workgroup. Typed record references preserve original indices after invalid-duration filtering and avoid conflating marker/dispatch clicks with residency lookup. Selecting a marker or dispatch focuses its interval and displays record details; marker details open the marker analysis page. No dispatch-to-wave association is inferred.

The inspected upstream waveglobal.cpp draws typed markers in dedicated depth tracks and draws decoder events/dispatch records separately. This milestone integrates the normalized marker and dispatch-duration records already in Capture. It does not claim support for the remaining extended raw decoder dispatcher/event payloads. Layer toggles remain transient application state, outside project persistence.

The unit fixture checks repeated kernel names with different dispatch IDs, skipped zero-duration dispatches, preserved original indices, hardware coordinates, marker nesting, points, unclosed scopes, selection and independently disabled layers. Browser coverage checks marker interval navigation and its analysis link, dispatch identity/range selection without wave navigation, and the existing global-to-wave/history regression. One ambiguous marker-page heading selector was corrected before the final suite.

Validation passed: all 63 frontend unit tests, all 50 browser regressions in one final run, production frontend build and Tauri debug build. The native app's global layer controls and marker selection were also visually inspected using the bundled synthetic example: selection focused 650–2800 cycles, displayed the source/code-object identity and retained the nested/point tracks. [native-global-markers.png](native-global-markers.png) records the result. The example contains zero-duration dispatch metadata, so dispatch-duration selection was qualified in the browser fixture rather than this native probe. The isolated native app was closed; no GPU workload ran. Rust source was unchanged.

## Capture-local global layer persistence

Global residency/marker/dispatch toggles now belong to each capture's workspace snapshot and are included in saved projects. Switching to another capture starts with that capture's settings, or all layers enabled when no settings exist. Reopening replaces subsequent edits with the saved values. The Rust project schema adds an optional, strictly typed boolean record; absent/null fields preserve compatibility with older version-1 projects. No input decoding or clock semantics changed.

The Rust roundtrip test verifies stored layer values; an additional test reads a project without the field and rejects a string in place of a boolean. Browser project coverage saves disabled marker/dispatch layers, changes both afterward, reopens the same capture ID and verifies restoration. Capture-switch coverage verifies a second capture starts with markers enabled and returning to the original restores its disabled setting. The global-record navigation regression also passed.

Validation passed: all 63 frontend unit tests, all five Rust project tests, the three affected browser regressions, production frontend and Tauri debug builds. No new native GUI probe or GPU workload ran. The remaining per-view persistence, docking and other baseline gaps remain on the checklist.

## Decoded dispatcher and trace-event records

The occupancy JSON importer now retains every typed record from the per-SE events map instead of discarding non-dispatch events. It preserves original record identity, SE, clock, kind and all fields. Field values remain strings across IPC, preserving u64 payloads, addresses, byte offsets and nested entry-point JSON exactly; the original clock value is also retained as text, while plotting uses the existing numeric capture clock. Malformed event arrays/records, missing or invalid clocks and invalid shader-engine keys are rejected, and the aggregate event count is bounded by the existing two-million-record limit. Captures containing only events are supported. Events participate in capture bounds, including records before the first wave. Existing dispatch resource metadata remains available without inventing dispatch durations.

The Global Events layer groups point records by SE/ME/pipe, labels known decoder event types, retains unknown types and fields, and displays full record details on selection. The type mapping was checked against upstream records.h and the installed decoder records.py; import shape was checked against upstream json_emitter.cpp and the installed rcv.py exporter. The layer is independently selectable and project-persisted; older saved layer records default Events to enabled. Diagnostic manifests include its record count. Event-group filtering and upstream overlay styling remain open.

An importer test covers event-only captures, bounds, mixed dispatch/event records, unknown types, u64-max payloads/nested addresses, offsets beyond JavaScript's safe integer range and malformed clock rejection. Renderer tests preserve zero-duration geometry and exact payload text. A boundary fix keeps a point at the viewport's right endpoint two pixels wide inside the plot without changing its timestamp. The browser test selects such a packet-loss event at capture end, verifies its exact payload and focused one-clock range, and hides/restores the Events layer. Project tests also cover older layer records without the Events field.

The real decoder V2 test imported the archived gfx1101 explicit manifest and retained 14 decoder timeline events, alongside 8 waves, 3,277 ISA records, 64 residency intervals and 96,020 tokens in the first wave. Every event lay within the resulting capture bounds. No GPU workload ran.

Validation passed: 53 ordinary Rust tests plus the real archived decoder test, all 64 frontend unit tests, all 51 browser regressions, production frontend and Tauri debug builds. The five Rust project tests were rerun after adding the legacy-layer assertion. No new native GUI probe ran in this milestone.

## Upstream decoder event groups

The Global Event groups menu uses the six bit groups and classification in upstream src/wave/overlay_utils.h: dispatch starts, flush/timestamps, code objects, SQTT stalls/flushes, GC rinse and other events. The default mask is 47, matching upstream Global's exclusion of GC rinse. Dispatch-end, packet-loss, SPM-sample, context, DIDT and barrier events belong to Other; unknown types/kinds are retained there. This classification does not change event data or infer durations. The Events layer toggle preserves group choices, and filtering retains original record indices.

Group masks persist inside each capture's global-layer project settings. Older layer records default to mask 47. The Rust project reader accepts zero/all masks and rejects bits outside the six supported groups. Unit coverage checks every defined event type plus unknown kinds, dispatch starts, default exclusion, empty/all selection and retained record identity. Browser coverage isolates GC rinse after hiding Dispatch starts and Other events, selects its record, and saves/reopens nondefault group choices. The project test was corrected to reopen the collapsed group menu after switching views.

Validation passed: all 65 frontend unit tests, all five Rust project tests, the two affected global/event browser regressions plus the corrected project regression, and production frontend/Tauri debug builds. No GPU workload or new native GUI probe ran. Full-height upstream event overlays, wider hardware qualification and the other baseline items remain open.

## Full-height global decoder overlays

Enabled decoder events now draw full-height lines over visible residency and marker rows with the same explicit shader-engine identity. Dispatch starts use 5-pixel widths and other events 4, following upstream overlay_utils.h. Queue-duration rows and other engines are excluded. Adjacent matching rows are merged into spans rather than allocating one copy per row. Overlays retain exact record identity, remain independent of token-detail aggregation and use the same rectangles for drawing and hit testing. Overlay selection takes precedence over the underlying residency bar. Their unique-record count is displayed separately, and PNG export includes the rendered lines and count. The application's palette is retained.

Two geometry tests cover matching/nonmatching engines, adjacent and disjoint row spans, marker rows, queue exclusion, visible-page restrictions, time clipping, lane heights, endpoint clipping and unchanged timestamps. A browser regression hovers outside a residency bar but inside the full-row overlay, checks that another SE has no event hit, selects the event without opening a wave, and removes it using its event-group filter.

The final native application imported the archived gfx1101 manifest through GTK. Global displayed five enabled event overlays across the first visible residency rows (GC rinse remained excluded by the upstream default). Clicking a dispatch line selected record 0:13 at clock 55,870, focused 55,870–55,871 and displayed its entry-point/code-object/resource fields while remaining in Global. [native-global-overlays.png](native-global-overlays.png) and [native-global-overlay-selection.png](native-global-overlay-selection.png) record the inspected states. The isolated application was closed afterward; no GPU workload ran.

Validation passed: all 67 frontend unit tests, all 52 browser regressions in one run, production frontend and Tauri debug builds, and the native import/overlay-selection probe. Rust source was unchanged. Compute-unit overlays and wider hardware qualification remain open baseline work.

## CU decoder overlays and independent controls

CU waves now includes decoded event tracks and full-height overlays restricted to the selected shader engine. Event projection is shared with Global, while the CU surface applies upstream showTraceEvent exclusions for CS partial flush, code-object load/unload and SPM samples. Dispatch-start records remain eligible regardless of those trace-event exclusions. The default group mask is all six groups, matching upstream CU behavior. Visibility and group choices are separate from Global, retained per capture and included as optional project fields; older projects default to enabled/all groups. The Rust validator rejects unsupported group bits. Event selection displays its details and retains CU view rather than selecting a token or switching to Global.

The new projection unit test verifies all four excluded types, packet loss, GC rinse, unknown types, dispatch starts and another shader engine. The browser fixture has one CU wave plus mixed event types/engines: exactly three eligible events overlay its row, clicking the GC-rinse line selects its details, and hiding CU events does not affect Global visibility. The project regression saves disabled CU events and a nondefault mask, changes visibility, then reopens and verifies both settings alongside the independent Global choices. Rust roundtrip assertions verify the saved fields.

Validation passed: all 68 frontend unit tests, all five Rust project tests, four affected browser regressions (CU events, Global overlays, global-to-wave history and project restoration), production frontend and Tauri debug builds. No additional native GUI probe or GPU workload ran. Broader hardware/topology qualification and the other baseline checklist items remain open.

## Heterogeneous CU lane identity

CU-wave lanes now include recorded CU identity in addition to SIMD/slot, and utilization lanes include CU identity before SIMD/shared-pipe identity. This prevents distinct CUs in one SE from merging visually. Unknown CU identities use the wave ID instead of assuming all unknown waves share one CU. Hover details include SE, CU, SIMD, slot and wave coordinates; CU zero remains a known identity.

A unit test covers distinct known CUs, unknown identities and shared-pipe separation. A browser fixture places two detailed waves on different CUs with identical SE/SIMD/slot coordinates: CU and utilization each retain two rows, hover reports CU 0 correctly, and clicking the second row selects CU 4's wave and ISA instruction. Existing linked-workspace, architecture projection and CU-overlay regressions passed.

Validation passed: all 69 frontend unit tests, the four affected browser regressions, production frontend and Tauri debug builds. No Rust changes, native GUI probe or GPU workload ran. Inspection also identified that hidden-latency orchestration still groups by SE/SIMD without CU; mixed-CU analysis needs a separate correction and is not qualified by these display tests. The parity checklist retains that requirement.

## CU-aware hidden-latency orchestration

Hidden-latency orchestration now partitions detailed waves by SE/CU/SIMD when CU identities are known. When known and unknown CUs coexist, each unknown-CU wave is isolated rather than merged with known or other unknown waves. SE-wide other-SIMD activity is applied only under the single-traced-CU convention; it is omitted for mixed/partially unknown CU captures with an explicit underestimation warning. The association decision considers the entire shader engine, not just one SIMD. Legacy captures whose waves all lack CU identities retain upstream's single-traced-CU-per-SE convention, now with a visible assumption warning. Existing token-count limits and the underlying pipe-priority algorithm remain unchanged.

Warnings are returned with the analysis, displayed in the workspace, cleared by a successful warning-free recomputation, and included in diagnostic manifests. The bundled demo-hidden.json was regenerated through the rebuilt Rust CLI. Explicit CU/WGP attribution of supplemental other-SIMD data and hardware golden qualification remain open; omission is not evidence that other-SIMD activity was zero.

Three new Rust tests compare known same/different CU scopes, test ambiguous supplemental activity across different SIMDs of one SE, and verify mixed-unknown isolation versus legacy behavior. The unchanged upstream C++ oracle still exactly matches all 40 generated legacy multi-SE/SIMD captures. A browser regression checks warning visibility and removal on recomputation; the diagnostic test verifies warning retention.

The real archived gfx1101 decoder test also ran the corrected hidden analysis: it returned all eight waves with no CU-association warnings, retaining the previously checked 14 events, 3,277 ISA records, 64 residency intervals and 96,020 tokens in the first wave. This validates the decoder/analysis path, not a hardware performance oracle. No GPU workload ran.

Validation passed: all 56 ordinary Rust tests, the archived decoder/analysis test, the 40-case C++ oracle, all 69 frontend unit tests, all 55 browser regressions, production frontend and final Tauri debug builds. No new native GUI probe ran. The baseline goal remains open for the remaining checklist requirements.
