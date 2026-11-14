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
