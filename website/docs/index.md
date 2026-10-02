---
layout: home
title: rocprof studio
hero:
  name: rocprof studio
  text: See what your GPU actually did
  tagline: A Rust + Tauri desktop app and headless core for reading rocprofv3, ROCPD, ATT and RADV captures — with an MCP server and kernel/shader tuning when you ask for them.
  actions:
    - theme: brand
      text: Get started
      link: /installation
    - theme: alt
      text: MCP mode
      link: /mcp
---

<script setup>
import { withBase } from 'vitepress'

const cards = [
  {
    link: '/views#trace-workspace',
    img: '/img/view-trace.png',
    pos: '18% 30%',
    title: 'Timeline → wave → instruction → source',
    line: 'Follow a wave from hardware execution to the ISA that caused it and back to the source line.',
    alt: 'The trace workspace timeline (synthetic example)',
  },
  {
    link: '/views#instruction-hotspots',
    img: '/img/view-hotspots.png',
    pos: '50% 40%',
    title: 'Instruction hotspots',
    line: 'Which instructions dominated, by hits and by attributed cost.',
    alt: 'Instruction hotspot histograms (synthetic example)',
  },
  {
    link: '/views#occupancy',
    img: '/img/view-occupancy.png',
    pos: '50% 35%',
    title: 'Occupancy',
    line: 'Per-SE and per-kernel wave residency over time, exportable as SVG.',
    alt: 'Occupancy plots (synthetic example)',
  },
  {
    link: '/views#source-flamegraph',
    img: '/img/view-flame.png',
    pos: '50% 45%',
    title: 'Source flamegraph',
    line: 'ISA cost rolled up through the source hierarchy.',
    alt: 'Source-to-ISA flamegraph (synthetic example)',
  },
  {
    link: '/views#counters',
    img: '/img/view-counters.png',
    pos: '50% 30%',
    title: 'Counters',
    line: 'Scalar expressions and named-axis tensor definitions over recorded series.',
    alt: 'Counter plots and expressions (synthetic example)',
  },
  {
    link: '/mcp',
    img: '/img/mcp-connection.png',
    pos: '0% 20%',
    title: 'Automatable over MCP',
    line: 'A loopback, bearer-token server with 31 tools for import, analysis, profiling and tuning.',
    alt: 'The MCP connection panel',
  },
  {
    link: '/kernel-tuning',
    img: '/img/kernel-tuning.png',
    pos: '30% 15%',
    title: 'Kernel tuning',
    line: 'Capture a kernel’s inputs, replay it in isolation, compare variants. Opt-in, needs a GPU.',
    alt: 'The Kernel laboratory window',
  },
  {
    link: '/ui-tour#the-trace-timeline',
    img: '/img/native-timeline-detail.png',
    pos: '50% 55%',
    title: 'Detail on your terms',
    line: 'Exact events, upstream mip reduction or pixel aggregation — never silent sampling.',
    alt: 'Timeline detail aggregation menu (desktop app)',
  },
]
</script>

<div class="rps-frame-wrap">
  <div class="rps-frame">
    <div class="rps-frame-bar"><span></span><span></span><span></span><i>rocprof studio — trace workspace (bundled synthetic example)</i></div>
    <img :src="withBase('/img/workspace.png')" alt="The rocprof studio trace workspace showing the bundled synthetic example" />
  </div>
</div>

<div class="rps-built">
  <span>Built with</span>
  <span class="rps-pill">Rust + Tauri 2</span>
  <span class="rps-pill">ROCm · rocprofv3</span>
  <span class="rps-pill">MCP</span>
</div>

<div class="rps-grid">
  <a v-for="c in cards" :key="c.link" class="rps-card" :href="withBase(c.link)">
    <div class="rps-shot">
      <img :src="withBase(c.img)" :alt="c.alt" :style="{ objectPosition: c.pos }" />
    </div>
    <h3>{{ c.title }}</h3>
    <p>{{ c.line }}</p>
  </a>
</div>

::: warning These screenshots use the bundled synthetic example
Most screenshots on this site were captured from `public/demo.json`, a synthetic trace shipped for exploring the interface. The app labels it *"Example capture · Synthetic trace for exploring the interface. These are not GPU measurements."* Treat every number in those images as illustrative, never as a hardware measurement. Screenshots taken from real recorded captures (ROCPD, ATT) are captioned accordingly.
:::

## Read this first

rocprof studio (package `rocprof-studio`, version 0.2.0, badged PREVIEW in the app) says no more than it can back up. Three things first:

- **Importing a capture is read-only.** The import path never writes into your capture directories or databases.
- **Two surfaces are not read-only, and only run when you invoke them.** The MCP tool `profile_run` (and `profile_start`) launches a program under a profiler; the kernel- and shader-tuning workers compile candidate variants and can write replacement source back to disk. See [MCP mode](/mcp), [Kernel tuning](/kernel-tuning) and [Shader tuning](/shader-tuning).
- **This is not full ROCprof Compute Viewer parity.** The project tracks [ROCprof Compute Viewer](/validation) as its baseline and is not complete against it. It is also not a pixel-identical Qt reimplementation, and it has no LLM source-iteration feature despite the "Kernel experiments" placeholder in the sidebar.

## What it is

It inspects ROCm GPU profiling captures. A single Rust analysis core (`crates/profile-core`) does the importing and analysis; a Tauri 2 shell (`src-tauri`) wraps it with a React/TypeScript UI (`src`), and the same core also runs as a headless importer CLI and behind the MCP server. You point it at the artifacts a profiler already produced and it reconstructs the timeline, the per-wave execution, the ISA, the source linkage, occupancy, counters and sampling, then lets you export evidence about what you saw.

It reads decoded rocprofv3 directories, kernel-trace and counter CSVs, SPM and PC-sampling JSON, ROCPD/legacy databases, raw ATT and RADV `.rgp` — all through one analysis core shared by the GUI and the headless CLI. Synthetic values are labelled, "not collected" is shown distinctly from an observed zero, and summed-duration vs interval-union accounting is kept separate.

If you are new, start with [Installation](/installation), then [Running & modes](/running), [Input formats](/input-formats) and the [UI tour](/ui-tour). If you came for automation, go straight to [MCP mode](/mcp).
