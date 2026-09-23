# Blocked inputs and the producer contract

This document records the external inputs the project is waiting on, and the evidence behind each. It exists so those questions are answered from the repository rather than re-derived from working notes every session: `.codex/` is an untracked working directory and is not present in a clone.

## The rule

**Nothing here is unblocked by guessing.** A private or inferred schema is not a producer contract, and a filename that looks like a dispatch ID is not a dispatch relationship. Where identity cannot be established, the application keeps it explicitly unknown and says so in the UI. An adapter written against a guessed layout would silently misattribute measurements, which is worse than the current refusal to load.

## 1. Embedded ROCPD ATT adapter

**Needed:** a supported producer's versioned table/column/payload contract for ATT bytes stored inside a ROCPD database, or a representative ROCPD file that contains ATT data.

**Evidence.** The upstream viewer's `RocpdEmitter::run` is a skeleton: it names embedded blobs and relative external paths in comments, throws `"rocpd support not yet implemented"`, and supplies no table names, field names, schema version or implemented query.

The official-schema-generated ROCPD fixtures for 3.0.0 through 3.0.4 were reopened read-only and every table and column recorded. Across the five fixtures there are **109 tables, zero columns declared `BLOB`, and no table named for `att`, `thread_trace`, `artifact` or similar**; there are **89 generic JSONB `extdata` columns**. The code-object table stores a URI, a load range and FILE/MEMORY provenance, and defines no ATT payload contract.

This is evidence about these schemas, not proof that no third-party or future ROCPD extension can carry trace bytes. SQLite's dynamic typing and the generic `extdata` columns mean column declarations alone cannot rule out embedded data — which is exactly why an adapter must not be written speculatively.

## 2. Automatic ATT-to-dispatch association

**Needed:** paired producer ROCPD/ATT data that retains capture mode and process/node/GUID identity.

**Evidence.** In the producer, the ATT output stem is built from agent, shader engine and a value called `dispatch_id`; when user data is zero, that value comes from `att_device_trace_id` for device, consecutive and marker modes. It is therefore **not unconditionally a kernel dispatch ID**. The JSON export flattens the in-memory ATT filename map to basenames, losing its structured keys, and the same agent/trace key is reused for `ui_output_agent_*_dispatch_*` directory names — so the directory spelling does not repair the ambiguity either. Upstream documentation also describes consecutive-kernel and selected-region captures.

Consequently a basename or directory match **cannot** establish a dispatch relationship. External-file discovery may offer candidates, but an association workflow has to retain explicit provenance and keep dispatch traces, device/region traces and unknown-mode traces distinct. Independent shader clocks stay separate until measured alignment exists, and code-object URI/load metadata keeps its process/node/GUID scope.

### Required acceptance tests

These are the cases the remaining ROCPD artifact work must satisfy. They cannot be written until input 1 or 2 arrives.

1. Single-dispatch producer metadata with explicit scope and a known capture mode.
2. A device-mode trace ID colliding with a real dispatch ID produces **no** automatic dispatch link.
3. Duplicate agent/dispatch numbers across process/node/GUID partitions produce **no** cross-partition association.
4. Multi-dispatch, consecutive and selected-region traces retain multi-dispatch or unknown ownership.
5. Missing, relocated, duplicate and changed external files produce explicit outcomes and preserve recorded provenance.
6. An embedded payload adapter requires a real versioned table/column contract or a representative producer fixture, with bounded extraction and decoder integration. Arbitrary `BLOB` or `extdata` fields must not be guessed to be ATT.

## 3. Hardware qualification

**Needed:** eligible GPU hosts, or representative captures, for the outstanding topology, clock, collection-loss/wrap, multi-SE SPM/SQTT and architecture-specific export cases.

**Evidence.** The qualification host reports gfx1151 (Radeon 8060S, 40 CUs). It exposes SPM configuration intervals of 32–65,504 shader-clock cycles but **no eligible SPM counters** under either the installed SDK 1.3.5 or a local SDK 1.4.0: the AQLprofile event filter excludes gfx1151 even though configuration enumeration succeeds. The capability listing reports no PC-sampling-capable agent. A bounded HIP workload passed all 65,536 CPU-oracle comparisons, but no SPM collection was forced and none is claimed. These are live capability queries, not a successful hardware collection, and not a claim about every driver/SDK combination.

A single real capture carrying **explicit architecture and counter metadata** would unblock the summary model, marker goldens, counter semantics and wave-state semantics together — currently four separate hardware-blocked items.

### Thread traces and counters are obtainable on this host (2026-09-22)

The paragraph above remains true of **ROCm SPM**. It is not true of this hardware, and it was never true of ROCm thread traces.

