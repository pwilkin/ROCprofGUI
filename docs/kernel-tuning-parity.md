# Kernel-tuning parity: HIP (Kerncap) vs Vulkan (vkcap)

rocprof studio ships two single-dispatch capture/replay stacks. The tuning
subsystem picks one at session creation from the capture's format
(`src-tauri/src/tuning.rs`: `let vulkan = capture.format.starts_with("RADV RGP")`).

- **Vulkan** is in-tree: `tools/vkcap/vkcap_layer.cpp` (capture layer) and
  `tools/vkcap/vkreplay.cpp` (replay), driven by `src-tauri/src/shader_worker.py`.
- **HIP** is external and patched: `AMDResearch/intellikit` pinned at `c5b2098`,
  built by `scripts/setup-kernel-tuning.py` with `scripts/kerncap-replay.patch`
  (GPU-event replay timing) and `scripts/kerncap-capture-scope.patch` (the
  reachable capture scope), driven by `src-tauri/src/tuning_worker.py`.

The Vulkan harness grew a richer feature set. This document records what the HIP
path does and does not match, and why — so the asymmetry is answered from the
repository rather than re-derived each session.

## Replay conditions

The Vulkan `run` op builds its replay command from `conditions_of` / `replay_argv`
(`shader_worker.py`), exposing `--mode`, `--memory`, `--flush-mb`, `--repeats` and
`--counters`. The HIP `run` op now has the analogous `conditions_of` / `replay_argv`
(`tuning_worker.py`) for the conditions Kerncap can honor honestly:

| Condition | Vulkan (vkreplay) | HIP (kerncap-replay) | Notes |
|---|---|---|---|
| **mode** stateless / sustained | `--mode stateless\|sustained` | **yes** — stateless default, sustained via Kerncap's existing `--no-recopy` | Kerncap reports its back-to-back path as `stateful`; the worker maps `sustained→stateful` and verifies the echoed mode. No runtime rebuild needed. |
| **repeats** (fresh allocations) | `--repeats`, looped in the worker | **yes** — looped in the worker, reports `Replay spread` (max−min of the per-replay average) | Pure worker-side; no replay-binary support needed. |
| **memory** device / app / gtt / auto | `--memory …` | **no** (deliberate) | See below. |
| **flush-mb** (cache evict) | `--flush-mb N` | **no** (deliberate) | See below. |
| **counters** as a replay flag | `--counters` (`VK_KHR_performance_query`) | **no** — HIP reads counters by wrapping the replay in `rocprofv3 --pmc` instead (unchanged) | A different, already-working mechanism, not a replay condition. |

Both paths compute timing deltas only against the latest earlier run made under the
same conditions, and validation stays stateless regardless of the timing mode
(output is compared byte-for-byte against the application's own dispatch).

### Why memory placement and cache flush are not on the HIP path

These are not cheap wins, and implementing them blind would misrepresent results:

1. **Exact-address restore.** `kerncap-replay` restores each captured region at its
   *exact captured virtual address* (`hsa_amd_vmem_address_reserve` at
   `r.aligned_base`, then `hsa_amd_vmem_handle_create` from a single backing pool, then
   `hsa_amd_vmem_map`). Placement is fixed by the capture; a `--memory` flag cannot move
   a region, only change the pool its handle is created from.
2. **One coarse-grain pool.** The backing pool is "the GPU agent's first
   runtime-allocable global pool" (coarse-grain on AMD). Forcing an alternate
   (host-visible / fine-grain) pool for `hsa_amd_vmem_handle_create` is untested and may
   not succeed for VMM handles.
3. **Unified memory.** The qualification host is gfx1151 (Strix Halo APU) with unified
   memory. There, device-local and GTT/host-visible pools map to the same physical RAM,
   so `--memory device` vs `gtt` would change allocation *grain/coherence*, not physical
   placement — the timing signal vkreplay's `--memory` isolates on a discrete GPU is
   largely absent here (compare the Vulkan "buffers usually land in the VRAM carve-out"
   caveat in the README's shader-tuning limits).

Per the project's standing rule (see `docs/blocked-inputs.md`,
`docs/profiling-caveats.md`): an honest refusal beats a knob that silently measures
the wrong thing. If a discrete-GPU host and a Kerncap patch that selects the region
pool arrive, `conditions_of` can gain `memory`/`flushMb` the same way Vulkan has them,
and `replay_argv` would forward `--memory`/`--flush-mb` to a rebuilt binary. The worker
already verifies the replay echoes the requested condition, so a pre-patch runtime
would refuse rather than silently ignore the flag.

## Build discovery

Vulkan discovers the shader build with `ggml_build` (`shader_worker.py`): prefer
`config.buildDir`, else scan `build*/` for the generated-SPIR-V marker, then resolve
`glslc` from config → `CMakeCache.txt` → `PATH`.

HIP does not scan for a build. Instead, a session created from an MCP `profile_run`
carries the profiled command: `recorded_profile` reads the job's `profile.json`
(`tuning.rs`), and `hip_config` derives the `sourceTree` by walking up from `argv[0]`
(joined with the recorded `cwd`) to the directory containing `.git`. The worker's
`compile_context` then scans that tree's `compile_commands.json` for the entry that
builds the captured source, and `clean_compile` reuses that exact command against an
`-ivfsoverlay` redirect of the edited source. Opened any other way, `sourceTree` and
`argv` are left empty for the user to fill in — nothing is guessed. This path is
exercised by `hip_sessions_rerun_the_profiled_command` in `tuning.rs` (absolute vs
relative `argv[0]`, missing `.git`, and standalone opens) and verified working; no fix
was required.

## The other recorded gaps

Two further HIP↔Vulkan asymmetries are known and left as-is, because closing them
honestly needs inputs this driver does not have:

- **Identity.** Vulkan matches a captured shader to its source by SPIR-V structural
  digest (`shader_worker.py` `normalized`). HIP matches by a textual kernel-base search
  over `compile_commands.json` and HSACO symbols (`tuning_worker.py` `kernel_base`).
  There is no HSACO→source byte identity equivalent to the SPIR-V digest.
- **Reachability fallback.** The reachable capture scope
  (`scripts/kerncap-capture-scope.patch`) keeps only allocations the kernarg segment
  can reach; there is no automatic fallback to `all` when a kernel reaches memory another
  way — the session surfaces the hint and the user re-captures with
  `config.captureScope "all"`.
