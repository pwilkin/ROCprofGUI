# What the driver would need to report waves, ISA and counters for Loom/hrX kernels

Design study, 2026-11-14, on this host (gfx1151, ROCm 10.1 / SDK 1.4.1, DKMS
`amdgpu` 6.19.14). The study itself was read-only in `/devel/tools/hrx-system`
with nothing built or run; the two spikes it called for ran the same day and are
reported under [Spike result](#spike-result-2026-11-14) (SQTT) and
[Counters spike result](#counters-spike-result-2026-11-15). It answers: **now that Loom dispatches PM4 packets on its own KFD user
queue** (see [hrx-profiling.md](hrx-profiling.md) for how we collect today), what
would it take to get the three things we cannot currently produce — waves, ISA
rows and hardware counters?

**Status, 2026-11-15.** Waves and ISA are implemented and verified end to end:
the driver producer on hrx-system branch `profiling/executable-traces` (local,
four commits, not upstream) and the importer arm in this repo. See
[hrx-profiling.md, "Thread traces"](hrx-profiling.md#thread-traces-executable-traces)
for what it does, the real capture and its limits. Hardware counters are **not**
implemented; the validated sequence below is still the recipe. Two things this
study got wrong or did not know, found while building it:

- `loom-serve` dispatches everything through recorded command buffers, so the
  whole-recording bracket is the main path, not a first cut.
- The in-ring `WAIT_REG_MEM` on `SQ_THREAD_TRACE_STATUS` works on this queue for
  a condition that is not yet true, so the whole bracket fits one publication.

**Short answer.** The gap is almost entirely in the GPU driver's *producer*.
The schema, the option bits, the tooling flags, an in-tree ATT decoder and a
reusable importer path in rocprof-studio already exist. **No kernel change is
needed for SQTT.** The one question no source here could settle was whether MEC
firmware on gfx1151 accepts SQTT register writes from a
`KFD_IOC_QUEUE_TYPE_COMPUTE` queue. **The spike settled it: it does, and the
trace decodes** — every executed instruction of a test kernel was attributed.
**A second spike settled counters the same way: they work, exactly** — wave,
workitem and instruction counts matched the predicted values to the unit, with
no root, and another process's waves are not counted. One register must never
be read from the ring (it reset the GPU); see
[Counters spike result](#counters-spike-result-2026-11-15).

Path prefixes: `H` = `/devel/tools/hrx-system`, `D` =
`H/runtime/src/iree/hal/driver/amd/gpu`, `K` =
`/usr/src/amdgpu-6.19.14-2364437.26.04` (the loaded DKMS module), `R` =
`/devel/tools/rocprofgui`, `O` = `/opt/rocm/core-10.1`.

## Per-layer findings

| Layer | Exists | Missing | What to write |
|---|---|---|---|
| **1. Schema / frontend** | Content types `executable-traces`, `counter-sets`, `counters`, `counter-samples` (`H/runtime/src/iree/hal/profile_schema.h:94-102,121-122`). Trace record plus raw bytes, one artifact per chunk, format `AMDGPU_ATT = 1` (`:1934,1943-1974`). Counter set/counter/sample records (`:1438,1497,1572`). Family bits (`profile_options.h:56,69,91`). CLI parsing (`H/runtime/src/iree/tooling/device_profiling.c:244-254`). | The GPU producer: `D/profile.c:626-634` rejects both families, `:635-639` rejects any counter set, and a test pins that (`D/profile_test.cc:632-640`). No per-trace options (target CU, SIMD, SE mask, buffer size) in `profile_options.h:241-269`. | Accept the two families in `iree_hal_amd_gpu_profile_begin`; emit trace and counter chunks at flush. The XDNA backend already emits counter chunks and is the template (`H/runtime/src/iree/hal/driver/amd/xdna/profile_counters.c:571-584`). |
| **1b. Upstream / dropped** | Tooling still refers to an aqlprofile-based capture path: `H/runtime/src/iree/tooling/profile/att/rocm.c:20-25` ("Match the AMDGPU runtime capture path knobs"), and the format comment "as returned by `aqlprofile_att_iterate_data`" (`profile_schema.h:1933`). | No such driver in this tree: the only `aqlprofile` hits outside docs are those tooling references. The string `amdgpu.att` does not occur anywhere in the tree. | Nothing to port in-tree. The earlier producer depended on ROCr and aqlprofile, which the native driver does not link. |
| **2. PM4 queue** | Each profiled dispatch is already bracketed by `CS_PARTIAL_FLUSH` + GPU-clock `COPY_DATA` on the direct path (`D/queue/pm4/publication.c:913-931`) and the recorded path (`D/queue/pm4/command_buffer.c:860-878`). Per-queue sample storage (`D/queue/pm4/queue.c:242-249`). Filter predicate (`D/profile.c:944`). ELF images and load records are already emitted (`D/profile.c:437,475`; `load_delta` at `:81`). | No SQTT or perfmon code at all (only a comment, `D/queue/pm4/launch.c:176`). Writers cover only `SET_SH_REG` (`D/queue/pm4/commands.c:42-55`), `CS_PARTIAL_FLUSH` (`:76-79`), clock and memory copies (`:226-258`). | New writers: `SET_UCONFIG_REG`, `EVENT_WRITE` thread-trace start/finish, register-source `COPY_DATA`, register-space `WAIT_REG_MEM`, `GRBM_GFX_INDEX` select/restore. A new thread-trace module and a counters module hooked at the existing bracket. |
| **3. Kernel / KFD** | KFD 1.23 with `AMDKFD_IOC_PROFILER` (PMC, PC_SAMPLE, VERSION, PTL), `RLC_SPM`, `PC_SAMPLE` (`K/include/uapi/linux/kfd_ioctl.h:53-54,1750-1755,1966-1995`). Perfmon clock control (`K/amd/amdgpu/gfx_v11_0.c:5452-5469`), driven by stable pstate (`K/amd/amdgpu/soc21.c:528-540`). GC 11.5.1 carries `AMD_CG_SUPPORT_GFX_PERF_CLK` (`soc21.c:723-730`). | No kernel SQTT service: the only thread-trace code is SQ interrupt decoding (`K/amd/amdkfd/kfd_int_process_v11.c:74-93`). PC sampling has no GFX11 entry (`K/amd/amdkfd/kfd_pc_sampling.c:69-80`). | Nothing in the kernel. Userspace must enter stable pstate; `libamdf` already holds a render fd and a DRM context (`H/libamdf/src/gpu/umd/kfd/vm.c:129-134`). |
| **4. PMC** | See the mechanism list below. | — | — |
| **5. Decoding** | `librocprof-trace-decoder.so.0.2.2` and `librocprofiler-sdk.so.1.4.1` in `O/lib`. API: `O/include/rocprofiler-sdk/experimental/thread-trace/trace_decoder.h:54,86,138`. In-tree binding: `H/runtime/src/iree/tooling/profile/att/rocprofiler.c:19-35`, `decode.c:245-249,294-297`. | A stream the decoder can parse: the right token mask, plus code-object identity. | Program aqlprofile's token-exclusion set, and place code objects by load address in the consumer. |
| **6. Consumer** | IRPF importer (`R/crates/profile-core/src/irpf.rs`) and a RADV arm that decodes marker-less gfx11 SQTT with placed code objects (`R/crates/profile-core/src/rgp.rs:914-966`, `decode.py:41-82`). | `irpf.rs` reads no trace or counter chunk and says so (`irpf.rs:1069`). | Two new importer arms; details below. |

## Layer notes

**PM4 hook and selection.** Per-SE and per-CU selection maps to hardware fields,
not to queues:

- `--att-target-cu` is the WGP select and `--att-simd-select` is a SIMD id on
  gfx10+; both live in the SQTT mask register.
- The shader-engine mask decides which SEs get a buffer, via `GRBM_GFX_INDEX`.
- gfx1151 has 2 SEs (`array_count 4`, `simd_arrays_per_engine 2` in
  `/sys/class/kfd/kfd/topology/nodes/1/properties`).
- aqlprofile's GFX11 limits are 128 KiB to under 512 MiB per SE
  (`H/docs/reference/amd/gpu/profiling/thread-trace.md:22-26`).

**Register lists are not in the tree.** (The spike now carries a working
GFX11.5 set, transcribed from Mesa: see [Spike result](#spike-result-2026-11-14).)
`thread-trace.md` gives only the ordering:

- Start (`:32`): buffer size, then base, then control last.
- Stop (`:54-71`): clear `COMPUTE_THREAD_TRACE_ENABLE`, send
  `THREAD_TRACE_FINISH`, per SE wait for finish-done, set mode off, wait for
  not-busy, copy write pointer, status and dropped count.
- The GFX11 write pointer needs base subtraction (`:66-70`). Measured on
  gfx1151 (GFX11.5): it does **not** — the pointer counts 32-byte units from
  zero, as Mesa also assumes for GFX11.5.

Addresses, field layouts and the gfx11.5 values must come from the sources it
cites: PAL `gfx9PerfExperiment.cpp` L2975-3025 and L3084-3204, and aqlprofile
`gfx11_primitives.h` L538-550 and `pm4/sqtt_builder.h`. For counters,
`H/docs/reference/amd/gpu/pm4/counters.md:182-191` gives `CP_PERFMON_CNTL` and
select-register field bits but no addresses. The counters spike carries a
working address and value set (below), taken from Mesa's `gfx115.json` and from
the aqlprofile checkout on this machine
(`/devel/tools/rocm-systems/projects/rocprofiler-sdk/source/lib/aqlprofile`).

**Recorded command buffers.** Resident IBs are immutable and replayed, so
tracing one dispatch inside a recording needs a recording variant. The simplest
first cut traces a whole recording execution and attributes by code object.

**Counter mechanisms for a PM4 user queue.**

- **Own-IB perf counters (per dispatch): open, measured.** This is the
  pattern the tree documents for Tinygrad's dispatch collector
  (`H/docs/reference/amd/gpu/profiling/counter-sampling.md:212-227`). The
  queue must write `COMPUTE_PERFCOUNT_ENABLE` itself, because the kernel sets
  it in the MQD only while a profiler owner exists
  (`K/amd/amdkfd/kfd_mqd_manager_v11.c:183-186`); without it nothing counts.
  The perfmon clock is needed only for the GRBM block on this GPU. The
  *registers* are device-global, but the *counts* are not: only queues that
  set that enable bit are counted, and a per-VMID mask narrows it further
  (`counters.md:35-42` describes PAL's always-count policy, not the hardware
  limit).
- **KFD PMC ownership: privileged.** `perfmon_capable()` is required
  (`K/amd/amdkfd/kfd_chardev.c:3645`). It is not needed for own-IB sampling; it
  only arbitrates collectors and forces the enable bit on other queues.
- **aqlprofile / rocprofiler dispatch counting: closed.** It needs AQL
  interception.
- **PC sampling: absent on gfx1151.**
- **SPM streaming: capable but unused.** The ioctl is registered without a
  capability flag (`K/amd/amdkfd/kfd_chardev.c:3913-3914`). The GFX11 backend was
  not verified.

**Decoder input.** Raw per-SE SQTT bytes from gfx1151 decode with no header
prepended: the RADV arm writes the buffer straight to a `.att` file
(`rgp.rs:950-953`). Two conditions apply:

- The token mask must be aqlprofile's set: exclude VMEM-exec, ALU-exec, event,
  utilisation-counter and wave-alloc tokens, and leave BOP events off
  (`R/docs/radv-rocprof-compat.patch:10-13`). With RADV's default mask only 8 of
  1,052 executed instructions were attributed (`R/docs/radv-profiling.md:19`).
- Without aqlprofile's in-stream code-object markers, PCs come back absolute with
  `code_object_id == 0`; the consumer translates them using base and size
  (`decode.py:52-58`).

**Consumer work.**

- **Traces:** a new `irpf.rs` arm writes each `executable-traces` chunk as
  `*_shader_engine_N_0.att`, writes the embedded ELFs, and builds a placed
  manifest from the load records. It then reuses the RADV decode path.
- **Raw-ATT path:** it would *not* take the stream unchanged. It expects
  in-trace markers and `code_object_id_N` file names (`decode.py:90,119`).
  Measured: it imports the waves but attributes no instruction and names the
  kernel by its absolute entry address.
- **Counters:** a new arm maps `counter-sets`, `counters` and `counter-samples`
  onto the existing counter model, joined by `dispatch_event_id`.

## Spike result (2026-11-14)

**Verdict: yes.** A non-AQL `KFD_IOC_QUEUE_TYPE_COMPUTE` PM4 user queue on
gfx1151 produces SQTT data that `librocprof-trace-decoder.so.0.2.2` decodes and
our decoder script consumes unchanged. No kernel or firmware change was needed,
and no root.

Setup: a standalone test on libamdf's own CTS fixture, so the queue is created
exactly as libamdf creates Loom's, publishing straight to the ring. The traced
kernel is hand-assembled for gfx1151 with a known count: 325 executed
instructions per wave. Packets are translated from Mesa 26.2.0 `ac_sqtt.c`.
Host: GC 11.5.1, MEC firmware 36, amdgpu-dkms 6.19.14, kernel 7.0.0-43.

- Source and how to rerun: `.synagent/scratch/hrx-loom-profiling/spike/`
  (`README.md`, `sqtt_spike_test.cc`).
- Evidence: `.synagent/artifacts/hrx-loom-profiling/50-sqtt-pm4-spike/`
  (`README.md` lists every run).

| Gate | What was measured | Result |
|---|---|---|
| **0. Do uconfig writes take effect?** | Read 10 registers with `COPY_DATA`; write `SQ_THREAD_TRACE_TOKEN_MASK`; write `SQ_THREAD_TRACE_MASK` with a different value per SE through `GRBM_GFX_INDEX`; read back; restore. | Every write read back exactly. Register reads work through both `COPY_DATA` source selectors. Per-SE addressing works. |
| **A. Does a buffer fill?** | Program both SEs, trace one dispatch of 8,192 waves, `THREAD_TRACE_FINISH`, poll status from the host, copy out. 1 MiB per SE. | SE0 120,256 bytes, SE1 116,384 bytes. `FINISH_DONE` set on the first poll, no write error, owner VMID 8. Dropped counters 41,268 and 183,996. |
| **B. Does it decode?** | Those bytes through `crates/profile-core/src/decode.py` with the kernel ELF placed at its load address; then the importer CLI. | 163 waves, 52,972 instructions, **all attributed** to the dispatched kernel. Per-instruction hit counts match the kernel exactly (loop body 10,432 = 163 x 64). 160 waves have exactly 325 instructions; the decoder warned "Data Lost". Two dispatches recovered, one per SE. |
| **C. Is stable pstate needed?** | The same run with `AMDGPU_CTX_OP_SET_STABLE_PSTATE` (STANDARD) on a render-node context, no root; 5 runs without, 8 with. | Not needed to get a trace. It changes data loss: see below. |

**Gate C detail.** The request worked without root: sysfs
`power_dpm_force_performance_level` went from `auto` to `profile_standard` and
back when the context closed.

| 8,192-wave dispatch | Runs with dropped data | Decoder "Data Lost" |
|---|---|---|
| No stable pstate | 5 of 5 | 5 of 5 |
| Stable pstate | 2 of 8 | 2 of 8 |

(The 8 include three packet variants that changed nothing else: no
`COMPUTE_THREAD_TRACE_ENABLE`, an added in-ring wait, dispatch in an indirect
buffer.)

- A 256-wave dispatch dropped nothing either way (3 runs), nor did 80 waves.
- So loss follows trace volume, and stable pstate reduces it. Which part of the
  pstate does this (the perfmon clock or the fixed clock level) was not
  isolated.
- Every run decoded, and everything decoded was attributed. But the RADV arm of
  our importer rejects a capture whose decoder reports "Data Lost"
  (`rgp.rs:972-977`), so loss matters to the consumer.

**What the consumer takes.**

- `decode.py`'s placed-manifest branch (the RADV path): unchanged.
- The importer CLI on the decoded directory: unchanged — 163 waves, 11 ISA rows,
  both dispatches named after the kernel.
- The importer CLI on the raw `.att`: imports waves, attributes nothing.
- The token mask matters here too: with RADV's default mask the same dispatch
  decoded to 291 "waves" and 19,915 instructions, of which only 398 landed on
  the kernel and no wave had the right count.

**Packet-level findings for the port.**

- Working sequence per SE: `GRBM_GFX_INDEX` select, `BUF0_SIZE` (with the high
  address bits), `BUF0_BASE`, `MASK`, `TOKEN_MASK` = `0x003f0683`, `CTRL` =
  `0x80023d41` last; then restore broadcast. Stop: `THREAD_TRACE_FINISH`
  (event 55), wait for `STATUS.FINISH_DONE`, `CTRL` = `0x80023d40`, wait for
  not-busy, read `WPTR`, `STATUS`, `DROPPED_CNTR`.
- `SPI_CONFIG_CNTL` must have its SQG event bits set (`0xc362c688`), or
  `FINISH_DONE` never sets. The trace is still captured without it. It is a
  global register; the spike restores `0xc062c688`, the value found.
- `COMPUTE_THREAD_TRACE_ENABLE` made no difference on this queue.
- Register-space `WAIT_REG_MEM` executes in the ring (tested only on a condition
  already true). Host-side polling also works and cannot wedge the queue.
- A dispatch inside an indirect buffer called from the ring is traced the same
  way, which is the shape of Loom's recorded command buffers.
- A full buffer shows as the write pointer stopping one 32-byte unit short of
  the size.

**Process isolation (extra check).** With `CTRL.ALL_VMID` clear, as Mesa leaves
it, the trace records only the tracing process's waves.

- A process holding a trace open for 1.5 s recorded **0 waves** while a second
  process ran 474 untraced dispatches of the same kernel.
- With `ALL_VMID` set, the same setup filled both 1 MiB buffers: 3,208 waves,
  with 162,438 instructions on the second process's code and none on the
  tracer's. Another 214,428 instructions had no code object; what they were was
  not identified (the buffers were full and waves were cut off).

**No GPU hang, reset or kernel message occurred**: `journalctl -k` is empty for
the whole spike. The user's `loom-serve` on 8082 was not running during the
gates: the agent service whose cgroup it lived in restarted at 23:08, before the
spike began, and took it down. It was restarted at 23:33, after which two traced
dispatches (256 and 8,192 waves) ran next to it, idle: both decoded exactly, and
the server answered a completion afterwards. Tracing next to a *busy* server was
not tried.

**Not covered by the spike.** One SIMD of one WGP per SE is traced, so about 2%
of waves are seen (163 of 8,192); that is how SQTT selects, not a fault. A real
Loom kernel, several dispatches in one trace, buffers above 1 MiB and the
overhead on the traced workload were not run. Perf counters were run
separately: next section.

## Counters spike result (2026-11-15)

**Verdict: yes.** The same PM4 user queue programs, runs and reads back
dispatch-scoped hardware counters, unprivileged, and the values are exact.
Setup, kernel and host are the SQTT spike's; the test kernel is wave32 with a
known count per wave: 325 instructions (130 VALU, 129 SALU), 32 workitems. The
sequence follows aqlprofile's GFX11 PMC builder, cross-checked against tinygrad
and RADV.

- Source and how to rerun: `.synagent/scratch/hrx-loom-profiling/spike/`
  (`pmc_spike_test.cc`, `README.md` "Perf counters").
- Evidence: `.synagent/artifacts/hrx-loom-profiling/51-pmc-pm4-spike/`
  (`README.md` lists every run).

**It cost two GPU resets, both from one mistake, neither from the counter
path.** The first run tried to prove the perfmon clock by reading
`RLC_CGTT_MGCG_OVERRIDE` with `COPY_DATA`. That read never completed, MES
stopped responding, and the kernel recovered with a full reset at 23:58:56. It
broke the user's `loom-serve`; stopping that dead process at 00:01:55 triggered
the second reset. The server was restarted at 00:07:59 and verified. The 49
runs after that produced no kernel message at all.

| Gate | What was measured | Result |
|---|---|---|
| **P0. Perfmon clock without root?** | `AMDGPU_CTX_OP_SET_STABLE_PSTATE` (STANDARD) on a render-node context; sysfs; `GRBM_GUI_ACTIVE` with and without the request. | Obtainable: sysfs goes `auto` -> `profile_standard`, and `GRBM_GUI_ACTIVE` counts in 37 of 38 runs with the request (the 38th took no sample) and reads 0 in all 4 samples without. The clock register itself cannot be read from the ring. |
| **P1. Does MEC accept the programming?** | Write both block controls, both `CTRL2`, 8 SQG selects, 4 per-WGP selects and a GRBM select; read back broadcast and per SE; walk `CP_PERFMON_CNTL`. | Every write read back. Nothing dropped, reset or faulted. The sample bit is a strobe and reads 0. |
| **P2. Do the values mean anything?** | `SQG_WAVES` around one dispatch, against the predicted wave count, on five grids; then workitems and per-WGP instruction counts. | Exact everywhere: see below. |
| **P3. Does it break anything?** | Computed result against an uncounted reference dispatch; registers before and after; kernel log per run. | All lanes correct in every run, registers restored in every run, no kernel message. |
| **P4. Whose waves are counted?** | A second process dispatching 20,000 waves inside the counting window; a scan of single-VMID masks; the user's server answering a completion inside the window. | Only queues that set their own enable bit, and only VMIDs in the mask. Contamination 0 by default. |

**P2 detail.** Measured against expected, difference 0 in every row:

| Dispatch | `SQG_WAVES` | Other counters |
|---|---|---|
| 256 workgroups x 32 | 256 (128 per SE) | `WAVES_32` 256, `WAVES_64` 0, `ITEMS` 8,192 |
| 8,192 x 32 | 8,192 | `ITEMS` 262,144 |
| 1,000 x 32 | 1,000 | per-WGP `SQ_WAVES` 1,000 (50 on each of 20 WGPs), `INSTS_VALU` 130,000, `INSTS_SALU` 129,000, `INSTS_ALL` 325,000 |
| 1 x 32 | 1 | `ITEMS` 32 |
| 500 x 64 (2 waves each) | 1,000 | `ITEMS` 32,000 |
| 4 x 1,000 in one session | 1,000 per dispatch | cumulative 4,000 |

The instruction total matches what the SQTT spike decoded for the same kernel
(325 per wave), so the two mechanisms agree. No `rocprofv3 --pmc` comparison was
run: it needs a ROCr workload, and four independent predicted quantities
already matched. The cycle counters (`SQG_BUSY_CYCLES`, `SQG_WAVE_CYCLES`,
`SQG_CYCLES`) return plausible non-zero values but were not checked against
anything.

**P4 detail.** Process A counts for 3 s; process B dispatches 20 x 1,000 waves
of the same kernel inside that window.

| A's setup | B | A counted |
|---|---|---|
| `CTRL2` at its default (all VMIDs, no `FORCE_EN`) | plain queue | **0** |
| same, A also dispatches 1,000 | plain queue | **1,000** |
| same | B sets its own enable bit | 20,000 |
| mask holds only A's VMID | B sets its own enable bit | **0** |
| `FORCE_EN` in `SQG_PERFCOUNTER_CTRL2` | plain queue | 111,566: B's 20,000 plus 91,566 wave64 waves from the desktop |

Next to the user's `loom-serve` while it answered a 24-token completion inside
the window: 1,000 of 1,000 waves, 325,000 instructions, contamination 0.

So counter capture does **not** need to be exclusive against other workloads.
It does need to be the only *collector*: the select registers and
`CP_PERFMON_CNTL` are device-global, so a second collector (`rocprofv3 --pmc`,
RADV performance queries) would reprogram or reset them.

### The validated sequence

All writes are `SET_UCONFIG_REG` (opcode `0x79`, offset `(addr - 0x30000) >> 2`)
unless stated. Addresses are byte offsets from Mesa `gfx115.json`. "Broadcast"
is `GRBM_GFX_INDEX` (`0x30800`) = `0xe0000000`.

**Before any packet.** Hold `AMDGPU_CTX_OP_SET_STABLE_PSTATE` with
`AMDGPU_CTX_STABLE_PSTATE_STANDARD` on a DRM context of the render node for the
whole capture. Closing the context drops it.

**Start, once per capture.**

1. `EVENT_WRITE` `CS_PARTIAL_FLUSH`, then broadcast. (The spike used libamdf's
   system barrier here and in step 12, which adds an `ACQUIRE_MEM`; whether
   that half matters was not isolated.)
2. `CP_PERFMON_CNTL` (`0x36020`) = `0` — disables and clears every counter.
3. `SQ_PERFCOUNTER_CTRL` (`0x36780`) = `0x55` (PS, GS, HS, CS stage enables).
4. Optional: `SQ_PERFCOUNTER_CTRL2` (`0x36788`) and `SQG_PERFCOUNTER_CTRL2`
   (`0x36768`) = `1 << (vmid + 1)` to count this process only. Otherwise leave
   both at the default `0x1fffe`. Bit 0 (`FORCE_EN`) stays 0.
5. `SQG_PERFCOUNTER<n>_SELECT` (`0x36740 + 4n`, n < 8) = the event number, all
   other fields 0. Used: `WAVES` `0x14`, `WAVES_32` `0x15`, `WAVES_64` `0x16`,
   `ITEMS` `0x18`, `BUSY_CYCLES` `0x10`, `WAVE_CYCLES` `0x11`, `CYCLES` `0x0f`,
   `WAVES_SAVED` `0x0d`.
6. `SQG_PERFCOUNTER_CTRL` (`0x36760`) = `0x55`.
7. Optional GRBM: `GRBM_PERFCOUNTER0_SELECT` (`0x36100`) = `2` (`GUI_ACTIVE`).
8. Optional per-WGP: `SQ_PERFCOUNTER<2k>_SELECT` (`0x36700 + 8k`) = the event.
   Used: `WAVES` `0x04`, `INSTS_VALU` `0x3e`, `INSTS_SALU` `0x3a`, `INSTS_ALL`
   `0x30`. Only even select registers; select `2k` feeds result register `k`.
9. Broadcast.
10. `SET_SH_REG` `COMPUTE_PERFCOUNT_ENABLE` (`0xb82c`) = `1`.
11. `CP_PERFMON_CNTL` = `0`, then = `1`.
12. `EVENT_WRITE` `CS_PARTIAL_FLUSH`.

**Sample, before and after each dispatch** (after the dispatch's own
`CS_PARTIAL_FLUSH`):

1. `CP_PERFMON_CNTL` = `0x401` (keep counting, strobe a sample).
2. Broadcast, then `EVENT_WRITE` `CS_PARTIAL_FLUSH`.
3. For each shader engine `se` (2 here): `GRBM_GFX_INDEX` =
   `(se << 16) | 0x60000000`, then one `COPY_DATA` (`0x40`) per half:
   control `0x00100204` (source 4 "perf", destination 2 memory, write
   confirm), source `addr >> 2`, destination VA. `SQG_PERFCOUNTER<n>_LO` is
   `0x34790 + 8n`, `_HI` is 4 above.
4. Broadcast; `GRBM_PERFCOUNTER0_LO`/`_HI` (`0x34100`/`0x34104`) the same way.
5. Per-WGP, for each `(se, sa, wgp)` whose two bits are set in the kernel's
   `cu_bitmap[se][sa]` (`AMDGPU_INFO_DEV_INFO`; `0x3ff` here, so 5 WGPs per
   array, 20 in all): `GRBM_GFX_INDEX` = `(se << 16) | (sa << 8) | (wgp << 2)`,
   then `COPY_DATA` of `SQ_PERFCOUNTER<k>_LO` (`0x34700 + 8k`, 32 bits, no high
   half).
6. Broadcast.

A dispatch's value is the difference between its two samples: SQG and GRBM as
64-bit, summed over shader engines; per-WGP as 32-bit, summed over WGPs.

**Stop, once.** Broadcast; `CP_PERFMON_CNTL` = `2`; `CS_PARTIAL_FLUSH`;
`COMPUTE_PERFCOUNT_ENABLE` = `0`. Then put back what was found:
`CP_PERFMON_CNTL` `0`, selects `0x1ff`, both block controls `0`, both `CTRL2`
`0x1fffe`.

### Gotchas

- **Never read `RLC_CGTT_MGCG_OVERRIDE` (`0x3b120`) from the ring.** One
  `COPY_DATA` of it hung MEC and reset the GPU. Treat every register outside
  the perfmon set above as unreadable until proven otherwise. After a reset a
  KFD process that was running is dead in place, and tearing it down can reset
  the GPU again.
- **`COMPUTE_PERFCOUNT_ENABLE` is mandatory.** Without it every counter reads
  0, even with `FORCE_EN` in `SQ_PERFCOUNTER_CTRL2`. It is queue state: it
  reads back, and it survived separate publications and 3-8 s of idle.
- **The sample strobe is mandatory.** The result registers are latches: with
  no strobe they read 0, and after a strobe they keep that value. The strobe
  bit reads back as 0.
- **`CP_PERFMON_CNTL` = 0 clears, 2 then 1 does not.** Sampling with `0x402`
  and restarting with `1` accumulates just like `0x401`.
- **Do not set `FORCE_EN`.** The rule that fits every run:

  | Block (its `CTRL2`) | `FORCE_EN` 0 | `FORCE_EN` 1 |
  |---|---|---|
  | SQG (`SQG_PERFCOUNTER_CTRL2`) | queue enable **and** VMID in mask | everything on the device |
  | per-WGP SQ (`SQ_PERFCOUNTER_CTRL2`) | queue enable **and** VMID in mask | VMID in mask, enable ignored |

  aqlprofile writes `SQ_PERFCOUNTER_CTRL2` = `0x1fe01` (`FORCE_EN`, VMIDs
  8-15). Copying that made the per-WGP counters read 21,000 for 1,000 own
  waves next to another KFD process.
- **The VMID is not fixed.** This process had VMID 8 alone and 9 with the
  server running. The spike finds it with 16 tiny dispatches, one per mask
  bit; a thread trace reports the same number in `SQ_THREAD_TRACE_STATUS`
  bits 31:28. Leaving the mask at its default is enough unless another process sets
  its own enable bit — which the kernel does for every queue while a
  privileged profiler owns PMC (`kfd_mqd_manager_v11.c:183-186`).
- **The stable pstate matters only for GRBM here.** SQ and SQG counted
  exactly without it; `GRBM_GUI_ACTIVE` read 0.
- **Not needed:** `PERF_COUNTER_WINDOW`, the SQG event bits in
  `SPI_CONFIG_CNTL`, RADV's `0xf000` in the selects, the
  `PERFCOUNTER_SAMPLE` event, any `PERFMON_ENABLE_MODE` other than 0. Both
  `COPY_DATA` source selectors read the counters.
- **Shape.** Start, dispatch and sample work in one publication, and around a
  dispatch inside an indirect buffer. A sample of 8 SQG counters plus GRBM is
  about 240 ring words; its time cost was not measured.
- **Reset defaults**, useful as a "nobody else is collecting" check:
  `CP_PERFMON_CNTL` 0, block controls 0, both `CTRL2` `0x1fffe`, selects
  `0x1ff`.

## Minimum viable path: waves + ISA

1. **Spike first.** Done, and it passed: see
   [Spike result](#spike-result-2026-11-14).
2. Add the packet writers in `D/queue/pm4/commands.{h,c}`.
3. Add a thread-trace module with a gfx11.5 register table, start/stop builders
   and one finite buffer per SE. The spike's sequence is the template,
   including the `SPI_CONFIG_CNTL` write and its restore.
4. Hook it at the existing bracket for the first N dispatches matching the
   capture filter.
5. At retirement, copy the written extent to host and emit one
   `executable-traces` chunk per SE with `dispatch_event_id`.
6. Accept the family in `D/profile.c`; add driver options for CU, SIMD, SE mask
   and buffer size.
7. Add the importer arm in `R`.

Rough effort: **~10-12 files, 1.5-2.5k lines** including tests on the driver
side, plus **~300-500 lines** in rocprof-studio. The spike does not change the
size; it removes the risk. Of the three blockers listed before it:

- **The firmware question: answered, yes.**
- **The register table: in hand**, transcribed from Mesa and proven on this GPU.
- **Stable pstate: not a prerequisite**, but the driver should request it (no
  root needed) or bound the traced volume, because large traces otherwise lose
  data and our importer rejects lossy captures.

What remains is ordinary driver work: deciding which dispatches to trace,
buffer sizing and full-buffer handling, and the recorded-buffer variant.

## Minimum viable path: counters

**Go, with conditions.** The sequence is proven on this GPU and the open
questions that sized the estimate are answered (see
[Counters spike result](#counters-spike-result-2026-11-15)).

1. Reuse the packet writers from the trace work: `SET_UCONFIG_REG`,
   register-source `COPY_DATA`, `GRBM_GFX_INDEX` select and restore. Counters
   need no `EVENT_WRITE` beyond `CS_PARTIAL_FLUSH` and no `WAIT_REG_MEM`.
2. Hold a stable pstate for the capture. `libamdf` already owns a render fd and
   a DRM context (`H/libamdf/src/gpu/umd/kfd/vm.c:129-134`).
3. Add a counters module with the gfx11.5 table and the start, sample and stop
   builders from [the validated sequence](#the-validated-sequence). Start with
   the SQG set (waves, workitems, busy and wave cycles): two instances, 64-bit.
4. Sample at the existing bracket's begin and end, into a widened per-dispatch
   slot; subtract at retirement.
5. Emit `counter-sets`, `counters` and `counter-samples` from `D/profile.c`,
   following the XDNA code.
6. Add the importer arm in `R`.
7. Later: per-WGP instruction counters. They need the `cu_bitmap` instance
   loop and are 32-bit.

Conditions:

- Read and write only the perfmon registers listed in the sequence.
- Set the queue's own `COMPUTE_PERFCOUNT_ENABLE`; leave `FORCE_EN` clear.
- One collector at a time. Refuse to start, or warn, when `CP_PERFMON_CNTL` or
  a select register is not at its reset default.
- Every queue of the process that sets the enable bit is counted together, so
  a per-dispatch value needs that process's other dispatches joined first.
- Per-dispatch sampling inside a recorded command buffer needs the same
  recording variant as tracing; bracketing a whole recording works today.

Rough effort: **~6-8 files, 1-1.3k lines** for the SQG set, as before at the
low end, plus roughly 200 lines for the per-WGP counters. The spike removes
the risk rather than the work: no kernel support, no privilege, no
`PERF_COUNTER_WINDOW`, and no exclusivity against other workloads.

## Hard blockers

- **PC sampling on gfx1151:** absent in the loaded driver
  (`K/amd/amdkfd/kfd_pc_sampling.c:69-80`). Userspace cannot supply it.
- **Perfmon clock: not a blocker.** Only the kernel toggles it
  (`K/amd/amdgpu/gfx_v11_0.c:5452-5469`), but userspace can request it without
  root through `AMDGPU_CTX_OP_SET_STABLE_PSTATE`
  (`K/amd/amdgpu/amdgpu_ctx.c:708-714`; the only `capable()` call in that file
  is at `:107`, for priority). Measured: with the request the GRBM counter
  counts, without it it reads 0; SQ and SQG counters count either way. The
  sysfs route is root-only. The clock register itself must not be read from a
  user queue: that reset the GPU.
- **One collector at a time:** SQTT and perf-counter registers are
  device-global, and the GFX11 SQTT mask does not encode a VMID
  (`thread-trace.md:15-19`). The *data* is isolated, though: measured on
  gfx1151, a trace records only the owning process's waves unless
  `CTRL.ALL_VMID` is set, and perf counters count only queues that set their
  own enable bit, within a per-VMID mask, unless `FORCE_EN` is set.
- **No kernel or firmware blocker for SQTT or counters:** measured — a PM4
  KFD queue programs, runs and reads back both with no kernel support.

## Not determined

Measured since the study (see the two spike sections): MEC executes SQTT and
perfmon uconfig writes from a PM4 KFD queue on gfx1151; SQTT does not need
stable pstate to produce a trace; counters work from the same queue, are exact,
need no `PERF_COUNTER_WINDOW`, and are not contaminated by other processes.
Still open:

- **Whether `COMPUTE_PERFCOUNT_ENABLE` survives a context save and restore.**
  It survived idle periods and separate publications, but no run was ever
  preempted: `SQG_WAVES_SAVED` read 0 in all 41 counted runs. Forcing it needs
  more queues than hardware queues.
- **Whether a process's VMID can change during its lifetime.** It differed
  between runs (8 alone, 9 next to the server), never within one.
- **Whether `RLC_CGTT_MGCG_OVERRIDE` reads through the other `COPY_DATA`
  selector.** Not tried after the first attempt reset the GPU.
- **What the cycle counters mean in time.** They were not compared with a
  clock, and they differ between `auto` and the stable pstate.
- **The time cost of a sample**, and counter behaviour on a real Loom kernel.
- **Which blocks besides GRBM need the perfmon clock.** Only SQ, SQG and one
  GRBM event were programmed.
- **Why stable pstate reduces SQTT data loss**: the perfmon clock, or the fixed
  clock level. Separating them needs root.
- **Whether a trace's stall bits slow other processes** on the traced SIMD. An
  idle `loom-serve` survived a trace next to it; a busy one was not tried.
- **Overhead on the traced workload**, and behaviour with a real Loom kernel,
  many dispatches per trace, or buffers above 1 MiB.
- **Whether the results hold on other MEC firmware.** Measured on firmware 36
  only.
- **How the decoder infers the GFX generation from a header-less stream.** It
  works empirically on gfx1151 through the RADV arm; the mechanism is
  closed-source.
- **SPM backend completeness on GFX11.** Needs a read of
  `K/amd/amdgpu/amdgpu_amdkfd_rlc_spm.c`.

## The sidecar alternative (`rocprofv3 --att-no-intercept`)

It is **not** a route to waves for Loom on this SDK.

- **Present, but in-process.** The flag exists (`O/bin/rocprofv3:1201-1205`) and
  the tool library carries `att_no_intercept.cpp` and
  `att_no_intercept_quick_scan.cpp`. `rocprofv3` still injects the tool into the
  launched process.
- **Needs ROCr in that process.** The library contains "Device thread trace start
  requested before hsa_init", and the mode starts "after the first code-object
  upload for each selected GPU agent" (`O/bin/rocprofv3:2561-2563`). A Loom
  binary does neither.
- **No external transport.** The direct-KFD collector the tree docs describe
  (`kfd/resource.cpp`) is not in this 1.4.1 build; only `kfd_reader`,
  `signal_less`, `kfd_profiler` and `kfd.cpp` appear in its strings.
- **Wrapping a dummy ROCr process instead.** The hardware would capture Loom's
  waves on the selected WGP, but quick-scan matches kernel ranges only against
  its own memory-backed code objects. Whether output survives for foreign waves
  is not determinable without running it.
- **Root.** The library carries "Required permission (CAP_PERFMON) is not set"
  next to the thread-trace strings, so expect to need that capability.
- **Conflict.** It would not disturb the PM4 queue itself, but it owns the same
  global SQTT registers an in-driver tracer would use.
- **Counters.** Not covered: `--att-perfcounters` settings are skipped under
  no-intercept (`O/bin/rocprofv3:2438`).

## Recommended order

1. The spike — done; SQTT from the PM4 queue works and decodes.
2. Waves + ISA, because the schema, decoder and consumer path all exist already.
   First concrete step: add the four packet writers (`SET_UCONFIG_REG`,
   register-source `COPY_DATA`, `EVENT_WRITE`, register-space `WAIT_REG_MEM`)
   to `D/queue/pm4/commands.{h,c}` and a thread-trace module that brackets one
   direct-path dispatch with the spike's sequence.
3. Counters. The spike passed, so this is now ordinary driver work on top of
   the packet writers from step 2: a counters module that emits the validated
   start, sample and stop sequence at the same bracket.
