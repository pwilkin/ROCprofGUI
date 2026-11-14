import { test, expect } from "@playwright/test";
import { readFileSync } from "node:fs";

// The specs below stub __TAURI_INTERNALS__.invoke, so they verify the bridge ->
// invoke argument mapping but not the Rust registration. This source-level guard
// catches a dropped generate_handler! entry or a bridge rename: the four desktop
// commands must stay invoked by bridge.ts AND listed in main.rs.
test("the four live_profile_* commands stay registered and bridged", () => {
  const commands = [
    "live_profile_start",
    "live_profile_trigger",
    "live_profile_status",
    "live_profile_stop",
  ];
  const bridge = readFileSync(new URL("../src/bridge.ts", import.meta.url), "utf8");
  const mainRs = readFileSync(new URL("../src-tauri/src/main.rs", import.meta.url), "utf8");
  const handler = mainRs.slice(mainRs.indexOf("generate_handler!"));
  for (const command of commands) {
    expect(bridge, `${command} invoked by bridge.ts`).toContain(`"${command}"`);
    expect(handler, `${command} in generate_handler!`).toContain(command);
  }
});

// Drives the desktop "Live profiling" panel through a stubbed Tauri IPC bridge,
// the same convention as the wave-loading and target-mark-reuse specs. It checks
// that the panel calls the live_profile_* commands, passes the rocprofv3 window
// options (kernel filters, streams, timed vs triggered) through verbatim, and
// renders windows, attach attempts, collected files and warnings.
test("live profiling panel drives start, triggered + timed windows, and collect", async ({ page }) => {
  await page.addInitScript(() => {
    const w = window as any;
    w.isTauri = true;
    w.triggerCalls = [];
    let count = 0;
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, payload: any) => {
        if (command === "mcp_status")
          return { active: null, revision: 0, workspace: {}, server: null };
        const args = payload?.args ?? {};
        if (command === "live_profile_start")
          return { id: "live-1", pid: 4242, directory: "/jobs/live-1", stdout: "/jobs/live-1/stdout.log", stderr: "/jobs/live-1/stderr.log" };
        if (command === "live_profile_trigger") {
          w.triggerCalls.push(args);
          if (args.end) return { id: "live-1", ended: count, detached: true, directory: `/jobs/live-1/window-${count}`, warnings: [] };
          count += 1;
          const mode = args.triggered ? "triggered" : "timed";
          return { id: "live-1", window: count, mode, windowMs: args.windowMs ?? null, directory: `/jobs/live-1/window-${count}`, attachAttempts: 1, warnings: [] };
        }
        if (command === "live_profile_status")
          return { id: "live-1", running: true, windowRunning: false, windowTriggered: false, triggerCount: count, directory: "/jobs/live-1" };
        if (command === "live_profile_stop")
          return { status: "stopped", directory: "/jobs/live-1", files: ["/jobs/live-1/window-1/app_kernel_trace.csv"], triggerCount: count, warnings: ["window-2 produced no output files (rocprofv3 requires every window of one process to request the same streams)."] };
        throw Error(command);
      },
    };
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Live profiling", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Live profiling", level: 1 })).toBeVisible();

  // rocprofv3 is the default tracer; fill the application and start.
  await page.getByLabel("Command").fill("/opt/rocm/bin/llama-server --model m.gguf");
  await page.getByLabel("Working directory").fill("/work");
  await page.getByRole("button", { name: "Start profiling", exact: true }).click();
  await expect(page.getByText("Running live-1")).toBeVisible();

  // A timed window carrying a kernel include regex and a trace stream.
  await page.getByLabel("Kernel include regex").fill("mul_mat");
  await page.getByLabel("Traces").fill("kernel, hip");
  await page.getByRole("button", { name: "Trigger window", exact: true }).click();
  await expect(page.getByText("window-1 · timed")).toBeVisible();
  await expect(page.getByText("attach attempts: 1")).toBeVisible();

  const timed = await page.evaluate(() => (window as any).triggerCalls.at(-1));
  expect(timed.kernelIncludeRegex).toBe("mul_mat");
  expect(timed.traces).toEqual(["kernel", "hip"]);
  expect(timed.windowMs).toBe(5000);
  expect(timed.triggered).toBeUndefined();

  // A triggered window stays open until explicitly ended.
  await page.getByLabel("Window mode").selectOption("triggered");
  await page.getByRole("button", { name: "Trigger window", exact: true }).click();
  await expect(page.getByText("window-2 · triggered")).toBeVisible();
  const triggered = await page.evaluate(() => (window as any).triggerCalls.at(-1));
  expect(triggered.triggered).toBe(true);
  expect(triggered.windowMs).toBeUndefined();
  const endButton = page.getByRole("button", { name: "End window", exact: true });
  await expect(endButton).toBeVisible();
  await endButton.click();
  await expect(endButton).toHaveCount(0);
  const ended = await page.evaluate(() => (window as any).triggerCalls.at(-1));
  expect(ended.end).toBe(true);

  // Stop and collect reports files and the per-window warning.
  await page.getByRole("button", { name: "Stop and collect", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Collected", exact: true })).toBeVisible();
  await expect(page.getByText("1 file(s)", { exact: false })).toBeVisible();
  await expect(page.getByRole("alert").filter({ hasText: "same streams" })).toBeVisible();
});

