import { test, expect } from "@playwright/test";

test("kernel editor preserves edits, sends isolated runs, validates JSON, and shows real metric deltas", async ({
  page,
}) => {
  await page.addInitScript(() => {
    const w = window as any;
    w.requests = [];
    const config = {
      sourceTree: "/source",
      cwd: "/source",
      argv: ["llama-bench", "-hf", "unsloth/Qwen3.5-9B-GGUF:Q8_0"],
      occurrence: 2,
      captureLibrary: "/libkerncap.so",
      replay: "/kerncap-replay",
      profiler: "/rocprofv3",
      objdump: "/objdump",
      readobj: "/readobj",
      compile: { argv: ["clang", "-x", "hip", "kernel.cu"], cwd: "/build" },
    };
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, args: any) => {
        if (command === "tuning_status")
          return {
            running: false,
            error: null,
            log: "Captured real inputs",
            session: {
              calibration: {
                createdAt: 1700000000,
                fingerprint: "calibration-a",
                reused: true,
                path: "/calibration.json",
                device: { name: "Test GPU", arch: "gfx1201" },
                peaks: {
                  fp32: {
                    rate: 20,
                    unit: "TFLOP/s",
                    samples: 9,
                    coefficientOfVariation: 0.02,
                    quality: "stable",
                    method: "measured",
                  },
                },
                errors: {},
              },
              kernel: "scale_f32(float*)",
              directory: "/session",
              traceDispatch: {
                id: "trace-id",
                grid: "256 × 1 × 1",
                workgroup: "64 × 1 × 1",
              },
              config,
              source: { path: "/source/kernel.cu", line: 5 },
              code: "__global__ void scale_f32(float* p) { p[0] *= 2; }",
              isa: "v_mul_f32 v0, v0, 2",
              captured: "/snapshot",
              runs: [
                {
                  number: 2,
                  variant: true,
                  iterations: 20,
                  validation: { status: "bitwise match" },
                  metrics: {
                    "Compute speed of light": {
                      value: 10,
                      unit: "%",
                      method: "derived",
                      reason: "Of calibrated fp32 peak",
                      basis: { calibration: "calibration-a" },
                    },
                    "GPU duration": {
                      value: 4,
                      unit: "µs",
                      delta: -1,
                      deltaPercent: -20,
                    },
                    Occupancy: { value: 8, unit: "waves/CU" },
                    "LDS bank conflicts": {
                      value: null,
                      unit: "%",
                      reason: "No supported counter",
                    },
                  },
                },
              ],
            },
          };
        if (command === "tuning_start") {
          w.requests.push(args.request);
          return null;
        }
        if (command === "tuning_cancel") return null;
        throw Error(command);
      },
    };
  });
  await page.goto("/?tuning=test");
  await expect(
    page.getByLabel("Calibrate and characterize automatically"),
  ).toBeChecked();
  await expect(page.getByLabel("GPU calibration")).toContainText("Test GPU");
  await page.getByText("Calibrated peaks, variability and coverage").click();
  await expect(page.getByLabel("GPU calibration")).toContainText("20 TFLOP/s");
  await expect(page.getByLabel("GPU calibration")).toContainText("2%");
  await expect(page.locator(".tuning-metrics")).toContainText(
    "Compute · % of calibrated peak",
  );
  const editor = page.getByRole("textbox", { name: "Kernel source editor" });
  await expect(editor).toContainText("scale_f32");
  await editor.fill("__global__ void scale_f32(float* p) { p[0] *= 3; }");
  await expect(page.getByLabel("Kernel ISA disassembly")).toContainText(
    "v_mul_f32",
  );
  await expect(page.getByRole("status")).toContainText("bitwise match");
  await expect(page.locator(".tuning-metrics")).toContainText("Δ -1 µs (-20%)");
  await page
    .getByText("Build context, manual overrides & counter mappings")
    .click();
  await expect(
    page.getByLabel("Original compiler command · JSON argument array"),
  ).toHaveValue('["clang","-x","hip","kernel.cu"]');
  await page.getByLabel("Measured counters · JSON mappings").fill("{bad");
  await page.getByRole("button", { name: "Build & run edited kernel" }).click();
  await expect(page.getByRole("alert")).toBeVisible();
  expect(await page.evaluate(() => (window as any).requests)).toEqual([]);
  await page.getByLabel("Measured counters · JSON mappings").fill("[]");
  await page.getByRole("button", { name: "Build & run edited kernel" }).click();
  await expect
    .poll(() => page.evaluate(() => (window as any).requests.length))
    .toBe(1);
  expect(await page.evaluate(() => (window as any).requests[0])).toMatchObject({
    op: "run",
    variant: true,
    validate: true,
    autoCounters: true,
    autoCalibrate: true,
    code: "__global__ void scale_f32(float* p) { p[0] *= 3; }",
    config: {
      compile: { argv: ["clang", "-x", "hip", "kernel.cu"], cwd: "/build" },
    },
  });
  await expect(
    page.getByRole("button", { name: "Save to original" }),
  ).toBeEnabled();
  await page.getByRole("button", { name: "Save to original" }).click();
  await expect
    .poll(() => page.evaluate(() => (window as any).requests.length))
    .toBe(2);
  expect(await page.evaluate(() => (window as any).requests[1].op)).toBe(
    "save",
  );
  await expect(
    page.getByRole("button", { name: "Recalibrate GPU" }),
  ).toBeEnabled();
  await page.getByRole("button", { name: "Recalibrate GPU" }).click();
  await expect
    .poll(() => page.evaluate(() => (window as any).requests.length))
    .toBe(3);
  expect(await page.evaluate(() => (window as any).requests[2].op)).toBe(
    "calibrate",
  );
  await page.getByLabel("Calibrate and characterize automatically").uncheck();
  await expect(
    page.getByRole("button", { name: "Run captured baseline" }),
  ).toBeEnabled();
  await page.getByRole("button", { name: "Run captured baseline" }).click();
  await expect
    .poll(() => page.evaluate(() => (window as any).requests.length))
    .toBe(4);
  expect(
    await page.evaluate(() => (window as any).requests[3].autoCalibrate),
  ).toBe(false);
  await page.screenshot({
    path: ".codex/calibration/editor.png",
    fullPage: true,
  });
});
