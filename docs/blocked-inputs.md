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

### The ROCm path is blocked here; the Vulkan path is not (2026-09-22)

The paragraph above remains true **of ROCm**. It is not true of this hardware. The same GPU produces both thread traces and hardware counters through Mesa's RADV Vulkan driver, which does not use AQLprofile and is therefore unaffected by the filter that excludes gfx115x.

Measured on this host with Mesa 26.0.8, capturing a single-dispatch Vulkan compute workload with `MESA_VK_TRACE=rgp` and `MESA_VK_TRACE_PER_SUBMIT=1`:

- **Thread trace:** 11,172,512 bytes of SQTT payload across 2 shader engines, which ROCm's own `librocprof-trace-decoder.so` decodes into **20,450 waves** with per-wave instruction and timeline data, plus a disassembled code object. The decoder reports `gfxv: navi`, `gfxip: 11`.
- **Hardware counters:** the capture carries a `DERIVED_SPM_DB` chunk (RGP chunk type **128**, not type 8 — RADV emits only the derived form on GFX10+) with **804 samples at a 4,096-shader-clock interval**, holding **14 named counters and 19 components**: instruction/scalar/L0/L1/L2 cache hit, LDS bank conflict, fetch and write size, local video memory bytes, PCIe bytes, memory unit busy and stalled, and ray-box/ray-triangle tests. SPM is enabled by default; `RADV_THREAD_TRACE_CACHE_COUNTERS` is its master switch despite the cache-specific name.

Counters that should read zero for the workload do (no LDS use, no raytracing, unified-memory APU with no PCIe traffic), and `L0 cache hit` reads zero across every sample — a known Mesa defect where GFX11 uses the GFX10 TCP selector, fixed upstream for 26.2.4. That agreement between an upstream source claim and this capture is why the non-zero values are treated as real.

**Two limits of this route, both measured rather than assumed.** The SQTT wave timestamps are not usable as-is: the capture spans 458,443,249,327,780 shader cycles for a sub-millisecond dispatch, 351 of 400 sampled waves contain negative timeline durations, and 3 of 20,450 waves have `end < begin`, so the importer rejects the capture on its own validity guards. A ROCm gfx1200 capture decoded through the same library in the same session has **zero** negative durations and a 122,476-cycle span. That control differs from the RADV capture in **two** ways at once — the producer (RADV vs rocprofv3) and the architecture (GFX11.5 vs GFX12) — so it does not isolate the cause. The defect could be in how RADV frames or configures SQTT, or in the ROCm decoder's handling of GFX11.5, which AMD's own profiling stack excludes elsewhere (the AQLprofile filter above). Telling those apart needs a rocprofv3 ATT capture on this same gfx1151 GPU, which has not been attempted. What is established is narrower: the SPM timestamps in the same file span a sane 121,276 cycles, so the hardware clock itself is not the problem. Thread traces from this route are unusable until the cause is found; the counter data is unaffected.

`VK_KHR_performance_query`, which would give exact non-sampled per-dispatch totals, is gated to GFX10.3 in Mesa 26.0.x and reaches GFX11/11.5 only in 26.2.0. It is also mutually exclusive with `MESA_VK_TRACE` and is unavailable on compute-only queues. Mesa 26.2+ additionally adds `RADV_SPM_COUNTERS_CONFIG` for user-defined raw SPM counters.

**What this changes.** Counter metadata from real hardware is obtainable on this host now, without new hardware and without a producer contract. It does not close items 1 or 2 of this document, which are about ROCPD and ATT producer identity, and it does not by itself qualify the summary model — that formula is defined for MI200/MI300 and gfx1151 still has no validated model. It does remove "no counters are obtainable here" as a reason those items cannot progress.

## 4. Display qualification

**Needed:** a multiple-display or mixed-scale environment, or an explicitly agreed change to the acceptance scope.

**Evidence.** The host has a single 1920×1080 HDMI-A-1 output. Changing the user's monitor configuration is not a substitute for qualifying physical multi-monitor WebKit behaviour.

## 5. Memory acceptance

**Needed:** representative required workloads, and an acceptable memory ceiling.

**Evidence.** Post-close retained footprint is high, variable and concentrated in WebKit, and grows across open/close cycles. The Rust side is demonstrably released. No whole-app memory bound is claimed and none can be accepted or rejected until a ceiling is named. This does not preclude fixing a demonstrated defect; it blocks declaring memory behaviour acceptable.

## Status

All five remain unanswered. They are completion requirements, not a claim that every possible improvement is exhausted. Rephrasing them does not make them answered, and an audit that restates them is not progress.