// ATT sub-options and kernelIterationRange map through verbatim; a non-numeric
// window duration is rejected locally instead of silently defaulting to 5000 ms.
test("live profiling maps ATT sub-options and iteration range, and validates windowMs", async ({ page }) => {
  await page.addInitScript(() => {
    const w = window as any;
    w.isTauri = true;
    w.triggerCalls = [];
    let count = 0;
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, payload: any) => {
        if (command === "mcp_status") return { active: null, revision: 0, workspace: {}, server: null };
        const args = payload?.args ?? {};
        if (command === "live_profile_start")
          return { id: "live-1", pid: 1, directory: "/jobs/live-1" };
        if (command === "live_profile_trigger") {
          w.triggerCalls.push(args);
          count += 1;
          return { id: "live-1", window: count, mode: "timed", windowMs: args.windowMs ?? null, directory: `/jobs/live-1/window-${count}`, attachAttempts: 1, warnings: [] };
        }
        throw Error(command);
      },
    };
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Live profiling", exact: true }).click();
  await page.getByLabel("Command").fill("/app --model m.gguf");
  await page.getByLabel("Working directory").fill("/work");
  await page.getByRole("button", { name: "Start profiling", exact: true }).click();
  await expect(page.getByText("Running live-1")).toBeVisible();

  // The number input blocks non-numeric text at the browser level; an
  // out-of-range value (0 < 1) is rejected locally, so no trigger is sent and
  // the Rust range check is no longer unreachable from the UI.
  await page.getByLabel("Window (ms)").fill("0");
  await page.getByRole("button", { name: "Trigger window", exact: true }).click();
  await expect(page.getByRole("alert").filter({ hasText: "Window (ms) must be a whole number between 1 and 600000" })).toBeVisible();
  expect(await page.evaluate(() => (window as any).triggerCalls.length)).toBe(0);

  // Valid duration + ATT sub-options + iteration range all map through.
  await page.getByLabel("Window (ms)").fill("2000");
  await page.getByLabel("ATT thread trace").check();
  await page.getByLabel("ATT target CU").fill("3");
  await page.getByLabel("ATT buffer size (bytes)").fill("536870912");
  await page.getByLabel("Kernel iteration range").fill("0-3");
  await page.getByRole("button", { name: "Trigger window", exact: true }).click();
  await expect(page.getByText("window-1 · timed")).toBeVisible();
  const sent = await page.evaluate(() => (window as any).triggerCalls.at(-1));
  expect(sent.windowMs).toBe(2000);
  expect(sent.att).toEqual({ targetCu: 3, bufferSize: 536870912 });
  expect(sent.kernelIterationRange).toBe("0-3");
});

// The error path renders the rejection, and a status refresh does not wipe it.
test("live profiling surfaces errors and a status refresh keeps them", async ({ page }) => {
  await page.addInitScript(() => {
    const w = window as any;
    w.isTauri = true;
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string) => {
        if (command === "mcp_status") return { active: null, revision: 0, workspace: {}, server: null };
        if (command === "live_profile_start")
          return { id: "live-1", pid: 1, directory: "/jobs/live-1" };
        if (command === "live_profile_trigger")
          throw "A live profile is already running; stop it here first";
        if (command === "live_profile_status")
          return { id: "live-1", running: true, windowRunning: false, triggerCount: 0, directory: "/jobs/live-1" };
        throw Error(command);
      },
    };
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Live profiling", exact: true }).click();
  await page.getByLabel("Command").fill("/app");
  await page.getByLabel("Working directory").fill("/work");
  await page.getByRole("button", { name: "Start profiling", exact: true }).click();
  await expect(page.getByText("Running live-1")).toBeVisible();

  await page.getByRole("button", { name: "Trigger window", exact: true }).click();
  const err = page.getByRole("alert").filter({ hasText: "already running" });
  await expect(err).toBeVisible();
  // A status refresh must not clear the previous error (it uses clearError=false).
  await page.getByRole("button", { name: "Refresh status", exact: true }).click();
  await expect(err).toBeVisible();
});

