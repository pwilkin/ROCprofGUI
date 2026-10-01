# Shader tuning (Vulkan, experimental)

::: danger Experimental, GPU-required, writes to disk
Shader tuning is the Vulkan counterpart of [kernel tuning](/kernel-tuning). It captures one dispatch, replays it in isolation, and compiles edited variants against it. It is **experimental** and qualified on a single GPU.
:::

## What it does

**"Tune shader"** on a dispatch in a **RADV capture** (or `tuning_open` on a RADV dispatch) opens a Vulkan single-shader session:

1. **Capture** one dispatch.
2. **Find source.** For **ggml's Vulkan backend**, the captured SPIR-V is matched against the build's generated shaders (ignoring capabilities/extensions/execution-modes an app adds at runtime; ggml adds fp16 float controls). The generator is re-run through a logging `glslc` to recover the exact compile command, confirmed by rebuilding to identical bytes. Every file that compile reads becomes editable.
3. **Run.** The captured shader — or an edited variant compiled with the recovered command — is replayed in isolation. The driver's compiler statistics (VGPRs, SGPRs, LDS, spills, wave limit) and ISA are reported; where `VK_KHR_performance_query` is available, performance counters (GPU cycles, the clock the dispatch ran at, L0/L1/L2 hit rates) also report. A run can additionally be traced with RADV and opened here.

## Prerequisites

- A **GPU**, and a runtime built by **`scripts/setup-shader-tuning.py`** into **`~/.rocprofgui/shader-tuning/runtime`**: the `VK_LAYER_ROCPROF_capture` capture layer and the `vkreplay` harness, both from `tools/vkcap/`.
- The MCP tuning tools drive the same sessions. The replay conditions and counters are `vkreplay` options and can also be run by hand on a session's capture directory.

## What was actually measured

Qualified on **gfx1151** with llama.cpp's **IQ4_NL** matrix multiply:

- The captured shader replays **bit-exactly**; a comment-only edit compiles back to the captured SPIR-V byte for byte.
- A one-constant edit (shared-memory row padding 4 → 2) measured **36% and 28% faster** on two captured shapes with **bit-identical outputs**.
- In the **whole application**, the same change gave about **8%** on `llama-bench` pp512 — the largest matmuls gained 4–7% there against 30% in replay. (Pad 2 also breaks the 16-byte stride alignment Vulkan requires of cooperative-matrix loads; RADV happens to tolerate it.)

The gfx1201 validation report (`docs/gfx1201-validation.md`) records RADV/ROCm capture, replay, correctness and application-benchmark results on the Radeon AI PRO R9700.

## Honest limits

- **Source recovery is automatic only for ggml's Vulkan shader build.** Other projects can replay and time the captured SPIR-V but **not edit** it.
- **Captures are large.** A capture holds every bound range, and ggml binds the rest of its compute buffer as the output range, so one matrix-multiply capture is **≈250–530 MB**. Sessions live under `~/.rocprofgui/shader-tuning/sessions`. A failed capture is deleted; closing a session deletes its capture.
- **The driver matters.** Replay compiles with whatever driver the session's environment selects; a session created from an MCP `profile_run` inherits that run's `VK_DRIVER_FILES`. Mesa 26.0.8 and 26.2.0 compile the same IQ4_NL shader to 192 and 120 VGPRs at pad 2, and Mesa 26.2.0 does **not** advertise `VK_KHR_performance_query` on GFX12, so those counter fields are unavailable on gfx1201.

See [RADV / Vulkan profiling](/radv) for how to produce the captures this works on.
