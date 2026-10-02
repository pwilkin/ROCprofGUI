# Profiling validity lessons recovered from local handoffs

These are historical observations from this machine's ROCm work, not guarantees that every runtime version behaves identically. The handoff files were re-read for this implementation; the profiling workloads themselves were not rerun.

## Graph replay coverage

The 2026-09-08 profiling handoff documents a clean PM4 trace containing **301,692 non-graph kernels and zero kernels correlated with `hipGraphLaunch`**, while classic-graph attribution contained the same non-graph work plus **426,763 graph-replay kernels**. Valid-looking files and unique dispatch records did not prove coverage.

For that particular runtime, complete diagnostic attribution used `DEBUG_HIP_GRAPH_CLASSIC_PATH=1` and `DEBUG_HIP_GRAPH_PM4=0`. Native retained-PM4 runs supplied throughput. Those variables are a recorded workaround, not a universal default for the GUI.

**Design consequence:** imported dispatch data is coverage-unknown until independent API/replay evidence is attached. The UI calls percentages shares of recorded summed work; it does not call them shares of native application time.

## Timelines are not wall-time decompositions

Kernel intervals can overlap. Summed durations, interval union and application latency answer different questions. Graph disabling and synchronization can create apparent idle gaps that disappear in native execution. Host synchronization API duration often includes waiting for real GPU work.

**Design consequence:** retain separate clock domains and denominators. The dispatch view shows sum and union separately. Never infer a host bottleneck merely from large synchronization calls or profiled gaps.

## Counter contamination and units

The Q8 investigation documented extra waves in some selected counter windows. Samples were qualified using the expected `SQ_WAVES` count; groups without that counter could not exclude contamination. Repeated counter-pass rows required dispatch/grid-aware aggregation.

`SQ_WAIT_CNT_VMVS` was specified in cycles, while `SQ_WAVE_CYCLES` and `SQ_WAIT_INST_LDS` were specified in quadcycles. Ratios required conversion and represented aggregate wave cycles, not wall time. Cache hit ratios counted requests and did not directly measure the fraction of model weights served by a cache.

**Design consequence:** raw series retain names/scopes and no architecture-derived normalization is guessed. Scalar counter arithmetic requires matching scope and exact sample timestamps. The full dimension/unit-aware expression engine remains a parity milestone.

## ATT samples the first occurrence of each kernel name

The 2026-10-02 live-profiling campaign (`/devel/tools/rocprofgui-campaign-2026-10-02/`) established that rocprofv3 SDK 1.3.5 default per-dispatch ATT captures the **first** occurrence of each kernel identity in the target, and that reattach windows on the same already-sampled kernels produce no new `.att` files. A name regex (`kernelIncludeRegex`) narrows ATT to a name but still takes its first occurrence; it cannot discriminate different grids that share a kernel name. `--kernel-iteration-range` is the only way to target a later dispatch — on a fresh target, iteration 64 selected dispatch 7139. The MCP surfaces this as `kernelIterationRange` on `profile_run`/`profile_trigger`, and because rocprofv3 pickles it into the reattach option set alongside the name regexes, it is part of the same same-collection-per-process guard.

**Design consequence:** to profile a specific dispatch other than the first under a kernel name, select it by iteration range on a fresh target; do not expect a name filter alone to move the ATT sample.

## Tool compatibility and failure

Historical failures included:

- Split ROCm discovery requiring the correct `ROCM_PATH` and Python environment.
- High-level metric evaluation failing at `SQG_LEVEL_WGP_ACTIVE` before workload execution.
- Counters passing compatibility checks but hanging at runtime.
- No supported PC-sampling agent reported on the tested setup.
- Profiler signal-handler chaining failing during shutdown after output files were written.
- Intermittent profiler interaction with lazy model loading.

**Design consequence:** capability discovery, collection completion, artifact validity and workload success must be independent states. Collection needs timeouts, exact process ownership and retained logs; retries are explicit experiments, not silent success conversion.

## Resources and causal claims

Metadata VGPR counts differ from allocation granularity. More registers can improve instruction scheduling even if occupancy falls. Scratch bytes are not a spill count. Zero admission-pressure counters do not prove high achieved occupancy. LDS instruction activity is not a bank-conflict measurement.

**Design consequence:** the hardware diagram displays recorded residency separately from static register/LDS metadata. No occupancy percentages or bank-level colors are synthesized from missing hardware limits or counters.

Loom compile reports extend the same separation. They are static compiler facts about a compiled kernel, imported onto a surface kept apart from dispatches, counters and waves. Their occupancy percent and scheduled register pressure are compiler evidence, not observed GPU occupancy or hardware register counts, and the importer does not convert them into either. A compile report carries no cycles or time, and a field a report does not establish is unavailable, not zero.

## Experiment identity and qualification

Prior experiments found that missing library symlinks could load the same implementation in both A/B arms. Instrumented timing, concurrent compilation, thermal imbalance, partial correctness checks and full-model quality failures all invalidated otherwise attractive speedups.

**Design consequence:** future experiments must retain source/build hashes, loaded library maps, workload identity, warmup policy, every timing sample, temperature/contention evidence and the numerical oracle. Exact operator correctness and full application quality/performance remain separate gates.

## Local source evidence

- `/devel/tools/rocm-systems/.Codex/2026-09-08-profile/LIVE-HANDOFF.md` and `REPORT.md`: exact PM4 replay coverage trap, graph attribution and signal-handler outcomes.
- `/devel/tools/rocm-systems/.Codex/2026-09-05-flash-next/q8-deep/PROFILE.md`: counter units, contamination, unsupported PC sampling, high-level metric failure and counter hang.
- `/devel/tools/rocm-systems/.Codex/2026-09-06-q8-memory/PROFILE.md`: allocation granularity, controlled A/B and hardware-counter qualification.
- `/devel/tools/rocm-systems/.Codex/2026-09-06-decode-profile/REPORT.md`: interval unions and synchronization interpretation.
- `/devel/tools/rocm-systems/.Codex/2026-09-11-halogen-catchup/FINDINGS.md`: profiler-induced serialization gaps, spills, real runtime identity and full-model gates.
- The earlier llama.cpp Qwen3.8-27B profiling memory supplied the pointer to dispatch/grid counter deduplication and split ROCm discovery; this older run was not repeated.
