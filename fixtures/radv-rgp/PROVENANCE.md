# RADV RGP compute fixtures

Every `.rgp` here is a real capture from this project's qualification host, not synthetic data. The table describes `compute-spm.rgp`; the sections below give how each other file differs.

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

## `compute-occupancy.rgp`

The same shader and host, captured with `RADV_THREAD_TRACE_INSTRUCTION_TIMING=false`: 256 workgroups × 64 invocations × 256 iterations, `RADV_THREAD_TRACE_BUFFER_SIZE=4194304`, 38,644 bytes. Without instruction timing RADV emits no ALU/VMEM execution tokens, so the stream needs no sanitizing and decodes to **exactly 256 wave lifetimes, 128 per shader engine** — one per wave64 workgroup. That exact count is the reason this mode is recommended for anything larger than a toy dispatch.

## What `compute-spm.rgp` is not

It was captured with instruction timing on, so its SQTT sections contain 330 ALU/VMEM execution tokens per shader engine. rocprof-trace-decoder has no encoding for them; unsanitized, the decoder loses sync and produces timestamps billions of cycles apart. The importer overwrites them with NOP nibbles first, after which both waves decode with zero negative state durations and state durations that sum exactly to each wave's span. Because those tokens are also where RADV reports most VALU execution, per-instruction attribution from this capture is incomplete by construction — see `docs/blocked-inputs.md` §3.

## `compute-rocprof-compat.rgp`

The same shader at the same size (64 × 64 × 64), captured with **Mesa 26.2.0 patched with `docs/radv-rocprof-compat.patch`**, built from a copy of the source, with `RADV_THREAD_TRACE_ROCPROF_COMPAT=true` and `RADV_THREAD_TRACE_BUFFER_SIZE=268435456`: 41,332 bytes, sha256 `09fc3f07…`. The stream holds zero execution tokens and zero unknown headers. It decodes to 64 occupancy intervals and two traced waves, each attributing its 526 executed instructions exactly: every loop-body row 64 times, the loop test 65, straight-line code once, and `s_delay_alu` hints and padding `s_nop`s never.

## `compute-raw-counters.rgp` and `compute-raw-counters.conf`

The same 64 × 64 × 64 dispatch, from the same patched Mesa 26.2.0, with compat mode on and `RADV_SPM_COUNTERS_CONFIG=compute-raw-counters.conf`: 26,906 bytes, sha256 `bc5d1e93…`. The config lists ten raw events, using gfx11 event IDs from ROCm's `rocprofiler-sdk/basic_counters.xml`. The true totals are known — 64 wave64 workgroups, each one buffer load and one buffer store — and the capture falls short of them on this GPU. The five SQ events read zero in every sample. `TA_BUFFER_LOAD_WAVEFRONTS` and `TA_BUFFER_STORE_WAVEFRONTS` sum to 80 against a true 128, because RADV expands `ALL` to fewer TA instances than there are CUs. The tests pin these values so a parsing change cannot silently alter them; they are not correct totals.

## `llama-ggml-submit.rgp` and `llama-ggml-copy.rgp`

Two per-submit captures from llama.cpp (`ggml-org/llama.cpp` at `4ceb17191`) running `llama-bench -p 64 -n 0 -r 1 -ngl 99` on a 3B-parameter Q4_0 model through ggml's Vulkan backend. They used the same patched Mesa 26.2.0, with `RADV_THREAD_TRACE_ROCPROF_COMPAT=true`, `RADV_THREAD_TRACE_BUFFER_SIZE=67108864` and `GGML_VK_DEBUG_MARKERS=1`. No model data is in them, only shader code, trace data and ggml's debug labels.

- `llama-ggml-submit.rgp` (submit 2062 of 2,077; 514,808 bytes, sha256 `877b6c55…`) holds eight dispatches of eight pipelines, from `rope_norm_f32` to `rms_norm_mul_f32`. For every one, occupancy equals the RGP marker's workgroup count × waves per workgroup, 9,488 waves in all.
- `llama-ggml-copy.rgp` (submit 2042; 11,204 bytes, sha256 `b7d196af…`) holds one host-to-device upload, which RADV implements with its own compute shader. Its event marker records `vkCmdCopyBuffer` with 640 threads, which is 10 wave64 workgroups and 10 occupancy intervals.

## `compute-rocprof-compat.bundle.json`

As below, for the compat capture; `tests/radv-rgp.spec.ts` checks the GUI shows its exact per-instruction hits.

## `compute-spm.bundle.json`

The importer's own `--bundle` output for `compute-spm.rgp` (capture plus both waves), used by `tests/radv-rgp.spec.ts` to check the GUI renders a RADV capture. Regenerate with the decoder installed:

```bash
for f in compute-spm compute-rocprof-compat; do
  ROCPROF_GUI_DECODER_PACKAGE=$PWD/third_party/decoder-python/decoder-python.zip \
    cargo run -q -p profile-core -- fixtures/radv-rgp/$f.rgp --bundle > fixtures/radv-rgp/$f.bundle.json
done
```

Neither contains machine-specific paths.

## `gfx1201-add-occupancy.rgp`

Captured on 2026-09-23 from `test-backend-ops` in an isolated llama.cpp checkout at `7ab4ee7ba`, on Radeon AI PRO R9700 (GFX1201) with privately installed stock Mesa 26.2.0 and libdrm 2.4.133. The run selected one `ADD` case (`type=f32,ne=[10,5,4,3],nr=[1,1,2,2],nf=6`) with `GGML_VK_DEBUG_MARKERS=1`, `MESA_VK_TRACE=rgp`, `MESA_VK_TRACE_PER_SUBMIT=1`, and `RADV_THREAD_TRACE_INSTRUCTION_TIMING=false`. This is submit 22 of 24. It contains the compiled ADD shader, RGP markers, SQTT occupancy and derived SPM counters, with no model data. SHA-256: `ca6d83fec9d350864443318fd8bb86d2087d374ae96940fc8154cd71b79214c2`.

The GFX12 stream decodes without the GFX11 execution-token sanitizer: 24 decoded dispatch records, six markers per shader engine, 120 occupancy intervals across four shader engines, and no instruction hits as expected for occupancy mode. Applying the GFX11 sanitizer to this stream spuriously reports unknown token headers and rejects it.
