# Mip oracle source provenance

The following files were taken from ROCm/rocprof-compute-viewer commit `ea1ce2b87562b28e3fb5c8819b5e12573cc14424`:

- `token_mip.inc`: unchanged definitions from `static int gettype` through `TokenGroup::SetMipN()` in `src/data/wavemanager.cpp`, excluding Qt-dependent surrounding code and retaining the source license header.
- `util/wave_utils.h`: unchanged complete `src/util/wave_utils.h`.

`tests/oracles/mip-main.cpp` supplies minimal data types and an input/output driver for the unchanged reducer. `tests/oracles/mip-geometry-main.cpp` calls the upstream integer geometry helpers. Run `node scripts/check-mip-oracle.mjs`; retained outputs go to `.codex/mip-validation`. These oracles compare algorithm behavior, not full Qt widget rendering.


The unchanged files in data/waitcnt are copied from src/data/waitcnt at the same pinned upstream commit. tests/oracles/wait-main.cpp and wait-include provide minimal data types for an independent C++ executable. The comparison driver normalizes duplicate edges and checks token identities, not merely ISA totals. tests/oracles/correct-wait-oracle.py creates a separately labeled copy under .codex with two narrow changes (VSCNT operand parsing and flat-store-only draining); it never edits the vendored oracle.