// The RADV branch shows "Arm RGP trigger" and hides the rocprofv3-only inputs.
test("live profiling RADV branch arms an RGP trigger", async ({ page }) => {
  await page.addInitScript(() => {
    const w = window as any;
    w.isTauri = true;
    w.triggerCalls = [];
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, payload: any) => {
        if (command === "mcp_status") return { active: null, revision: 0, workspace: {}, server: null };
        if (command === "live_profile_start")
          return { id: "live-radv", pid: 2, directory: "/jobs/live-radv", triggerFile: "/jobs/live-radv/radv-trigger" };
        if (command === "live_profile_trigger") {
          w.triggerCalls.push(payload?.args ?? {});
          return { id: "live-radv", triggerFile: "/jobs/live-radv/radv-trigger", triggerCount: 1 };
        }
        throw Error(command);
      },
    };
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Live profiling", exact: true }).click();
  await page.getByLabel("Tracer").selectOption("radv");
  await page.getByLabel("Command").fill("/vk-app");
  await page.getByLabel("Working directory").fill("/work");
  await page.getByRole("button", { name: "Start profiling", exact: true }).click();
  await expect(page.getByText("Running live-radv")).toBeVisible();
  await expect(page.getByLabel("Window mode")).toHaveCount(0);
  await page.getByRole("button", { name: "Arm RGP trigger", exact: true }).click();
  const sent = await page.evaluate(() => (window as any).triggerCalls.at(-1));
  expect(sent).toEqual({ id: "live-radv" });
});

