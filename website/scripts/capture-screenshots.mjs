// Re-runnable documentation screenshot capture.
//
// Drives the rocprof-studio frontend in a headless browser against the Vite
// "browser preview" (the non-Tauri fallback in src/bridge.ts, which loads the
// bundled synthetic example public/demo.json). No display server and no GPU
// are required. For the MCP control surface and the kernel-tuning window it
// injects the same Tauri IPC stub the existing Playwright suite uses
// (window.isTauri + window.__TAURI_INTERNALS__.invoke), returning canned data.
//
// Everything produced from the synthetic demo is captioned as such on the site;
// nothing captured here is a hardware measurement.
//
// Usage (from repo root):
//   node website/scripts/capture-screenshots.mjs [--port 1526] [--keep-server]
// It starts its own Vite dev server, captures into website/docs/public/img/, then
// tears the server down. Requires the repo's node_modules (playwright) and an
// installed Playwright Chromium (~/.cache/ms-playwright or
// PLAYWRIGHT_CHROMIUM_EXECUTABLE).

import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { chromium } from "playwright";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(__dirname, "..", "..");
const OUT = path.join(REPO, "website", "docs", "public", "img");

const argPort = process.argv.indexOf("--port");
const PORT = argPort !== -1 ? Number(process.argv[argPort + 1]) : 1526;
const BASE = `http://127.0.0.1:${PORT}`;
const VIEWPORT = { width: 1500, height: 1050 };

function waitForServer(url, tries = 60) {
  return new Promise((resolve, reject) => {
    const attempt = (n) => {
      fetch(url)
        .then((r) => (r.ok ? resolve() : retry(n)))
        .catch(() => retry(n));
    };
    const retry = (n) =>
      n <= 0 ? reject(new Error("dev server did not start")) : setTimeout(() => attempt(n - 1), 500);
    attempt(tries);
  });
}

// The 12 sidebar views, captured from the synthetic demo in browser preview.
const VIEWS = [
  ["reference", "ISA reference", "view-reference"],
  ["trace", "Trace workspace", "view-trace"],
  ["hardware", "Compute fabric", "view-hardware"],
  ["summary", "Activity summary", "view-summary"],
  ["sampling", "PC sampling", "view-sampling"],
  ["occupancy", "Occupancy", "view-occupancy"],
  ["dispatch", "Kernel dispatches", "view-dispatch"],
  ["hotspots", "Instruction hotspots", "view-hotspots"],
  ["flame", "Source flamegraph", "view-flame"],
  ["markers", "Marker scopes", "view-markers"],
  ["counters", "Counters", "view-counters"],
  ["evidence", "Capture evidence", "view-evidence"],
];