**rocprofv3 thread tracing works on gfx1151.** `rocprofv3 --att` (SDK 1.3.5) on a HIP kernel decodes to 102 waves with 836,536 timeline states, none negative, and every instruction attributed. A wave64 build of the same kernel decodes with every loop-body instruction hit exactly once per iteration. These are the first live hardware thread traces this project has produced; before this, every trace was an archived or synthetic fixture (T15).

**Mesa's RADV Vulkan driver produces both thread traces and counters** on the same GPU, and does not go through AQLprofile. First measured with Mesa 26.0.8 and `MESA_VK_TRACE=rgp MESA_VK_TRACE_PER_SUBMIT=1`:

- **Counters.** A `DERIVED_SPM` chunk (RGP type **128**; RADV emits no raw type-8 `SPM_DB` on GFX10+) holds 14 named counters and 19 components, sampled every 4,096 clocks. Its timestamps are the GPU timestamp, which six `CLOCK_CALIBRATION` points spanning 0.1 s to 80 minutes measure at **100.003 MHz**. They are not shader clocks, and the 931 MHz `ASIC_INFO` clock is not the clock during capture. Counters that should read zero for the workload do (no LDS, no raytracing, no PCIe traffic on a unified-memory APU), and `L0 cache hit` reads zero throughout, a known Mesa defect fixed for 26.2.4. Summed over a dispatch with a known 16 KB footprint, however, the byte counters do **not** match it: fetch reads 3.9×, write 0.03×, local video memory 30×. They describe memory-side traffic, which caching and the profiler's own writes both distort.
- **Thread traces decode, with a precise limit.** See below.

#### Why RADV thread traces decoded to garbage, and what the fix covers

An earlier revision of this section blamed a clock problem. That was wrong: the timestamps spanning 458 trillion cycles, the negative state durations and the "Data Lost" reports were all symptoms of the decoder **losing sync with the token stream**.

The cause is RADV's thread-trace token mask. Both drivers program nearly identical `SQ_THREAD_TRACE_CTRL` values on GFX11. `SQ_THREAD_TRACE_TOKEN_MASK` differs: AMD's aqlprofile (`projects/aqlprofile/gfxip/gfx11/gfx11_primitives.h`) excludes five token classes, bits 0, 1, 7, 9 and 10 by Mesa's `gfx11.json` names: VMEM execution, ALU execution, event, utilization counter and wave allocation. It does not exclude perf. RADV (`src/amd/common/ac_sqtt.c`) excludes only perf when instruction timing is on. `rocprof-trace-decoder` is built for aqlprofile's mask, and its token enum contains **no ALU- or VMEM-execution type for any generation**. When it meets one, it steps 4 bits into it, reads the payload as tokens, and eventually decodes repeating data (`dcf001dcf001`) as a 36-bit timestamp delta.

Walking the stream independently with the decoder's own gfx11 encoding table identified two 24-bit token families (headers `1110` and `x111`). With those added, the stream parses exactly: every one of 328 gaps between consecutive realtime timestamps lands on the next realtime token. Blanking them to NOP nibbles before decoding gives, through the **unmodified** decoder, waves with zero negative states whose state durations sum exactly to each wave's span, and a shader clock that holds a steady ratio to the realtime clock end to end.

The decoder also needs to know where each shader was loaded. rocprofv3 writes that into the trace; RADV records it in the RGP `CODE_OBJECT_LOADER_EVENTS` chunk instead. Each ELF's `.internal_pipeline_hash` equals its loader event's code-object hash, so shaders are placed by hash. This decoder version returns `NOT_IMPLEMENTED` from its load-address API, so traced PCs are translated in the code index instead.

**What the fix does not cover.** Those tokens are not padding: RADV reports most VALU execution through them. In a wave64 loop of four VALU instructions per iteration, the correctly parsed stream has one VALU token per iteration and about five execution tokens. Blanking them keeps timing exact but drops those instruction events, so per-instruction attribution through loops fails and VALU activity is undercounted. Busier instruction-timing streams also contain a third undocumented header (`1000001`) and irregularities that brute-force length fitting did not resolve consistently; guessing further would risk wrong attribution, which is worse than none. Their format is not public.

**What works, measured on gfx1151 with Mesa 26.2.0.**

