# Installation

rocprof studio is a Tauri 2 desktop application with a Rust core and a React/TypeScript frontend. You can run it as a desktop app, as a browser preview (synthetic data only), or as a headless importer/MCP server.

## Prerequisites

| Requirement | Notes |
| --- | --- |
| **Node.js 22+** | Frontend build, Vitest/Playwright, and the build-time MCP-analysis bundle. Node is also a **runtime** requirement for the MCP `capture_analyze` tool. |
| **Rust 1.93+** | Builds `profile-core` and the Tauri shell. |
| **GTK 3, WebKitGTK 4.1, a C/C++ toolchain** | The Tauri Linux prerequisites (the app targets Linux). |
| `dpkg-shlibdeps` (`dpkg-dev`) | Only for building a local Debian package. |
| Python 3, the native decoder library, ROCm disassembly tools | **Only for raw ATT** decoding — decoded captures need none of these. See [RADV / Vulkan profiling](/radv) and the ATT notes below. |

## Install dependencies

```bash
npm ci
```

## Prepare the decoder ZIP first (required before `tauri dev`/`tauri build`)

The desktop build declares a bundle resource, `decoder-python.zip`, and the app **resolves it at startup — a missing resource aborts startup**. So you must build it *before* running or building the desktop app. It is a deterministic, pure-Python ZIP holding the qualified `rocprof-trace-decoder` **0.2.2** API plus `pyelftools` **0.33**; bundling it means no `PYTHONPATH` or package install is needed in the interpreter you select.

```bash
mkdir -p .codex/decoder-package/work
TMPDIR="$PWD/.codex/decoder-package/work" \
  python3 -m pip install --target .codex/decoder-package/dependencies pyelftools==0.33
python3 scripts/package-decoder-python.py \
  --decoder-source /path/to/rocprof-trace-decoder \
  --dependencies .codex/decoder-package/dependencies
```

The packager refuses any versions other than `rocprof-trace-decoder 0.2.2` / `pyelftools 0.33`, packages only `.py` files, writes a SHA-256 manifest into the ZIP, and produces byte-identical output for identical inputs. See [RADV / Vulkan profiling](/radv) and the project's `docs/profiling-caveats.md` for what raw ATT decoding needs at runtime.

::: tip The exact bundle path
In the repository's Tauri config the resource is mapped from `.codex/decoder-package/decoder-python.zip`. The README's "Raw ATT setup" section shows the same workflow under `third_party/decoder-python/`. Either location works as long as the path you build to matches the one Tauri bundles; the app looks it up through Tauri's resource directory at runtime.
:::

## Run the desktop app (development)

```bash
npm run tauri dev
```

This opens the single native window (1500×980, dark `#111318`). On every launch it also starts the [MCP server](/mcp) on `127.0.0.1:8765` and writes a bearer-token file — pass `--no-mcp` to disable that.

## Browser preview (no desktop build, synthetic data only)

```bash
npm run dev        # vite --host 127.0.0.1, port 1420 (strictPort)
```

The browser preview loads the **synthetic example capture** and has no filesystem access — directory and raw-trace import only work in the desktop app. It is what this site's view screenshots were captured from. See [Running & modes](/running).

## Build a desktop binary

```bash
# Unbundled debug build; keep the decoder ZIP beside the binary:
npm run tauri build -- --debug --no-bundle
# → target/debug/rocprof-studio  (+ target/debug/decoder-python.zip)
```

By default `tauri build` produces **no installer** (`bundle.active` is off in the Tauri config).

## Build a local Debian package

```bash
python3 scripts/bundle-linux.py          # release build (add --debug for a debug build)
```

It rebuilds the app, stages a copy, runs `dpkg-shlibdeps` on the executable to derive versioned shared-library dependencies, and writes `.codex/linux-package/receipt.json` (package path, profile, detected runtime dependencies, SHA-256s). The current build needs glibc 2.39+; GTK 3 / WebKitGTK 4.1 remain runtime requirements.

## Verify your build

Run the project's checks in this order (from the README "Verify" section):

```bash
cargo test -p profile-core
python3 scripts/check-hidden-oracle.py
python3 scripts/check-marker-oracle.py
node scripts/check-tensor-oracle.mjs
npm test                       # vitest run
npm run build                  # tsc -b && vite build
cargo check -p rocprof-studio
npx playwright install chromium
npm run test:ui                # playwright test (web server on port 1421)
```

See [Validation & parity](/validation) for what these checks do and do not prove.
