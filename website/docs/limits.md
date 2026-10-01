# Limits & caveats

This project's credibility rests on being honest about limits. The figures below are taken from the shipped code, not from prose; where the repository's own docs disagree, the code wins and the discrepancy is noted.

## Import caps

| Cap | Value | Notes |
| --- | --- | --- |
| Per-file size | **256 MiB** | Enforced on sidecar/JSON/CSV files. Not applied to a ROCPD/SQLite database, which is bounded per query instead; a DB larger than 256 MiB can be opened. |
| Main record arrays | **2,000,000 records** | |
| Open native captures | **6** (+ the bundled example) | |
| Raw-ATT import timeout | **300 s** (5 min) | Plus a Cancel button; also aborts if the decoder log exceeds 16 MiB. |

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

::: tip Known-stale figures in the repository docs
The summary cap is 2,000,000 samples — `docs/upstream-parity.md:34` still says 500,000 and is stale (`src/summary.ts:173`). The wave-statistics cap is 4096 waves (128 is only the *viewport* bound) — `docs/architecture.md:144` and `:152` say 128 for both and are wrong (`wave_index.rs:7-8`). This site follows the code.
:::

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
