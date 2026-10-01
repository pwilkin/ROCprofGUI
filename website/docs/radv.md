# RADV / Vulkan profiling

::: warning Experimental, qualified on gfx1151
This path reads RADV `.rgp` captures from Mesa's Vulkan driver. It is experimental and qualified on gfx1151 only; full per-instruction data needs a patched RADV.
:::

A `.rgp` capture written by Mesa's RADV driver can be opened directly, giving hardware counters, thread traces and kernel names from Vulkan compute. On gfx1151, which ROCm's AQLprofile filter excludes from SPM, this is the only route to hardware counters.

The full, authoritative build-and-capture steps are in the repository's `docs/radv-profiling.md`. This page summarises what to expect.

## Capturing

```bash
# Occupancy for every wave at any scale, plus counters (stock RADV):
MESA_VK_TRACE=rgp MESA_VK_TRACE_PER_SUBMIT=1 \
  RADV_THREAD_TRACE_INSTRUCTION_TIMING=false ./your-vulkan-app

# Full per-instruction data as well (RADV built with docs/radv-rocprof-compat.patch):
MESA_VK_TRACE=rgp MESA_VK_TRACE_PER_SUBMIT=1 \
  RADV_THREAD_TRACE_ROCPROF_COMPAT=true ./your-vulkan-app
# writes /tmp/<app>_<timestamp>_submit<N>.rgp
```

Thread traces are decoded with ROCm's `rocprof-trace-decoder`, the same decoder [raw ATT](/input-formats#raw-att-setup) uses, so it must be installed.

## What a trace holds depends on the token mask

| RADV configuration | What decodes |
| --- | --- |
| `RADV_THREAD_TRACE_ROCPROF_COMPAT=true` (patched RADV) | **Full.** Occupancy for every wave of every dispatch, and exact per-instruction hits/latencies/stalls for the traced CU, at any scale. |
| `RADV_THREAD_TRACE_INSTRUCTION_TIMING=false` | **Occupancy.** Exact wave lifetimes for every wave on every CU at any scale; no per-instruction data. |
| RADV default, small dispatch | Wave lifetimes, states and timing, but per-instruction data is effectively absent (see below). |
| RADV default, anything busier | Thread trace rejected with a reason; counters still import. |

**Why the default mask loses instructions.** RADV's default token mask includes ALU/VMEM-execution tokens that `rocprof-trace-decoder` has no encoding for (AMD's own profiler never enables them). Left in, they desynchronise the decoder, so the importer overwrites them with NOP nibbles, which keeps stream length, alignment and timing intact but means RADV's instruction-execution reporting is lost. On a 64-iteration loop kernel, only 8 of the 1,052 instructions the traced waves executed are attributed. Busier default-mask streams also contain undocumented token types and are rejected.

**The compatibility patch.** `docs/radv-rocprof-compat.patch` adds an opt-in `RADV_THREAD_TRACE_ROCPROF_COMPAT` to Mesa 26.2.0. On GFX11/GFX11.5 it programs the token mask AMD's aqlprofile uses, so the stream holds only tokens the decoder knows (default behaviour unchanged; GFX10/GFX12 untouched). On gfx1151 every instruction is then attributed exactly.

## Kernel names

RADV leaves the RGP pipeline-name field empty but records its own instrumentation markers inside the thread trace (one event per hardware dispatch, pipeline binds, debug labels). The importer reads them, so each kernel is named: by the application's debug label, by the command a RADV internal shader implements (e.g. `vkCmdCopyBuffer (RADV internal)`), or otherwise by the pipeline's API hash (`pipeline 834527b9cb9b4ffb`, stable across runs).

## GFX12 (gfx1201) status

On gfx1201 with stock Mesa 26.2.0, occupancy-mode RGP traces decode with named dispatches, four shader engines and SPM counters. The default instruction-timing trace lost data even for a small ADD dispatch; counters still import. The GFX11 compatibility patch does not enable GFX12 instruction timing.

## Decode integrity checks

The importer validates a decoded stream against several invariants: the shader clock stays consistent with the realtime clock the trace carries (realtime never goes backwards; the shader clock never runs far above its median rate, and cannot nearly stop across an idle gap because RADV inhibits clock gating while tracing), and every decoded wave/occupancy timestamp lies inside its shader engine's own trace window. A misparsed token breaks one of these.

## From a RADV capture you can also tune

A dispatch in a RADV capture is where [Shader tuning (Vulkan)](/shader-tuning) starts. The MCP [`profile_start`/`profile_trigger`/`profile_stop`](/mcp#profiling) tools automate the file-trigger capture of a long-lived Vulkan server (headless compute needs a RADV build with `docs/radv-compute-trigger.patch`, since stock Mesa checks file triggers only on swapchain presentation). The same tools, with `tracer: "rocprofv3"`, gate a HIP/ROCm application through bounded rocprofv3 attach windows instead — see the [profiling examples](/mcp#profiling-examples).
