# Decoder Python package provenance

`decoder-python.zip` is a build resource: the pure-Python dependencies the ATT decoder worker needs, packaged so the desktop app does not require installing anything into the selected interpreter. Tauri declares it as a bundled resource and the worker puts it first on its import path.

## Contents

| Component | Version |
|---|---|
| `rocprof-trace-decoder` Python API | 0.2.2 |
| `pyelftools` | 0.33 |

63 entries, SHA-256 `18551de9108223bb37cabca890c5a8035c7fc56156f493d88f7b171bd0cdb8bb`. Both upstream licenses are retained inside the archive, and `decoder-package.json` inside it records every packaged file's SHA-256 alongside the component versions.

## Rebuilding

`scripts/package-decoder-python.py` produces this file. It checks the dependency versions, retains both licenses and writes the per-file manifest; repeating it with identical inputs produces identical archive bytes, so a rebuild that changes the SHA-256 above means an input changed. The README's "Raw ATT setup" section gives the exact commands.

Build scratch (the pip `--target` directory and its `TMPDIR`) goes to `third_party/decoder-python/build/`, which is not tracked. Only the archive and this file are.

## What this package does not supply

Python itself, the native `librocprof-trace-decoder.so` V2 library and ROCm's disassembly tools remain external requirements. This archive contains only the pure-Python layer.
