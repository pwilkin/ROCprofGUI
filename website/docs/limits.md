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

## Correctness caveats (verbatim in substance)

- *"Synthetic example values are not GPU measurements."* The bundled demo exists to explore the interface.
- *"Coverage cannot be proven from a dispatch CSV alone."* A valid-looking file does not prove every dispatch was observed.
- *"No metric or diagram should be read as evidence that unobserved CUs, spills, bank conflicts or graph replays are zero."* Absence of a record is not a measured zero, and the app shows "not collected" distinctly from an observed numeric zero.
- Summed durations, interval union and application latency answer different questions, and the dispatch view keeps sum and union separate rather than collapsing them. See [Export & evidence](/export).
- Raw counter series retain their names and scopes; no architecture-derived normalization is guessed.
- Timeline **Detail** aggregation is a pixel-bucket approximation, not pixel-identical Qt rendering.

## What is not implemented

- **LLM source iteration.** The sidebar's "Kernel experiments · Isolation, tuning & AI iteration" placeholder is marked PLANNED; the AI-iteration part does not exist. (Kernel and shader *tuning* themselves are implemented and wired — see [Kernel tuning](/kernel-tuning) / [Shader tuning](/shader-tuning).)
- **Full ROCprof Compute Viewer parity.** See [Validation & parity](/validation).
- Several views/inputs are explicitly "Partial" (e.g. SIMD utilization, ROCPD input).

## A real coverage story

From the project's own `docs/profiling-caveats.md`: a clean PM4 trace showed 301,692 non-graph kernels and zero `hipGraphLaunch`-correlated kernels, while classic-graph attribution held 426,763 graph-replay kernels. The files looked complete and were not. This is why the app treats coverage as unproven until independently checked.
