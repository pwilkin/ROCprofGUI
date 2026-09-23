import { test, expect } from "@playwright/test";

test("Vulkan shader session edits recovered sources, sends only changed files, and shows validated runs", async ({
  page,
}) => {
  await page.addInitScript(() => {
    const w = window as any;
    w.requests = [];
    const source = "/llama.cpp/ggml/src/ggml-vulkan/vulkan-shaders/mul_mm.comp";
    const funcs = "/llama.cpp/ggml/src/ggml-vulkan/vulkan-shaders/mul_mm_funcs.glsl";
    const metric = (value: number, unit: string, delta?: number) => ({
      value,
      unit,
      reason: null,
      ...(delta === undefined ? {} : { delta, deltaPercent: (100 * delta) / (value - delta) }),
    });
    const session = {
      mode: "vulkan",
      kernel: "matmul_iq4_nl_f16_f16acc_aligned_2",
      directory: "/sessions/1",
      traceDispatch: { id: "0:74", grid: "16384 × 2 × 1", workgroup: "256 × 1 × 1" },
      config: {
        sourceTree: "/llama.cpp",
        cwd: "/llama.cpp/build/bin",
        argv: ["llama-bench", "-p", "512"],
        env: { GGML_VK_DEBUG_MARKERS: "1" },
        occurrence: 0,
        layer: "/runtime/lib/VkLayer_rocprof_capture.json",
        replay: "/runtime/bin/vkreplay",
      },
      captured: "/sessions/1/capture",
      dispatch: {
        name: "matmul_iq4_nl_f16_f16acc_aligned_2",
        labels: ["MUL_MAT node_13", "matmul_iq4_nl_f16_f16acc_aligned_2 (64,2,1)"],
        groups: [64, 2, 1],
        device: { name: "AMD Radeon 8060S Graphics (RADV STRIX_HALO)" },
        pushHex: "00010000",
        specValues: [{ id: 0, value: 256, hex: "00010000" }],
        bindings: [{ set: 0, binding: 2, group: 2, offset: 0, range: 253755392 }],
        buffers: [{ group: 0, bytes: 1, writtenBytes: 0 }, { group: 1, bytes: 1, writtenBytes: 0 }, { group: 2, bytes: 253755392, writtenBytes: 7340032 }],
        unknownFeatures: [],
      },
      recipe: {
        variant: "matmul_iq4_nl_f16_f16acc_cm1",
        source,
        glslc: "/sdk/bin/glslc",
        argv: ["-fshader-stage=compute", source, "-DDATA_A_IQ4_NL=1"],
        runtimePatch: { added: [[1, 2], [3, 4]], removed: [] },
      },
      files: [
        { path: funcs, hash: "a", code: "void load_a() {}\n" },
        { path: source, hash: "b", code: "layout(constant_id = 12) const uint SHMEM_STRIDE_PAD = 4;\n" },
      ],
      isa: "v_wmma_f16_16x16x16_f16 v[0:7], v[8:15], v[16:23], v[0:7]",
      runs: [
        { number: 1, variant: false, iterations: 50, directory: "/r1", validation: { status: "bitwise match" }, notes: [],
          metrics: { "GPU duration": metric(961.3, "µs"), VGPRs: metric(192, "registers/lane") } },
        { number: 2, variant: true, iterations: 50, directory: "/r2", edits: [source], trace: "/r2/trace.rgp",
          conditions: { mode: "sustained", memory: "auto", flushMb: 0, repeats: 3 }, counters: true, comparedWith: null,
          driver: { version: "26.2.0", deviceType: "integrated", device: "AMD Radeon 8060S Graphics", memory: "gtt" },
          validation: { status: "bitwise match", sustainedMatch: false },
          notes: ["The edited source compiles to the same SPIR-V as the captured shader: the edit does not change this variant."],
          metrics: { "GPU duration": metric(695.3, "µs"), VGPRs: metric(120, "registers/lane"), "L2 hit rate": metric(49.1, "%"),
            Clock: metric(2450, "MHz"), "Replay spread": metric(31.5, "%") } },
      ],
    };
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, args: any) => {
        if (command === "tuning_status") return { running: false, error: null, log: "glslc ok", session };
        if (command === "tuning_start") {
          w.requests.push(args.request);
          return null;
        }
        throw Error(command);
      },
    };
  });
  await page.goto("/?tuning=vulkan");

  await expect(page.getByText("Shader laboratory")).toBeVisible();
  await expect(page.getByText("DISPATCH CAPTURED")).toBeVisible();
  await expect(page.getByLabel("Shader ISA")).toContainText("v_wmma_f16_16x16x16_f16");
  // The recipe's own source opens first; every file the compile reads is a tab.
  const editor = page.getByRole("textbox", { name: "Shader source editor" });
  await expect(editor).toHaveValue(/SHMEM_STRIDE_PAD = 4/);
  await expect(page.getByRole("tab")).toHaveText(["mul_mm_funcs.glsl", "mul_mm.comp"]);
  await expect(page.getByRole("button", { name: "Save edited files" })).toBeDisabled();

  // Latest run: validation, the no-op warning, the trace path and deltas.
  await expect(page.getByRole("status").first()).toContainText("bitwise match");
  await expect(page.getByRole("note").first()).toContainText("does not change this variant");
  await expect(page.getByText("/r2/trace.rgp")).toBeVisible();
  // The run says how it was replayed, on what, and why it has no deltas.
  await expect(page.getByText(/back to back after one restore · buffers in system memory \(GTT\) · 3 replays with fresh allocations/)).toBeVisible();
  await expect(page.getByText("no earlier run under these conditions to compare with", { exact: false })).toBeVisible();
  await expect(page.getByText(/driver 26\.2\.0/)).toBeVisible();
  await expect(page.getByRole("status").first()).toContainText("Two back-to-back dispatches: outputs differ");
  await expect(page.locator(".tuning-metrics h3")).toHaveText(["GPU duration", "Replay spread", "Clock", "L2 hit rate", "VGPRs"]);
  await expect(page.getByRole("option", { name: /Run 2 .* sustained auto ×3/ })).toBeAttached();

  await editor.fill("layout(constant_id = 12) const uint SHMEM_STRIDE_PAD = 2;\n");
  await expect(page.getByRole("tab", { name: "mul_mm.comp •" })).toBeVisible();
  await page.getByLabel("Trace the run (RADV thread trace)").check();
  await page.getByRole("button", { name: "Application-like" }).click();
  await expect(page.getByLabel("Replay mode")).toHaveValue("sustained");
  await expect(page.getByLabel("Replays with fresh allocations")).toHaveValue("3");
  await page.getByRole("button", { name: "Build & run edited shader" }).click();
  await expect.poll(() => page.evaluate(() => (window as any).requests.length)).toBe(1);
  const run = await page.evaluate(() => (window as any).requests[0]);
  expect(run).toMatchObject({ op: "run", variant: true, iterations: 20, validate: true, trace: true, autoCounters: true,
    conditions: { mode: "sustained", memory: "auto", flushMb: 0, repeats: 3 } });
  // Only the changed file is sent.
  expect(Object.keys(run.edits)).toEqual(["/llama.cpp/ggml/src/ggml-vulkan/vulkan-shaders/mul_mm.comp"]);
  expect(run.config.env).toEqual({ GGML_VK_DEBUG_MARKERS: "1" });

  await page.getByRole("button", { name: "Save edited files" }).click();
  await expect.poll(() => page.evaluate(() => (window as any).requests.length)).toBe(2);
  expect(await page.evaluate(() => (window as any).requests[1].op)).toBe("save");

  // Invalid environment JSON is refused before anything runs. Setup is folded
  // away once the dispatch is captured.
  await page.getByText("1 · Application and capture").click();
  await page.getByLabel("Application environment").fill("[1]");
  await page.getByRole("button", { name: "Run captured baseline" }).click();
  await expect(page.getByRole("alert")).toContainText("JSON object");
  expect(await page.evaluate(() => (window as any).requests.length)).toBe(2);
});
