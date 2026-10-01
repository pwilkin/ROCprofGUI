# Validation & parity

## The parity baseline

The project tracks **ROCprof Compute Viewer (RCV)** — `github.com/ROCm/rocprof-compute-viewer`, baseline commit `ea1ce2b…`, a Qt/C++ desktop app that visualises rocprofv3 thread-trace data. RCV is distinct from rocprofiler-compute's high-level metric analysis.

The repository's `docs/upstream-parity.md` is a **31-row checklist**: **28 Implemented, 3 Partial, 0 Missing**. Even so, **full parity is not claimed** — the "complete" status in the repo was reached by an explicit user scope agreement, not by closing every row, and several rows are "Partial" (e.g. SIMD utilization, ROCPD input). It is also explicitly **not** a pixel-identical Qt reimplementation.

## What the checks prove

The [Verify](/installation#verify-your-build) sequence runs:

- **`cargo test -p profile-core`** — the Rust analysis core's unit and integration tests.
- **Oracle differentials** — `check-hidden-oracle.py`, `check-marker-oracle.py`, `check-tensor-oracle.mjs` compare the app's hidden-latency, marker and tensor-counter maths against independent reference implementations (some vendored from upstream C++).
- **`npm test`** (Vitest) — the pure frontend analysis modules.
- **`npm run build`** + **`cargo check`** — the app compiles.
- **`npm run test:ui`** (Playwright) — the UI specs drive the real frontend in a browser against a dev server on port 1421, using a Tauri IPC stub for native calls.

## What the checks do *not* prove

- They do **not** prove hardware correctness. Decoded-capture qualification (`docs/validation.md`) records real decodes of archived traces (gfx1101, gfx942, gfx1200) but **no GPU workload was launched** for them.
- Raw-ATT decoding has an **opt-in** integration test gated on a real trace:
  ```bash
  ROCPROF_GUI_TEST_ATT=/path/to/raw/trace \
    cargo test -p profile-core --test decoder \
    real_att_decode_and_lazy_wave_read -- --ignored --nocapture
  ```
  There are ~10 such `#[ignore]` fixture tests, each gated on an environment variable pointing at real data.
- Several test-count figures in the repo docs disagree with each other; `docs/validation.md` is a **chronological log**, where later sections supersede earlier ones. Read it as history, not as a single contract.

## Where the repo docs are stale (follow the code)

| Claim in repo docs | Reality in code |
| --- | --- |
| Summary limits input to **500,000** (`docs/upstream-parity.md:34`) | **2,000,000** (`src/summary.ts:173`). |
| `read_wave_stats` accepts at most **128** waves (`docs/architecture.md:144`, `:152`) | **4096** (`wave_index.rs:8`); 128 is the viewport bound. |
| "**eight**" enabled fixture tests (`docs/upstream-parity.md:53`) | **10** `#[ignore]` tests in the tree. |

These are tracked in the project's `docs/open-work.md` and `docs/blocked-inputs.md`. For the architecture behind the checks, see [Architecture](/architecture).
