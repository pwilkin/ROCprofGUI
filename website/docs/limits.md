# Limits & caveats

This project's credibility rests on being honest about limits. The figures below are taken from the shipped code, not from prose; where the repository's own docs disagree, the code wins and the discrepancy is noted.

## Import caps

| Cap | Value | Notes |
| --- | --- | --- |
| Per-file size | **256 MiB** | Enforced on sidecar/JSON/CSV and `.rgp` files. Not applied to a ROCPD/SQLite database, which is bounded per query instead; a DB larger than 256 MiB can be opened. |
| Main record arrays | **2,000,000 records** | |
| Open native captures | **6** (+ the bundled example) | |
| Raw-ATT import timeout | **300 s** (5 min) | Plus a Cancel button; also aborts if the decoder log exceeds 16 MiB. |

::: tip Several caps are environment-configurable
`crates/profile-core/src/caps.rs` resolves these from `ROCPROF_MAX_*` variables, each defaulting to the shipped value (a blank, non-numeric or zero value falls back to the default). Raising one only moves the point of explicit rejection — nothing is silently sampled — and the message interpolates the effective value. They apply to desktop, headless CLI and MCP alike.

| Variable | Cap it moves | Default |
| --- | --- | --- |
| `ROCPROF_MAX_RECORDS` | main record arrays (and the streamed wave-token limit) | 2,000,000 |
| `ROCPROF_MAX_FILE_MIB` | per-file size for sidecar/JSON/CSV/`.rgp` inputs | 256 |
| `ROCPROF_MAX_HIDDEN_TOKENS` | tokens hidden-latency analysis will load | 4,000,000 |
| `ROCPROF_MAX_ATT_TRACES` | trace files in one ATT manifest | 256 |
| `ROCPROF_MAX_ATT_CODE_OBJECTS` | code-object files in one ATT manifest | 256 |

The six-open-capture limit and the wave-query budgets below (128-wave / 500,000-token viewport, 4,096-wave statistics) are **not** configurable: that path is not yet paged and its numbers are mirrored in the frontend.
:::

## View / analysis caps (the UI refuses or clamps — it does not silently sample)

| Area | Cap |
| --- | --- |
| Visible-wave query | 128 wave files or 500,000 tokens per window — rejected above, not sampled. |
| Wave-statistics query | 4096 waves and 65,536 distinct instruction IDs. |
| Hidden-latency analysis | 4,000,000 detailed tokens. |
| Activity summary | 2,000,000 samples. |
| Scalar counters | 64 definitions/capture; synchronous eval over 500,000 samples. |
| Tensor counters | 1,000,000 input samples, 4,000,000 raw cells, 8,000,000 cumulative allocated cells, 2,000,000 result cells, 256 KiB definition text, 30 s timeout — exceeding throws. |
| Counter-definition files | 256 KiB each, 32 files, 2 MiB per library. |
| Source snapshots (inline) | 2 MiB/file, 16 MiB/capture. |
| Native source mappings (referenced) | up to 512 MiB/file, 200-line pages. |
| Project file | 128 MiB. |
| Timeline HTML export | 512 MiB document, 256 time segments, 512 pages, and a two-mode drawn-record bound: 2,000,000 (native streaming) vs 100,000 (browser). |
| Portable archive | 100,000 files, 128 MiB manifest, 128 GiB total, 2M instruction/observation caps. |

## MCP query and analysis limits

These bounds are separate from import caps and are not moved by `ROCPROF_MAX_FILE_MIB`. Imported capture storage remains resident; encoded bytes and record counts do not guarantee a fixed RSS.

| Area | Limit / behavior |
| --- | --- |
| Paging | Default 100, compatible clamp 1000; additive `effectiveLimit`; traverse by `nextOffset: null`, with a positive limit. Zero is count-only. |
| Query encoded bytes | 4 MiB per scanned/selected row; 16 MiB selected array or unpaged response. Oversized rows are refused, never skipped. |
| Manifest / NDJSON record | 1 MiB each; 64 KiB read buffer; trailing newline and exact counts required. |
| Kernel/timeline input spool | 2 GiB per job, preflighted before worker spawn. |
| Kernel reducer state | 65,536 groups, 16 MiB unique UTF-8 name bytes, two million matching lifetime pairs. Host preflight conservatively refuses more than two million total residency records, even if unmatched. |
| Kernel result spool | 32 MiB total; rows at most 1 MiB. Query/page arguments apply after exact full-capture aggregation. |
| Timeline window | 100,000 marks and 16 MiB encoded marks; input string fields at most 64 KiB UTF-8. Exact windows subdivide up to 256 segments; persistent density is refused. |
| Timeline output | Inline 16 MiB / 100,000 glyphs; new file 512 MiB / two million glyphs. `maxOutputBytes` only lowers the mode cap. 1 MiB chunks, 512 pages, input timePages 1–64 and lanesPerPage 1–24. |
| Additive response context | 1 MiB for complete clock/identity/coverage/warnings; oversized context requires capture export/offline inspection, not warning truncation. |
| Other analysis input | 256 MiB encoded relevant projection; diagnostics retains the full capture under that preflight. Reducers still materialize these inputs/results. |
| Worker | Production Node heap 1024 MiB, deadline 120 s, nonstreaming stdout/stderr guards 64 MiB, streaming receipt stdout 64 KiB. |

