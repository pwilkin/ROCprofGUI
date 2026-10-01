# Architecture

A short map. The authoritative, line-anchored design document is `docs/architecture.md` in the repository; this page summarises the shape and points at where behaviour actually lives.

## Three layers, one direction of dependency

```
src/ (React + TypeScript, ~124 modules)      the UI and the pure analysis modules
  │  Tauri IPC (invoke) — the single seam is src/bridge.ts
  ▼
src-tauri/ (Tauri 2 shell: main.rs, mcp.rs, tuning.rs)   commands, MCP server, tuning
  │  depends on ↓, never the reverse
  ▼
crates/profile-core/ (Rust: import + analysis)    the one analysis core
```

- **`crates/profile-core`** does all importing and analysis (≈45 files). It also builds the **headless importer CLI** (`cargo run -p profile-core -- …`) and backs the MCP server. The GUI and the CLI use *different* import dispatchers (`project::load_input_options` vs `import::load_capture_options`); only the GUI path accepts `.rocprof-att`.
- **`src-tauri`** registers **46 Tauri commands** (`generate_handler!` at `main.rs:1563` — 39 in `main.rs`, 3 in `mcp.rs`, 4 in `tuning.rs`), hosts the [MCP server](/mcp), and owns the [kernel-tuning](/kernel-tuning) subsystem. Its capability set is deliberately tiny: `core:default` + `dialog:allow-open`, with a strict CSP and only the dialog plugin. Every file save/pick goes through Rust.
- **`src`** is React with **no store library** — plain hooks in one large `App` component, four Contexts, a `useRef` per-capture "shadow store", and `window` CustomEvents for cross-surface IPC. `src/bridge.ts` is the single IPC seam and the only place the browser-preview fallback is decided. The frontend's **pure `.ts` modules carry the analysis math and have sibling Vitest tests**; the `.tsx` components are thin and are covered through Playwright.

## Two things worth knowing

- **The frontend analysis modules are shared with the backend.** `build.rs` bundles several `src/*` modules into a Node CLI (`mcp-analysis.cjs`) so the MCP `capture_analyze` tool runs the *same* code as the webview. That is why **Node.js is both a build-time and (for that one tool) a runtime requirement**, and why editing `src/` triggers a Rust rebuild.
- **The decoder ZIP is resolved at startup.** The desktop app loads `decoder-python.zip` from Tauri's resource directory during `.setup(...)`; a missing resource **aborts startup**. See [Installation](/installation).

## Paging and budgets

Large captures are transferred to the UI in **pages** (catalog/observation pages, 4096 rows / 4 MiB each) and bounded wave queries load each selected wave fully before filtering by the viewport. The app pages timeline lanes and ISA rows; it is **not** an indexed out-of-core viewer, though an on-disk token index exists and is preferred when available. All the caps are in [Limits & caveats](/limits).

## Where behaviour lives (quick index)

| You want to change… | Look at |
| --- | --- |
| How a format is imported | `crates/profile-core/src/import.rs` (and `project.rs` for the GUI path) |
| A Tauri command | `src-tauri/src/main.rs`, registered at `main.rs:1563` |
| The MCP tool surface | `src-tauri/src/mcp.rs` (`fn tools()`), and [MCP mode](/mcp) |
| Kernel tuning | `src-tauri/src/tuning.rs`, `tuning_worker.py` |
| A view or analysis | a pure `src/<name>.ts` module (+ its `.test.ts`) and a `src/components/*.tsx` panel |
