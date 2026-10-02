# Loom compile-report fixtures

`shared_memory_vector_tile.benchmark-envelope.compile-report.json` is a **real**
`iree-benchmark-loom` compile-report envelope, not synthetic data. It is the
sidecar the benchmark tool wrote while compiling and running one kernel once on
GPU, vendored here so the importer is tested against a producer-generated
version-zero report rather than a hand-built one.

| | |
|---|---|
| Producer | `iree-benchmark-loom`, built from ROCm/hrx-system at commit `32b86d42d96a8e3098d9a56bea201e5f83d44f85` |
| Build | `python3 dev.py bazel build //loom/src/loom/tools/iree-benchmark-loom:iree-benchmark-loom --//runtime/config/hal:drivers=amdgpu,task --//runtime/src/iree/hal/drivers/amdgpu:targets=gfx1151` |
| Tool sha256 | `8d83fecff907d06b98c369e8485813161442f363579c26967c7045a99c5de8df` |
| Source | `shared_memory_vector_tile.loom` (sha256 `bbc4bc2e83e646ab6dea53d22138957a5e69269bbc2e3ed39f3ac8d4f1e7b493`) |
| Device / target | gfx1151 (AMD Radeon 8060S / STRIX_HALO), `--device=amdgpu --target=amdgpu:gfx1151` |
| Captured | 2026-10-02 |
| Size / sha256 | 116,031 bytes, `0390e3e2083054e4eaac6f18263c7af27b78395be423c773e786baa17c7b5747` |

Exact invocation (cwd of the producer run):

```sh
iree-benchmark-loom shared_memory_vector_tile.loom \
  --device=amdgpu --target=amdgpu:gfx1151 \
  --artifact-bundle-dir=.../bundle --artifact-bundle-policy=debug \
  --compile-report=details --output-format=jsonl \
  --iterations=1 --warmup-iterations=0 --batch-size=1 \
  --min-time-ms=0 --max-batches=1 --stable-p90-to-p50-ppm=0 --input-ring-count=1
```

## Shape

This is an **envelope**: the outer object is a benchmark result
(`type=compile_report`, `state=ok`, with `run_id`, `candidate_id`,
`candidate_index`, `benchmark`, `case`, `entry` and absolute artifact links),
and the version-zero report is nested at `compile_report`:

- `kind=loom.compile_report`, `schema_version=0`, `mode=details`
- one entry, `backend=amdgpu-hal`, `target_key=gfx1151`, `artifact_kind=hal-executable`
- report `detail_flags` `260026047` (`0xf7faebf`); entry `detail_flags` `260022846` (`0xf7fa23e`)

The importer validates the nested report with the same strict loader as a direct
`loom.compile_report`, records the outer run/candidate/case/benchmark/sample
context as envelope context, and never treats any of it as a measurement — a
compile report carries no cycle or time data.

## Notes

- The outer envelope keeps the producer's **absolute** artifact paths
  (`target_artifact_path`, `hal_executable_path`, …). The importer does not
  follow them; it reads only the JSON body, so the fixture is self-contained.
- Full provenance for the producing run (build discovery, exact argv, env
  overrides, timestamps, memory watchdog, cleanup verification) lives beside the
  original bundle at
  `/devel/tools/loom-reports/benchmark-live/{README.md,manifest.json,commands.json,provenance.json}`
  and `provider-rebuild-20261002/validation.json`.
- Regenerate by rebuilding the provider-enabled tool at the pinned commit and
  re-running the invocation above. Compile reports are version-zero,
  same-compiler-horizon diagnostics: a report from a different checkout is a
  different artifact, not a newer version of this one.
