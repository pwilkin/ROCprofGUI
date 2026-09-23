# Profiling Vulkan compute with RADV (experimental)

rocprof studio can open `.rgp` captures written by Mesa's RADV Vulkan driver: hardware counters, wave occupancy, per-instruction timing and kernel names from Vulkan compute workloads such as llama.cpp's Vulkan backend. This path is **experimental**:

- It has been qualified on one GPU only, gfx1151 (Radeon 8060S, RDNA 3.5), with Mesa 26.0.8 and 26.2.0.
- Per-instruction data needs a patched RADV.
- Raw hardware counters are incomplete on that GPU.

The evidence for each claim below is in [blocked-inputs.md](blocked-inputs.md) §3.

## What you get

| Capture | Needs | Result |
|---|---|---|
| Counters only | stock RADV | RADV's derived SPM counters, on a nanosecond axis |
| Occupancy | stock RADV | Lifetime of every wave on every CU, exact at any scale, plus counters and kernel names |
| Full | RADV with [`radv-rocprof-compat.patch`](radv-rocprof-compat.patch) | Occupancy, plus exact per-instruction hits, latencies and stalls for the traced CU |

Without the patch, RADV's default instruction-timing traces lose nearly all per-instruction data: on a test kernel, 8 of 1,052 executed instructions were attributed. Busy ones are rejected outright. Use occupancy mode on a stock driver, or build the patched one.

## Requirements

- **Mesa 26.2.0 source** (for the patched driver). The patch is against the 26.2.0 release. It changes 3 files (11 lines added, 1 removed) and is MIT licensed, like the files it modifies. It is local to this project and has not been proposed to Mesa.
- **libdrm 2.4.133 or newer**, which Mesa 26.2 requires. If your distribution ships an older one, build libdrm into a separate prefix and put its library directory on `LD_LIBRARY_PATH` when running the application.
- **Meson and a Python with `mako`**, for Mesa's build.
- **ROCm's `rocprof-trace-decoder`** (the native V2 library) and the decoder Python package, which rocprof studio already needs for raw ATT; see the README's "Raw ATT setup". Counters-only import works without them.

## Step 1: build the patched driver

Build from a copy of the source into a private prefix; this replaces nothing on the system:

```bash
tar xf mesa-26.2.0.tar.xz && cd mesa-26.2.0
patch -p1 < /path/to/rocprofgui/docs/radv-rocprof-compat.patch
meson setup build --prefix="$HOME/radv-compat" -Dbuildtype=release -Dlibdir=lib \
  -Dvulkan-drivers=amd -Dgallium-drivers=[] -Dplatforms=[] -Dvulkan-layers=[] \
  -Dglx=disabled -Degl=disabled -Dgbm=disabled -Dvideo-codecs=[] -Dtools=[] \
  -Dbuild-tests=false -Damd-use-llvm=false
ninja -C build install
```

The patch adds one option, `RADV_THREAD_TRACE_ROCPROF_COMPAT`, which defaults to off, so the patched driver behaves exactly like stock 26.2.0 unless it is set. With it on, GFX11 and GFX11.5 program the thread-trace token-exclusion set AMD's aqlprofile uses:
- VMEM execution, ALU execution, event, utilization counter and wave allocation are excluded;
- BOP events, which aqlprofile does not set, are dropped;
- RADV's register tokens are kept, because the decoder recovers dispatches from them.

GFX10 and GFX12 are untouched.

## Step 2: capture

Point the Vulkan loader at the patched driver and enable per-submit tracing:

```bash
export VK_DRIVER_FILES=$HOME/radv-compat/share/vulkan/icd.d/radeon_icd.x86_64.json
export LD_LIBRARY_PATH=$HOME/radv-compat/lib:$LD_LIBRARY_PATH   # plus a newer libdrm if needed
MESA_VK_TRACE=rgp MESA_VK_TRACE_PER_SUBMIT=1 RADV_THREAD_TRACE_ROCPROF_COMPAT=true ./your-vulkan-app
```

RADV writes one file per queue submit, `/tmp/<process>_<timestamp>_submit<N>.rgp`; the directory is fixed in Mesa. Without a swapchain, the frame-based trigger never fires, so compute programs need `MESA_VK_TRACE_PER_SUBMIT`.

Useful variables:

| Variable | Effect |
|---|---|
| `RADV_THREAD_TRACE_ROCPROF_COMPAT=true` | Full mode (patched driver only) |
| `RADV_THREAD_TRACE_INSTRUCTION_TIMING=false` | Occupancy mode, on any driver |
| `RADV_THREAD_TRACE_BUFFER_SIZE=<bytes>` | Per-shader-engine trace buffer. A trace that overflows it is rejected on import, not truncated silently |
| `RADV_THREAD_TRACE_CACHE_COUNTERS=false` | No SPM counters (they are on by default) |
| `RADV_SPM_COUNTERS_CONFIG=<file>` | Raw hardware events instead of RADV's derived counters (Mesa 26.2); see below |
| `GGML_VK_DEBUG_MARKERS=1` | llama.cpp: emit debug labels, so every kernel is named after its ggml shader |

Tracing slows the application down, and every submit writes a file: a llama.cpp model load alone writes one per upload chunk. Keep runs short, and watch free space in `/tmp`.

## Step 3: open

Open a `.rgp` file with **Open capture** (or `capture_open` over MCP). Each file is one submit. The importer:

1. decodes the thread trace with `rocprof-trace-decoder`, after checking the stream is one it can decode, and rejects the whole trace if any of four consistency checks fails (counters still import);
2. places each shader at the load address the capture records;
3. infers the capture mode from the stream, since the file does not record it, and says so;
4. names kernels after the application's debug labels, RADV's internal copy/fill commands, or the pipeline's API hash;
5. matches each dispatch to RADV's marker for it and records its grid.

Warnings in **Capture evidence** say what could not be trusted.

## Raw counters

`RADV_SPM_COUNTERS_CONFIG` takes a file of `NAME=BLOCK,EVENT,INSTANCE|ALL,sum|max|avg` lines under `[Group]` headers; [`compute-raw-counters.conf`](../fixtures/radv-rgp/compute-raw-counters.conf) is an example using gfx11 event IDs from ROCm's `basic_counters.xml`. On gfx1151, against dispatches of known size:

- TA counts read 61–63% of the true total, because RADV expands `ALL` to fewer instances than there are CUs;
- SQ events read zero;
- SQ event IDs above 35 are rejected, and GRBM has no free SPM slots;
- any rejected line makes device creation fail.

The importer marks these as hardware counters, warns that they are not GPU totals, and flags any that read zero throughout.

## Known limits

- Qualified only on gfx1151; the patch does nothing on GFX10 or GFX12.
- Instruction-level detail covers the one CU the hardware traces. Occupancy covers every CU.
- Dispatches are point events. Kernel time comes from wave occupancy, not from dispatch start/end records.
- A capture holds one submit. There is no multi-submit timeline yet.
- Decoding large captures is memory-hungry: an 8 MB capture peaked near 1 GB in the decoder process.
