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
