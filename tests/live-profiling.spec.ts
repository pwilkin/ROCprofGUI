import { test, expect } from "@playwright/test";

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

// Outside the desktop app the panel explains it is desktop-only and runs no IPC.
test("live profiling is desktop-only in the browser preview", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Live profiling", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Live profiling", level: 1 })).toBeVisible();
  await expect(page.getByText("runs only in the desktop app")).toBeVisible();
  await expect(page.getByRole("button", { name: "Start profiling" })).toHaveCount(0);
});
