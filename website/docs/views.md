# Views reference

One section per sidebar view. Unless a caption says otherwise, each screenshot is the bundled synthetic example in browser preview; the values are illustrative, not GPU measurements.

One rule applies to every view: no metric or diagram below is evidence that unobserved CUs, spills, bank conflicts or graph replays are zero. "Not collected" is always shown distinctly from an observed numeric zero. See [Limits & caveats](/limits).

## ISA reference

![ISA reference view](/img/view-reference.png)

An offline catalogue of AMD instruction definitions for CDNA 1–4 and RDNA 1/2/3/3.5/4 (≈10,628 entries): encodings, operand formats, and WMMA/MFMA/SWMMAC/SMFMAC matrix-type compatibility. It is searchable and is reachable from a selected trace instruction via **Instruction reference**. The data is regenerated with `scripts/build-isa-reference.py`.

![Matrix-type compatibility (desktop application)](/img/isa-matrix-types.png)
*The matrix-type compatibility matrix.*

## Trace workspace

![Trace workspace view](/img/view-trace.png)

The core view: the execution timeline over five projections (**Target wave** by instruction class, **CU waves** by SIMD/slot, **SIMD utilization**, **Global residency**, **Dispatch timeline**), with a linked **ISA explorer** and **Source snapshot** underneath.

What the numbers mean:

- **Recorded wave instances / ISA instructions / Capture span** summarise the capture; *Coverage status* reads "Unqualified · Independent replay check required" because coverage cannot be proven from the import alone.
- In the ISA table, **Hits** is the number of recorded executions of that instruction, **Cycles** the attributed cost under the current latency scope, **Stall** the stalled portion, and **PC samples** the sampling hits where available. The **Latency scope** selector switches between whole-capture sum/mean, target-wave maximum/selected-occurrence, and visible-range sums/means.
- The **Source snapshot** overlays per-line cost. "Inline locations are inclusive; one instruction can contribute to multiple lines. An em dash means no recorded value; ≥ marks a partial sum." Wait dependencies are reconstructed from named counters when imported links are absent.

## Compute fabric

![Compute fabric view](/img/view-hardware.png)

A CU/SIMD residency diagram plus static register/LDS metadata, with explicit unknown states where a value was not recorded. Use it to see which compute units a dispatch actually occupied.

![Compute fabric (desktop application, alternate capture)](/img/compute-fabric.png)

## Activity summary

![Activity summary view](/img/view-summary.png)

Activity cost plus a hardware-activity model (MI200/MI300 ATT activity bars, peak ratios) and a per-CU table. The hardware-activity model runs in a Web Worker; on an unsupported architecture, a missing ATT counter, or an input above the sample cap it falls back to a synchronous path and reports the limitation rather than guessing. Input is capped at 2,000,000 samples.

![Activity summary (desktop application)](/img/native-activity-summary.png)

## PC sampling

![PC sampling view](/img/view-sampling.png)

Totals, stall-reason breakdown and a flamegraph for host-trap or stochastic PC-sampling data. Host-trap and stochastic samples are kept as distinct identities; raw observation rows are searchable.

![PC sampling (desktop application)](/img/native-pc-sampling.png)

## Occupancy

![Occupancy view](/img/view-occupancy.png)

Per-SE and per-kernel occupancy plots: how many waves were resident over time. Export is available as SVG.

## Kernel dispatches

![Kernel dispatches view](/img/view-dispatch.png)

The dispatch list/timeline with host-API call association. This view reports summed durations and interval union separately, because kernel intervals overlap and those two numbers answer different questions (total work vs wall-clock span). See [Export & evidence](/export). A selected dispatch is also where **"Tune this kernel"** ([Kernel tuning](/kernel-tuning)) is offered.

## Instruction hotspots

![Instruction hotspots view](/img/view-hotspots.png)

Instruction-cost histograms with an independent mean-latency axis: which instructions dominated, by hits and by cost.

## Source flamegraph

![Source flamegraph view](/img/view-flame.png)

A source-to-ISA latency flamegraph: cost rolled up through the source hierarchy. The viewport is sampled for responsiveness, and above its budget it refuses rather than silently downsampling.

## Marker scopes

![Marker scopes view](/img/view-markers.png)

An instrumentation-marker timeline and flamegraph with an integrated marker → source → ISA hierarchy and a separate cumulative-duration viewport.

![Global markers overlay (desktop application)](/img/native-global-markers.png)
*Marker overlays on the global timeline (desktop app).*

## Counters

![Counters view](/img/view-counters.png)

Two kinds of counter work:

- **Scalar expressions** (*Add counter*): safe arithmetic over recorded counters (`C0`, …). Up to 64 definitions per capture; synchronous evaluation over up to 500,000 samples.
- **Named-axis tensor definitions**: reductions, broadcasting, slices/deltas, comparisons, temporary variables, and a multi-file definition library. These run in a Web Worker with hard caps (1,000,000 input samples, 4,000,000 raw cells, …; see [Limits](/limits)) and a 30-second timeout; exceeding a cap throws.

Raw counter series keep their names and scopes; no architecture-derived normalization is guessed.

![Counter expressions (desktop application)](/img/counter-expressions.png)
*Scalar counter expressions in the desktop app.*

![Counter definition files (desktop application)](/img/counter-definition-files.png)
*The multi-file counter-definition library.*

## Capture evidence

![Capture evidence view](/img/view-evidence.png)

The honesty surface: capture warnings, code-object list with byte-range extraction and relocations, trace↔dispatch associations, coverage diagnostics, and the export actions (diagnostic manifest, selection JSON, SVG/PNG, archive). Covered in detail in [Export & evidence](/export).
