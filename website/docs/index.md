---
layout: home
title: rocprof studio
hero:
  name: rocprof studio
  text: Inspect ROCm GPU profiling captures
  tagline: A Rust + Tauri desktop app and headless core for reading rocprofv3, ROCPD, ATT and RADV captures — with an MCP server and kernel/shader tuning when you ask for them.
  image:
    src: /img/workspace.png
    alt: The rocprof studio trace workspace showing the bundled synthetic example
  actions:
    - theme: brand
      text: Get started
      link: /installation
    - theme: alt
      text: MCP mode
      link: /mcp
    - theme: alt
      text: Views reference
      link: /views
features:
  - title: Read what the profiler produced
    details: Opens decoded rocprofv3 directories, kernel-trace and counter CSVs, SPM and PC-sampling JSON, ROCPD/legacy databases, raw ATT and RADV .rgp — all in one analysis core shared by the GUI and a headless CLI.
  - title: Timeline → wave → instruction → source
    details: Follow a wave from hardware execution to the ISA that caused it and back to the source line, with occupancy, counters, hotspots, flamegraphs, PC sampling and marker scopes.
  - title: Honest about coverage
    details: Synthetic values are labelled, "not collected" is shown distinctly from an observed zero, and summed-duration vs interval-union accounting is kept separate. Nothing claims parity it does not have.
  - title: Automatable over MCP
    details: A loopback, bearer-token MCP server (on by default) exposes 31 tools for import, analysis, evidence, profiling and tuning — drivable from Claude Code, Claude Desktop or any MCP client.
---

## Read this first

rocprof studio (package `rocprof-studio`, version **0.1.0**, badged **PREVIEW** in the app) is deliberately careful about what it claims. Three things are worth stating up front:

- **Importing a capture is read-only.** The import path never writes into your capture directories or databases.
- **Two surfaces are *not* read-only, and only run when you invoke them.** The MCP tool `profile_run` (and `profile_start`) launches a program under a profiler; the kernel- and shader-**tuning** workers compile candidate variants and can write replacement source back to disk. See [MCP mode](/mcp), [Kernel tuning](/kernel-tuning) and [Shader tuning](/shader-tuning).
- **This is not full ROCprof Compute Viewer parity.** The project tracks [ROCprof Compute Viewer](/validation) as its baseline and is *not* complete against it. It is also explicitly **not** a pixel-identical Qt reimplementation, and it has no LLM source-iteration feature despite the "Kernel experiments" placeholder in the sidebar.

::: warning These screenshots use the bundled synthetic example
Most screenshots on this site were captured from `public/demo.json`, a **synthetic trace shipped for exploring the interface**. The app labels it *"Example capture · Synthetic trace for exploring the interface. These are not GPU measurements."* Treat every number in those images as illustrative, never as a hardware measurement. Screenshots taken from real recorded captures (ROCPD, ATT) are captioned accordingly.
:::

## What it is

It inspects **ROCm GPU profiling captures**. A single Rust analysis core (`crates/profile-core`) does the importing and analysis; a Tauri 2 shell (`src-tauri`) wraps it with a React/TypeScript UI (`src`), and the same core also runs as a headless importer CLI and behind the MCP server. You point it at the artifacts a profiler already produced and it reconstructs the timeline, the per-wave execution, the ISA, the source linkage, occupancy, counters and sampling — then lets you export evidence about what you saw.

If you are new, read [Installation](/installation) → [Running & modes](/running) → [Input formats](/input-formats) → [UI tour](/ui-tour). If you came for automation, go straight to [MCP mode](/mcp).