// The hrx branch: its options travel with the start request as the MCP `hrx`
// object, there is nothing to trigger, and stop renders the bundle summary and
// warnings so a device that recorded no dispatch is visible.
test("live profiling hrx branch starts with the hrx object and shows the bundle summary", async ({ page }) => {
  await page.addInitScript(() => {
    const w = window as any;
    w.isTauri = true;
    w.calls = [];
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, payload: any) => {
        if (command === "mcp_status") return { active: null, revision: 0, workspace: {}, server: null };
        const args = payload?.args ?? {};
        w.calls.push({ command, args });
        if (command === "live_profile_start")
          return { id: "live-hrx", pid: 77, directory: "/jobs/live-hrx", bundle: "/jobs/live-hrx/hrx-profile.irpf", captureMode: "whole-run" };
        if (command === "live_profile_status")
          return { id: "live-hrx", running: true, exitCode: null, bundle: "/jobs/live-hrx/hrx-profile.irpf", bundleBytes: 4917850, directory: "/jobs/live-hrx" };
        if (command === "live_profile_stop")
          return {
            status: "stopped",
            exitCode: 0,
            directory: "/jobs/live-hrx",
            files: ["/jobs/live-hrx/hrx-profile.irpf"],
            bundle: {
              bytes: 4917850, finalized: true, sessions: 2, producers: ["amd-gpu", "xdna"], functions: 117,
              dispatchEvents: 21490, queueEvents: 16406, truncated: [], failedSessions: [],
              perProducer: [
                { producer: "amd-gpu", sessions: 1, dispatchEvents: 21490, queueEvents: 16277 },
                { producer: "xdna", sessions: 1, dispatchEvents: 0, queueEvents: 129 },
              ],
            },
            warnings: ["No dispatch events were recorded for device session(s) xdna: work on that device is absent from the dispatch table and from per-kernel time."],
          };
        throw Error(command);
      },
    };
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Live profiling", exact: true }).click();
  await page.getByLabel("Tracer").selectOption("hrx");

  // The limits are stated before anything runs, and the rocprofv3 field is gone.
  const limits = page.getByRole("list", { name: "What the hrx tracer records" });
  await expect(limits).toContainText("no window to trigger");
  await expect(limits).toContainText("No waves, no ISA, no hardware counters");
  await expect(limits).toContainText("XDNA device may record no dispatches");
  await expect(page.getByLabel("rocprofv3 path (optional)")).toHaveCount(0);

  await page.getByLabel("Command").fill("/loom/loom-serve --devices=/i/devices.json --model=/i/deployment.json --listen=127.0.0.1:8093");
  await page.getByLabel("Working directory").fill("/work");

  // An out-of-range flush interval is rejected locally: nothing is started.
  await page.getByLabel("Flush interval (ms)").fill("60001");
  await page.getByRole("button", { name: "Start profiling", exact: true }).click();
  await expect(page.getByRole("alert").filter({ hasText: "hrx flushIntervalMs must be an integer between 0 and 60000" })).toBeVisible();
  expect(await page.evaluate(() => (window as any).calls.length)).toBe(0);

  await page.getByLabel("Flush interval (ms)").fill("10");
  await page.getByLabel("Data families").selectOption("custom");
  await page.getByLabel("queue-events", { exact: true }).uncheck();
  await page.getByLabel("Other families").fill("host-execution");
  await page.getByLabel("Function filter (optional glob)").fill("g_gemm_*");
  await page.getByLabel("Allow partial families per device").check();
  await page.getByRole("button", { name: "Start profiling", exact: true }).click();
  await expect(page.getByText("Running live-hrx")).toBeVisible();
  const started = await page.evaluate(() => (window as any).calls.at(-1));
  expect(started.command).toBe("live_profile_start");
  expect(started.args).toEqual({
    argv: ["/loom/loom-serve", "--devices=/i/devices.json", "--model=/i/deployment.json", "--listen=127.0.0.1:8093"],
    cwd: "/work",
    tracer: "hrx",
    hrx: { families: ["dispatch-events", "executable-metadata", "host-execution"], flushIntervalMs: 10, filterFunction: "g_gemm_*", allowPartial: true },
  });

  // Whole-run recording: no trigger of either kind, no rocprofv3 window fields.
  await expect(page.getByText("/jobs/live-hrx/hrx-profile.irpf")).toBeVisible();
  await expect(page.getByRole("button", { name: "Trigger window" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Arm RGP trigger" })).toHaveCount(0);
  await expect(page.getByLabel("Window mode")).toHaveCount(0);
  await page.getByRole("button", { name: "Refresh status", exact: true }).click();
  await expect(page.getByText("Recording the whole run · bundle 4,917,850 bytes so far")).toBeVisible();

  await page.getByRole("button", { name: "Stop and collect", exact: true }).click();
  const summary = page.getByRole("list", { name: "Profile bundle summary" });
  await expect(summary).toContainText("/jobs/live-hrx/hrx-profile.irpf · 4,917,850 bytes · finalized");
  await expect(summary).toContainText("Dispatch events: 21,490");
  await expect(summary).toContainText("xdna: no dispatch events, 129 queue events");
  await expect(summary).toContainText("Waves, ISA and hardware counters: none");
  await expect(page.getByRole("alert").filter({ hasText: "device session(s) xdna" })).toBeVisible();
  expect(await page.evaluate(() => (window as any).calls.map((c: any) => c.command))).toEqual([
    "live_profile_start",
    "live_profile_status",
    "live_profile_stop",
  ]);
});

// With no option touched the request carries an empty hrx object (the core's
// defaults), and a bundle that was never written is stated, not shown as empty.
test("live profiling hrx defaults send an empty hrx object and report a missing bundle", async ({ page }) => {
  await page.addInitScript(() => {
    const w = window as any;
    w.isTauri = true;
    w.calls = [];
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, payload: any) => {
        if (command === "mcp_status") return { active: null, revision: 0, workspace: {}, server: null };
        w.calls.push({ command, args: payload?.args ?? {} });
        if (command === "live_profile_start")
          return { id: "live-hrx", pid: 78, directory: "/jobs/live-hrx", bundle: "/jobs/live-hrx/hrx-profile.irpf", captureMode: "whole-run" };
        if (command === "live_profile_stop")
          return { status: "exited", exitCode: 1, directory: "/jobs/live-hrx", files: [], bundle: null, warnings: ["A device rejected a requested data family and the target aborted profiling for the whole device group."] };
        throw Error(command);
      },
    };
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Live profiling", exact: true }).click();
  await page.getByLabel("Tracer").selectOption("hrx");
  await page.getByLabel("Command").fill("/loom/loom-serve --listen=127.0.0.1:8093");
  await page.getByLabel("Working directory").fill("/work");
  await page.getByRole("button", { name: "Start profiling", exact: true }).click();
  await expect(page.getByText("Running live-hrx")).toBeVisible();
  const started = await page.evaluate(() => (window as any).calls.at(-1));
  expect(started.args).toEqual({ argv: ["/loom/loom-serve", "--listen=127.0.0.1:8093"], cwd: "/work", tracer: "hrx", hrx: {} });

  await page.getByRole("button", { name: "Stop and collect", exact: true }).click();
  await expect(page.getByRole("list", { name: "Profile bundle summary" })).toContainText("No profile bundle was written.");
  await expect(page.getByText("0 file(s)", { exact: false })).toBeVisible();
  await expect(page.getByRole("alert").filter({ hasText: "rejected a requested data family" })).toBeVisible();
});

// Outside the desktop app the panel explains it is desktop-only and runs no IPC.
test("live profiling is desktop-only in the browser preview", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Live profiling", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Live profiling", level: 1 })).toBeVisible();
  await expect(page.getByText("runs only in the desktop app")).toBeVisible();
  await expect(page.getByRole("button", { name: "Start profiling" })).toHaveCount(0);
});
