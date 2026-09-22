# RADV RGP compute fixture

`compute-spm.rgp` is a real capture from this project's qualification host, not synthetic data.

| | |
|---|---|
| GPU | AMD Radeon 8060S Graphics, `RADV STRIX_HALO`, gfx1151 (device `0x1586`) |
| Driver | Mesa 26.0.8, RADV |
| Captured | 2026-09-22 |
| Workload | a single Vulkan compute dispatch, 64 workgroups × 64 invocations, 64 loop iterations each, one `vkQueueSubmit` |
| Capture | `MESA_VK_TRACE=rgp`, `MESA_VK_TRACE_PER_SUBMIT=1`, `RADV_THREAD_TRACE_BUFFER_SIZE=1048576` |

It contains an `ASIC_INFO` chunk, two `SQTT_DESC`/`SQTT_DATA` pairs, a `CODE_OBJECT_DATABASE`, and a `DERIVED_SPM` chunk (type 128) holding 329 samples of 14 counters and 19 components at a 4,096-shader-clock interval.

## Why the zeros are load-bearing

Several counters read zero, and that is the point: the shader uses no LDS (bank conflicts), performs no raytracing (ray-box and ray-triangle tests), and the host is a unified-memory APU (PCIe bytes). A change that makes those non-zero is a regression, not an improvement.

`L0 cache hit` also reads zero across every sample. That is a known Mesa defect — GFX11 uses the GFX10 TCP selector — fixed upstream for Mesa 26.2.4. Do not treat it as a parser bug.

## What it is not

The SQTT sections are present but are **not** imported. RADV thread-trace timestamps are not usable without normalisation: a larger capture from the same workload family spans 458,443,249,327,780 shader cycles for a sub-millisecond dispatch, and the majority of sampled waves contain negative timeline durations. The SPM timestamps in the same file are sane. See `docs/blocked-inputs.md` §3.
