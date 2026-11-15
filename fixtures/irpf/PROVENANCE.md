# IRPF fixtures: native AMD GPU driver (PM4 user queues)

Real IREE HAL profile bundles written by the hrx-system native AMD GPU driver on
gfx1151 (Radeon 8060S, `amdgpu://0`), which submits PM4 to its own KFD user
queues instead of going through ROCr. Produced from hrx-system
`23e9b32232df60549325095f4d7c8dc1c955aa22` (branch `loom-working`), built with
`-DLIBHRX_BUILD=OFF -DIREE_HAL_DRIVER_AMD_GPU=ON -DIREE_HAL_DRIVER_AMD_XDNA=ON`.

| File | Bytes | SHA-256 |
|---|---:|---|
| `loom-native-pm4-increment.irpf` | 17,238 | `27d9315252d9911e93b8e95aded215e0d7bd2a25160a468d98493ebb0587a880` |
| `loom-hybrid-aborted-session.irpf` | 1,578 | `9359b67c88b30a2f3c0962eb5b18606af1b4cad523b22c601d023317aa9a158f` |

## `loom-native-pm4-increment.irpf`

The final profiled batch of one `loom-benchmark` dispatch benchmark: 64 dispatches
of the `increment` kernel (grid 1×1×1, workgroup 1×1×1), with device timestamps,
one embedded ELF code object and the four default compute queues of the device.

```shell
loom-benchmark loom/src/loom/tools/loom-benchmark/testdata/dispatch_benchmarks.loom \
  --device=amdgpu://0 --measure=dispatch_complete --batch-size=64 \
  --profile-final-batch=true --profile-artifacts-dir=profiles --output=results.json
```

The file is `profiles/r…_c0_sample0.irpf` of that run.

## `loom-hybrid-aborted-session.irpf`

What `loom-serve` leaves behind when a GPU + XDNA device group is asked for
`dispatch-events`: the GPU session begins, the XDNA driver rejects the family
(`xdna/profile.c:85: UNIMPLEMENTED; unsupported XDNA profiling data families
0x8`), the tool ends profiling and exits 1. The bundle is finalized but holds one
GPU session with device and queue records only: no functions, no queue events,
no dispatches.

```shell
loom-serve --devices=devices.json --model=deployment.json --listen=127.0.0.1:8093 \
  --device_profiling_mode=dispatch-events,executable-metadata \
  --device_profiling_output=hybrid.irpf
```

`devices.json` names `amdgpu://0` and `xdna://0?columns=8`
(`loom/models/qwen3.8-27b-hybrid`).

## `synthetic-hrx-counters.irpf` (synthetic, not a capture)

Not written by a driver. `scripts/make-irpf-counter-fixture.py` builds it byte by
byte from the record layouts in hrx-system `runtime/src/iree/hal/profile_file.h`
and `profile_schema.h`, so the importer's hardware-counter arm and the hrx
collection report can be tested without a GPU. Every number in it is made up.
2,935 bytes, SHA-256 `33b8f1a76e819d283b5d5bb0f8c828728658d0c5d1e1abb0c3a77273c5bd8341`.

One `amd-gpu` session with three dispatch events of `g_gemm_q4_k_store_m256`
(4, 8 and 2 workgroups of 64 × 1 × 1), a counter set `amdgpu.perfmon` of the
driver's five default counters (two value slots each, one per shader engine),
two counter samples joined to the first two dispatch events, and one
`counter-captures` record: bound 2 (reached), exclusive window, stable power
state.

```shell
python3 scripts/make-irpf-counter-fixture.py fixtures/irpf/synthetic-hrx-counters.irpf
```
