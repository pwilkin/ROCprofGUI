# Troubleshooting / FAQ

## The app won't start / "decoder-python.zip" errors

The desktop app resolves the bundled `decoder-python.zip` at startup and aborts if it is missing. Build it before `tauri dev`/`tauri build` (see [Installation](/installation)), and for an unbundled debug build keep the ZIP beside the executable (`target/debug/decoder-python.zip`).

## I only see the synthetic example / I can't open a directory

You are in browser preview (`npm run dev`). It loads the synthetic example and has no filesystem access; directory import and raw-trace import need the desktop app (`npm run tauri dev`). The header shows "Browser preview" vs "Local desktop".

## Raw ATT import fails or times out

Raw ATT needs the `rocprof-trace-decoder` package, its `pyelftools` dependency, and the native `librocprof-trace-decoder.so` (V2). Decoded captures need none of these. Point the app at them with `ROCPROF_GUI_PYTHON`, `ROCPROF_GUI_DECODER_LIB`, `ROCPROF_GUI_DECODER_PACKAGE`. Imports time out after 5 minutes and have a Cancel button; the import also aborts if the decoder log exceeds 16 MiB. If filenames are non-standard, use **ATT setup** to build a `.rocprof-att` manifest. (The headless CLI cannot open manifests; use the desktop app.)

## An MCP client gets 401 / 403 / "Too many connections"

- **401** — missing or wrong `Authorization: Bearer <token>`. Read the token from `~/.rocprofgui/mcp/token` (or your `--mcp-token-file`).
- **403** — a non-loopback `Host`/`Origin`. The server is loopback-only.
- **503 "Too many MCP connections" / "Too many sessions"** — you are over the 32-connection / 16-session caps; close idle connections.
- **"Send initialize and notifications/initialized first"** — the handshake must happen on one persistent connection. Use a real MCP client or the legacy SSE transport (see [MCP walkthrough](/mcp#walkthrough)).

## `capture_analyze` fails

It requires Node.js at runtime (it runs the frontend's analysis bundle as a Node CLI). Install Node 22+.

## Where are my MCP job outputs / tuning sessions?

Under `~/.rocprofgui` (relocate with `ROCPROFGUI_HOME`): `mcp/jobs/` for profiling output (kept until you delete it), `kernel-tuning/sessions/` and `shader-tuning/sessions/` for tuning. Analysis jobs delete their folder on success and keep only logs on failure.

## RADV `.rgp` opens but has no per-instruction data

That is expected with RADV's default token mask — see [RADV / Vulkan profiling](/radv). Capture with `RADV_THREAD_TRACE_INSTRUCTION_TIMING=false` for occupancy, or build RADV with `docs/radv-rocprof-compat.patch` and set `RADV_THREAD_TRACE_ROCPROF_COMPAT=true` for full per-instruction data (GFX11/GFX11.5). GFX12 instruction timing is not enabled by the patch.

## Numbers look too clean / are they real?

If the capture tab is badged **DEMO**, it is the synthetic example, not a measurement. For real captures, remember that coverage cannot be proven from a dispatch CSV alone, and that summed durations and interval union answer different questions ([Limits](/limits), [Export & evidence](/export)).

## Is this a drop-in replacement for ROCprof Compute Viewer?

No. It tracks RCV as a baseline but does not claim full parity and is not a pixel-identical Qt reimplementation. See [Validation & parity](/validation).

## Does it support macOS/Windows?

The project targets Linux (GTK 3 / WebKitGTK 4.1, Debian packaging). Other platforms are not documented.