- **Occupancy.** `RADV_THREAD_TRACE_INSTRUCTION_TIMING=false` excludes exactly the execution tokens, and the stream then needs no sanitizing. At 4,096 × 4,096 it decodes to **exactly 4,096 waves**, one per workgroup, with zero clock anomalies.
- **Full per-instruction attribution, with a RADV patch.** `docs/radv-rocprof-compat.patch` adds an opt-in `RADV_THREAD_TRACE_ROCPROF_COMPAT` option that programs aqlprofile's GFX11 exclusion set; default behaviour is unchanged. Built from a copy of Mesa 26.2.0, it gives streams with zero execution tokens and zero unknown headers at every size. The unmodified decoder then attributes every instruction exactly:
  - a 64-iteration loop body is hit 64 times per wave, the loop test 65, straight-line code once, and `s_delay_alu` hints and padding never;
  - at 4,096 workgroups × 4,096 iterations, all 4,096 waves appear in occupancy, and each of 101 traced waves hits every loop-body row exactly 4,096 times.

  With RADV's default mask, the same small kernel attributes **8 of 1,052** executed instructions. So "undercounted", this document's earlier word, understated it: per-instruction data is effectively absent without the patch.
- **A real workload.** Every submit of a llama.cpp prompt-processing run (3B model, Q4_0, pp64) was imported: 2,077 submits per run, twice, with no rejections.
  - **Control flow.** In a random sample of 120 submits, all 12,612 traced waves start at their shader's entry, end on `s_endpgm`, have no negative state, and have states summing exactly to their span.
  - **Occupancy.** On the vendored submit, occupancy per kernel equals each dispatch's workgroup count × waves per workgroup for all eight kernels, 9,488 waves in all.
  - **A defect this found.** The earlier realtime-cadence check rejected 93% of these submits, because realtime tokens pause while the GPU idles between dispatches. It now tests whether the shader clock nearly stopped across a gap. RADV inhibits clock gating while tracing, so it cannot legitimately stop, and the corrupted captures the old check was added for are still rejected.
- **Kernel names and grids.** RADV leaves RGP's pipeline names empty, but writes RGP instrumentation markers into the trace as `SQ_THREAD_TRACE_USERDATA_2/3` register tokens: debug labels, pipeline binds, and one event per hardware dispatch. With `GGML_VK_DEBUG_MARKERS=1`, every llama.cpp dispatch is named after its ggml shader, and RADV's internal copy shader after `vkCmdCopyBuffer`. The markers' event counts matched the decoder's dispatch records on every submit. The pipeline each marker says was bound matched the one the decoder saw run.
- **Raw counters.** Mesa 26.2's `RADV_SPM_COUNTERS_CONFIG` imports through the same derived-SPM path, marked as hardware counters. Against a dispatch of known size, they fall short on this GPU:
  - TA buffer-wavefront counts read 80 of 128 and 624 of 1,024 (61–63%): RADV expands `ALL` to `max_good_cu_per_sa` instances;
  - SQ events 3, 4, 24, 26 and 35 read zero in every sample;
  - SQ event IDs above 35 are rejected, which excludes the instruction-count events, and GRBM has no free SPM slots;
  - a rejected line makes `vkCreateDevice` fail.

**What remains upstream.** The compat option exists only in this project's patch; nothing has been proposed to Mesa. Decoding RADV's default mask needs `rocprof-trace-decoder` support for the execution tokens, from AMD, which holds the format.

`VK_KHR_performance_query`, which would give exact per-dispatch totals, reaches GFX11/11.5 only in Mesa 26.2.0. It is mutually exclusive with `MESA_VK_TRACE` and unavailable on compute-only queues. Its results go to the application, not into a capture, so there is nothing for this viewer to import.

**What this changes.** Real hardware counters and thread traces are obtainable here now, without new hardware or a producer contract. They do not close items 1 or 2, which concern ROCPD and ATT producer identity, and they do not qualify the summary model, which is defined for MI200/MI300. They do remove "nothing is obtainable on this host" as a reason those items cannot progress.

## 4. Display qualification

**Needed:** a multiple-display or mixed-scale environment, or an explicitly agreed change to the acceptance scope.

**Evidence.** The host has a single 1920×1080 HDMI-A-1 output. Changing the user's monitor configuration is not a substitute for qualifying physical multi-monitor WebKit behaviour.

## 5. Memory acceptance

**Needed:** representative required workloads, and an acceptable memory ceiling.

**Evidence.** Post-close retained footprint is high, variable and concentrated in WebKit, and grows across open/close cycles. The Rust side is demonstrably released. No whole-app memory bound is claimed and none can be accepted or rejected until a ceiling is named. This does not preclude fixing a demonstrated defect; it blocks declaring memory behaviour acceptable.

## Status

All five remain unanswered. They are completion requirements, not a claim that every possible improvement is exhausted. Rephrasing them does not make them answered, and an audit that restates them is not progress.
