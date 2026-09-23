# GFX1201 validation — Radeon AI PRO R9700

Tested on 2026-09-23 against system Mesa 26.0.8-1ubuntu0.3 and a private stock Mesa 26.2.0 build. The Vulkan workload is an isolated llama.cpp checkout at 7ab4ee7ba and Qwen3.5-9B IQ4_NL. The spec-compliant shared-memory patch from .codex/handoff/mul_mm-stage-in-buf-a.patch was applied only to that checkout, with separate baseline and patched builds.

## RADV traces and tuning

- An isolated Mesa 26.2.0 build with `docs/radv-compute-trigger.patch` captured a headless Vulkan compute dispatch through MCP `profile_start` / `profile_trigger` / `profile_stop`. The test initialized its Vulkan device and compute pipeline before arming the trigger, then wrote and verified 1,024 buffer values. RADV saved one 86,010-byte RGP file; rocprof-studio imported it as a thread trace with four shader-engine dispatch records. The patched ICD lives under `.codex/radv-26.2.0-trigger`; the stock private Mesa build remains separate. A full llama-server inference has not been profiled with this path, so the first triggered submit still needs inspection.
- Stock Mesa 26.2.0 occupancy mode imported the GFX1201 ADD capture: 24 decoded dispatch records (six per shader engine), 120 occupancy intervals across four shader engines, named markers, dispatch grids, and 14 sampled SPM counters. The importer must skip its GFX11 execution-token sanitizer for GFX12. The captured submit is retained as fixtures/radv-rgp/gfx1201-add-occupancy.rgp.
- Default instruction timing on the same small ADD case reported lost trace data. Limiting instruction timing to shader engine 0 did not fix it. The importer kept the SPM counters and rejected the incomplete thread trace. Per-instruction RADV timing is therefore unverified on this card.
- A Vulkan tuning session opened from the ADD trace, captured the application dispatch, found its GLSL source, recovered the compile recipe, and replayed both the original and an edited variant with bitwise matching output. The Best case and Application-like presets ran; automatic memory placement selected device-local memory.
- Mesa 26.2.0 intentionally excludes GFX12 from VK_KHR_performance_query in radv_perf_query_supported. Shader replay reports compiler resources, ISA and GPU timestamp duration, but cannot report GPU cycles, clocks or L0/L1/L2 hit rates through that extension. RGP's sampled counters use a separate path.
- The gfx1151 raw SPM recipe uses GFX11 event IDs. The installed ROCm basic_counters.xml has no GFX12 section, so those IDs were not treated as validated GFX1201 counters.

## ROCm

- Native HIP SAXPY ran correctly on gfx1201. rocprofv3 collected raw ATT and a kernel trace; the app imported the ATT with one dispatch, 16 detailed waves and working wave-read, hotspot and occupancy analyses.
- The pinned Kerncap runtime captured that HIP dispatch and replayed it with bitwise matching output, including a compiled source variant. Its install RPATH needed the ROCm library directory, and the worker needed to compare the ATT mangled symbol with Kerncap's demangled name.
- Native rocprofv3 reported SQ_WAVES_sum = 2048, SQ_INSTS_VALU = 0 and GL2C_EA_RDREQ_sum = 0 for the active SAXPY dispatch. The tuning session's MeanOccupancyPerCU also reported 0, despite active waves. These zero instruction/occupancy values are unreliable on this setup. LDSBankConflict = 0 is plausible for this kernel.
- The app now imports rocprofv3 counter_collection.csv as one dispatch plus three per-dispatch counter values, placed at the dispatch midpoint. It labels them as per-dispatch measurements.

## Shared-memory experiment

The actual prompt-processing aligned_2 matmul used 32 × 4 × 1 workgroups and contained 32 static WMMA instructions in gfx1201 ISA. Both captured shaders replayed with bitwise matching output.

| Driver | Build | LDS bytes | VGPRs | Compiler wave limit per SIMD | Static instructions |
| --- | --- | ---: | ---: | ---: | ---: |
| Mesa 26.2.0 | baseline | 22,528 | 132 | 8 | 3,812 |
| Mesa 26.2.0 | patched | 20,480 | 120 | 12 | 4,686 |
| Mesa 26.0.8 | baseline | 22,528 | 132 | 8 | 3,862 |
| Mesa 26.0.8 | patched | 20,480 | 120 | 12 | 4,690 |

The Mesa 26.2.0 isolated replay measured 437.04 vs 433.80 microseconds in Best case and 409.88 vs 409.96 microseconds in Application-like conditions (baseline vs patched). The latter used sustained dispatches and three fresh allocations; both outputs matched the application byte for byte, and two consecutive dispatches produced the same output. The patch gave no meaningful sustained replay gain.

Application pp512 benchmarks used three interleaved baseline/patched pairs per driver, with three repetitions per run (nine samples per build and driver). Mean prompt throughput was 3065 vs 3095 tokens/s on Mesa 26.2.0, and 2990 vs 3006 tokens/s on Mesa 26.0.8 (baseline vs patched). The paired changes were +1.36%, +0.89%, +0.61% on 26.2.0 and -2.70%, +2.37%, +1.99% on 26.0.8. The larger compiler wave limit does not translate into a material application gain on this card. Static instruction count rose about 23% in the main pipeline; without reliable cycles/cache counters, that is a possible contributor, not an established cause.

## Correctness and regression checks

- Patched MUL_MAT: 1,128 supported cases passed on each Mesa driver; 554 other cases reported unsupported on each. Patched MUL_MAT_ID: 937/937 passed on each driver.
- Rust workspace tests passed. All 21 RADV RGP tests, including decoder-dependent fixtures, passed separately. The counter CSV importer tests passed.
- Vitest: 198/198 passed. Its first run shared CPU with other suites and one large-sample test hit the 5-second timeout; the isolated rerun passed.
- Python tuning workers: 31/31 passed.
- Playwright: 133 passed and 3 skipped. The run log is at .codex/gfx1201/final-playwright.log.

Raw runs and captures are retained under .codex/gfx1201 and ~/.rocprofgui. The large model RGP run produced 20 files totaling about 420 MiB; decoding individual prompt-processing submits reached more than a million occupancy intervals, so it was not used for whole-model timing.
