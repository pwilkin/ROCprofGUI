# Running & modes

rocprof studio is one binary and one analysis core that run in four ways.

## 1. Desktop GUI

```bash
npm run tauri dev           # development
./rocprof-studio            # an installed/built binary
```

The full interface: capture tabs, the sidebar, the dockable workspace and every analysis view ([UI tour](/ui-tour), [Views reference](/views)). A desktop launch also starts the [MCP server](/mcp) by default and reuses a service already running on the selected MCP port if there is one.

## 2. Browser preview

```bash
npm run dev                 # vite --host 127.0.0.1, port 1420 (strictPort)
```

A pure-browser build that loads the synthetic example (`public/demo.json`) and has no filesystem access. Directory import and raw-trace import need the Rust desktop app; the browser preview says so when you try. It is intended for exploring the interface, and it is what the view screenshots on this site were captured from.

## 3. Headless importer CLI

The Rust core builds a standalone importer with no GUI:

```bash
cargo run -p profile-core -- /path/to/ui_output_capture
cargo run -p profile-core -- /path/to/kernel_trace.csv
cargo run -p profile-core -- /path/to/spm_results.json
cargo run -p profile-core -- /path/to/ui_output_capture se0_simd0_slot0_wave0.json   # one wave
cargo run -q -p profile-core -- fixtures/transpose-ui --bundle                        # embed sources
cargo run -q -p profile-core -- fixtures/transpose-ui --hidden                        # hidden-latency analysis
```

The CLI banner documents `CAPTURE_PATH [WAVE_ID | --bundle]`; `--hidden` is also accepted. Note the CLI uses a different import dispatcher from the GUI and cannot open a `.rocprof-att` manifest (use the desktop app for those); it also computes no project fingerprint and passes no cancellation flag.

## 4. Headless MCP server

```bash
rocprof-studio --headless                 # no window; MCP required
rocprof-studio --headless --mcp-port 8791 --mcp-token-file /secure/token
rocprof-studio --no-mcp                    # desktop GUI with the server disabled
```

A full chapter is in [MCP mode](/mcp).

## CLI flags

The Tauri binary parses these itself (no `clap`). From `--help`:

```
rocprof-studio [--headless] [--mcp-port 8765] [--mcp-token-file PATH] [--no-mcp]
```

| Flag | Default | Meaning |
| --- | --- | --- |
| `--headless` | off | No window; run as an MCP service only (MCP must be enabled). |
| `--mcp-port <PORT>` | `8765` | Loopback port for the MCP HTTP server. |
| `--mcp-token-file <PATH>` | `~/.rocprofgui/mcp/token` | Bearer-token file; auto-created (0600, 32 random hex bytes) if absent. |
| `--no-mcp` | — | Disable the MCP server entirely. |
| `--help` | — | Print the endpoint and bearer-token line. |

::: tip The installed binary's token default is `~/.rocprofgui/mcp/token`
The repository source defaults the token file to `.codex/mcp/token`, but the installed/built binary's `--help` reports `~/.rocprofgui/mcp/token` and relocates the whole data directory with `ROCPROFGUI_HOME`. This site uses the installed binary's behaviour.
:::

## Environment variables

### Application data directory

Everything the app writes for itself lives under `~/.rocprofgui`: the bearer token (`mcp/token`), MCP job output (`mcp/jobs/`), and tuning sessions (`kernel-tuning/sessions/`, `shader-tuning/sessions/`). An analysis job deletes its folder on success and keeps only logs on failure; `profile_run`/`profile_start` output stays until you delete it (it is the capture itself). Nothing is written into the working directory.

| Variable | Effect |
| --- | --- |
| `ROCPROFGUI_HOME` | Relocate the entire data directory (token, jobs, tuning sessions). |
| `ROCPROF_GUI_PYTHON` | Interpreter used to launch the raw-ATT decoder worker (default `python3`). |
| `ROCPROF_GUI_DECODER_LIB` | Path to `librocprof-trace-decoder.so` (V2). Without it the decoder uses ROCm's own discovery. |
| `ROCPROF_GUI_DECODER_PACKAGE` | Path to the decoder Python ZIP; overrides the bundled Tauri resource (useful for the headless importer). A configured-but-missing/malformed ZIP fails explicitly. |
| `ROCPROF_TUNING_ROOT` | Root for kernel-tuning sessions (otherwise under the data dir). |
| `ROCPROF_TUNING_PYTHON` | Interpreter for the tuning worker (default `python3`). |

## Ports used in development

| Port | What | Set by |
| --- | --- | --- |
| 1420 | Browser preview / Tauri dev URL (`strictPort`) | `vite.config.ts`, `tauri.conf.json` |
| 1421 | Playwright UI-test web server (separate from the preview) | `playwright.config.ts` |
| 8765 | Default MCP server | `--mcp-port` |

For an existing Playwright Chromium, set `PLAYWRIGHT_CHROMIUM_EXECUTABLE` to its path.