A smaller kernel page cannot reduce exact aggregate input or union state. For offline exact aggregation, traverse dispatches and intervals with positive `limit` and `nextOffset`, and union lifetimes globally; do not sum per-part unions. Timeline `limit` is inapplicable: narrow `range` for density failures, or supply a new `output` path for inline byte/glyph exhaustion. A single oversized query row needs `capture_export` for offline inspection; counters and compile-report entries are not split into new paging shapes. Sources retain their resident path inventory. No refusal publishes partial output or treats missing evidence as measured zero.

## Correctness caveats (verbatim in substance)

- *"Synthetic example values are not GPU measurements."* The bundled demo exists to explore the interface.
- *"Coverage cannot be proven from a dispatch CSV alone."* A valid-looking file does not prove every dispatch was observed.
- *"No metric or diagram should be read as evidence that unobserved CUs, spills, bank conflicts or graph replays are zero."* Absence of a record is not a measured zero, and the app shows "not collected" distinctly from an observed numeric zero.
- Summed durations, interval union and application latency answer different questions, and the dispatch view keeps sum and union separate rather than collapsing them. See [Export & evidence](/export).
- Raw counter series retain their names and scopes; no architecture-derived normalization is guessed.
- Timeline **Detail** aggregation is a pixel-bucket approximation, not pixel-identical Qt rendering.

## HRX / Loom native-driver profiling {#hrx}

Tracer `hrx` and the IRPF importer cover HRX/Loom applications on the native AMD driver, which rocprofv3 cannot observe. These limits are stated by the capture itself (`metadata.coverage`, the `hrxTrace*` and `hrxCounter*` metadata keys, and warnings); the numbers below were measured on gfx1151 only. The full account is in [HRX profiling](https://github.com/pwilkin/ROCprofGUI/blob/main/docs/hrx-profiling.md).

| Limit | What it means |
| --- | --- |
| Driver build | Waves and ISA need a Loom tool built from hrx-system branch `profiling/executable-traces`; hardware counters need `profiling/counters`. Both are local branches, not upstream. A stock tool gives dispatch names, grids and timing only. |
| Hardware | The driver implements thread traces and counters for gfx11.5 and USER-published PM4 queues. Other GPU generations, AQL queues and KERNEL-published queues return unimplemented. |
| Wave coverage | A thread trace is a sample of about 2% of waves: one SIMD of one workgroup processor per shader engine. Wave counts, instruction hits and hotspots describe the sampled waves only. Per-kernel time still comes from every dispatch event. |
| Trace extent | A recorded command buffer is traced whole or not at all, and a full buffer truncates the trace. The driver default of 16 MiB per shader engine covered 31 ms of a 577 ms recorded execution. One trace is in flight per device. |
| Trace decode time | `capture_open` decodes every trace partition: about 1.5 s per MiB of trace (79 s for a 64 MiB capture). |
| Counter values | Window sums over the sampled dispatches. Event counts are the dispatch's own only among the producing device's queues and only when the capture reports an exclusive sample window; otherwise they are upper bounds. Cycle counters (`SQG_BUSY_CYCLES`, `SQG_WAVE_CYCLES`, `SQG_CYCLES`) are never per-dispatch. |
| Counter coverage | Only the first dispatches matching the filter are sampled, up to `hrx.counters.maxSamples` (driver default 4,096). Totals cover the sampled dispatches only; a counter with no sample reads "not collected", never zero. |
| Counter cost | About 44 µs per dispatch for the `sqg` set and about 210 µs with the `instructions` set added; dispatch intervals moved by about 2%. |
| Counter width | The per-workgroup-processor `SQ_*` counters are 32 bits wide. |
| Collection window | The target records from start to exit. There is no trigger, no window and no attach to a running server. |
| GPU + NPU groups | Dispatch timing needs a Loom tool built with the allow-partial patch and `hrx.allowPartial`. The XDNA device records queue events and executable metadata only; kernels placed on the NPU never appear as dispatches. |
| Time base | Dispatch ticks are device-relative; events are not placed on a host timeline. |

Loom kernels on gfx1151 run as 32-lane waves, so the expected `SQG_WAVES` of a dispatch is its workgroup count times `ceil(threads per workgroup / 32)`: a 256-thread workgroup is 8 waves. The workgroup count is the capture's grid divided by the workgroup size per dimension.

## What is not implemented

- **LLM source iteration.** The sidebar's "Kernel experiments · Isolation, tuning & AI iteration" placeholder is marked PLANNED; the AI-iteration part does not exist. (Kernel and shader *tuning* themselves are implemented and wired — see [Kernel tuning](/kernel-tuning) / [Shader tuning](/shader-tuning).)
- **Full ROCprof Compute Viewer parity.** See [Validation & parity](/validation).
- Several views/inputs are explicitly "Partial" (e.g. SIMD utilization, ROCPD input).

## A real coverage story

From the project's own `docs/profiling-caveats.md`: a clean PM4 trace showed 301,692 non-graph kernels and zero `hipGraphLaunch`-correlated kernels, while classic-graph attribution held 426,763 graph-replay kernels. The files looked complete and were not. This is why the app treats coverage as unproven until independently checked.