async function main() {
  const server = spawn("npm", ["run", "dev", "--", "--port", String(PORT), "--strictPort"], {
    cwd: REPO,
    stdio: "inherit",
  });
  const shutdown = () => {
    if (!process.argv.includes("--keep-server")) server.kill("SIGTERM");
  };
  process.on("exit", shutdown);

  try {
    await waitForServer(BASE + "/");
    const browser = await chromium.launch({
      executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE || undefined,
    });

    // --- Browser-preview views (synthetic demo, no stub) ---
    const ctx = await browser.newContext({ viewport: VIEWPORT, deviceScaleFactor: 1 });
    const page = await ctx.newPage();
    await page.goto(BASE + "/", { waitUntil: "networkidle" });
    await page.getByRole("button", { name: "Trace workspace", exact: true }).waitFor();

    for (const [, label, file] of VIEWS) {
      await page.getByRole("button", { name: new RegExp("^" + label) }).click();
      await page.waitForTimeout(900);
      await page.screenshot({ path: path.join(OUT, file + ".png") });
      console.log("captured", file);
    }

    // Appearance: light theme on the trace workspace.
    await page.getByRole("button", { name: "Trace workspace", exact: true }).click();
    await page.getByLabel("Color theme").selectOption("light");
    await page.waitForTimeout(700);
    await page.screenshot({ path: path.join(OUT, "appearance-light.png") });
    console.log("captured appearance-light");
    await ctx.close();

    // --- MCP control surface (Tauri IPC stub) ---
    const mcpStub = (active) => `
      window.isTauri = true;
      window.__TAURI_INTERNALS__ = {
        invoke: async (cmd) => {
          if (cmd === "mcp_status") return {
            active: ${active ? `{ requestId: 7, tool: "capture_analyze", cancelling: false, elapsedSeconds: 3 }` : "null"},
            revision: 1, workspace: {},
            server: { url: "http://127.0.0.1:8765/mcp", sse: "http://127.0.0.1:8765/sse", tokenFile: "~/.rocprofgui/mcp/token" }
          };
          throw new Error("stubbed: " + cmd);
        }
      };`;

    for (const [active, file, expand] of [
      [false, "mcp-connection", true],
      [true, "mcp-lock", false],
    ]) {
      const c = await browser.newContext({ viewport: VIEWPORT, deviceScaleFactor: 1 });
      const p = await c.newPage();
      await p.addInitScript(mcpStub(active));
      await p.goto(BASE + "/", { waitUntil: "networkidle" });
      await p.waitForTimeout(1200);
      if (expand) {
        const summary = p.getByText("MCP server", { exact: true });
        if (await summary.count()) await summary.first().click();
        await p.waitForTimeout(300);
      }
      await p.screenshot({ path: path.join(OUT, file + ".png") });
      console.log("captured", file);
      await c.close();
    }

    // --- Kernel tuning window (?tuning=, Tauri IPC stub) ---
    const tuningStub = `
      window.isTauri = true;
      window.__TAURI_INTERNALS__ = {
        invoke: async (cmd) => {
          if (cmd === "mcp_status") return { active: null, revision: 0, workspace: {}, server: null };
          if (cmd === "tuning_status") return {
            running: false,
            runs: [],
            session: {
              phase: "configured",
              kernel: "transpose_tiled<float, 32>",
              directory: "~/.rocprofgui/kernel-tuning/sessions/demo",
              traceDispatch: { id: "7", grid: "256x256x1", workgroup: "32x32x1" },
              config: {
                sourceTree: "/devel/tools/llama.cpp",
                cwd: "/devel/tools/llama.cpp",
                argv: ["/devel/tools/llama.cpp/build/bin/llama-bench", "-p", "32", "-n", "0"],
                occurrence: 0,
                captureScope: "reachable",
                captureLibrary: "runtime/lib/libkerncap.so",
                replay: "runtime/bin/kerncap-replay",
                profiler: "/opt/rocm/bin/rocprofv3",
                objdump: "/opt/rocm/llvm/bin/llvm-objdump",
                readobj: "/opt/rocm/llvm/bin/llvm-readobj",
                compile: { argv: [], cwd: "/devel/tools/llama.cpp" }
              }
            }
          };
          throw new Error("stubbed: " + cmd);
        }
      };`;
    const tc = await browser.newContext({ viewport: VIEWPORT, deviceScaleFactor: 1 });
    const tp = await tc.newPage();
    await tp.addInitScript(tuningStub);
    const errs = [];
    tp.on("pageerror", (e) => errs.push(String(e)));
    await tp.goto(BASE + "/?tuning=demo", { waitUntil: "networkidle" });
    await tp.waitForTimeout(1200);
    await tp.screenshot({ path: path.join(OUT, "kernel-tuning.png") });
    console.log("captured kernel-tuning (stub)", errs.length ? "WITH pageerror: " + errs[0] : "");
    await tc.close();

    await browser.close();
  } finally {
    shutdown();
  }
}

main().then(
  () => process.exit(0),
  (e) => {
    console.error(e);
    process.exit(1);
  },
);
