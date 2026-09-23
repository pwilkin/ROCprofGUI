import { test, expect } from "@playwright/test";
test("linked trace workspace, source, occupancy, hardware and expressions", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "Trace workspace", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByText("Synthetic trace for exploring the interface.", {
      exact: false,
    }),
  ).toBeVisible();
  await expect(page.locator("canvas")).toBeVisible();
  await page.getByRole("button", { name: "CU waves", exact: true }).click();
  await expect(page.locator(".plot-footer").first()).toContainText("12 lanes");
  await page.getByLabel("Latency scope").selectOption("all");
  await expect(
    page.locator("tbody tr").filter({ hasText: "s_load_dwordx4" }),
  ).toContainText("36");
  await page.getByRole("button", { name: "Target wave", exact: true }).click();
  await page.getByLabel("Search ISA").fill("ds_read");
  await expect(page.locator("tbody tr")).toHaveCount(1);
  await page.locator("tbody tr").click();
  await expect(page.locator(".source-selected")).toContainText("transposed");
  await page.getByLabel("Search ISA").fill("");
  await page.getByLabel("Loop occurrence").fill("1");
  await page
    .getByRole("button", { name: "Compute fabric", exact: true })
    .click();
  await expect(
    page.getByRole("heading", {
      name: "Compute fabric",
      exact: true,
      level: 1,
    }),
  ).toBeVisible();
  await expect(page.getByText("Bank conflicts", { exact: true })).toBeVisible();
  await page
    .getByRole("button", { name: "CU 3", exact: false })
    .first()
    .click();
  await expect(
    page.getByRole("heading", { name: "CU 3 · SIMD residency" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Occupancy", exact: true }).click();
  await expect(
    page.getByRole("img", { name: "Shader engine 0 occupancy" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  await page.getByRole("button", { name: "Add counter", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Derived ratio", exact: true }),
  ).toBeVisible();
  await page.getByLabel("Counter expression").fill("C0 / 0");
  await page.getByRole("button", { name: "Add counter", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("Division by zero");
  await page
    .getByRole("button", { name: "Source flamegraph", exact: true })
    .click();
  await expect(page.locator(".flame-root")).toContainText(
    "All mapped source lines",
  );
  await page.locator(".flames > div > button").first().click();
  await expect(page.getByRole("button", { name: "Reset zoom" })).toBeEnabled();
  await page
    .getByRole("button", { name: "Capture evidence", exact: false })
    .click();
  await expect(
    page.getByText(
      "Unknown — no independent replay coverage evidence attached",
    ),
  ).toBeVisible();
  expect(errors).toEqual([]);
});
test("timeline zoom, scope selection, export and preview import guidance", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.locator("canvas")).toBeVisible();
  const before = await page.getByLabel("Range end").inputValue();
  await page.locator("canvas").hover();
  await page.mouse.wheel(0, -200);
  await expect(page.getByLabel("Range end")).not.toHaveValue(before);
  await page.getByRole("button", { name: "Reset visible range" }).click();
  await page.getByLabel("Latency scope").selectOption("wave");
  const download = page.waitForEvent("download");
  await page.getByRole("button", { name: "Export selection" }).click();
  expect((await download).suggestedFilename()).toBe("rocprof-selection.json");
  await page.getByRole("button", { name: "Open capture", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("Rust desktop app");
});
test("renders desktop-sized workspace without horizontal page overflow", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.locator("canvas")).toBeVisible();
  await page.screenshot({
    path: test.info().outputPath("workspace.png"),
    fullPage: true,
  });
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= window.innerWidth,
    ),
  ).toBe(true);
  await page
    .getByRole("button", { name: "Compute fabric", exact: true })
    .click();
  await page.screenshot({
    path: test.info().outputPath("compute-fabric.png"),
    fullPage: true,
  });
});

test("hidden latency, folding, histogram selection and complete navigation history", async ({
  page,
}) => {
  await page.goto("/");
  await expect(
    page.getByRole("button", { name: "Recompute hidden latency" }),
  ).toBeVisible();
  const waitRow = page
    .locator("tbody tr")
    .filter({ hasText: "s_waitcnt vmcnt(0)" });
  const total = await waitRow.locator("td").nth(4).textContent();
  await page.getByLabel("Latency contribution").selectOption("nonhidden");
  await expect(waitRow.locator("td").nth(4)).not.toHaveText(total!);
  await page
    .getByRole("button", { name: "Collapse ISA section 0", exact: true })
    .click();
  await expect(page.locator("tbody tr")).toHaveCount(1);
  await page.getByRole("button", { name: "Expand all ISA sections" }).click();
  await expect(page.locator("tbody tr")).toHaveCount(12);
  await page.getByLabel("Search ISA").fill("ds_read");
  await page.locator("tbody tr").click();
  await page.getByLabel("Search ISA").fill("");
  await page.getByLabel("Loop occurrence").fill("1");
  await page.getByLabel("Range start").fill("1200");
  const rangeBefore = await page.getByLabel("Range start").inputValue();
  await page
    .locator("tbody tr")
    .filter({ hasText: "s_waitcnt vmcnt(0)" })
    .click();
  await page.getByRole("button", { name: "Previous selection" }).click();
  await expect(page.getByLabel("Loop occurrence")).toHaveValue("1");
  await expect(page.getByLabel("Range start")).toHaveValue(rangeBefore);
  await expect(page.locator("tbody tr.selected")).toContainText("ds_read");
  await page
    .getByRole("button", { name: "Instruction hotspots", exact: true })
    .click();
  await page.getByLabel("Histogram range start").fill("0");
  await page.getByLabel("Histogram range end").fill("200");
  await page.getByLabel("Histogram range end").press("Enter");
  await page.locator(".histogram button:not([disabled])").first().click();
  await expect(
    page.getByRole("heading", { name: "Trace workspace", exact: true }),
  ).toBeVisible();
  await expect(page.locator("tbody tr.selected")).toHaveCount(1);
});

test("wait dependencies use ISA identities across loop occurrences", async ({
  page,
}) => {
  await page.goto("/");
  await page
    .locator("tbody tr")
    .filter({ hasText: "s_waitcnt vmcnt(0)" })
    .click();
  const dependency = page.getByRole("button", { name: "ISA 3 → ISA 4" });
  await expect(dependency).toContainText("global_load_dword");
  await expect(
    page.getByRole("img", {
      name: "Memory instructions linked to selected wait instruction",
    }),
  ).toBeVisible();
  await page.getByLabel("Loop occurrence").fill("2");
  await expect(dependency).toBeVisible();
  await dependency.click();
  await expect(page.locator("tbody tr.selected")).toContainText(
    "global_load_dword",
  );
});

test("dimensioned counter definitions reduce hardware axes and persist formulas", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  const formula =
    "Temp := sum[SQ_INSTS_VALU, axis=[XCC,SE,CU]]\nAverage := mean[Temp, axis=Time]";
  await page.getByLabel("Counter definitions").fill(formula);
  await page.getByRole("button", { name: "Evaluate definitions" }).click();
  await expect(page.locator(".tensor-result")).toHaveCount(2);
  await expect(page.locator(".tensor-result output")).not.toHaveText(
    "No valid samples",
  );
  await page.screenshot({
    path: test.info().outputPath("counter-expressions.png"),
    fullPage: true,
  });
  await page.reload();
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  await expect(page.getByLabel("Counter definitions")).toHaveValue(formula);
  await page.getByLabel("Counter definitions").fill("A := B\nB := A");
  await page.getByRole("button", { name: "Evaluate definitions" }).click();
  await expect(page.getByRole("alert")).toContainText("Circular definition");
});

test("ROCPD dispatch and sampled-counter data renders without detailed wave assumptions", async ({
  page,
}) => {
  const { readFileSync } = await import("node:fs");
  const capture = JSON.parse(
    readFileSync(
      new URL("../fixtures/rocpd/sdk-3.0.4.json", import.meta.url),
      "utf8",
    ),
  );
  await page.route("**/demo.json", (route) =>
    route.fulfill({ json: { capture, waves: {} } }),
  );
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto("/");
  await page
    .getByRole("button", { name: "Kernel dispatches", exact: true })
    .click();
  await expect(page.locator("tbody tr")).toHaveCount(1);
  await expect(
    page.locator("tbody tr").first().locator("td").nth(1),
  ).toHaveText("2");
  await expect(page.locator("tbody tr").first()).toContainText("myKernel");
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  await expect(page.getByText("2 raw series")).toBeVisible();
  await page
    .getByLabel("Counter definitions")
    .fill("Total := sum[SQ_WAVES,axis=[XCC,SE,CU]]");
  await page.getByRole("button", { name: "Evaluate definitions" }).click();
  await expect(page.locator(".tensor-result output")).toHaveText("32");
  expect(errors).toEqual([]);
});

test("instrumentation markers preserve nesting, flame widths and source navigation", async ({
  page,
}) => {
  await page.goto("/");
  await page
    .getByRole("button", { name: "Marker scopes", exact: true })
    .click();
  await expect(
    page.getByRole("img", { name: "Nested marker duration flamegraph" }),
  ).toBeVisible();
  await expect(page.locator("tbody tr")).toHaveCount(4);
  await page.getByRole("button", { name: "Load tile", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Load tile", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Zoom to marker" }).click();
  await expect(page.getByLabel("Range start", { exact: true })).toHaveValue(
    "700",
  );
  await expect(page.getByLabel("Range end", { exact: true })).toHaveValue(
    "1250",
  );
  await page.getByRole("button", { name: "Go to marker source" }).click();
  await expect(
    page.getByRole("heading", { name: "Trace workspace", exact: true }),
  ).toBeVisible();
  await expect(page.locator("tbody tr.selected")).toContainText(
    "global_load_dword",
  );
});

test("offline ISA reference resolves trace instructions and architecture-specific matrix types", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto("/");
  await page.getByLabel("Search ISA").fill("ds_read");
  await page.locator("tbody tr").click();
  await page
    .getByRole("button", { name: "Instruction reference", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "DS_LOAD_B32", exact: true }),
  ).toBeVisible();
  await page
    .getByLabel("Search instruction reference")
    .fill("v_wmma_f32_16x16x16_f16");
  await page.locator(".isa-list button").click();
  await expect(page.locator(".isa-matrix-summary")).toContainText("F16 → F32");
  await page.getByLabel("Reference architecture").selectOption("rdna2");
  await expect(
    page.getByText("No matching instruction in AMD RDNA 2.", { exact: false }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Matrix data types", exact: true })
    .click();
  const rdna4 = page
    .locator(".isa-matrix tbody tr")
    .filter({ hasText: "AMD RDNA 4" })
    .filter({ hasText: "Dense" });
  await expect(rdna4).toContainText("FP8_BF8 → F32");
  await expect(
    page.locator(".isa-matrix tbody tr").filter({ hasText: "AMD RDNA 1" }),
  ).toContainText("No WMMA / MFMA");
  await rdna4.locator("summary").click();
  await rdna4
    .getByRole("button", { name: "V_WMMA_F32_16X16X16_FP8_BF8", exact: false })
    .click();
  await expect(
    page.getByRole("heading", {
      name: "V_WMMA_F32_16X16X16_FP8_BF8",
      exact: true,
    }),
  ).toBeVisible();
  await expect(page.getByLabel("Reference architecture")).toHaveValue("rdna4");
  await page.screenshot({
    path: test.info().outputPath("isa-reference.png"),
    fullPage: true,
  });
  await page.getByLabel("Reference architecture").selectOption("cdna1");
  await page
    .getByLabel("Search instruction reference")
    .fill("v_mfma_f32_16x16x16_f16");
  await page.locator(".isa-list button").click();
  await expect(page.locator(".isa-matrix-summary")).toContainText("MFMA");
  expect(errors).toEqual([]);
});

test("ISA documentation remains available without an open capture or external network", async ({
  page,
}) => {
  await page.route("**/*", (route) => {
    const url = new URL(route.request().url());
    return url.hostname === "127.0.0.1" ? route.continue() : route.abort();
  });
  await page.goto("/");
  await page
    .getByRole("button", { name: "Close LDS transpose · example capture" })
    .click();
  await page
    .getByRole("button", { name: "ISA reference", exact: true })
    .click();
  await page.getByLabel("Reference architecture").selectOption("cdna4");
  await page
    .getByLabel("Search instruction reference")
    .fill("v_mfma_scale_f32_16x16x128_f8f6f4");
  await page.locator(".isa-list button").click();
  await expect(page.locator(".isa-matrix-summary")).toContainText("MFMA SCALE");
  await expect(
    page.getByText("CBSZ selects A and BLGP selects B independently.", {
      exact: false,
    }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Matrix data types", exact: true })
    .click();
  await expect(
    page
      .locator(".isa-matrix tbody tr")
      .filter({ hasText: "AMD CDNA 4" })
      .filter({ hasText: "MFMA SCALE" }),
  ).toContainText("F8F6F4 → F32");
  await page.screenshot({
    path: test.info().outputPath("isa-matrix-types.png"),
    fullPage: true,
  });
});

test("definition files import, persist drafts, evaluate and export unchanged text", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  const contents =
    "# custom counter file\r\nTotal := sum[SQ_INSTS_VALU, axis=[XCC,SE,CU]]\r\n";
  await page.getByLabel("Import definition files").setInputFiles({
    name: "custom.def",
    mimeType: "text/plain",
    buffer: Buffer.from(contents),
  });
  await expect(page.getByRole("status")).toContainText("Imported 1");
  await page.screenshot({
    path: test.info().outputPath("counter-definition-files.png"),
    fullPage: true,
  });
  await expect(page.getByLabel("Definition file", { exact: true })).toHaveCount(
    1,
  );
  await expect(page.getByLabel("Counter definitions")).toHaveValue(
    contents.replaceAll("\r\n", "\n"),
  );
  await page
    .getByRole("button", { name: "Evaluate definitions", exact: true })
    .click();
  await expect(page.locator(".tensor-result")).toHaveCount(1);
  await page.getByLabel("Rename definition file").fill("renamed.def");
  await page.getByRole("button", { name: "Rename file", exact: true }).click();
  const downloadPromise = page.waitForEvent("download");
  await page
    .getByRole("button", { name: "Export definitions", exact: true })
    .click();
  const download = await downloadPromise;
  expect(download.suggestedFilename()).toBe("renamed.def");
  const { readFile } = await import("node:fs/promises");
  expect(await readFile((await download.path())!, "utf8")).toBe(contents);
  await page.reload();
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  await expect(
    page
      .getByLabel("Definition file", { exact: true })
      .locator("option:checked"),
  ).toHaveText("renamed.def");
  await page
    .getByRole("button", { name: "New definition file", exact: true })
    .click();
  await page.getByLabel("Counter definitions").fill("Draft := 17");
  await page
    .getByRole("button", { name: "Trace workspace", exact: true })
    .click();
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  await expect(page.getByLabel("Counter definitions")).toHaveValue(
    "Draft := 17",
  );
  await page.getByLabel("Import definition files").setInputFiles({
    name: "invalid.def",
    mimeType: "text/plain",
    buffer: Buffer.from([255, 0]),
  });
  await expect(page.getByRole("alert")).toBeVisible();
  await expect(page.getByLabel("Counter definitions")).toHaveValue(
    "Draft := 17",
  );
  await page.getByRole("button", { name: "Remove draft", exact: true }).click();
  await expect(
    page.getByLabel("Definition file", { exact: true }).locator("option"),
  ).toHaveCount(2);
});

test("switching from a wave capture to native ROCPD never requests the previous capture wave", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  const capture = JSON.parse(
    await readFile("fixtures/rocpd/sdk-3.0.4.json", "utf8"),
  );
  await page.addInitScript((capture) => {
    const w = window as any;
    w.isTauri = true;
    w.waveRequests = [];
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, args: any) => {
        if (command === "import_source_file")
          return {
            name: "mapped.hip",
            text: Array.from({ length: 20 }, () => "mapped native source").join(
              "\n",
            ),
          };
        if (command === "plugin:dialog|open") return "/fixture.rocpd";
        if (command === "open_capture")
          return { ...capture, id: "native-rocpd" };
        if (command === "read_wave") {
          w.waveRequests.push(args);
          throw Error("Wave is not part of this capture");
        }
        throw Error(`Unexpected IPC: ${command}`);
      },
    };
  }, capture);
  await page.goto("/");
  await expect(page.locator("canvas")).toBeVisible();
  await page
    .getByRole("button", { name: "Map source file", exact: true })
    .click();
  await expect(page.locator(".source-selected")).toContainText(
    "mapped native source",
  );
  await page
    .getByRole("button", { name: "Open capture file", exact: true })
    .click();
  await expect(
    page.getByText("This capture has no detailed wave tokens.", {
      exact: false,
    }),
  ).toBeVisible();
  await expect(page.locator(".error-banner")).toHaveCount(0);
  expect(await page.evaluate(() => (window as any).waveRequests)).toEqual([]);
  await page
    .getByRole("button", { name: "Kernel dispatches", exact: true })
    .click();
  await expect(page.locator("tbody tr")).toHaveCount(1);
  await page
    .getByRole("button", {
      name: "LDS transpose · example capture DEMO",
      exact: true,
    })
    .click();
  await expect(page.locator("canvas")).toBeVisible();
  await expect(page.locator(".source-selected")).toContainText(
    "mapped native source",
  );
  expect(await page.evaluate(() => (window as any).waveRequests)).toEqual([]);
});

test("counter workers can be cancelled, discarded on edits, and replaced without stale results", async ({
  page,
}) => {
  await page.addInitScript(() => {
    const NativeWorker = window.Worker;
    (window as any).terminatedCounterWorkers = 0;
    window.Worker = class extends NativeWorker {
      terminate() {
        (window as any).terminatedCounterWorkers++;
        super.terminate();
      }
    };
  });
  await page.route("**/counter.worker.ts*", (route) =>
    route.fulfill({
      contentType: "text/javascript",
      body: 'self.onmessage = () => { const until = Date.now() + 5000; while (Date.now() < until) {} self.postMessage({error: "stale worker result"}); };',
    }),
  );
  await page.goto("/");
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  await page
    .getByRole("button", { name: "Evaluate definitions", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Cancel evaluation", exact: true })
    .click();
  await expect(page.getByRole("alert")).toContainText("cancelled");
  expect(
    await page.evaluate(() => (window as any).terminatedCounterWorkers),
  ).toBe(1);
  await page
    .getByRole("button", { name: "Evaluate definitions", exact: true })
    .click();
  await page.getByLabel("Counter definitions").fill("Total := 17");
  await expect(
    page.getByRole("button", { name: "Cancel evaluation", exact: true }),
  ).toHaveCount(0);
  expect(
    await page.evaluate(() => (window as any).terminatedCounterWorkers),
  ).toBe(2);
  await page
    .getByRole("button", { name: "Evaluate definitions", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Trace workspace", exact: true })
    .click();
  expect(
    await page.evaluate(() => (window as any).terminatedCounterWorkers),
  ).toBe(3);
  await page.unroute("**/counter.worker.ts*");
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  await page
    .getByRole("button", { name: "Evaluate definitions", exact: true })
    .click();
  await expect(page.locator(".tensor-result output")).toHaveText("17");
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("architecture summary distinguishes unsupported hardware and measured activity", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto("/");
  await page
    .getByRole("button", { name: "Activity summary", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Hardware summary unavailable" }),
  ).toBeVisible();
  await expect(
    page.getByText("gfx1151 has no validated summary model.", { exact: false }),
  ).toBeVisible();
  let completeCosts = true;
  await page.route("**/demo.json", async (route) => {
    const response = await route.fetch();
    const bundle = await response.json();
    bundle.capture.architecture = "gfx942";
    bundle.capture.metadata.instruction_costs = completeCosts
      ? "complete"
      : "incomplete";
    bundle.capture.metadata.gfxip = "9";
    bundle.capture.counters = [
      ["BUSY_CU_CYCLES", 10, 30],
      ["ACTIVE_INST_VALU", 20, 60],
      ["ACTIVE_INST_LDS", 5, 15],
      ["VALU_MFMA_BUSY_CYCLES", 20, 60],
    ].flatMap(([name, a, b]) =>
      Array.from({ length: 35 }, (_, n) => ({
        name: `SQ_${name}`,
        unit: "raw count",
        scope: `SE ${Math.floor(n / 16)} · CU ${n % 16} · bank 0`,
        points: [
          [0, a],
          [64, b],
        ],
      })),
    );
    await route.fulfill({ json: bundle });
  });
  await page.reload();
  await page
    .getByRole("button", { name: "Activity summary", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Hardware summary unavailable" }),
  ).toHaveCount(0);
  await expect(
    page.locator(".summary-category").filter({ hasText: "VALU" }),
  ).toContainText("200%");
  await expect(
    page.locator(".summary-category").filter({ hasText: "MFMA" }),
  ).toContainText("50%");
  await expect(page.locator(".summary-table tbody tr")).toHaveCount(33);
  await page.getByRole("button", { name: "Next CUs" }).click();
  await expect(page.locator(".summary-table tbody tr")).toHaveCount(4);
  await expect(page.locator(".summary-table")).toContainText("SE 2 · CU 2");
  await page.getByRole("button", { name: "Show accumulated counters" }).click();
  await expect(
    page.getByRole("columnheader", {
      name: "VALU_MFMA_BUSY_CYCLES",
      exact: true,
    }),
  ).toBeVisible();
  await expect(
    page.getByText("Upstream-compatible accumulated counters:", {
      exact: false,
    }),
  ).toBeVisible();
  await page
    .getByText("Counter formulas and collection", { exact: true })
    .click();
  await expect(
    page.getByText("Different counter banks may sample different periods.", {
      exact: false,
    }),
  ).toBeVisible();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= window.innerWidth,
    ),
  ).toBe(true);
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({
    path: test.info().outputPath("activity-summary.png"),
    fullPage: false,
  });
  completeCosts = false;
  await page.reload();
  await page
    .getByRole("button", { name: "Activity summary", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Instruction costs unavailable" }),
  ).toBeVisible();
  await expect(page.locator(".summary-stack")).toHaveCount(0);
  expect(errors).toEqual([]);
});

test("PC sampling preserves missing measurements, reason identities and ISA navigation", async ({
  page,
}) => {
  let overflow = false;
  await page.route("**/demo.json", async (route) => {
    const bundle = await (await route.fetch()).json();
    bundle.capture.waves = [];
    bundle.capture.instructions = bundle.capture.instructions
      .slice(0, 3)
      .map((i: object, n: number) => ({
        ...i,
        pcIssued: overflow ? Number.MAX_SAFE_INTEGER : n === 0 ? 0 : 2,
        pcStalled: n === 0 ? 10 : null,
        stallReasons: n === 0 ? [0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 7] : null,
      }));
    await route.fulfill({ json: bundle });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "PC sampling", exact: true }).click();
  await expect(
    page.getByText("Unknown category 10", { exact: false }),
  ).toBeVisible();
  await expect(
    page.getByText("1 / 3 rows reported", { exact: true }),
  ).toBeVisible();
  await expect(page.getByLabel("Range start")).toHaveCount(0);
  await page
    .getByRole("button", { name: "Reasons for line 1", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Stall reasons unavailable" }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "All sampled instructions", exact: true })
    .click();
  await expect(
    page.getByText("Unknown category 10", { exact: false }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Show sample flamegraph" }).click();
  await expect(page.locator(".flame-root")).toContainText("10 samples");
  await page.getByLabel("Sample flamegraph weight").selectOption("pcIssued");
  await expect(page.locator(".flame-root")).toContainText("4 samples");
  await page.getByRole("button", { name: "Open ISA 1", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Trace workspace", exact: true }),
  ).toBeVisible();
  await expect(page.locator(".isa-table tr.selected")).toContainText(
    "s_load_dwordx4",
  );
  await page.getByRole("button", { name: "PC sampling", exact: true }).click();
  await page
    .getByLabel("Search sampled instructions")
    .fill("no-matching-instruction");
  await expect(
    page.getByText("No sampled instructions match this search."),
  ).toBeVisible();
  overflow = true;
  await page.reload();
  await page.getByRole("button", { name: "PC sampling", exact: true }).click();
  await page.getByRole("button", { name: "Show sample flamegraph" }).click();
  await page.getByLabel("Sample flamegraph weight").selectOption("pcIssued");
  await expect(
    page.getByRole("heading", {
      name: "Sample total exceeds exact integer range",
    }),
  ).toBeVisible();
});

test("source remapping preserves capture identity and renders bounded line pages", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const bundle = await (await route.fetch()).json();
    bundle.capture.instructions[1].source = "/old/build/kernel.hip:501";
    bundle.capture.sources = {};
    await route.fulfill({ json: bundle });
  });
  await page.goto("/");
  await expect(
    page.getByText("No captured source for this instruction.", {
      exact: false,
    }),
  ).toBeVisible();
  const content = Array.from(
    { length: 850 },
    (_, n) => `source line ${n + 1}`,
  ).join("\n");
  await page.getByLabel("Source replacement file").setInputFiles({
    name: "local.hip",
    mimeType: "text/plain",
    buffer: Buffer.from(content),
  });
  await expect(page.locator(".source-selected .source-line-number")).toHaveText(
    "501",
  );
  await expect(page.locator(".source-selected code")).toHaveText(
    "source line 501",
  );
  await expect(page.locator(".source > button")).toHaveCount(200);
  await page.getByRole("button", { name: "Next source lines" }).click();
  await expect(page.getByText("Lines 601–800 of 850")).toBeVisible();
  await page
    .getByRole("button", { name: "Selected source line", exact: true })
    .click();
  await expect(page.locator(".source-selected")).toContainText("501");
  await page
    .getByRole("button", { name: "Activity summary", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Trace workspace", exact: true })
    .click();
  await expect(page.locator(".source-selected")).toContainText(
    "source line 501",
  );
  await page.getByLabel("Source replacement file").setInputFiles({
    name: "invalid.hip",
    mimeType: "text/plain",
    buffer: Buffer.from("bad\0text"),
  });
  await expect(page.getByRole("alert")).toContainText("NUL");
  await expect(page.locator(".source-selected")).toContainText(
    "source line 501",
  );
  await page.getByRole("button", { name: "Restore captured source" }).click();
  await expect(
    page.getByText("No captured source for this instruction.", {
      exact: false,
    }),
  ).toBeVisible();
});

test("lazy source requests ignore obsolete selections and surface read errors", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  const bundle = JSON.parse(await readFile("public/demo.json", "utf8"));
  const capture = {
    ...bundle.capture,
    id: "lazy-source",
    name: "Lazy source test",
    waves: [],
    sources: {},
    sourceFiles: ["/a.hip", "/b.hip"],
  };
  capture.instructions[1].source = "/a.hip:2";
  capture.instructions[2].source = "/b.hip:2";
  await page.addInitScript((capture) => {
    const w = window as any;
    w.isTauri = true;
    w.sourceReads = [];
    w.sourcePending = {};
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, args: any) => {
        if (command === "plugin:dialog|open") return "/capture";
        if (command === "open_capture") return capture;
        if (command === "read_source_page") {
          w.sourceReads.push(args.original);
          return new Promise(
            (resolve, reject) =>
              (w.sourcePending[args.original] = { resolve, reject }),
          );
        }
        throw Error(command);
      },
    };
  }, capture);
  await page.goto("/");
  await page
    .getByRole("button", { name: "Open capture file", exact: true })
    .click();
  await expect(page.getByText("Loading captured source…")).toBeVisible();
  await page.getByLabel("Search ISA").fill("v_mad_u32_u24");
  await page.locator(".isa-table tbody tr").click();
  await expect
    .poll(() => page.evaluate(() => (window as any).sourceReads))
    .toEqual(["/a.hip", "/b.hip"]);
  await page.evaluate(() =>
    (window as any).sourcePending["/b.hip"].resolve({
      startLine: 1,
      totalLines: 2,
      lines: ["b line 1", "selected B"],
    }),
  );
  await expect(page.locator(".source-selected")).toContainText("selected B");
  await page.evaluate(() =>
    (window as any).sourcePending["/a.hip"].resolve({
      startLine: 1,
      totalLines: 2,
      lines: ["a line 1", "obsolete A"],
    }),
  );
  await expect(page.locator(".source-selected")).toContainText("selected B");
  await page.getByLabel("Search ISA").fill("s_load_dwordx4");
  await page.locator(".isa-table tbody tr").click();
  await expect
    .poll(() => page.evaluate(() => (window as any).sourceReads.length))
    .toBe(3);
  await page.evaluate(() =>
    (window as any).sourcePending["/a.hip"].reject("Source file exceeds 2 MiB"),
  );
  await expect(page.getByRole("alert")).toContainText("2 MiB");
  await expect(page.locator(".source-selected")).toHaveCount(0);
  await page.getByRole("button", { name: "Retry captured source" }).click();
  await expect
    .poll(() => page.evaluate(() => (window as any).sourceReads.length))
    .toBe(4);
  await page.evaluate(() =>
    (window as any).sourcePending["/a.hip"].resolve({
      startLine: 1,
      totalLines: 2,
      lines: ["a line 1", "retried A"],
    }),
  );
  await expect(page.locator(".source-selected")).toContainText("retried A");
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("same-clock token stacks select visible bars and preserve loop occurrence", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const b = await (await route.fetch()).json();
    b.capture.start = 0;
    b.capture.end = 1000;
    const id = b.capture.waves[0].id;
    b.capture.waves = [{ ...b.capture.waves[0], start: 0, end: 1000 }];
    b.waves[id] = {
      ...b.waves[id],
      start: 0,
      end: 1000,
      tokens: [
        { start: 100, duration: 200, stall: 0, category: 6, instruction: 1 },
        { start: 100, duration: 200, stall: 0, category: 6, instruction: 2 },
        { start: 100, duration: 200, stall: 0, category: 6, instruction: 3 },
        { start: 400, duration: 100, stall: 0, category: 6, instruction: 1 },
      ],
    };
    await route.fulfill({ json: b });
  });
  await page.goto("/");
  const canvas = page.locator("canvas");
  await expect(page.getByLabel("Token layout")).toHaveValue("stacked");
  const box = await canvas.boundingBox();
  if (!box) throw Error("Missing timeline");
  const x = 155 + 0.2 * (box.width - 169);
  await canvas.click({ position: { x, y: 47 } });
  await expect(page.locator(".isa-table tr.selected td").first()).toHaveText(
    "1",
  );
  await canvas.click({ position: { x, y: 42 } });
  await expect(page.locator(".isa-table tr.selected td").first()).toHaveText(
    "2",
  );
  await canvas.click({ position: { x, y: 35 } });
  await expect(page.locator(".isa-table tr.selected td").first()).toHaveText(
    "3",
  );
  await canvas.click({
    position: { x: 155 + 0.45 * (box.width - 169), y: 40 },
  });
  await expect(page.getByLabel("Loop occurrence")).toHaveValue("1");
  await page.getByLabel("Token layout").selectOption("flat");
  await canvas.click({ position: { x, y: 47 } });
  await expect(page.locator(".isa-table tr.selected td").first()).toHaveText(
    "3",
  );
});

test("architecture issue projection retains full stalls and updates LDS routing", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const b = await (await route.fetch()).json();
    b.capture.architecture = "gfx942";
    b.capture.start = 0;
    b.capture.end = 1000;
    const id = b.capture.waves[0].id;
    b.capture.waves = [{ ...b.capture.waves[0], start: 0, end: 1000 }];
    b.waves[id] = {
      ...b.waves[id],
      start: 0,
      end: 1000,
      tokens: [
        { start: 100, duration: 200, stall: 200, category: 6, instruction: 1 },
        { start: 400, duration: 100, stall: 0, category: 9, instruction: 2 },
        { start: 600, duration: 100, stall: 0, category: 3, instruction: 3 },
        { start: 800, duration: 100, stall: 0, category: 5, instruction: 4 },
      ],
    };
    await route.fulfill({ json: b });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Utilization", exact: true }).click();
  await expect(
    page.getByText("minimum issue quantum 4 shader clocks", { exact: false }),
  ).toBeVisible();
  await expect(page.locator(".plot-footer").first()).toContainText("3 lanes");
  const canvas = page.locator("canvas");
  const box = await canvas.boundingBox();
  if (!box) throw Error("Missing utilization timeline");
  const x = (fraction: number) => 155 + fraction * (box.width - 169);
  await canvas.hover({ position: { x: x(0.298), y: 40 } });
  await expect(page.locator(".plot-footer").first()).toContainText(
    "ISA 1 · stall 200 · 4",
  );
  await canvas.hover({ position: { x: x(0.498), y: 68 } });
  await expect(page.locator(".plot-footer").first()).toContainText(
    "IMMED · ISA 2 · stall 0 · 4",
  );
  await canvas.click({ position: { x: x(0.498), y: 68 } });
  await expect(page.locator(".isa-table tr.selected td").first()).toHaveText(
    "2",
  );
  await page.getByLabel("Separate LDS pipe").check();
  await expect(page.locator(".plot-footer").first()).toContainText("4 lanes");
  await page.getByLabel("Separate LDS pipe").uncheck();
  await expect(page.locator(".plot-footer").first()).toContainText("3 lanes");
});

test("unknown architectures do not silently inherit an issue clock model", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const b = await (await route.fetch()).json();
    b.capture.architecture = "unknown";
    b.capture.metadata = {};
    await route.fulfill({ json: b });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Utilization", exact: true }).click();
  await expect(
    page.getByText("Issue projection is unavailable:", { exact: false }),
  ).toBeVisible();
  await expect(page.locator("canvas")).toBeVisible();
  await expect(page.getByLabel("Timeline detail summary")).toContainText(
    "0 drawn marks",
  );
  await page.getByRole("button", { name: "Target wave", exact: true }).click();
  await expect(page.locator("canvas")).toBeVisible();
});

test("timeline vertical scaling preserves selection and independent saved view options", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const b = await (await route.fetch()).json();
    b.capture.start = 0;
    b.capture.end = 1000;
    const id = b.capture.waves[0].id;
    b.capture.waves = [{ ...b.capture.waves[0], start: 0, end: 1000 }];
    b.waves[id] = {
      ...b.waves[id],
      start: 0,
      end: 1000,
      tokens: [
        { start: 100, duration: 200, stall: 0, category: 6, instruction: 1 },
        { start: 100, duration: 200, stall: 0, category: 6, instruction: 2 },
        { start: 100, duration: 200, stall: 0, category: 6, instruction: 3 },
      ],
    };
    await route.fulfill({ json: b });
  });
  await page.goto("/");
  await page.getByLabel("Timeline lane height").selectOption("56");
  const canvas = page.locator("canvas");
  const box = await canvas.boundingBox();
  if (!box) throw Error("Missing timeline");
  const x = 155 + 0.2 * (box.width - 169);
  await canvas.click({ position: { x, y: 65 } });
  await expect(page.locator(".isa-table tr.selected td").first()).toHaveText(
    "1",
  );
  await canvas.click({ position: { x, y: 40 } });
  await expect(page.locator(".isa-table tr.selected td").first()).toHaveText(
    "3",
  );
  await canvas.press("s");
  await expect(page.getByLabel("Timeline lane height")).toHaveValue("42");
  await canvas.press("w");
  await expect(page.getByLabel("Timeline lane height")).toHaveValue("56");
  await page.getByLabel("Token layout").selectOption("flat");
  await page.getByRole("button", { name: "Global", exact: true }).click();
  await expect(page.getByLabel("Timeline lane height")).toHaveValue("28");
  await page.getByLabel("Timeline lane height").selectOption("20");
  await page.getByRole("button", { name: "Target wave", exact: true }).click();
  await expect(page.getByLabel("Timeline lane height")).toHaveValue("56");
  await expect(page.getByLabel("Token layout")).toHaveValue("flat");
  await page.reload();
  await expect(page.getByLabel("Timeline lane height")).toHaveValue("56");
  await expect(page.getByLabel("Token layout")).toHaveValue("flat");
  await page.getByRole("button", { name: "Global", exact: true }).click();
  await expect(page.getByLabel("Timeline lane height")).toHaveValue("20");
});

test("initial capture loading preserves a view chosen while waiting", async ({
  page,
}) => {
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route("**/demo.json", async (route) => {
    await gate;
    await route.continue();
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  release();
  await expect(page.getByLabel("Counter definitions")).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "Counters", exact: true }),
  ).toBeVisible();
});

test("late example loading preserves a capture opened during startup", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  const capture = JSON.parse(
    await readFile("fixtures/rocpd/sdk-3.0.4.json", "utf8"),
  );
  await page.addInitScript((capture) => {
    const w = window as any;
    w.isTauri = true;
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string) => {
        if (command === "plugin:dialog|open") return "/fixture.rocpd";
        if (command === "open_capture")
          return { ...capture, id: "native-rocpd", name: "Early import" };
        throw Error(command);
      },
    };
  }, capture);
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route("**/demo.json", async (route) => {
    await gate;
    await route.continue();
  });
  await page.goto("/");
  await page
    .getByRole("button", { name: "Open capture file", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Kernel dispatches", exact: true })
    .click();
  await expect(page.locator("tbody tr")).toHaveCount(1);
  release();
  await expect(
    page.getByRole("button", {
      name: "LDS transpose · example capture DEMO",
      exact: true,
    }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Early import", exact: true }),
  ).toBeVisible();
  await expect(page.locator("tbody tr").first()).toContainText("myKernel");
});

test("plot exports produce standalone PNG and styled SVG files", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  await page.goto("/");
  await expect(page.locator("canvas")).toBeVisible();
  const pngPending = page.waitForEvent("download");
  await page.getByRole("button", { name: /Export .* as PNG/ }).click();
  const png = await pngPending;
  const pngPath = await png.path();
  if (!pngPath) throw Error("No PNG download");
  const bytes = await readFile(pngPath);
  expect(Array.from(bytes.subarray(0, 8))).toEqual([
    137, 80, 78, 71, 13, 10, 26, 10,
  ]);
  expect(bytes.readUInt32BE(16)).toBeGreaterThan(500);
  expect(bytes.readUInt32BE(20)).toBeGreaterThan(215);
  expect(png.suggestedFilename()).toMatch(/\.png$/);
  await page.getByRole("button", { name: "Occupancy", exact: true }).click();
  const svgPending = page.waitForEvent("download");
  await page
    .getByRole("button", { name: /Export .* as SVG/ })
    .first()
    .click();
  const svg = await svgPending;
  const svgPath = await svg.path();
  if (!svgPath) throw Error("No SVG download");
  const xml = await readFile(svgPath, "utf8");
  expect(xml).toContain('xmlns="http://www.w3.org/2000/svg"');
  expect(xml).toContain("Visible range");
  expect(xml).toContain('fill="#14181f"');
  expect(xml).toContain('font-family="monospace"');
  expect(
    await page.evaluate(async (xml) => {
      const parsed = new DOMParser().parseFromString(xml, "image/svg+xml");
      if (parsed.querySelector("parsererror")) return false;
      const url = URL.createObjectURL(
        new Blob([xml], { type: "image/svg+xml" }),
      );
      const image = new Image();
      image.src = url;
      try {
        await image.decode();
        return image.naturalWidth === 900 && image.naturalHeight > 145;
      } finally {
        URL.revokeObjectURL(url);
      }
    }, xml),
  ).toBe(true);
});

test("native plot export reports cancellation and write errors", async ({
  page,
}) => {
  await page.addInitScript(() => {
    const w = window as any;
    w.isTauri = true;
    w.exports = [];
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, args: any) => {
        if (command !== "export_plot") throw Error(command);
        w.exports.push(args);
        if (w.exports.length === 1) return false;
        throw Error("Destination is read-only");
      },
    };
  });
  await page.goto("/");
  await expect(page.locator("canvas")).toBeVisible();
  const button = page.getByRole("button", { name: /Export .* as PNG/ });
  await button.click();
  await expect
    .poll(() => page.evaluate(() => (window as any).exports.length), {
      timeout: 15000,
      message:
        "PNG encoding must finish and dispatch the native export request",
    })
    .toBe(1);
  await expect(page.getByRole("status")).toHaveText("Export cancelled");
  const request = await page.evaluate(() => (window as any).exports[0]);
  expect(request.format).toBe("png");
  expect(request.bytes.slice(0, 8)).toEqual([137, 80, 78, 71, 13, 10, 26, 10]);
  await button.click();
  await expect(page.getByRole("status")).toContainText(
    "Destination is read-only",
  );
  await expect(button).toBeEnabled();
});

test("analysis SVG exports retain histogram bins, focused source weights, markers and fabric counts", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  const exportXml = async (name: string) => {
    const pending = page.waitForEvent("download");
    await page
      .getByRole("button", { name: `Export ${name} as SVG`, exact: true })
      .click();
    const file = await (await pending).path();
    if (!file) throw Error("Missing export");
    const xml = await readFile(file, "utf8");
    expect(
      await page.evaluate(async (xml) => {
        const doc = new DOMParser().parseFromString(xml, "image/svg+xml");
        if (doc.querySelector("parsererror")) return false;
        const url = URL.createObjectURL(
          new Blob([xml], { type: "image/svg+xml" }),
        );
        try {
          const image = new Image();
          image.src = url;
          await image.decode();
          return image.naturalWidth > 0;
        } finally {
          URL.revokeObjectURL(url);
        }
      }, xml),
    ).toBe(true);
    return xml;
  };
  await page.goto("/");
  await page
    .getByRole("button", { name: "Instruction hotspots", exact: true })
    .click();
  await page.getByLabel("Histogram bins").selectOption("8");
  const tips = await page
    .locator(".histogram button")
    .evaluateAll((els) => els.map((e) => e.getAttribute("title")));
  const hist = await exportXml("Instruction cost distribution");
  expect(hist).toContain("whole capture");
  expect(hist).toContain("idle excluded");
  const totals = await page.evaluate(
    (xml) =>
      Array.from(
        new DOMParser()
          .parseFromString(xml, "image/svg+xml")
          .querySelectorAll("g title"),
      )
        .filter((el) => el.textContent?.includes("accumulated cycles"))
        .map((el) => el.textContent),
    hist,
  );
  expect(totals).toHaveLength(8);
  tips.forEach((tip, i) =>
    expect(totals[i]).toContain(
      (tip || "")
        .split(" · ")[1]
        .replace(" accumulated", "")
        .replaceAll(",", ""),
    ),
  );
  await page
    .getByRole("button", { name: "Source flamegraph", exact: true })
    .click();
  await page.locator(".flames > div > button").first().click();
  const focused = await page
    .locator(".flames > div > button")
    .first()
    .innerText();
  const flame = await exportXml("Source flamegraph");
  expect(flame).toContain("source-to-ISA aggregation");
  expect(flame).toContain(focused.split("\n")[0].trim());
  expect(flame).toContain("ISA ");
  await page
    .getByRole("button", { name: "Marker scopes", exact: true })
    .click();
  const marker = await exportXml("Marker flamegraph");
  expect(marker).toContain("Load tile");
  expect(marker).toContain("open scopes/points excluded");
  await page
    .getByRole("button", { name: "Compute fabric", exact: true })
    .click();
  const fabric = await exportXml("Compute fabric");
  expect(fabric).toContain("untraced CUs unknown");
  expect(fabric).toContain("recorded active waves");
  expect(fabric).toContain("gfx1151");
});

test("fabric detail exports preserve slots, zero resources, unknowns and dispatch-time matching", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  await page.route("**/demo.json", async (route) => {
    const b = await (await route.fetch()).json();
    b.capture.start = 0;
    b.capture.end = 100;
    b.capture.intervals = [3, 5].map((slot) => ({
      start: 0,
      end: 100,
      se: 0,
      cu: 2,
      simd: 1,
      slot,
      kernel: "resourceKernel",
    }));
    const d = {
      id: "live",
      kernel: "resourceKernel",
      agent: "SE 0",
      queue: "0",
      start: 0,
      end: 70,
      vgpr: 64,
      sgpr: 32,
      lds: 0,
      scratch: null,
      grid: "1",
      workgroup: "1",
    };
    b.capture.dispatches = [
      { ...d, id: "old", start: -100, end: -1, vgpr: 999 },
      d,
      { ...d, id: "ambiguous1", start: 70, end: 100 },
      { ...d, id: "ambiguous2", start: 70, end: 100, vgpr: 128 },
    ];
    await route.fulfill({ json: b });
  });
  const exportXml = async (label: string) => {
    const pending = page.waitForEvent("download");
    await page
      .getByRole("button", { name: `Export ${label} as SVG`, exact: true })
      .click();
    const path = await (await pending).path();
    if (!path) throw Error("Missing SVG");
    return readFile(path, "utf8");
  };
  await page.goto("/");
  await page
    .getByRole("button", { name: "Compute fabric", exact: true })
    .click();
  const slots = await exportXml("SIMD residency");
  expect(slots).toContain("SIMD 1 slot 3");
  expect(slots).toContain("SIMD 1 slot 5");
  expect(slots).toContain("hardware capacity not inferred");
  const resources = await exportXml("Kernel resources");
  expect(resources).toContain("VGPR metadata: 64 registers");
  expect(resources).not.toContain("999");
  expect(resources).toContain("LDS allocation: 0 bytes / workgroup");
  expect(resources).toContain("Private scratch: Not collected");
  const slider = page.getByLabel("Hardware inspection cycle");
  await slider.focus();
  await slider.press("End");
  await slider.press("ArrowLeft");
  const ambiguous = await exportXml("Kernel resources");
  expect(ambiguous).toContain(
    "multiple matching dispatches; resources unavailable",
  );
  expect(ambiguous).toContain("VGPR metadata: Not collected");
  await slider.focus();
  await slider.press("End");
  const empty = await exportXml("SIMD residency");
  expect(empty).toContain("No active waves recorded");
});

test("independent timeline ranges survive switching, reset and capture tabs", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  const capture = JSON.parse(
    await readFile("fixtures/rocpd/sdk-3.0.4.json", "utf8"),
  );
  await page.addInitScript((capture) => {
    const w = window as any;
    w.isTauri = true;
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string) => {
        if (command === "plugin:dialog|open") return "/fixture.rocpd";
        if (command === "open_capture")
          return { ...capture, id: "range-other", name: "Range other" };
        throw Error(command);
      },
    };
  }, capture);
  await page.goto("/");
  const link = page.getByLabel("Link timeline ranges");
  await expect(link).toBeChecked();
  await link.uncheck();
  await page.getByLabel("Range end").fill("2000");
  await page.getByLabel("Range start").fill("1000");
  await page.getByRole("button", { name: "Global", exact: true }).click();
  await expect(page.getByLabel("Range start")).toHaveValue("1000");
  await page.getByLabel("Range end").fill("4000");
  await page.getByLabel("Range start").fill("3000");
  await page.getByRole("button", { name: "Target wave", exact: true }).click();
  await expect(page.getByLabel("Range start")).toHaveValue("1000");
  await expect(page.getByLabel("Range end")).toHaveValue("2000");
  await page.getByRole("button", { name: "Target wave", exact: true }).click();
  await expect(page.getByLabel("Range end")).toHaveValue("2000");
  await page.getByRole("button", { name: "Reset visible range" }).click();
  await page.getByRole("button", { name: "Global", exact: true }).click();
  await expect(page.getByLabel("Range start")).toHaveValue("3000");
  await expect(page.getByLabel("Range end")).toHaveValue("4000");
  await page
    .getByRole("button", { name: "Open capture file", exact: true })
    .click();
  await expect(link).toBeChecked();
  await page
    .getByRole("button", {
      name: "LDS transpose · example capture DEMO",
      exact: true,
    })
    .click();
  await expect(link).not.toBeChecked();
  await expect(page.getByLabel("Range start")).toHaveValue("3000");
  await page.getByRole("button", { name: "Target wave", exact: true }).click();
  await expect(page.getByLabel("Range start")).toHaveValue("500");
  await expect(page.getByLabel("Range end")).toHaveValue("7240");
  await page.getByRole("button", { name: "Global", exact: true }).click();
  await link.check();
  await page.getByRole("button", { name: "Target wave", exact: true }).click();
  await expect(page.getByLabel("Range start")).toHaveValue("3000");
  await expect(page.getByLabel("Range end")).toHaveValue("4000");
});

test("global-to-wave navigation and history preserve independent timeline ranges", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const b = await (await route.fetch()).json();
    b.capture.start = 0;
    b.capture.end = 1000;
    const id = b.capture.waves[0].id;
    b.capture.waves = [
      { ...b.capture.waves[0], se: 0, simd: 0, slot: 0, start: 100, end: 900 },
    ];
    b.waves[id] = {
      ...b.waves[id],
      cu: 2,
      start: 100,
      end: 900,
      tokens: [
        { start: 200, duration: 200, stall: 0, category: 6, instruction: 1 },
      ],
    };
    b.capture.intervals = [
      {
        start: 100,
        end: 900,
        se: 0,
        cu: 2,
        simd: 0,
        slot: 0,
        kernel: "navigation",
      },
    ];
    await route.fulfill({ json: b });
  });
  await page.goto("/");
  await page.getByLabel("Link timeline ranges").uncheck();
  await page.getByRole("button", { name: "Global", exact: true }).click();
  const canvas = page.locator("canvas");
  const box = await canvas.boundingBox();
  if (!box) throw Error("No global timeline");
  await canvas.click({ position: { x: 155 + 0.5 * (box.width - 169), y: 40 } });
  await expect(
    page.getByRole("button", { name: "Target wave", exact: true }),
  ).toHaveClass("active");
  await expect(page.getByLabel("Range start")).toHaveValue("100");
  await expect(page.getByLabel("Range end")).toHaveValue("900");
  await page
    .getByRole("button", { name: "Previous selection", exact: true })
    .click();
  await expect(
    page.getByRole("button", { name: "Global", exact: true }),
  ).toHaveClass("active");
  await expect(page.getByLabel("Range start")).toHaveValue("0");
  await expect(page.getByLabel("Range end")).toHaveValue("1000");
  await page
    .getByRole("button", { name: "Next selection", exact: true })
    .click();
  await expect(
    page.getByRole("button", { name: "Target wave", exact: true }),
  ).toHaveClass("active");
  await expect(page.getByLabel("Range start")).toHaveValue("100");
  await page.getByRole("button", { name: "Global", exact: true }).click();
  await expect(page.getByLabel("Range end")).toHaveValue("1000");
});

test("line plots keep labels readable as the workspace resizes", async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 2284, height: 900 });
  await page.goto("/");
  await page.getByRole("button", { name: "Occupancy", exact: true }).click();
  const chart = page.getByRole("img", { name: "Shader engine 0 occupancy" });
  const measure = () => chart.evaluate((svg: SVGSVGElement) => {
    const bounds = svg.getBoundingClientRect();
    const label = svg.querySelector("text")!.getBoundingClientRect();
    return { width: bounds.width, height: bounds.height, viewWidth: svg.viewBox.baseVal.width, labelWidth: label.width };
  });
  await expect.poll(async () => (await measure()).viewWidth).toBeGreaterThan(1800);
  const wide = await measure();
  expect(wide.height).toBeGreaterThan(300);
  expect(Math.abs(wide.viewWidth - (wide.width - 30))).toBeLessThan(2);
  await chart.screenshot({ path: testInfo.outputPath("occupancy-wide.png") });
  await page.setViewportSize({ width: 1000, height: 900 });
  await expect.poll(async () => (await measure()).viewWidth).toBeLessThan(850);
  const narrow = await measure();
  expect(narrow.height).toBeLessThan(wide.height);
  expect(Math.abs(narrow.viewWidth - (narrow.width - 30))).toBeLessThan(2);
  expect(Math.abs(narrow.labelWidth - wide.labelWidth)).toBeLessThan(2);
  await chart.screenshot({ path: testInfo.outputPath("occupancy-narrow.png") });
});

test("line plots apply independent vertical bounds, clip exports and reject invalid ranges", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  await page.goto("/");
  await page.getByRole("button", { name: "Occupancy", exact: true }).click();
  const plots = page.locator(".line-plot");
  const first = plots.first();
  await first.locator(".plot-y-axis > summary").click();
  await first.getByLabel(/Y minimum for/).fill("-1");
  await first.getByLabel(/Y maximum for/).fill("1");
  await first.getByRole("button", { name: "Apply axis bounds" }).click();
  await expect(first.locator(".plot-y-axis > summary")).toContainText(
    "custom · -1–1",
  );
  await expect(plots.nth(1).locator(".plot-y-axis > summary")).toContainText(
    "automatic",
  );
  const clip = await first.locator("clipPath").getAttribute("id");
  await expect(first.locator("svg path")).toHaveAttribute(
    "clip-path",
    `url(#${clip})`,
  );
  expect(await plots.nth(1).locator("clipPath").getAttribute("id")).not.toBe(
    clip,
  );
  await first.getByLabel(/Y maximum for/).fill("-1");
  await first.getByRole("button", { name: "Apply axis bounds" }).click();
  await expect(first.getByRole("alert")).toContainText(
    "maximum greater than minimum",
  );
  await expect(first.locator(".plot-y-axis > summary")).toContainText(
    "custom · -1–1",
  );
  const pending = page.waitForEvent("download");
  await first.getByRole("button", { name: /Export .* as SVG/ }).click();
  const path = await (await pending).path();
  if (!path) throw Error("Missing export");
  const svg = await readFile(path, "utf8");
  expect(svg).toContain("Y -1–1 (custom)");
  expect(svg).toContain(`clip-path="url(#${clip})"`);
  expect(svg).toContain(`<clipPath id="${clip}"`);
  await first.getByRole("button", { name: "Automatic axis" }).click();
  await expect(first.locator(".plot-y-axis > summary")).toContainText(
    "automatic",
  );
  await expect(first.getByRole("alert")).toHaveCount(0);
});

test("raw ATT run chooser cancels without decoding and imports only the selected run", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  const capture = JSON.parse(
    await readFile("fixtures/rocpd/sdk-3.0.4.json", "utf8"),
  );
  await page.addInitScript((capture) => {
    const w = window as any;
    w.isTauri = true;
    w.decoded = [];
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, args: any) => {
        if (command === "plugin:dialog|open") return "/raw";
        if (command === "discover_att_runs")
          return [
            {
              prefix: "process_A",
              run: 3,
              shaderEngines: [0, 2],
              bytes: 1234,
              path: "/raw/a_shader_engine_0_3.att",
            },
            {
              prefix: "process_B",
              run: 7,
              shaderEngines: [1],
              bytes: 9876,
              path: "/raw/b_shader_engine_1_7.att",
            },
          ];
        if (command === "open_capture") {
          w.decoded.push(args.path);
          return { ...capture, id: "selected-run", name: "Selected run 7" };
        }
        throw Error(command);
      },
    };
  }, capture);
  await page.goto("/");
  await page.getByRole("button", { name: "Open capture", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Choose a profiling run" });
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("Shader engines: 0, 2");
  expect(await page.evaluate(() => (window as any).decoded)).toEqual([]);
  await dialog.getByRole("button", { name: "Cancel run selection" }).click();
  await expect(dialog).toHaveCount(0);
  expect(await page.evaluate(() => (window as any).decoded)).toEqual([]);
  await page.getByRole("button", { name: "Open capture", exact: true }).click();
  await page.getByLabel("ATT profiling run").selectOption("1");
  await expect(dialog).toContainText("9,876 bytes");
  await dialog.getByRole("button", { name: "Decode selected run" }).click();
  await expect(
    page.getByRole("button", { name: "Selected run 7", exact: true }),
  ).toBeVisible();
  expect(await page.evaluate(() => (window as any).decoded)).toEqual([
    "/raw/b_shader_engine_1_7.att",
  ]);
  await expect(dialog).toHaveCount(0);
});

test("project save and reopen restore same-ID navigation and sources while failed reopen preserves work", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  const bundle = JSON.parse(await readFile("public/demo.json", "utf8"));
  await page.addInitScript((capture) => {
    const w = window as any;
    w.isTauri = true;
    w.saved = null;
    w.failProject = false;
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, args: any) => {
        if (command === "import_source_file")
          return {
            name: "project.hip",
            text: Array.from({ length: 30 }, () => "saved project source").join(
              "\n",
            ),
          };
        if (command === "save_project") {
          w.saved = structuredClone(args);
          return true;
        }
        if (command === "open_project") {
          if (w.failProject) throw Error("Project input is missing");
          return {
            captures: [{ ...capture, id: "demo" }],
            project: {
              schemaVersion: 1,
              active: w.saved.active,
              entries: w.saved.entries.map((e: any) => ({
                input: null,
                state: e.state,
                sources: e.sources,
              })),
            },
          };
        }
        throw Error(command);
      },
    };
  }, bundle.capture);
  await page.goto("/");
  await expect(page.locator("canvas")).toBeVisible();
  await page.getByLabel("Link timeline ranges").uncheck();
  await page.getByLabel("Range end").fill("3000");
  await page.getByLabel("Range start").fill("1000");
  await page
    .getByRole("button", { name: "Map source file", exact: true })
    .click();
  await expect(page.locator(".source-selected")).toContainText(
    "saved project source",
  );
  await page.getByRole("button", { name: "Occupancy", exact: true }).click();
  const axis = page.locator(".line-plot").first();
  await axis.locator(".plot-lod > summary").click();
  await axis.getByLabel("LOD bias", { exact: true }).fill("-3");
  await expect(
    page.locator(".line-plot").nth(1).locator(".plot-lod > summary"),
  ).toContainText("bias -3");
  await axis.locator(".plot-y-axis > summary").click();
  await axis.getByLabel(/Y minimum for/).fill("-2");
  await axis.getByLabel(/Y maximum for/).fill("3");
  await axis.getByRole("button", { name: "Apply axis bounds" }).click();
  await axis.locator(".plot-x-range > summary").click();
  await axis.getByLabel("Follow workspace range").uncheck();
  await axis.getByLabel(/Plot range for .* end/).fill("2500");
  await axis.getByLabel(/Plot range for .* end/).press("Enter");
  await page
    .getByRole("button", { name: "Trace workspace", exact: true })
    .click();
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  await page.getByLabel("Derived counter name").fill("Saved scalar");
  await page.getByLabel("Counter expression", { exact: true }).fill("C0 / 2");
  await page.getByRole("button", { name: "Add counter", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Saved scalar", exact: true }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Trace workspace", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Instruction hotspots", exact: true })
    .click();
  await page.getByLabel("Histogram bins").selectOption("32");
  await page.getByLabel("Histogram range end").fill("200");
  await page.getByLabel("Histogram range end").press("Enter");
  await page
    .getByRole("button", { name: "Trace workspace", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Source flamegraph", exact: true })
    .click();
  await page.getByLabel("Flamegraph range end").fill("123");
  await page.getByLabel("Flamegraph range end").press("Enter");
  await page
    .getByRole("button", { name: "Trace workspace", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Marker scopes", exact: true })
    .click();
  await page.getByLabel("Marker flamegraph range end").fill("777");
  await page.getByLabel("Marker flamegraph range end").press("Enter");
  await page
    .getByRole("button", { name: "Trace workspace", exact: true })
    .click();
  await page.getByRole("button", { name: "Global", exact: true }).click();
  await page.getByLabel("Show global markers").uncheck();
  await page.getByLabel("Show global dispatches").uncheck();
  await page.getByText("Event groups", { exact: true }).click();
  await page.getByLabel("Decoder group GC rinse", { exact: true }).check();
  await page
    .getByLabel("Decoder group Flush / timestamps", { exact: true })
    .uncheck();
  await page.getByRole("button", { name: "Target wave", exact: true }).click();
  await page.getByRole("button", { name: "CU waves", exact: true }).click();
  await page.getByLabel("Show CU events").uncheck();
  await page.getByText("CU event groups", { exact: true }).click();
  await page.getByLabel("CU decoder group GC rinse", { exact: true }).uncheck();
  await page.getByRole("button", { name: "Target wave", exact: true }).click();
  await page.getByRole("button", { name: "Save project", exact: true }).click();
  await expect(page.getByRole("status")).toContainText("Project saved");
  const saved = await page.evaluate(() => (window as any).saved);
  expect(saved.entries[0].state.location.range).toEqual([1000, 3000]);
  expect(saved.entries[0].state.independentRanges).toBe(true);
  expect(saved.entries[0].state.analysisPreferences["plot.lodBias"]).toBe(-3);
  expect(Object.values(saved.entries[0].state.plotRanges)).toEqual(
    expect.arrayContaining([
      [1000, 2500],
      [0, 200],
    ]),
  );
  expect(saved.entries[0].state.histogramBins).toBe(32);
  expect(saved.entries[0].state.cuEvents).toBe(false);
  expect(saved.entries[0].state.cuEventGroups).toBe(47);
  expect(saved.entries[0].state.globalLayers).toEqual({
    residency: true,
    markers: false,
    dispatches: false,
    events: true,
    eventGroups: 61,
  });
  expect(saved.entries[0].state.scalarCounters[0].formula).toBe("C0 / 2");
  expect(Object.values(saved.entries[0].state.plotAxes)).toEqual([[-2, 3]]);
  await page.getByRole("button", { name: "Occupancy", exact: true }).click();
  await expect(axis.locator(".plot-y-axis > summary")).toContainText(
    "custom · -2–3",
  );
  await expect(axis.locator(".plot-lod > summary")).toContainText("bias -3");
  await axis.locator(".plot-y-axis > summary").click();
  await axis.getByRole("button", { name: "Automatic axis" }).click();
  await axis.locator(".plot-x-range > summary").click();
  await axis.getByLabel("Follow workspace range").check();
  await page
    .getByRole("button", { name: "Trace workspace", exact: true })
    .click();
  expect(Object.keys(saved.entries[0].sources)).toHaveLength(1);
  await page
    .getByRole("button", { name: "Restore captured source", exact: true })
    .click();
  await page.getByRole("button", { name: "Global", exact: true }).click();
  await page.getByLabel("Show global markers").check();
  await page.getByLabel("Show global dispatches").check();
  await page.getByText("Event groups", { exact: true }).click();
  await page.getByLabel("Decoder group GC rinse", { exact: true }).uncheck();
  await page
    .getByLabel("Decoder group Flush / timestamps", { exact: true })
    .check();
  await page.getByLabel("Range end").fill("4000");
  await page.getByLabel("Range start").fill("2000");
  await page
    .getByRole("button", { name: "Instruction hotspots", exact: true })
    .click();
  await page.getByLabel("Histogram bins").selectOption("8");
  await page
    .getByRole("button", { name: "Automatic histogram range", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Source flamegraph", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Full flamegraph range", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Marker scopes", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Full marker flamegraph range", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Trace workspace", exact: true })
    .click();
  await page.getByRole("button", { name: "CU waves", exact: true }).click();
  await page.getByLabel("Show CU events").check();
  await page.getByRole("button", { name: "Open project", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "Target wave", exact: true }),
  ).toHaveClass("active");
  await expect(page.getByLabel("Range start")).toHaveValue("1000");
  await expect(page.getByLabel("Range end")).toHaveValue("3000");
  await expect(page.getByLabel("Link timeline ranges")).not.toBeChecked();
  await page.getByRole("button", { name: "CU waves", exact: true }).click();
  await expect(page.getByLabel("Show CU events")).not.toBeChecked();
  await page.getByText("CU event groups", { exact: true }).click();
  await expect(
    page.getByLabel("CU decoder group GC rinse", { exact: true }),
  ).not.toBeChecked();
  await page.getByRole("button", { name: "Target wave", exact: true }).click();
  await page.getByRole("button", { name: "Global", exact: true }).click();
  await expect(page.getByLabel("Show global markers")).not.toBeChecked();
  await expect(page.getByLabel("Show global dispatches")).not.toBeChecked();
  await expect(page.getByLabel("Show global residency")).toBeChecked();
  await page.getByText("Event groups", { exact: true }).click();
  await expect(
    page.getByLabel("Decoder group GC rinse", { exact: true }),
  ).toBeChecked();
  await expect(
    page.getByLabel("Decoder group Flush / timestamps", { exact: true }),
  ).not.toBeChecked();
  await page.getByRole("button", { name: "Target wave", exact: true }).click();
  await expect(page.locator(".source-selected")).toContainText(
    "saved project source",
  );
  await page.getByRole("button", { name: "Occupancy", exact: true }).click();
  await expect(axis.locator(".plot-y-axis > summary")).toContainText(
    "custom · -2–3",
  );
  await expect(axis.locator(".plot-lod > summary")).toContainText("bias -3");
  await page
    .getByRole("button", { name: "Trace workspace", exact: true })
    .click();
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  const scalar = page.locator("section").filter({
    has: page.getByRole("heading", { name: "Saved scalar", exact: true }),
  });
  await expect(scalar).toBeVisible();
  await scalar.getByRole("button", { name: "Remove", exact: true }).click();
  await page
    .getByRole("button", { name: "Trace workspace", exact: true })
    .click();
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Saved scalar", exact: true }),
  ).toHaveCount(0);
  await page
    .getByRole("button", { name: "Trace workspace", exact: true })
    .click();
  await page.getByRole("button", { name: "Occupancy", exact: true }).click();
  await expect(axis.locator(".plot-x-range > summary")).toContainText(
    "independent · 1,000–2,500",
  );
  await page
    .getByRole("button", { name: "Trace workspace", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Instruction hotspots", exact: true })
    .click();
  await expect(page.getByLabel("Histogram bins")).toHaveValue("32");
  await expect(page.getByLabel("Histogram range end")).toHaveValue("200");
  await page
    .getByRole("button", { name: "Trace workspace", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Source flamegraph", exact: true })
    .click();
  await expect(page.getByLabel("Flamegraph range end")).toHaveValue("123");
  await page
    .getByRole("button", { name: "Trace workspace", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Marker scopes", exact: true })
    .click();
  await expect(page.getByLabel("Marker flamegraph range end")).toHaveValue(
    "777",
  );
  await page
    .getByRole("button", { name: "Trace workspace", exact: true })
    .click();
  await page.getByLabel("Range start").fill("1800");
  await page.evaluate(() => ((window as any).failProject = true));
  await page.getByRole("button", { name: "Open project", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText(
    "Project input is missing",
  );
  await expect(page.getByLabel("Range start")).toHaveValue("1800");
  await expect(page.locator(".source-selected")).toContainText(
    "saved project source",
  );
});

test("range keyboard drafts commit once, retain fractions and reject invalid bounds", async ({
  page,
}) => {
  await page.goto("/");
  const start = page.getByLabel("Range start");
  const end = page.getByLabel("Range end");
  await expect(start).toHaveValue("500");
  const duration = page.locator(".range-duration");
  const initial = await duration.textContent();
  await start.fill("");
  await start.pressSequentially("1200.5");
  await expect(start).toHaveValue("1200.5");
  await expect(duration).toHaveText(initial!);
  await start.press("Enter");
  await expect(duration).not.toHaveText(initial!);
  await expect(start).toHaveValue("1200.5");
  const committed = await duration.textContent();
  await start.fill("1500");
  await start.press("Escape");
  await expect(start).toHaveValue("1200.5");
  await expect(duration).toHaveText(committed!);
  await end.fill("1000");
  await end.press("Enter");
  await expect(end).toHaveValue("7240");
  await expect(page.locator(".range-error")).toContainText([
    "",
    "Enter a finite bound",
  ]);
  await expect(duration).toHaveText(committed!);
  await end.fill("");
  await end.press("Tab");
  await expect(end).toHaveValue("7240");
  await expect(duration).toHaveText(committed!);
  await start.fill("100");
  await start.press("Tab");
  await expect(start).toHaveValue("500");
  await expect(end).toHaveValue("7240");
  await end.fill("4000.75");
  await end.press("Tab");
  await expect(end).toHaveValue("4000.75");
  await page.getByRole("button", { name: "Reset visible range" }).click();
  await expect(end).toHaveValue("7240");
});

test("saved plot axes are isolated between captures and automatic reset survives navigation", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  const capture = JSON.parse(
    await readFile("public/demo.json", "utf8"),
  ).capture;
  await page.addInitScript((capture) => {
    const w = window as any;
    w.isTauri = true;
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, args: any) => {
        if (command === "begin_wave_query") return args.requestId;
        if (command === "cancel_wave_query") return null;
        if (command === "plugin:dialog|open") return "/other.json";
        if (command === "open_capture")
          return { ...capture, id: "other", name: "Other capture", waves: [] };
        if (command === "analyze_hidden") return {};
        throw Error(command);
      },
    };
  }, capture);
  await page.goto("/");
  await page.getByRole("button", { name: "Global", exact: true }).click();
  await page.getByLabel("Show global markers").uncheck();
  await page.getByRole("button", { name: "Occupancy", exact: true }).click();
  const first = page.locator(".line-plot").first();
  await first.locator(".plot-y-axis > summary").click();
  await first.getByLabel(/Y minimum for/).fill("-4");
  await first.getByLabel(/Y maximum for/).fill("8");
  await first.getByRole("button", { name: "Apply axis bounds" }).click();
  await page
    .getByRole("button", { name: "Instruction hotspots", exact: true })
    .click();
  await page.getByLabel("Histogram bins").selectOption("64");
  await page.getByLabel("Histogram range end").fill("333");
  await page.getByLabel("Histogram range end").press("Enter");
  await page.getByRole("button", { name: "Occupancy", exact: true }).click();
  await page
    .getByRole("button", { name: "Open capture file", exact: true })
    .click();
  await page.getByRole("button", { name: "Occupancy", exact: true }).click();
  await expect(first.locator(".plot-y-axis > summary")).toContainText(
    "automatic",
  );
  await page
    .getByRole("button", { name: "Trace workspace", exact: true })
    .click();
  await page.getByRole("button", { name: "Global", exact: true }).click();
  await expect(page.getByLabel("Show global markers")).toBeChecked();
  await page
    .getByRole("button", { name: "Instruction hotspots", exact: true })
    .click();
  await expect(page.getByLabel("Histogram bins")).toHaveValue("16");
  await expect(page.locator(".axis-label")).toContainText("Automatic range");
  await page.getByRole("button", { name: "Occupancy", exact: true }).click();
  await page
    .getByRole("button", {
      name: "LDS transpose · example capture DEMO",
      exact: true,
    })
    .click();
  await expect(first.locator(".plot-y-axis > summary")).toContainText(
    "custom · -4–8",
  );
  await page
    .getByRole("button", { name: "Instruction hotspots", exact: true })
    .click();
  await expect(page.getByLabel("Histogram bins")).toHaveValue("64");
  await expect(page.getByLabel("Histogram range end")).toHaveValue("333");
  await page.getByRole("button", { name: "Occupancy", exact: true }).click();
  await first.locator(".plot-y-axis > summary").click();
  await first.getByRole("button", { name: "Automatic axis" }).click();
  await page
    .getByRole("button", { name: "Trace workspace", exact: true })
    .click();
  await page.getByRole("button", { name: "Occupancy", exact: true }).click();
  await expect(first.locator(".plot-y-axis > summary")).toContainText(
    "automatic",
  );
  await page
    .getByRole("button", { name: "Trace workspace", exact: true })
    .click();
  await page.getByRole("button", { name: "Global", exact: true }).click();
  await expect(page.getByLabel("Show global markers")).not.toBeChecked();
});

test("project tensor libraries restore independently of machine drafts", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  const capture = JSON.parse(
    await readFile("public/demo.json", "utf8"),
  ).capture;
  await page.addInitScript((capture) => {
    const w = window as any;
    w.isTauri = true;
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, args: any) => {
        if (command === "save_project") {
          w.saved = structuredClone(args);
          return true;
        }
        if (command === "open_project")
          return {
            captures: [{ ...capture, id: "demo" }],
            project: {
              schemaVersion: 1,
              active: 0,
              definitionLibrary: w.saved.definitionLibrary,
              entries: w.saved.entries.map((e: any) => ({
                input: null,
                state: e.state,
                sources: e.sources,
              })),
            },
          };
        throw Error(command);
      },
    };
  }, capture);
  await page.goto("/");
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  const editor = page.getByLabel("Counter definitions");
  await editor.fill("Captured := 2");
  await page.getByRole("button", { name: "Save project", exact: true }).click();
  await expect(page.getByRole("status")).toContainText("Project saved");
  await editor.fill("Machine := 3");
  await page.getByRole("button", { name: "Open project", exact: true }).click();
  await expect(editor).toHaveValue("Captured := 2");
  await expect(
    page.getByText("Project definition library.", { exact: false }),
  ).toBeVisible();
  await editor.fill("Project := 4");
  await page
    .getByRole("button", { name: "Trace workspace", exact: true })
    .click();
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  await expect(editor).toHaveValue("Project := 4");
  expect(
    await page.evaluate(
      () =>
        JSON.parse(localStorage.getItem("rocprof:definition-library:v1")!)
          .files[0].text,
    ),
  ).toBe("Machine := 3");
  await page.getByRole("button", { name: "Save project", exact: true }).click();
  await expect(page.getByRole("status")).toContainText("Project saved");
  expect(
    await page.evaluate(
      () => (window as any).saved.definitionLibrary.files[0].text,
    ),
  ).toBe("Project := 4");
  await page.reload();
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  await expect(editor).toHaveValue("Machine := 3");
});

test("source folder batches report missing paths and preserve mappings on failure", async ({
  page,
}) => {
  await page.addInitScript(() => {
    const w = window as any;
    w.isTauri = true;
    w.folderCalls = 0;
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, args: any) => {
        if (command === "import_source_file")
          return { name: "old.hip", text: "old mapping" };
        if (command !== "import_source_directory") throw Error(command);
        w.folderArgs = args;
        if (++w.folderCalls === 1)
          return {
            files: {
              "/transpose.hip": {
                name: "transpose.hip",
                text: Array.from(
                  { length: 30 },
                  () => "mapped folder source",
                ).join("\n"),
              },
            },
            missing: ["missing.hip"],
          };
        if (w.folderCalls === 2)
          return {
            files: { "/transpose.hip": { name: "bad", text: "\0" } },
            missing: [],
          };
        return null;
      },
    };
  });
  await page.goto("/");
  await page
    .getByRole("button", { name: "Map source file", exact: true })
    .click();
  await page.getByText("Map source directory", { exact: true }).click();
  await page.getByLabel("Recorded source directory prefix").fill("/");
  const choose = page.getByRole("button", {
    name: "Choose replacement source folder",
    exact: true,
  });
  await choose.click();
  await expect(page.locator(".source-selected")).toContainText(
    "mapped folder source",
  );
  await expect(page.getByRole("status")).toContainText(
    "Mapped 1 files; 1 matching paths missing",
  );
  expect(await page.evaluate(() => (window as any).folderArgs)).toEqual({
    captureId: "demo",
    prefix: "/",
  });
  await choose.click();
  await expect(page.getByRole("alert")).toContainText("NUL");
  await expect(page.locator(".source-selected")).toContainText(
    "mapped folder source",
  );
  await choose.click();
  await expect(page.getByRole("status")).toContainText("cancelled");
  await page
    .getByRole("button", { name: "Restore captured source", exact: true })
    .click();
  await expect(page.locator(".source-selected")).not.toContainText(
    "mapped folder source",
  );
});

test("obsolete hidden analysis cannot overwrite a reopened project or clear its busy state", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  const capture = JSON.parse(
    await readFile("public/demo.json", "utf8"),
  ).capture;
  const pending: import("@playwright/test").Route[] = [];
  await page.route("**/demo-hidden.json", (route) => {
    pending.push(route);
  });
  await page.addInitScript((capture) => {
    const w = window as any;
    w.isTauri = true;
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, args: any) => {
        if (command === "save_project") {
          w.saved = structuredClone(args);
          return true;
        }
        if (command === "open_project")
          return {
            captures: [{ ...capture, id: "demo" }],
            project: {
              schemaVersion: 1,
              active: 0,
              entries: w.saved.entries.map((e: any) => ({
                input: null,
                state: e.state,
                sources: e.sources,
              })),
            },
          };
        throw Error(command);
      },
    };
  }, capture);
  await page.goto("/");
  await expect.poll(() => pending.length).toBe(1);
  await page.getByRole("button", { name: "Save project", exact: true }).click();
  await expect(page.getByRole("status")).toContainText("Project saved");
  await page.getByRole("button", { name: "Open project", exact: true }).click();
  await expect.poll(() => pending.length).toBe(2);
  const obsoleteResponse = page.waitForResponse((response) =>
    response.url().endsWith("/demo-hidden.json"),
  );
  await pending[0].fulfill({
    json: { byInstruction: {}, byWave: {}, model: "obsolete model" },
  });
  await (await obsoleteResponse).finished();
  await page.evaluate(
    () =>
      new Promise<void>((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
      ),
  );
  await expect(
    page.getByRole("button", { name: "Analyzing…", exact: true }),
  ).toBeDisabled();
  await expect(
    page
      .getByLabel("Latency contribution")
      .locator('option[value="nonhidden"]'),
  ).toHaveAttribute("disabled", "");
  await expect(page.locator('[title="obsolete model"]')).toHaveCount(0);
  await pending[1].fulfill({
    json: { byInstruction: {}, byWave: {}, model: "current model" },
  });
  await expect(page.locator('[title="current model"]')).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Recompute hidden latency", exact: true }),
  ).toBeEnabled();
  await page.getByRole("button", { name: "Open project", exact: true }).click();
  await expect.poll(() => pending.length).toBe(3);
  await page
    .getByRole("button", {
      name: "Close LDS transpose · example capture",
      exact: true,
    })
    .click();
  await expect(page.locator(".capture-tab")).toHaveCount(0);
  const failed = page.waitForEvent("requestfailed", (request) =>
    request.url().endsWith("/demo-hidden.json"),
  );
  await pending[2].abort();
  await failed;
  await page
    .getByRole("button", { name: "ISA reference", exact: true })
    .click();
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("independent line-plot ranges survive navigation and drive exported samples", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  await page.goto("/");
  await page.getByRole("button", { name: "Occupancy", exact: true }).click();
  const plots = page.locator(".line-plot"),
    first = plots.first();
  await first.locator(".plot-x-range > summary").click();
  await first.getByLabel("Follow workspace range").uncheck();
  await first.getByLabel(/Plot range for .* start/).fill("1000");
  await first.getByLabel(/Plot range for .* start/).press("Enter");
  await first.getByLabel(/Plot range for .* end/).fill("2000");
  await first.getByLabel(/Plot range for .* end/).press("Enter");
  await page.getByLabel("Range end", { exact: true }).fill("5000");
  await page.getByLabel("Range end", { exact: true }).press("Enter");
  await page.getByLabel("Range start", { exact: true }).fill("3000");
  await page.getByLabel("Range start", { exact: true }).press("Enter");
  await expect(first.locator(".plot-x-range > summary")).toContainText(
    "independent · 1,000–2,000",
  );
  await expect(plots.nth(1).locator(".plot-x-range > summary")).toContainText(
    "workspace · 3,000–5,000",
  );
  const pending = page.waitForEvent("download");
  await first.getByRole("button", { name: /Export .* as SVG/ }).click();
  const file = await (await pending).path();
  expect(await readFile(file!, "utf8")).toContain(
    "Visible range 1000–2000 (independent)",
  );
  await page
    .getByRole("button", { name: "Trace workspace", exact: true })
    .click();
  await page.getByRole("button", { name: "Occupancy", exact: true }).click();
  await expect(first.locator(".plot-x-range > summary")).toContainText(
    "independent · 1,000–2,000",
  );
  await first.locator(".plot-x-range > summary").click();
  await first.getByRole("button", { name: "Full plot range" }).click();
  await expect(first.getByLabel(/Plot range for .* start/)).toHaveValue("500");
  await expect(first.getByLabel(/Plot range for .* end/)).toHaveValue("7240");
  await first.getByLabel("Follow workspace range").check();
  await expect(first.locator(".plot-x-range > summary")).toContainText(
    "workspace · 3,000–5,000",
  );
});

test("event PMC values preserve missingness and ambiguous dispatch links separately from SPM", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  const capture = JSON.parse(
    await readFile("fixtures/rocpd/sdk-3.0.4.json", "utf8"),
  );
  const record = {
    counterId: "pmc:guid:1",
    unit: "waves",
    scope: "process 1 · agent guid:0",
  };
  capture.eventCounters = [
    {
      ...record,
      id: "zero",
      name: "ZERO_PMC",
      value: 0,
      event: "event:guid:4",
      dispatches: [capture.dispatches[0].id],
    },
    {
      ...record,
      id: "missing",
      name: "MISSING_PMC",
      value: null,
      event: null,
      dispatches: [],
    },
    {
      ...record,
      id: "ambiguous",
      name: "AMBIGUOUS_PMC",
      value: 12.5,
      event: "event:guid:5",
      dispatches: capture.dispatches.map((d: any) => d.id),
    },
    {
      ...record,
      id: "orphan",
      name: "EVENT_PMC",
      value: 4,
      event: "event:guid:6",
      dispatches: [],
    },
  ];
  await page.addInitScript((capture) => {
    const w = window as any;
    w.isTauri = true;
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string) => {
        if (command === "plugin:dialog|open") return "/fixture.rocpd";
        if (command === "open_capture") return { ...capture, id: "event-pmc" };
        throw Error(`Unexpected IPC: ${command}`);
      },
    };
  }, capture);
  await page.goto("/");
  await expect(page.locator("canvas")).toBeVisible();
  await page
    .getByRole("button", { name: "Open capture file", exact: true })
    .click();
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  const table = page.getByRole("table", {
    name: "Event-associated counter values",
  });
  await expect(table.getByRole("row")).toHaveCount(5);
  const zero = table.getByRole("row").filter({ hasText: "ZERO_PMC" });
  await expect(zero.getByRole("cell").nth(1)).toHaveText("0");
  const missing = table.getByRole("row").filter({ hasText: "MISSING_PMC" });
  await expect(missing).toContainText("Not collected");
  await expect(missing).toContainText("No event association");
  await table.getByText("Ambiguous: 2 dispatches", { exact: true }).click();
  await expect(
    table
      .getByRole("row")
      .filter({ hasText: "AMBIGUOUS_PMC" })
      .getByRole("button"),
  ).toHaveCount(2);
  await expect(
    table.getByRole("row").filter({ hasText: "EVENT_PMC" }),
  ).toContainText("Event without a kernel dispatch");
  await expect(page.locator(".line-plot")).toHaveCount(capture.counters.length);
  await page.getByLabel("Filter event counters").fill("MISSING_PMC");
  await expect(table.getByRole("row")).toHaveCount(2);
  await page.getByLabel("Filter event counters").fill("");
  await zero.getByRole("button").click();
  await expect(page.getByLabel("Range start", { exact: true })).toHaveValue(
    String(capture.dispatches[0].start),
  );
  await expect(page.getByLabel("Range end", { exact: true })).toHaveValue(
    String(capture.dispatches[0].end),
  );
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("light theme persists and updates canvas and standalone exports", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  await page.goto("/");
  await expect(page.locator("canvas")).toBeVisible();
  const pixel = () =>
    page
      .locator("canvas")
      .first()
      .evaluate((node: HTMLCanvasElement) => {
        const ratio = node.width / node.getBoundingClientRect().width;
        return Array.from(
          node
            .getContext("2d")!
            .getImageData(Math.floor(ratio), Math.floor(28 * ratio), 1, 1).data,
        );
      });
  await expect.poll(pixel).toEqual([20, 24, 31, 255]);
  await page.getByLabel("Color theme").selectOption("light");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await expect.poll(pixel).toEqual([255, 255, 255, 255]);
  await expect(page.locator(".topbar")).toHaveCSS(
    "background-color",
    "rgb(255, 255, 255)",
  );
  const pngPending = page.waitForEvent("download");
  await page.getByRole("button", { name: /Export .* as PNG/ }).click();
  const pngPath = await (await pngPending).path();
  const pngBytes = Array.from(await readFile(pngPath!));
  expect(
    await page.evaluate(async (bytes) => {
      const url = URL.createObjectURL(
        new Blob([new Uint8Array(bytes)], { type: "image/png" }),
      );
      try {
        const image = new Image();
        image.src = url;
        await image.decode();
        const canvas = document.createElement("canvas");
        canvas.width = image.width;
        canvas.height = image.height;
        const context = canvas.getContext("2d")!;
        context.drawImage(image, 0, 0);
        return Array.from(context.getImageData(1, 1, 1, 1).data);
      } finally {
        URL.revokeObjectURL(url);
      }
    }, pngBytes),
  ).toEqual([255, 255, 255, 255]);
  await page.screenshot({
    path: test.info().outputPath("light-theme.png"),
    fullPage: true,
    animations: "disabled",
  });
  await page.reload();
  await expect(page.getByLabel("Color theme")).toHaveValue("light");
  await expect.poll(pixel).toEqual([255, 255, 255, 255]);
  await page.getByRole("button", { name: "Occupancy", exact: true }).click();
  const pending = page.waitForEvent("download");
  await page
    .getByRole("button", { name: /Export .* as SVG/ })
    .first()
    .click();
  const path = await (await pending).path();
  const xml = await readFile(path!, "utf8");
  expect(xml).toContain('fill="#ffffff"');
  expect(xml).toContain("stroke: rgb(186, 199, 213)");
  expect(xml).toContain("fill: rgb(82, 100, 119)");
  await page
    .getByRole("button", { name: "ISA reference", exact: true })
    .click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await page.getByLabel("Color theme").selectOption("dark");
  await page.reload();
  await expect(page.getByLabel("Color theme")).toHaveValue("dark");
  await expect.poll(pixel).toEqual([20, 24, 31, 255]);
});

test("unavailable preference storage leaves theme selection usable", async ({
  page,
}) => {
  await page.addInitScript(() => {
    Storage.prototype.setItem = () => {
      throw new Error("Storage unavailable");
    };
  });
  await page.goto("/");
  await page.getByLabel("Color theme").selectOption("light");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await expect(
    page.getByText(
      "Theme changed for this session; preference storage is unavailable.",
      { exact: true },
    ),
  ).toBeVisible();
  await page.getByLabel("Color theme").selectOption("dark");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
});

test("histogram bounds commit drafts, persist independently and reset to data", async ({
  page,
}) => {
  await page.goto("/");
  await page
    .getByRole("button", { name: "Instruction hotspots", exact: true })
    .click();
  const end = page.getByLabel("Histogram range end");
  const start = page.getByLabel("Histogram range start");
  const automatic = await end.inputValue();
  const titles = () =>
    page
      .locator(".histogram button")
      .evaluateAll((nodes) => nodes.map((n) => n.title));
  const before = await titles();
  await end.fill("200.5");
  expect(await titles()).toEqual(before);
  await end.press("Enter");
  expect(await titles()).not.toEqual(before);
  await start.fill("10.5");
  await start.press("Enter");
  const applied = await titles();
  await end.fill("");
  await end.press("Enter");
  await expect(end).toHaveValue("200.5");
  expect(await titles()).toEqual(applied);
  await start.fill("250");
  await start.press("Enter");
  await expect(start).toHaveValue("10.5");
  expect(await titles()).toEqual(applied);
  await end.fill("300");
  await end.press("Escape");
  await expect(end).toHaveValue("200.5");
  await page.getByLabel("Histogram bins").selectOption("8");
  await page.getByRole("button", { name: "Occupancy", exact: true }).click();
  await page.getByLabel("Range end", { exact: true }).fill("3000");
  await page.getByLabel("Range end", { exact: true }).press("Enter");
  await page
    .getByRole("button", { name: "Instruction hotspots", exact: true })
    .click();
  await expect(end).toHaveValue("200.5");
  await expect(start).toHaveValue("10.5");
  await expect(page.getByLabel("Histogram bins")).toHaveValue("8");
  await expect(page.locator(".histogram button")).toHaveCount(8);
  const pending = page.waitForEvent("download");
  await page
    .getByRole("button", {
      name: "Export Instruction cost distribution as SVG",
      exact: true,
    })
    .click();
  const path = await (await pending).path();
  const { readFile } = await import("node:fs/promises");
  const xml = await readFile(path!, "utf8");
  expect(xml).toContain(
    "custom histogram range · mean 10.5–200.5 cycles/issue",
  );
  await page
    .getByRole("button", { name: "Automatic histogram range", exact: true })
    .click();
  await expect(start).toHaveValue("0");
  await expect(end).toHaveValue(automatic);
});

test("source flamegraph viewport clips weights, persists navigation and exports its visible interval", async ({
  page,
}) => {
  await page.goto("/");
  await page
    .getByRole("button", { name: "Source flamegraph", exact: true })
    .click();
  const start = page.getByLabel("Flamegraph range start"),
    end = page.getByLabel("Flamegraph range end");
  const total = Number(await end.inputValue());
  const geometryErrors = await page.locator(".flames").evaluate((host) => {
    const width = host.getBoundingClientRect().width;
    const errors: number[] = [];
    for (const group of host.children) {
      const actual = group.getBoundingClientRect().width;
      errors.push(
        Math.abs(
          actual -
            (width * parseFloat((group as HTMLElement).style.width)) / 100,
        ),
      );
      for (const leaf of group.querySelectorAll(".flame-leaves button"))
        errors.push(
          Math.abs(
            leaf.getBoundingClientRect().width -
              (actual * parseFloat((leaf as HTMLElement).style.width)) / 100,
          ),
        );
    }
    return errors;
  });
  expect(Math.max(...geometryErrors)).toBeLessThan(0.1);
  await page
    .getByRole("button", { name: "Zoom flamegraph in", exact: true })
    .click();
  await expect(start).toHaveValue(String(total / 4));
  await expect(end).toHaveValue(String((total * 3) / 4));
  await page
    .getByRole("button", { name: "Pan flamegraph right", exact: true })
    .click();
  await expect(start).toHaveValue(String(total / 2));
  await expect(end).toHaveValue(String(total));
  await page.locator(".flames > div > button").first().click();
  await expect(start).toHaveValue("0");
  await page.getByRole("button", { name: "Reset zoom", exact: true }).click();
  await expect(start).toHaveValue(String(total / 2));
  await page
    .getByRole("button", { name: "Instruction hotspots", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Source flamegraph", exact: true })
    .click();
  await expect(start).toHaveValue(String(total / 2));
  await page
    .getByRole("button", { name: "Full flamegraph range", exact: true })
    .click();
  await expect(start).toHaveValue("0");
  await expect(end).toHaveValue(String(total));
  await end.fill("0.5");
  await end.press("Enter");
  await expect(end).toHaveValue("0.5");
  const leaves = page.locator(".flame-leaves button");
  await expect(leaves).toHaveCount(1);
  const instruction = await leaves.first().textContent();
  const pending = page.waitForEvent("download");
  await page
    .getByRole("button", {
      name: "Export Source flamegraph as SVG",
      exact: true,
    })
    .click();
  const path = await (await pending).path();
  const { readFile } = await import("node:fs/promises");
  const xml = await readFile(path!, "utf8");
  expect(xml).toContain("visible cumulative weight 0–0.5 cycles");
  expect(xml).toContain("visible weight interval 0–0.5");
  await leaves.first().click();
  await expect(page.locator("tbody tr.selected")).toContainText(instruction!);
});

test("marker flamegraph viewport clips nested durations and keeps hardware scopes independent", async ({
  page,
}) => {
  await page.goto("/");
  await page
    .getByRole("button", { name: "Marker scopes", exact: true })
    .click();
  const start = page.getByLabel("Marker flamegraph range start"),
    end = page.getByLabel("Marker flamegraph range end");
  await expect(end).toHaveValue("2150");
  await end.fill("1000");
  await end.press("Enter");
  await start.fill("500");
  await start.press("Enter");
  const flame = page.getByRole("img", {
    name: "Nested marker duration flamegraph",
  });
  const load = flame.getByRole("button", {
    name: "Load tile: 550 cycles",
    exact: true,
  });
  await expect(load.locator("rect")).toHaveAttribute("x", "0");
  await expect(load.locator("rect")).toHaveAttribute("width", "100");
  const transpose = flame.getByRole("button", {
    name: "Transpose tile: 1,100 cycles",
    exact: true,
  });
  await expect(transpose.locator("rect")).toHaveAttribute("x", "100");
  await expect(transpose.locator("rect")).toHaveAttribute("width", "900");
  await load.press("Enter");
  await expect(
    page.getByRole("heading", { name: "Load tile", exact: true }),
  ).toBeVisible();
  const pending = page.waitForEvent("download");
  await page
    .getByRole("button", {
      name: "Export Marker flamegraph as SVG",
      exact: true,
    })
    .click();
  const path = await (await pending).path();
  const { readFile } = await import("node:fs/promises");
  const xml = await readFile(path!, "utf8");
  expect(xml).toContain("visible cumulative duration 500–1000");
  expect(xml).toContain('width="100"');
  expect(xml).toContain('width="900"');
  await page.getByLabel("Marker hardware scope").selectOption({ index: 1 });
  await expect(start).toHaveValue("0");
  await expect(end).toHaveValue("2150");
  await page.getByLabel("Marker hardware scope").selectOption("all");
  await expect(start).toHaveValue("500");
  await expect(end).toHaveValue("1000");
  await page
    .getByRole("button", { name: "Zoom marker flamegraph in", exact: true })
    .click();
  await expect(start).toHaveValue("625");
  await expect(end).toHaveValue("875");
  await page
    .getByRole("button", { name: "Pan marker flamegraph right", exact: true })
    .click();
  await expect(start).toHaveValue("750");
  await expect(end).toHaveValue("1000");
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  await page
    .getByRole("button", { name: "Marker scopes", exact: true })
    .click();
  await expect(start).toHaveValue("750");
  await expect(end).toHaveValue("1000");
  await page
    .getByRole("button", { name: "Full marker flamegraph range", exact: true })
    .click();
  await expect(start).toHaveValue("0");
  await expect(end).toHaveValue("2150");
  await page.getByLabel("Range start", { exact: true }).fill("1000");
  await page.getByLabel("Range start", { exact: true }).press("Enter");
  await expect(end).toHaveValue("1800");
  await page.route("**/demo.json", async (route) => {
    const response = await route.fetch();
    const data = await response.json();
    data.capture.unit = "ns";
    await route.fulfill({ json: data });
  });
  await page.reload();
  await page
    .getByRole("button", { name: "Marker scopes", exact: true })
    .click();
  await expect(
    page.getByRole("button", { name: "Load tile: 550 ns", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByText("Cumulative root duration (ns)", { exact: true }),
  ).toBeVisible();
});

test("raw ATT setup maps arbitrary files, preserves 64-bit IDs and saves before decoding", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  const capture = JSON.parse(
    await readFile("public/demo.json", "utf8"),
  ).capture;
  await page.addInitScript((capture) => {
    const w = window as any;
    w.isTauri = true;
    w.picks = [
      "/capture/unlabeled.att",
      ["/capture/second.bin"],
      ["/capture/kernel.bin"],
    ];
    w.opened = [];
    w.manifests = [];
    w.cancelManifest = true;
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, args: any) => {
        if (command === "begin_wave_query") return args.requestId;
        if (command === "cancel_wave_query") return null;
        if (command === "plugin:dialog|open") return w.picks.shift();
        if (command === "save_att_manifest") {
          w.manifests.push(args.manifest);
          return w.cancelManifest ? null : "/capture/session.rocprof-att";
        }
        if (command === "open_capture") {
          w.opened.push(args.path);
          return {
            ...capture,
            id: "explicit-att",
            name: "Explicit ATT",
            waves: [],
          };
        }
        if (command === "analyze_hidden")
          return { byInstruction: {}, byWave: {}, model: "test" };
        throw Error(command);
      },
    };
  }, capture);
  await page.goto("/");
  await expect(page.locator("canvas")).toBeVisible();
  await page.getByRole("button", { name: "ATT setup", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "Save manifest and decode", exact: true }),
  ).toBeDisabled();
  await page
    .getByRole("button", { name: "Cancel ATT setup", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Open capture file", exact: true })
    .click();
  const dialog = page.getByRole("dialog", { name: "Raw ATT input setup" });
  await expect(dialog).toBeVisible();
  await expect(page.getByLabel("Shader engine 1", { exact: true })).toHaveValue(
    "",
  );
  await page
    .getByRole("button", { name: "Add ATT files", exact: true })
    .click();
  await page.getByLabel("Shader engine 1", { exact: true }).fill("0");
  await page.getByLabel("Shader engine 2", { exact: true }).fill("0");
  await page
    .getByRole("button", { name: "Save manifest and decode", exact: true })
    .click();
  await expect(dialog.getByRole("alert")).toContainText("unique shader engine");
  expect(await page.evaluate(() => (window as any).manifests.length)).toBe(0);
  await page.getByLabel("Shader engine 2", { exact: true }).fill("2");
  await page.getByLabel("ATT run identifier").fill("7");
  await page
    .getByRole("button", { name: "Add code objects", exact: true })
    .click();
  await page
    .getByLabel("Code object ID 1", { exact: true })
    .fill("18446744073709551615");
  await page
    .getByRole("button", { name: "Save manifest and decode", exact: true })
    .click();
  await expect(dialog).toBeVisible();
  expect(await page.evaluate(() => (window as any).opened)).toEqual([]);
  await page.evaluate(() => ((window as any).cancelManifest = false));
  await page
    .getByRole("button", { name: "Save manifest and decode", exact: true })
    .click();
  await expect(dialog).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "Explicit ATT DEMO", exact: true }),
  ).toBeVisible();
  const recorded = await page.evaluate(() => ({
    manifest: (window as any).manifests.at(-1),
    opened: (window as any).opened,
  }));
  expect(recorded.opened).toEqual(["/capture/session.rocprof-att"]);
  expect(recorded.manifest).toEqual({
    version: 1,
    run: 7,
    traces: [
      { path: "/capture/unlabeled.att", shaderEngine: 0 },
      { path: "/capture/second.bin", shaderEngine: 2 },
    ],
    codeObjects: [{ path: "/capture/kernel.bin", id: "18446744073709551615" }],
  });
});

test("diagnostic download records analysis context without claiming performance qualification", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  await page.goto("/");
  await page
    .getByRole("button", { name: "Capture evidence", exact: false })
    .click();
  const pending = page.waitForEvent("download");
  await page
    .getByRole("button", { name: "Export diagnostic manifest", exact: true })
    .click();
  const download = await pending;
  expect(download.suggestedFilename()).toBe("rocprof-diagnostics.json");
  const report = JSON.parse(await readFile((await download.path())!, "utf8"));
  expect(report.kind).toBe("rocprof-studio-diagnostics");
  expect(report.application.version).toBe("0.1.0");
  expect(report.runtime.environment).toBe("browser preview");
  expect(report.inventory.instructions).toBe(12);
  expect(report.inventory.waveIndexes).toBe(24);
  expect(report.workspace.view).toBe("evidence");
  expect(report.workspace.location.range).toEqual([500, 7240]);
  expect(report.capture.warnings.length).toBeGreaterThan(0);
  expect(report.qualification.replayCoverageVerified).toBe(false);
  expect(report.sourceRegistry).toContain("/transpose.hip");
  expect(report.capture).not.toHaveProperty("sources");
  await expect(page.getByRole("status")).toContainText(
    "Diagnostic manifest saved",
  );
});

test("native diagnostic export preserves source hashes and reports cancellation or write errors", async ({
  page,
}) => {
  await page.addInitScript(() => {
    const w = window as any;
    w.isTauri = true;
    w.reportResult = false;
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, args: any) => {
        if (command === "import_source_file")
          return { name: "mapped.hip", text: "abc" };
        if (command === "diagnostic_context")
          return {
            environment: "desktop",
            backendVersion: "0.1.0",
            inputPath: null,
            decoder: { status: "configuration only" },
          };
        if (command === "export_diagnostics") {
          w.report = JSON.parse(
            new TextDecoder().decode(new Uint8Array(args.bytes)),
          );
          if (w.reportResult === "error") throw Error("Read-only destination");
          return w.reportResult;
        }
        throw Error(command);
      },
    };
  });
  await page.goto("/");
  await page
    .getByRole("button", { name: "Map source file", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Capture evidence", exact: false })
    .click();
  const button = page.getByRole("button", {
    name: "Export diagnostic manifest",
    exact: true,
  });
  await button.click();
  await expect(page.getByRole("status")).toContainText(
    "Diagnostic export cancelled",
  );
  const report = await page.evaluate(() => (window as any).report);
  expect(report.runtime.environment).toBe("desktop");
  expect(report.sourceReplacements[0].sha256).toBe(
    "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
  );
  expect(report.sourceReplacements[0]).not.toHaveProperty("text");
  await page.evaluate(() => ((window as any).reportResult = "error"));
  await button.click();
  await expect(page.getByRole("status")).toContainText("Read-only destination");
  await expect(button).toBeEnabled();
  await page.evaluate(() => ((window as any).reportResult = true));
  await button.click();
  await expect(page.getByRole("status")).toContainText(
    "Diagnostic manifest saved",
  );
});

test("timeline detail aggregates dense events without arbitrary selection and persists per view", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const response = await route.fetch();
    const bundle = await response.json();
    const id = bundle.capture.waves[0].id;
    bundle.capture.start = 0;
    bundle.capture.end = 1000;
    bundle.capture.waves = [
      { ...bundle.capture.waves[0], start: 0, end: 1000 },
    ];
    bundle.waves[id] = {
      ...bundle.waves[id],
      start: 0,
      end: 1000,
      tokens: [
        ...Array.from({ length: 400 }, (_, n) => ({
          start: 100 + n / 100,
          duration: 0.005,
          stall: 0,
          category: 6,
          instruction: 8,
        })),
        { start: 800, duration: 20, stall: 0, category: 6, instruction: 1 },
      ],
    };
    await route.fulfill({ json: bundle });
  });
  await page.goto("/");
  const detail = page.getByLabel("Timeline detail", { exact: true });
  await expect(detail).toHaveValue("0");
  await expect(page.getByLabel("Timeline detail summary")).toContainText(
    "401 events · 401 drawn marks",
  );
  await detail.selectOption("8");
  const summary = await page
    .getByLabel("Timeline detail summary")
    .textContent();
  const drawn = Number(
    summary!.match(/· ([\d,]+) drawn/)![1].replaceAll(",", ""),
  );
  expect(drawn).toBeLessThan(10);
  const canvas = page.locator("canvas");
  const box = await canvas.boundingBox();
  if (!box) throw Error("Missing timeline");
  await canvas.hover({
    position: { x: 155 + 0.101 * (box.width - 169), y: 40 },
  });
  await expect(canvas.locator("..").locator(".plot-footer")).toContainText(
    "aggregated span",
  );
  await canvas.click({
    position: { x: 155 + 0.101 * (box.width - 169), y: 40 },
  });
  await expect
    .poll(
      async () =>
        Number(
          await page.getByLabel("Range end", { exact: true }).inputValue(),
        ) -
        Number(
          await page.getByLabel("Range start", { exact: true }).inputValue(),
        ),
    )
    .toBeLessThan(10);
  await expect(page.locator(".isa-table tr.selected td").first()).toHaveText(
    "1",
  );
  await page.getByRole("button", { name: "Global", exact: true }).click();
  await expect(detail).toHaveValue("0");
  await page.getByRole("button", { name: "Target wave", exact: true }).click();
  await expect(detail).toHaveValue("8");
  await page.reload();
  await expect(detail).toHaveValue("8");
  await detail.selectOption("0");
  await expect(page.getByLabel("Timeline detail summary")).toContainText(
    "401 events · 401 drawn marks",
  );
});

test("ISA capture mean, wave maximum and selected occurrence retain their scopes", async ({
  page,
}) => {
  await page.goto("/");
  const scope = page.getByLabel("Latency scope");
  const row = page
    .locator(".isa-table tbody tr")
    .filter({ hasText: "s_load_dwordx4" });
  await scope.selectOption("capture-mean");
  await expect(row.locator("td").nth(4)).toHaveText("18");
  await scope.selectOption("max");
  await expect(row.locator("td").nth(4)).toHaveText("18");
  const end = page.getByLabel("Range end", { exact: true });
  await end.fill("501");
  await end.press("Enter");
  await expect(row.locator("td").nth(4)).toHaveText("18");
  await scope.selectOption("iteration");
  const occurrence = page.getByLabel("Loop occurrence");
  await occurrence.fill("1");
  await expect(row.locator("td").nth(3)).toHaveText("1");
  await expect(row.locator("td").nth(4)).toHaveText("18");
  await occurrence.fill("2");
  await expect(row.locator("td").nth(3)).toHaveText("1");
  await expect(row.locator("td").nth(4)).toHaveText("18");
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  await page
    .getByRole("button", { name: "Trace workspace", exact: true })
    .click();
  await expect(scope).toHaveValue("iteration");
  await expect(occurrence).toHaveValue("2");
});

test("global dispatch and marker layers navigate their own records without guessing waves", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const b = await (await route.fetch()).json();
    b.capture.start = 0;
    b.capture.end = 1000;
    b.capture.intervals = [
      { start: 100, end: 900, se: 0, cu: 2, simd: 0, slot: 0, kernel: "same" },
    ];
    const d = b.capture.dispatches[0];
    b.capture.dispatches = [
      { ...d, id: "ignored", start: 0, end: 0 },
      { ...d, id: "42", kernel: "same", start: 400, end: 600 },
      { ...d, id: "43", kernel: "same", start: 700, end: 900 },
    ];
    b.capture.markers = [
      {
        id: 0,
        parent: null,
        markerId: 7,
        codeObject: "1",
        name: "Load scope",
        kind: "scope",
        source: "transpose.hip:9",
        start: 100,
        end: 700,
        point: false,
        depth: 0,
        se: 0,
        cu: 2,
        simd: 0,
        slot: 0,
      },
    ];
    await route.fulfill({ json: b });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Global", exact: true }).click();
  await page.getByLabel("Show global residency").uncheck();
  await page.getByLabel("Show global dispatches").uncheck();
  const canvas = page.locator("canvas");
  const box = await canvas.boundingBox();
  if (!box) throw Error("No global canvas");
  await canvas.click({ position: { x: 155 + 0.3 * (box.width - 169), y: 40 } });
  await expect(page.getByLabel("Timeline record details")).toContainText(
    "Load scope",
  );
  await expect(page.getByLabel("Range start", { exact: true })).toHaveValue(
    "100",
  );
  await expect(page.getByLabel("Range end", { exact: true })).toHaveValue(
    "700",
  );
  await page
    .getByRole("button", { name: "Open marker analysis", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Marker scopes", exact: true, level: 1 }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Trace workspace", exact: true })
    .click();
  await page.getByLabel("Show global markers").uncheck();
  await page.getByLabel("Show global dispatches").check();
  await page
    .getByRole("button", { name: "Reset visible range", exact: true })
    .click();
  const nextBox = await canvas.boundingBox();
  if (!nextBox) throw Error("No dispatch canvas");
  await canvas.click({
    position: { x: 155 + 0.5 * (nextBox.width - 169), y: 40 },
  });
  await expect(page.getByLabel("Timeline record details")).toContainText(
    "Dispatch 42",
  );
  await expect(page.getByLabel("Range start", { exact: true })).toHaveValue(
    "400",
  );
  await expect(page.getByLabel("Range end", { exact: true })).toHaveValue(
    "600",
  );
  await expect(
    page.getByRole("button", { name: "Global", exact: true }),
  ).toHaveClass("active");
});

test("decoder point events preserve payloads and remain selectable at capture end", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const b = await (await route.fetch()).json();
    b.capture.start = 0;
    b.capture.end = 100;
    b.capture.intervals = [];
    b.capture.markers = [];
    b.capture.dispatches = [];
    b.capture.decoderEvents = [
      {
        id: "2:rinse",
        se: 2,
        time: 50,
        kind: "event",
        fields: { type: "16", me_id: "1", pipe_id: "0" },
      },
      {
        id: "2:0",
        se: 2,
        time: 10,
        kind: "dispatch",
        fields: { kernel_name: "same", me_id: "1", pipe_id: "0" },
      },
      {
        id: "2:1",
        se: 2,
        time: 100,
        kind: "event",
        fields: {
          type: "6",
          payload: "18446744073709551615",
          me_id: "1",
          pipe_id: "0",
        },
      },
    ];
    await route.fulfill({ json: b });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Global", exact: true }).click();
  const canvas = page.locator("canvas");
  const box = await canvas.boundingBox();
  if (!box) throw Error("Missing event plot");
  await canvas.click({ position: { x: box.width - 15, y: 40 } });
  await expect(page.getByLabel("Timeline record details")).toContainText(
    "Packet loss",
  );
  await expect(page.getByLabel("Timeline record details")).toContainText(
    "18446744073709551615",
  );
  await expect(page.getByLabel("Range start", { exact: true })).toHaveValue(
    "99",
  );
  await expect(page.getByLabel("Range end", { exact: true })).toHaveValue(
    "100",
  );
  await page.getByLabel("Show global events").uncheck();
  await expect(
    page.getByText("No records for this view", { exact: true }),
  ).toBeVisible();
  await page.getByLabel("Show global events").check();
  await expect(canvas).toBeVisible();
  await page
    .getByRole("button", { name: "Reset visible range", exact: true })
    .click();
  await page.getByText("Event groups", { exact: true }).click();
  await expect(
    page.getByLabel("Decoder group GC rinse", { exact: true }),
  ).not.toBeChecked();
  await page
    .getByLabel("Decoder group Other events", { exact: true })
    .uncheck();
  await page
    .getByLabel("Decoder group Dispatch starts", { exact: true })
    .uncheck();
  await expect(
    page.getByText("No records for this view", { exact: true }),
  ).toBeVisible();
  await page.getByLabel("Decoder group GC rinse", { exact: true }).check();
  await expect(page.getByLabel("Timeline detail summary")).toContainText(
    "1 events",
  );
  const filteredBox = await canvas.boundingBox();
  if (!filteredBox) throw Error("Missing filtered event plot");
  await canvas.click({
    position: { x: 155 + 0.5 * (filteredBox.width - 169) + 1, y: 40 },
  });
  await expect(page.getByLabel("Timeline record details")).toContainText(
    "GC rinse",
  );
});

test("decoder overlays select full-height matching-SE rows and obey group filters", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const b = await (await route.fetch()).json();
    b.capture.start = 0;
    b.capture.end = 1000;
    b.capture.dispatches = [];
    b.capture.markers = [];
    b.capture.intervals = [0, 1].map((se) => ({
      start: 100,
      end: 900,
      se,
      cu: 2,
      simd: 0,
      slot: 0,
      kernel: "same",
    }));
    b.capture.decoderEvents = [
      {
        id: "0:0",
        se: 0,
        time: 500,
        kind: "event",
        fields: { type: "6", me_id: "0", pipe_id: "0" },
      },
    ];
    await route.fulfill({ json: b });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Global", exact: true }).click();
  await expect(page.getByLabel("Timeline overlay summary")).toContainText(
    "1 decoder events",
  );
  const canvas = page.locator("canvas"),
    box = await canvas.boundingBox();
  if (!box) throw Error("Missing overlay canvas");
  const x = 155 + 0.5 * (box.width - 169);
  await canvas.hover({ position: { x, y: 80 } });
  await expect(canvas.locator("..").locator(".plot-footer")).not.toContainText(
    "Packet loss",
  );
  await canvas.hover({ position: { x, y: 52 } });
  await expect(canvas.locator("..").locator(".plot-footer")).toContainText(
    "Packet loss",
  );
  await canvas.click({ position: { x, y: 52 } });
  await expect(page.getByLabel("Timeline record details")).toContainText(
    "Packet loss",
  );
  await expect(
    page.getByRole("button", { name: "Global", exact: true }),
  ).toHaveClass("active");
  await page.getByText("Event groups", { exact: true }).click();
  await page
    .getByLabel("Decoder group Other events", { exact: true })
    .uncheck();
  await expect(page.getByLabel("Timeline overlay summary")).toHaveCount(0);
});

test("CU decoder overlays retain SE scope and independent event controls", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const b = await (await route.fetch()).json();
    b.capture.start = 0;
    b.capture.end = 1000;
    const index = b.capture.waves[0];
    b.capture.waves = [{ ...index, start: 100, end: 900 }];
    b.waves[index.id] = {
      ...b.waves[index.id],
      start: 100,
      end: 900,
      tokens: [
        { start: 100, duration: 800, stall: 0, category: 6, instruction: 1 },
      ],
    };
    b.capture.decoderEvents = [
      ...[1, 7, 8, 17].map((type) => ({
        id: `excluded${type}`,
        se: 0,
        time: 400,
        kind: "event",
        fields: { type: String(type) },
      })),
      { id: "rinse", se: 0, time: 500, kind: "event", fields: { type: "16" } },
      { id: "loss", se: 0, time: 600, kind: "event", fields: { type: "6" } },
      { id: "dispatch", se: 0, time: 300, kind: "dispatch", fields: {} },
      { id: "otherSE", se: 1, time: 500, kind: "event", fields: { type: "6" } },
    ];
    await route.fulfill({ json: b });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "CU waves", exact: true }).click();
  await expect(page.getByLabel("Timeline overlay summary")).toContainText(
    "3 decoder events",
  );
  const canvas = page.locator("canvas"),
    box = await canvas.boundingBox();
  if (!box) throw Error("No CU timeline");
  await canvas.click({ position: { x: 155 + 0.5 * (box.width - 169), y: 52 } });
  await expect(page.getByLabel("Timeline record details")).toContainText(
    "GC rinse",
  );
  await expect(
    page.getByRole("button", { name: "CU waves", exact: true }),
  ).toHaveClass("active");
  await page.getByLabel("Show CU events").uncheck();
  await expect(page.getByLabel("Timeline overlay summary")).toHaveCount(0);
  await page.getByRole("button", { name: "Global", exact: true }).click();
  await expect(page.getByLabel("Show global events")).toBeChecked();
  await page.getByRole("button", { name: "CU waves", exact: true }).click();
  await expect(page.getByLabel("Show CU events")).not.toBeChecked();
});

test("CU and utilization lanes separate heterogeneous CUs with identical SIMD slots", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const b = await (await route.fetch()).json();
    const first = b.capture.waves[0],
      data = b.waves[first.id];
    b.capture.start = 0;
    b.capture.end = 1000;
    b.capture.waves = [0, 1].map((wid) => ({
      ...first,
      id: `wave${wid}`,
      wid,
      start: 100,
      end: 900,
    }));
    b.waves = Object.fromEntries(
      [0, 1].map((wid) => [
        `wave${wid}`,
        {
          ...data,
          id: `wave${wid}`,
          cu: wid ? 4 : 0,
          start: 100,
          end: 900,
          tokens: [
            {
              start: 200,
              duration: 500,
              stall: 0,
              category: 6,
              instruction: wid + 1,
            },
          ],
        },
      ]),
    );
    await route.fulfill({ json: b });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "CU waves", exact: true }).click();
  const canvas = page.locator("canvas");
  await expect(canvas.locator("..").locator(".plot-footer")).toContainText(
    "2 lanes",
  );
  const box = await canvas.boundingBox();
  if (!box) throw Error("Missing CU plot");
  await canvas.hover({ position: { x: 155 + 0.4 * (box.width - 169), y: 40 } });
  await expect(canvas.locator("..").locator(".plot-footer")).toContainText(
    "CU 0 · SIMD 0 · slot 0 · wave 0",
  );
  await canvas.click({ position: { x: 155 + 0.4 * (box.width - 169), y: 68 } });
  await expect(page.getByLabel("Wave ID")).toHaveValue("1");
  await expect(page.locator(".isa-table tr.selected td").first()).toHaveText(
    "2",
  );
  await page.getByRole("button", { name: "Utilization", exact: true }).click();
  await expect(canvas.locator("..").locator(".plot-footer")).toContainText(
    "2 lanes",
  );
});

test("hidden-analysis qualification warnings are visible and clear after recomputation", async ({
  page,
}) => {
  let calls = 0;
  await page.route("**/demo-hidden.json", async (route) => {
    const result = await (await route.fetch()).json();
    result.warnings =
      ++calls === 1
        ? [
            "SE 0: other-SIMD activity omitted because its CU association is unresolved in this mixed-CU capture; hidden latency may be underestimated.",
          ]
        : [];
    await route.fulfill({ json: result });
  });
  await page.goto("/");
  await expect(page.getByRole("alert")).toContainText(
    "other-SIMD activity omitted",
  );
  await page
    .getByRole("button", { name: "Recompute hidden latency", exact: true })
    .click();
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("other-SIMD tracks expose recorded provenance without selecting an ISA instruction", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const b = await (await route.fetch()).json();
    b.capture.start = 0;
    b.capture.end = 1000;
    const first = b.capture.waves[0];
    b.capture.waves = [{ ...first, start: 0, end: 1000 }];
    b.waves[first.id].tokens = [];
    b.capture.otherSimd = [
      {
        se: first.se,
        wgp: 7,
        source: "other.json",
        start: 200,
        duration: 300,
        category: 4,
      },
      {
        se: first.se + 1,
        wgp: 8,
        source: "different-se.json",
        start: 100,
        duration: 600,
        category: 2,
      },
    ];
    await route.fulfill({ json: b });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Utilization", exact: true }).click();
  const canvas = page.locator("canvas");
  await expect(canvas.locator("..").locator(".plot-footer")).toContainText(
    "1 lanes",
  );
  await expect(page.locator(".isa-table tr.selected td").first()).toHaveText(
    "1",
  );
  await page.locator(".isa-table tbody tr").nth(2).click();
  const before = await page
    .locator(".isa-table tr.selected td")
    .first()
    .textContent();
  const box = await canvas.boundingBox();
  if (!box) throw Error("Missing utilization plot");
  await canvas.click({ position: { x: 155 + 0.3 * (box.width - 169), y: 40 } });
  await expect(
    page
      .getByRole("status", { name: "Timeline record details" })
      .getByText(/Other-SIMD activity · SE.*recorded WGP scope/),
  ).toBeVisible();
  await expect(page.locator(".isa-table tr.selected td").first()).toHaveText(
    before!,
  );
});

test("overlap chooser reaches covered utilization occurrences and paginates without changing ordinary clicks", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const b = await (await route.fetch()).json();
    const first = b.capture.waves[0];
    b.capture.start = 0;
    b.capture.end = 1000;
    b.capture.waves = [{ ...first, start: 0, end: 1000 }];
    b.waves[first.id].tokens = Array.from({ length: 25 }, () => ({
      start: 100,
      duration: 700,
      stall: 0,
      category: 6,
      instruction: 2,
    }));
    await route.fulfill({ json: b });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Utilization", exact: true }).click();
  const canvas = page.locator("canvas");
  await expect(page.getByLabel("Timeline detail summary")).toContainText(
    "25 events",
  );
  const box = await canvas.boundingBox();
  if (!box) throw Error("Missing utilization plot");
  const position = { x: 155 + 0.4 * (box.width - 169), y: 40 };
  await canvas.click({ position });
  await expect(page.locator(".iteration input")).toHaveValue("24");
  await canvas.click({ position, modifiers: ["Alt"] });
  const chooser = page.getByRole("region", {
    name: "Overlapping timeline records",
  });
  await expect(chooser).toContainText("25 records at this point");
  await expect(chooser.locator("li")).toHaveCount(20);
  await chooser.getByRole("button", { name: "Next overlap page" }).click();
  await expect(chooser.locator("li")).toHaveCount(5);
  await chooser.getByRole("button", { name: /token 0$/ }).click();
  await expect(page.locator(".iteration input")).toHaveValue("0");
  await expect(chooser).toHaveCount(0);
  await canvas.click({ position, modifiers: ["Alt"] });
  await expect(chooser).toBeVisible();
  await canvas.press("Escape");
  await expect(chooser).toHaveCount(0);
  await canvas.hover({ position });
  await page
    .getByRole("button", { name: "Choose overlaps", exact: true })
    .click();
  await expect(chooser).toBeVisible();
  await chooser.getByRole("button", { name: "Dismiss overlaps" }).click();
  await expect(chooser).toHaveCount(0);
  await canvas.press("o");
  await expect(chooser).toBeVisible();
  await canvas.press("Escape");
  await canvas.click({ position, modifiers: ["Shift", "Alt"] });
  await expect(chooser).toHaveCount(0);
  await expect(page.locator(".iteration input")).toHaveValue("0");
});

test("upstream mip detail reduces dense waves, zooms to original records and persists per view", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const b = await (await route.fetch()).json();
    const first = b.capture.waves[0];
    b.capture.start = 0;
    b.capture.end = 100000;
    b.capture.waves = [{ ...first, start: 0, end: 100000 }];
    b.waves[first.id].tokens = Array.from({ length: 100 }, (_, n) => ({
      start: 10000 + n,
      duration: 2,
      stall: 0,
      category: 6,
      instruction: 2,
    }));
    await route.fulfill({ json: b });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Utilization", exact: true }).click();
  await expect(page.getByLabel("Timeline detail summary")).toContainText(
    "100 drawn marks",
  );
  await page.getByLabel("Timeline detail", { exact: true }).selectOption("-1");
  await expect(page.getByLabel("Timeline detail summary")).toContainText("Mip");
  await expect(page.getByLabel("Timeline detail summary")).toContainText(
    "1 drawn marks",
  );
  const before = await page
    .locator(".isa-table tr.selected td")
    .first()
    .textContent();
  const canvas = page.locator("canvas"),
    box = await canvas.boundingBox();
  if (!box) throw Error("Missing mip timeline");
  await canvas.click({
    position: { x: 155 + 0.1 * (box.width - 169) + 1, y: 40 },
  });
  await expect(page.getByLabel("Timeline detail summary")).toContainText(
    "100 drawn marks",
  );
  await expect(page.locator(".isa-table tr.selected td").first()).toHaveText(
    before!,
  );
  await page.getByRole("button", { name: "Target wave", exact: true }).click();
  await expect(page.getByLabel("Timeline detail", { exact: true })).toHaveValue(
    "0",
  );
  await page.getByRole("button", { name: "Utilization", exact: true }).click();
  await expect(page.getByLabel("Timeline detail", { exact: true })).toHaveValue(
    "-1",
  );
  await page.reload();
  await page.getByRole("button", { name: "Utilization", exact: true }).click();
  await expect(page.getByLabel("Timeline detail", { exact: true })).toHaveValue(
    "-1",
  );
});

test("discrete mip zoom anchors at the pointer, respects limits and persists independently", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Utilization", exact: true }).click();
  await page.getByLabel("Timeline zoom mode").selectOption("mip");
  await page.getByLabel("Timeline mip level").selectOption("3");
  await expect(page.getByLabel("Timeline mip level")).toHaveValue("3");
  const range = async () => [
    Number(await page.getByLabel("Range start", { exact: true }).inputValue()),
    Number(await page.getByLabel("Range end", { exact: true }).inputValue()),
  ];
  const before = await range();
  const canvas = page.locator("canvas"),
    box = await canvas.boundingBox();
  if (!box) throw Error("Missing zoom plot");
  await page.evaluate(() =>
    window.addEventListener(
      "wheel",
      (event) => {
        const el = event.target as HTMLElement;
        const rect = el.getBoundingClientRect();
        (window as any).__zoomAnchor =
          (event.clientX - rect.left - 155) / (rect.width - 169);
      },
      { once: true, capture: true },
    ),
  );
  await canvas.hover({
    position: { x: 155 + 0.25 * (box.width - 169), y: 40 },
  });
  await page.mouse.wheel(0, -120);
  await expect(page.getByLabel("Timeline mip level")).toHaveValue("2");
  await expect
    .poll(async () => {
      const current = await range();
      return current[1] - current[0];
    })
    .toBeCloseTo((before[1] - before[0]) / 2, 5);
  const after = await range();
  const anchor = await page.evaluate(
    () => (window as any).__zoomAnchor as number,
  );
  expect(after[1] - after[0]).toBeCloseTo((before[1] - before[0]) / 2, 5);
  expect(after[0] + anchor * (after[1] - after[0])).toBeCloseTo(
    before[0] + anchor * (before[1] - before[0]),
    4,
  );
  await page.getByRole("button", { name: "Zoom out one mip level" }).click();
  await expect(page.getByLabel("Timeline mip level")).toHaveValue("3");
  await page.getByLabel("Timeline mip level").selectOption("-2");
  await expect(
    page.getByRole("button", { name: "Zoom in one mip level" }),
  ).toBeDisabled();
  await page.getByRole("button", { name: "Target wave", exact: true }).click();
  await expect(page.getByLabel("Timeline zoom mode")).toHaveValue("continuous");
  await page.getByRole("button", { name: "Utilization", exact: true }).click();
  await expect(page.getByLabel("Timeline zoom mode")).toHaveValue("mip");
  await page.reload();
  await page.getByRole("button", { name: "Utilization", exact: true }).click();
  await expect(page.getByLabel("Timeline zoom mode")).toHaveValue("mip");
  await page.getByLabel("Timeline zoom mode").selectOption("continuous");
  await expect(page.getByLabel("Timeline mip level")).toHaveCount(0);
});

test("wave timeline controls remain mounted across empty-window refreshes", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const b = await (await route.fetch()).json(),
      first = b.capture.waves[0];
    b.capture.start = 0;
    b.capture.end = 1000;
    b.capture.waves = [{ ...first, start: 0, end: 100 }];
    b.waves[first.id].tokens = [
      { start: 20, duration: 50, stall: 0, category: 6, instruction: 2 },
    ];
    await route.fulfill({ json: b });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Utilization", exact: true }).click();
  await expect(page.getByLabel("Timeline detail summary")).toContainText(
    "1 drawn marks",
  );
  const canvas = page.locator("canvas");
  await canvas.evaluate((el) => {
    el.dataset.mountIdentity = "original";
  });
  await page.getByLabel("Range start", { exact: true }).fill("200");
  await page.getByLabel("Range start", { exact: true }).press("Enter");
  await page.getByLabel("Range end", { exact: true }).fill("400");
  await page.getByLabel("Range end", { exact: true }).press("Enter");
  await expect(page.getByLabel("Timeline detail summary")).toContainText(
    "0 drawn marks",
  );
  await expect(canvas).toHaveAttribute("data-mount-identity", "original");
  await expect(page.getByLabel("Timeline zoom mode")).toBeVisible();
  await page.getByRole("button", { name: "Reset visible range" }).click();
  await expect(page.getByLabel("Timeline detail summary")).toContainText(
    "1 drawn marks",
  );
  await expect(canvas).toHaveAttribute("data-mount-identity", "original");
});

test("captured source paging jumps to distant ISA lines without fetching the full file", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  const b = JSON.parse(await readFile("public/demo.json", "utf8"));
  const capture = {
    ...b.capture,
    id: "large-source",
    name: "Large source",
    waves: [],
    sources: {},
    sourceFiles: ["/large.hip"],
  };
  capture.instructions[1].source = "/large.hip:250005";
  await page.addInitScript((capture) => {
    const w = window as any;
    w.isTauri = true;
    w.sourcePageReads = [];
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, args: any) => {
        if (command === "plugin:dialog|open") return "/capture";
        if (command === "open_capture") return capture;
        if (command === "read_source_page") {
          w.sourcePageReads.push({
            startLine: args.startLine,
            count: args.count,
          });
          return {
            startLine: args.startLine,
            totalLines: 250401,
            lines: Array.from(
              { length: Math.min(200, 250402 - args.startLine) },
              (_, i) => `source line ${args.startLine + i}`,
            ),
          };
        }
        throw Error(command);
      },
    };
  }, capture);
  await page.goto("/");
  await page
    .getByRole("button", { name: "Open capture file", exact: true })
    .click();
  await expect(page.locator(".source-selected")).toContainText(
    "source line 250005",
  );
  await expect(page.locator(".source button")).toHaveCount(200);
  await expect
    .poll(() => page.evaluate(() => (window as any).sourcePageReads))
    .toContainEqual({ startLine: 250001, count: 200 });
  await page.getByRole("button", { name: "Next source lines" }).click();
  await expect(page.locator(".source button").first()).toContainText(
    "source line 250201",
  );
  await page.getByRole("button", { name: "Selected source line" }).click();
  await expect(page.locator(".source-selected")).toContainText(
    "source line 250005",
  );
  await expect
    .poll(() =>
      page.evaluate(() =>
        (window as any).sourcePageReads.every((r: any) => r.count === 200),
      ),
    )
    .toBe(true);
});

test("native source references page large mappings and surface changed-file errors", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  const b = JSON.parse(await readFile("public/demo.json", "utf8"));
  const capture = {
    ...b.capture,
    id: "reference-source",
    waves: [],
    sources: {},
    sourceFiles: [],
  };
  capture.instructions[1].source = "/build/large.hip:250005";
  await page.addInitScript((capture) => {
    const w = window as any;
    w.isTauri = true;
    w.referenceReads = [];
    w.referenceError = false;
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, args: any) => {
        if (command === "plugin:dialog|open") return "/capture";
        if (command === "open_capture") return capture;
        if (command === "save_project") {
          w.savedReferenceProject = structuredClone(args);
          return true;
        }
        if (command === "open_project")
          return {
            captures: [capture],
            project: {
              schemaVersion: 2,
              active: 0,
              entries: w.savedReferenceProject.entries
                .filter((e: any) => e.captureId === capture.id)
                .map((e: any) => ({
                  input: "/capture",
                  state: e.state,
                  sources: e.sources,
                })),
            },
          };
        if (command === "import_source_file")
          return {
            name: "large.hip",
            text: "",
            reference: {
              path: "/local/large.hip",
              size: 4000000,
              sha256: "a".repeat(64),
            },
          };
        if (command === "read_source_reference_page") {
          w.referenceReads.push(args);
          if (w.referenceError)
            throw Error(
              "Referenced source changed since it was mapped. Map the file again to accept its current contents.",
            );
          return {
            startLine: args.startLine,
            totalLines: 300000,
            lines: Array.from(
              { length: 200 },
              (_, i) => "referenced line " + (args.startLine + i),
            ),
          };
        }
        throw Error(command);
      },
    };
  }, capture);
  await page.goto("/");
  await page
    .getByRole("button", { name: "Open capture file", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Map source file", exact: true })
    .click();
  await expect(page.locator(".source-selected")).toContainText(
    "referenced line 250005",
  );
  await expect(page.locator(".source > button")).toHaveCount(200);
  await expect(
    page.getByText("checked against saved SHA-256", { exact: false }),
  ).toBeVisible();
  expect(
    await page.evaluate(
      () => (window as any).referenceReads.at(-1).reference.path,
    ),
  ).toBe("/local/large.hip");
  await page.getByRole("button", { name: "Save project", exact: true }).click();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as any).savedReferenceProject?.entries.find(
            (e: any) => e.captureId === "reference-source",
          )?.sources["/build/large.hip"]?.reference?.size,
      ),
    )
    .toBe(4000000);
  await page
    .getByRole("button", { name: "Restore captured source", exact: true })
    .click();
  await page.getByRole("button", { name: "Open project", exact: true }).click();
  await expect(page.locator(".source-selected")).toContainText(
    "referenced line 250005",
  );
  await page.evaluate(() => {
    (window as any).referenceError = true;
  });
  await page.getByRole("button", { name: "Next source lines" }).click();
  await expect(page.getByRole("alert")).toContainText(
    "changed since it was mapped",
  );
  await expect(page.locator(".source > button")).toHaveCount(0);
  await page.evaluate(() => {
    (window as any).referenceError = false;
  });
  await page.getByRole("button", { name: "Retry source reference" }).click();
  await expect(page.locator(".source > button")).toHaveCount(200);
  await page
    .getByRole("button", { name: "Restore captured source", exact: true })
    .click();
  await expect(
    page.getByText("No captured source for this instruction.", {
      exact: false,
    }),
  ).toBeVisible();
});

test("timeline preferences are isolated by capture and restored from projects without replacing machine defaults", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  const bundle = JSON.parse(await readFile("public/demo.json", "utf8"));
  await page.addInitScript((bundle) => {
    const w = window as any;
    w.isTauri = true;
    const second = {
      ...bundle.capture,
      id: "second",
      name: "Second preferences",
    };
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, args: any) => {
        if (command === "plugin:dialog|open") return "/second";
        if (command === "open_capture") return second;
        if (command === "read_wave") return bundle.waves[args.waveId];
        if (command === "begin_wave_query") return args.requestId;
        if (command === "cancel_wave_query") return null;
        if (command === "read_wave_window") return Object.values(bundle.waves);
        if (command === "save_project") {
          w.saved = structuredClone(args);
          return true;
        }
        if (command === "open_project")
          return {
            captures: [{ ...bundle.capture, id: "demo" }, second],
            project: {
              schemaVersion: 1,
              active: w.saved.active,
              entries: w.saved.entries.map((e: any) => ({
                input: null,
                state: e.state,
                sources: e.sources,
              })),
            },
          };
        throw Error(command);
      },
    };
  }, bundle);
  await page.goto("/");
  await expect(page.locator("canvas")).toBeVisible();
  await page.getByLabel("Timeline lane height").selectOption("56");
  await page.getByLabel("Timeline detail", { exact: true }).selectOption("4");
  await page.getByLabel("Token layout", { exact: true }).selectOption("flat");
  await page.getByLabel("Timeline zoom mode").selectOption("mip");
  await page
    .getByRole("button", { name: "Open capture file", exact: true })
    .click();
  await expect(page.getByLabel("Timeline lane height")).toHaveValue("56");
  await page.getByLabel("Timeline lane height").selectOption("20");
  await page.getByLabel("Timeline detail", { exact: true }).selectOption("8");
  await page
    .getByRole("button", { name: bundle.capture.name + " DEMO", exact: true })
    .click();
  await expect(page.getByLabel("Timeline lane height")).toHaveValue("56");
  await expect(page.getByLabel("Timeline detail", { exact: true })).toHaveValue(
    "4",
  );
  await expect(page.getByLabel("Token layout", { exact: true })).toHaveValue(
    "flat",
  );
  await page.getByRole("button", { name: "Global", exact: true }).click();
  await page.getByLabel("Timeline lane height").selectOption("42");
  await page.getByRole("button", { name: "Target wave", exact: true }).click();
  await page
    .getByRole("button", { name: "Marker scopes", exact: true })
    .click();
  await page.getByLabel("Timeline lane height").selectOption("20");
  await page
    .getByRole("button", { name: "Trace workspace", exact: true })
    .click();
  await page.getByRole("button", { name: "Save project", exact: true }).click();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as any).saved?.entries[0].state.timelinePreferences.wave
            .laneHeight,
      ),
    )
    .toBe(56);
  await page.getByLabel("Timeline lane height").selectOption("28");
  await page.getByLabel("Timeline zoom mode").selectOption("continuous");
  await page.getByRole("button", { name: "Open project", exact: true }).click();
  await expect(page.getByLabel("Timeline lane height")).toHaveValue("56");
  await expect(page.getByLabel("Timeline zoom mode")).toHaveValue("mip");
  expect(
    await page.evaluate(
      () =>
        JSON.parse(localStorage.getItem("rocprof.timeline.v1.wave")!)
          .laneHeight,
    ),
  ).toBe(28);
  await page.getByRole("button", { name: "Global", exact: true }).click();
  await expect(page.getByLabel("Timeline lane height")).toHaveValue("42");
  await page
    .getByRole("button", { name: "Marker scopes", exact: true })
    .click();
  await expect(page.getByLabel("Timeline lane height")).toHaveValue("20");
  await page
    .getByRole("button", { name: "Trace workspace", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Second preferences DEMO", exact: true })
    .click();
  await expect(page.getByLabel("Timeline lane height")).toHaveValue("20");
  await expect(page.getByLabel("Timeline detail", { exact: true })).toHaveValue(
    "8",
  );
});

test("pinned views share navigation, reorder, resize and restore through projects", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  const bundle = JSON.parse(await readFile("public/demo.json", "utf8"));
  await page.addInitScript((capture) => {
    const w = window as any;
    w.isTauri = true;
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, args: any) => {
        if (command === "plugin:dialog|open") return "/second";
        if (command === "open_capture")
          return {
            ...capture,
            id: "second-dock",
            name: "Second dock",
            waves: [],
          };
        if (command === "save_project") {
          w.saved = structuredClone(args);
          return true;
        }
        if (command === "open_project")
          return {
            captures: [{ ...capture, id: "demo" }],
            project: {
              schemaVersion: 1,
              active: 0,
              entries: w.saved.entries.map((e: any) => ({
                input: null,
                state: e.state,
                sources: e.sources,
              })),
            },
          };
        throw Error(command);
      },
    };
  }, bundle.capture);
  await page.goto("/");
  await expect(page.locator("canvas")).toBeVisible();
  await page
    .getByRole("button", { name: "Pin current view", exact: true })
    .click();
  await page.getByRole("button", { name: "Occupancy", exact: true }).click();
  const trace = page.getByRole("region", {
    name: "Trace workspace panel",
    exact: true,
  });
  const occupancy = page.getByRole("region", {
    name: "Occupancy panel",
    exact: true,
  });
  await expect(trace.locator("canvas")).toBeVisible();
  await expect(
    occupancy.getByRole("img", { name: "Shader engine 0 occupancy" }),
  ).toBeVisible();
  await page.getByLabel("Panel arrangement").selectOption("columns");
  await page.getByLabel("Left column width").fill("65");
  const t = await trace.boundingBox(),
    o = await occupancy.boundingBox();
  expect(o!.x).toBeGreaterThan(t!.x);
  expect(t!.width).toBeGreaterThan(o!.width);
  await page.screenshot({
    path: test.info().outputPath("side-by-side.png"),
    fullPage: true,
  });
  await page
    .getByRole("button", { name: "Pin current view", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Move occupancy panel earlier", exact: true })
    .click();
  await expect(page.locator(".dock-panel").first()).toHaveAttribute(
    "aria-label",
    "Occupancy panel",
  );
  await page.getByLabel("Range end", { exact: true }).fill("5000");
  await page.getByLabel("Range end", { exact: true }).press("Enter");
  await expect(
    occupancy.getByRole("img", { name: "Shader engine 0 occupancy" }),
  ).toBeVisible();
  await trace.getByLabel("Search ISA").fill("ds_read");
  await trace.locator("tbody tr").click();
  await expect(trace.locator(".source-selected")).toContainText("transposed");
  await expect(occupancy).toBeVisible();
  await page.getByRole("button", { name: "Save project", exact: true }).click();
  await expect
    .poll(() =>
      page.evaluate(() => (window as any).saved?.entries[0].state.dockLayout),
    )
    .toEqual({
      pinned: ["occupancy", "trace"],
      arrangement: "columns",
      split: 65,
    });
  await page
    .getByRole("button", { name: "Unpin all views", exact: true })
    .click();
  await expect(page.locator(".dock-panel")).toHaveCount(0);
  await page.getByRole("button", { name: "Open project", exact: true }).click();
  await expect(page.locator(".dock-panel")).toHaveCount(2);
  await expect(page.getByLabel("Left column width")).toHaveValue("65");
  await expect(page.locator(".dock-panel").first()).toHaveAttribute(
    "aria-label",
    "Occupancy panel",
  );
  await page.setViewportSize({ width: 1000, height: 900 });
  const narrowFirst = await page.locator(".dock-panel").first().boundingBox();
  const narrowSecond = await page.locator(".dock-panel").nth(1).boundingBox();
  expect(narrowSecond!.y).toBeGreaterThan(narrowFirst!.y);
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= window.innerWidth,
    ),
  ).toBe(true);
  await page
    .getByRole("button", { name: "Unpin occupancy panel", exact: true })
    .click();
  await expect(page.locator(".dock-panel")).toHaveCount(2);
  await expect(occupancy).toContainText("Active view");
  await page
    .getByRole("button", { name: "Trace workspace", exact: true })
    .click();
  await expect(page.locator(".dock-panel")).toHaveCount(1);
  await page
    .getByRole("button", { name: "Open capture file", exact: true })
    .click();
  await expect(page.locator(".dock-panel")).toHaveCount(0);
  await page
    .getByRole("button", { name: bundle.capture.name + " DEMO", exact: true })
    .click();
  await expect(page.locator(".dock-panel")).toHaveCount(1);
});

test("ISA search folding and widths survive view remount and project reopen", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  const bundle = JSON.parse(await readFile("public/demo.json", "utf8"));
  await page.addInitScript((capture) => {
    const w = window as any;
    w.isTauri = true;
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, args: any) => {
        if (command === "save_project") {
          w.saved = structuredClone(args);
          return true;
        }
        if (command === "open_project")
          return {
            captures: [{ ...capture, id: "demo" }],
            project: {
              schemaVersion: 1,
              active: 0,
              entries: w.saved.entries.map((e: any) => ({
                input: null,
                state: e.state,
                sources: e.sources,
              })),
            },
          };
        throw Error(command);
      },
    };
  }, bundle.capture);
  await page.goto("/");
  await page.getByText("ISA columns", { exact: true }).click();
  for (const name of [
    "Idle",
    "PC issued",
    "PC stalled",
    "Code object",
    "Source reference",
  ])
    await page.getByLabel(`Show ISA ${name}`, { exact: true }).check();
  await page.getByLabel("Show ISA Hits", { exact: true }).uncheck();
  await expect(
    page.locator(".isa-table").getByRole("columnheader", { name: /^Hits/ }),
  ).toHaveCount(0);
  await expect(
    page
      .locator(".isa-table")
      .getByRole("columnheader", { name: /^Code object/ }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Collapse ISA section 0", exact: true })
    .click();
  await page.getByLabel("Search ISA").fill("ds_read");
  await page.getByLabel("Cost order", { exact: true }).check();
  await page.getByLabel("ISA folding").selectOption("none");
  const resize = page.getByRole("separator", {
    name: "Resize Instruction",
    exact: true,
  });
  const box = await resize.boundingBox();
  await page.mouse.move(box!.x + box!.width / 2, box!.y + box!.height / 2);
  await page.mouse.down();
  await page.mouse.move(box!.x + box!.width / 2 + 80, box!.y + box!.height / 2);
  await page.mouse.up();
  await page.getByRole("button", { name: "Occupancy", exact: true }).click();
  await page
    .getByRole("button", { name: "Trace workspace", exact: true })
    .click();
  await expect(page.getByLabel("Search ISA")).toHaveValue("ds_read");
  await expect(page.getByLabel("Cost order", { exact: true })).toBeChecked();
  await expect(page.getByLabel("ISA folding")).toHaveValue("none");
  await page.getByLabel("Source line numbers", { exact: true }).uncheck();
  await page.getByLabel("Source cost metric").selectOption("pcIssued");
  await page.getByLabel("Source cost bar width").focus();
  await page.getByLabel("Source cost bar width").press("End");
  await page.getByRole("button", { name: "Save project", exact: true }).click();
  await expect
    .poll(() =>
      page.evaluate(
        () => (window as any).saved?.entries[0].state.isaState.widths[2],
      ),
    )
    .toBe(400);
  const saved = await page.evaluate(
    () => (window as any).saved.entries[0].state.isaState,
  );
  expect(saved.collapsed).toEqual([0]);
  expect(saved.widths).toHaveLength(12);
  expect(saved.hiddenColumns).toEqual(["hits"]);
  await page.getByText("ISA columns", { exact: true }).click();
  await page.getByLabel("Show ISA PC issued", { exact: true }).uncheck();
  await page.getByLabel("Search ISA").fill("");
  await page.getByLabel("Cost order", { exact: true }).uncheck();
  await resize.dblclick();
  await page.getByLabel("Source line numbers", { exact: true }).check();
  await page.getByLabel("Source cost metric").selectOption("latency");
  await page.getByRole("button", { name: "Open project", exact: true }).click();
  await expect(
    page.getByLabel("Source line numbers", { exact: true }),
  ).not.toBeChecked();
  await expect(page.getByLabel("Source cost metric")).toHaveValue("pcIssued");
  await expect(page.getByLabel("Source cost bar width")).toHaveValue("400");
  await expect(page.getByLabel("Search ISA")).toHaveValue("ds_read");
  await expect(page.getByLabel("Cost order", { exact: true })).toBeChecked();
  await expect(
    page
      .locator(".isa-table")
      .getByRole("columnheader", { name: /^PC issued/ }),
  ).toBeVisible();
  await expect(
    page.locator(".isa-table").getByRole("columnheader", { name: /^Hits/ }),
  ).toHaveCount(0);
  expect(
    await page.evaluate(() =>
      JSON.parse(localStorage.getItem("rocprof:isa-hidden-columns")!),
    ),
  ).toEqual(["hits", "issued"]);
  await expect(page.locator("col").nth(2)).toHaveAttribute(
    "style",
    "width: 400px;",
  );
  expect(
    await page.evaluate(
      () => JSON.parse(localStorage.getItem("rocprof:isa-columns")!)[2],
    ),
  ).toBe(320);
  await page.getByLabel("Search ISA").fill("");
  await page.getByLabel("Cost order", { exact: true }).uncheck();
  await page.getByLabel("ISA folding").selectOption("labels");
  await expect(
    page.getByRole("button", { name: "Expand ISA section 0", exact: true }),
  ).toBeVisible();
});

test("legacy monitor intervals preserve raw values and paginate within the visible range", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const b = await (await route.fetch()).json();
    b.capture.monitorIntervals = Array.from({ length: 102 }, (_, i) => ({
      id: String(i),
      deviceType: i === 1 ? "cpu" : "gpu",
      deviceId: "2",
      name: "temp",
      start: i === 101 ? 620 : 600 + i * 10,
      end: i === 101 ? 620 : 605 + i * 10,
      value: i === 0 ? "N/A" : i === 1 ? null : i === 2 ? "0" : String(i),
      unit: "unspecified",
    }));
    await route.fulfill({ json: b });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  const table = page.getByRole("table", {
    name: "Monitor interval records",
    exact: true,
  });
  await expect(table.locator("tbody tr")).toHaveCount(100);
  await expect(table).toContainText("N/A");
  await expect(table).toContainText("Not recorded");
  await expect(table.getByRole("img").first()).toHaveAttribute(
    "aria-label",
    "temp: 600–605 cycles",
  );
  await page
    .getByRole("button", { name: "Next monitor intervals", exact: true })
    .click();
  await expect(table.locator("tbody tr")).toHaveCount(2);
  await page.getByLabel("Search monitor intervals").fill("cpu");
  await expect(table.locator("tbody tr")).toHaveCount(1);
  await expect(table).toContainText("cpu 2");
  await expect(table).toContainText("Not recorded");
  await page.getByLabel("Search monitor intervals").fill("");
  await page.getByLabel("Range end", { exact: true }).fill("620");
  await page.getByLabel("Range end", { exact: true }).press("Enter");
  await expect(table.locator("tbody tr")).toHaveCount(3);
  await expect(
    table
      .getByRole("img", { name: "temp: 620–620 cycles", exact: true })
      .locator("rect"),
  ).toHaveAttribute("x", "159");
  await page.getByLabel("Search monitor intervals").fill("unknown");
  await expect(
    page.getByText("No monitor intervals match this range and filter.", {
      exact: true,
    }),
  ).toBeVisible();
});

test("host API timelines keep CPU durations distinct and navigate only recorded GPU links", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const b = await (await route.fetch()).json();
    b.capture.hostCalls = Array.from({ length: 102 }, (_, i) => ({
      id: "api:" + i,
      pid: "12",
      scope: i === 1 ? "GUID B · node 0" : "GUID A · node 0",
      category: "HIP",
      linkBasis: "Recorded correlation ID within GUID/node/process",
      tid: i === 1 ? "14" : "13",
      name: "hipCall" + i,
      arguments: i === 0 ? "arg=0" : null,
      start: 600 + i * 10,
      end: 605 + i * 10,
      operations: i === 0 ? [b.capture.dispatches[0].id, "legacy-op:copy"] : [],
    }));
    await route.fulfill({ json: b });
  });
  await page.goto("/");
  await page
    .getByRole("button", { name: "Kernel dispatches", exact: true })
    .click();
  const table = page.getByRole("table", {
    name: "Host API records",
    exact: true,
  });
  await expect(table.locator("tbody tr")).toHaveCount(100);
  await expect(table).toContainText("5 cycles");
  await table.getByRole("button", { name: "hipCall0", exact: true }).click();
  const detail = page.getByRole("region", {
    name: "Selected host call",
    exact: true,
  });
  await expect(detail).toContainText("arg=0");
  await expect(detail).toContainText("GUID A · node 0");
  await expect(detail).toContainText("HIP");
  await expect(detail).toContainText("Recorded correlation ID");
  await expect(detail).toContainText(
    "legacy-op:copy · no imported kernel dispatch",
  );
  await expect(
    detail.getByRole("button", { name: /Focus GPU dispatch/ }),
  ).toHaveCount(1);
  await page
    .getByRole("button", { name: "Next host calls", exact: true })
    .click();
  await expect(table.locator("tbody tr")).toHaveCount(2);
  await page.getByLabel("Search host calls").fill("hipCall1");
  await expect(
    table.getByRole("button", { name: "hipCall1", exact: true }),
  ).toBeVisible();
  await table.getByRole("button", { name: "hipCall1", exact: true }).click();
  await expect(detail).toContainText("Arguments not recorded");
  await page.getByLabel("Search host calls").fill("hipCall0");
  await table.getByRole("button", { name: "hipCall0", exact: true }).click();
  await detail
    .getByRole("button", { name: "Focus CPU call", exact: true })
    .click();
  await expect(page.getByLabel("Range start", { exact: true })).toHaveValue(
    "600",
  );
  await expect(page.getByLabel("Range end", { exact: true })).toHaveValue(
    "605",
  );
  await detail.getByRole("button", { name: /Focus GPU dispatch/ }).click();
  await expect(page.getByLabel("Range end", { exact: true })).not.toHaveValue(
    "605",
  );
});

test("detached trace panels keep independent ranges and survive project restore", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  const bundle = JSON.parse(await readFile("public/demo.json", "utf8"));
  await page.addInitScript((capture) => {
    const w = window as any;
    w.isTauri = true;
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, args: any) => {
        if (command === "plugin:dialog|open") return "/second";
        if (command === "open_capture")
          return {
            ...capture,
            id: "second-dock",
            name: "Second dock",
            waves: [],
          };
        if (command === "save_project") {
          w.saved = structuredClone(args);
          return true;
        }
        if (command === "open_project")
          return {
            captures: [{ ...capture, id: "demo" }],
            project: {
              schemaVersion: 1,
              active: 0,
              entries: w.saved.entries.map((e: any) => ({
                input: null,
                state: e.state,
                sources: e.sources,
              })),
            },
          };
        throw Error(command);
      },
    };
  }, bundle.capture);

  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto("/");
  await page.getByLabel("Add docked panel").selectOption("trace-cu");
  await page.getByLabel("Add docked panel").selectOption("trace-global");
  const cu = page.getByRole("region", { name: "CU waves panel", exact: true });
  const global = page.getByRole("region", {
    name: "Global residency panel",
    exact: true,
  });
  const trace = page.getByRole("region", {
    name: "Trace workspace panel",
    exact: true,
  });
  await expect(cu.locator("canvas")).toBeVisible();
  await expect(global.locator("canvas")).toBeVisible();
  await expect(trace.locator("canvas")).toBeVisible();
  await expect(cu.locator(".plot-footer")).toContainText("12 lanes");
  await expect(cu.getByLabel("Search ISA")).toHaveCount(0);
  await cu.getByLabel("Link timeline ranges", { exact: true }).uncheck();
  await cu.getByLabel("Timeline zoom mode").selectOption("mip");
  await cu
    .getByRole("button", { name: "Zoom in one mip level", exact: true })
    .click();
  await page.getByRole("button", { name: "Save project", exact: true }).click();
  const saved = await page.evaluate(
    () => (window as any).saved.entries[0].state,
  );
  expect(saved.dockLayout.pinned).toEqual(["trace-cu", "trace-global"]);
  expect(saved.timeline).toBe("wave");
  expect(saved.timelineRanges.cu).toBeDefined();
  expect(saved.timelineRanges.cu).not.toEqual(saved.location.range);
  expect(saved.timelinePreferences.cu.zoomMode).toBe("mip");
  await page
    .getByRole("button", { name: "Unpin all views", exact: true })
    .click();
  await page.getByRole("button", { name: "Open project", exact: true }).click();
  await expect(cu.locator("canvas")).toBeVisible();
  await expect(cu.getByLabel("Timeline zoom mode")).toHaveValue("mip");
  await expect(global.locator("canvas")).toBeVisible();
  await page.getByRole("button", { name: "Occupancy", exact: true }).click();
  await expect(cu.locator("canvas")).toBeVisible();
  await expect(global.locator("canvas")).toBeVisible();
  await expect(trace).toHaveCount(0);
  await expect(cu.locator(".plot-footer")).toContainText("12 lanes");
  await page.getByLabel("Panel arrangement").selectOption("columns");
  for (const panel of [cu, global]) {
    const bounds = await panel.boundingBox();
    for (const control of await panel
      .locator(".wave-toolbar select, .wave-toolbar button")
      .all()) {
      const box = await control.boundingBox();
      expect(box!.x).toBeGreaterThanOrEqual(bounds!.x);
      expect(box!.x + box!.width).toBeLessThanOrEqual(
        bounds!.x + bounds!.width,
      );
    }
  }
  await page.screenshot({
    path: test.info().outputPath("panels.png"),
    fullPage: true,
  });
  expect(errors).toEqual([]);
});

test("nested panel splits resize, collapse and restore through projects", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  const bundle = JSON.parse(await readFile("public/demo.json", "utf8"));
  await page.addInitScript((capture) => {
    const w = window as any;
    w.isTauri = true;
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, args: any) => {
        if (command === "plugin:dialog|open") return "/second";
        if (command === "open_capture")
          return {
            ...capture,
            id: "second-dock",
            name: "Second dock",
            waves: [],
          };
        if (command === "save_project") {
          w.saved = structuredClone(args);
          return true;
        }
        if (command === "open_project")
          return {
            captures: [{ ...capture, id: "demo" }],
            project: {
              schemaVersion: 1,
              active: 0,
              entries: w.saved.entries.map((e: any) => ({
                input: null,
                state: e.state,
                sources: e.sources,
              })),
            },
          };
        throw Error(command);
      },
    };
  }, bundle.capture);

  await page.goto("/");
  for (const id of ["trace-cu", "trace-global", "occupancy"])
    await page.getByLabel("Add docked panel").selectOption(id);
  await page.getByLabel("Panel arrangement").selectOption("custom");
  await page
    .getByLabel("Place trace-global panel", { exact: true })
    .selectOption("right:trace-cu");
  await page
    .getByLabel("Place occupancy panel", { exact: true })
    .selectOption("below:trace-global");
  await page.getByLabel("Split proportion root", { exact: true }).fill("60");
  await page.getByLabel("Split proportion 1", { exact: true }).fill("40");
  const cu = page.getByRole("region", { name: "CU waves panel", exact: true });
  const global = page.getByRole("region", {
    name: "Global residency panel",
    exact: true,
  });
  const occupancy = page.getByRole("region", {
    name: "Occupancy panel",
    exact: true,
  });
  await expect(cu.locator(".plot-footer")).toContainText("12 lanes");
  const c = await cu.boundingBox(),
    g = await global.boundingBox(),
    o = await occupancy.boundingBox();
  expect(g!.x).toBeGreaterThan(c!.x);
  expect(c!.width).toBeGreaterThan(g!.width);
  expect(o!.y).toBeGreaterThan(g!.y);
  await page.getByRole("button", { name: "Save project", exact: true }).click();
  const tree = await page.evaluate(
    () => (window as any).saved.entries[0].state.dockLayout.tree,
  );
  expect(tree).toMatchObject({
    axis: "horizontal",
    ratio: 60,
    second: { axis: "vertical", ratio: 40 },
  });
  await page
    .getByRole("button", { name: "Unpin trace-global panel", exact: true })
    .click();
  await expect(
    page.getByLabel("Split proportion 1", { exact: true }),
  ).toHaveCount(0);
  await page.getByRole("button", { name: "Open project", exact: true }).click();
  await expect(
    page.getByLabel("Split proportion root", { exact: true }),
  ).toHaveValue("60");
  await expect(
    page.getByLabel("Split proportion 1", { exact: true }),
  ).toHaveValue("40");
  await expect(cu.locator(".plot-footer")).toContainText("12 lanes");
  const plot = occupancy.getByRole("img", {
    name: "Shader engine 0 occupancy",
  });
  expect((await plot.boundingBox())!.height).toBeGreaterThan(100);
  await page.screenshot({
    path: test.info().outputPath("panels.png"),
    fullPage: true,
  });
  await page.setViewportSize({ width: 1000, height: 900 });
  const narrowCu = await cu.boundingBox(),
    narrowGlobal = await global.boundingBox();
  expect(narrowGlobal!.y).toBeGreaterThan(narrowCu!.y);
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= window.innerWidth,
    ),
  ).toBe(true);
});

test("whole timeline export includes every CU lane across time segments", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  await page.goto("/");
  await page.getByRole("button", { name: "CU waves", exact: true }).click();
  await expect(page.locator(".timeline .plot-footer")).toContainText(
    "12 lanes",
  );
  await page.getByText("Export timeline pages", { exact: true }).click();
  await page.getByLabel("Timeline export time segments").fill("2");
  await page.getByLabel("Timeline export lanes per page").fill("3");
  const download = page.waitForEvent("download");
  await page
    .getByRole("button", { name: "Export printable HTML", exact: true })
    .click();
  const output = await download;
  await output.saveAs(test.info().outputPath("whole-timeline.html"));
  const html = await readFile(
    test.info().outputPath("whole-timeline.html"),
    "utf8",
  );
  const bundle = JSON.parse(await readFile("public/demo.json", "utf8"));
  const expected = bundle.capture.waves
    .filter((wave: any) => wave.se === 0)
    .flatMap((wave: any) =>
      bundle.waves[wave.id].tokens.map(
        (_: unknown, index: number) => `${wave.id}:${index}`,
      ),
    );
  const exported = [
    ...new Set(
      [...html.matchAll(/data-key="([^"]+)"/g)].map((match) => match[1]),
    ),
  ];
  expect(exported.sort()).toEqual(expected.sort());
  expect(html.match(/<section /g)).toHaveLength(7);
  expect(html).toContain("time segment 2/2");
  expect(html).toContain("lanes 10–12 of 12");
  expect(html).toContain("data-key=");
  await expect(page.getByRole("status")).toContainText(
    "Timeline pages exported",
  );
  await page.setContent(html);
  await expect(page.locator("section").last()).toBeAttached();
  await page
    .locator("section")
    .first()
    .screenshot({ path: test.info().outputPath("first-page.png") });
});

test("verified project restore distinguishes saved input identity from source revision", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  const bundle = JSON.parse(await readFile("public/demo.json", "utf8"));
  await page.addInitScript((capture) => {
    const w = window as any;
    w.isTauri = true;
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, args: any) => {
        if (command === "plugin:dialog|open") return "/second";
        if (command === "open_capture")
          return {
            ...capture,
            id: "second-dock",
            name: "Second dock",
            waves: [],
          };
        if (command === "save_project") {
          w.saved = structuredClone(args);
          return true;
        }
        if (command === "open_project")
          return {
            captures: [{ ...capture, id: "demo" }],
            project: {
              schemaVersion: 3,
              active: 0,
              entries: w.saved.entries.map((e: any) => ({
                input: "/verified/filenames.json",
                fingerprint: {
                  files: {
                    "/verified/filenames.json": {
                      size: 1,
                      sha256: "a".repeat(64),
                    },
                  },
                },
                state: e.state,
                sources: e.sources,
              })),
            },
          };
        throw Error(command);
      },
    };
  }, bundle.capture);

  await page.goto("/");
  await page.getByRole("button", { name: "Save project", exact: true }).click();
  await page.getByRole("button", { name: "Open project", exact: true }).click();
  await expect(
    page.getByText(
      "Project capture inputs match their saved fingerprints. Source replacements remain revision-unverified.",
      { exact: true },
    ),
  ).toBeVisible();
});

test("portable archive controls open archive paths and report save cancellation and failures", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  const bundle = JSON.parse(await readFile("public/demo.json", "utf8"));
  await page.addInitScript((capture) => {
    const w = window as any;
    w.isTauri = true;
    w.archiveRequests = [];
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, args: any) => {
        if (command === "plugin:dialog|open")
          return "/portable.rocprof-capture";
        if (command === "open_capture") {
          w.openedPath = args.path;
          return {
            ...capture,
            id: "portable-native",
            name: "Portable native",
            waves: [],
            instructions: [],
            sourceFiles: [],
            sources: {},
          };
        }
        if (command === "export_capture_archive") {
          w.archiveRequests.push(args);
          if (w.archiveRequests.length === 1) return false;
          if (w.archiveRequests.length === 2)
            throw Error("Archive destination is read-only");
          if (w.archiveRequests.length === 3) return true;
          return new Promise((resolve) => {
            w.finishArchive = resolve;
          });
        }
        if (command === "cancel_import") {
          w.finishArchive(false);
          return;
        }
        throw Error(command);
      },
    };
  }, bundle.capture);
  await page.goto("/");
  const button = page.getByRole("button", {
    name: "Archive capture",
    exact: true,
  });
  await expect(button).toBeDisabled();
  await page
    .getByRole("button", { name: "Open capture file", exact: true })
    .click();
  await expect(button).toBeEnabled();
  expect(await page.evaluate(() => (window as any).openedPath)).toBe(
    "/portable.rocprof-capture",
  );
  await button.click();
  await expect(
    page.getByText("Capture archive export cancelled", { exact: true }),
  ).toBeVisible();
  await button.click();
  await expect(
    page.getByText("Error: Archive destination is read-only", { exact: true }),
  ).toBeVisible();
  await button.click();
  await expect(
    page.getByText(/Portable capture saved with decoded data/),
  ).toBeVisible();
  await button.click();
  await page
    .getByRole("button", { name: "Cancel archive", exact: true })
    .click();
  await expect(button).toBeEnabled();
  const requests = await page.evaluate(() => (window as any).archiveRequests);
  expect(requests).toHaveLength(4);
  expect(
    requests.every((request: any) => request.captureId === "portable-native"),
  ).toBe(true);
});

test("analysis filters and drafts persist across views captures and project restore", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  const bundle = JSON.parse(await readFile("public/demo.json", "utf8"));
  await page.addInitScript((capture) => {
    const w = window as any;
    w.isTauri = true;
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, args: any) => {
        if (command === "plugin:dialog|open") return "/second";
        if (command === "open_capture")
          return {
            ...capture,
            id: "second-dock",
            name: "Second dock",
            waves: [],
          };
        if (command === "save_project") {
          w.saved = structuredClone(args);
          return true;
        }
        if (command === "open_project")
          return {
            captures: [{ ...capture, id: "demo" }],
            project: {
              schemaVersion: 1,
              active: 0,
              entries: w.saved.entries.map((e: any) => ({
                input: null,
                state: e.state,
                sources: e.sources,
              })),
            },
          };
        throw Error(command);
      },
    };
  }, bundle.capture);

  await page.goto("/");
  await page
    .getByRole("button", { name: "Kernel dispatches", exact: true })
    .click();
  await page.getByLabel("Filter kernels").fill("transpose");
  await page.getByRole("button", { name: "Calls", exact: true }).click();
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  await page.getByLabel("Filter counters").fill("SQ_INSTS");
  await page.getByLabel("Derived counter name").fill("Saved draft");
  await page.getByLabel("Counter expression").fill("C0 + 7");
  const domain = (await page
    .getByLabel("Counter tensor domain")
    .locator("option")
    .last()
    .textContent())!;
  await page
    .getByLabel("Counter tensor domain")
    .selectOption({ label: domain });
  await page
    .getByRole("button", { name: "Kernel dispatches", exact: true })
    .click();
  await expect(page.getByLabel("Filter kernels")).toHaveValue("transpose");
  await page.getByRole("button", { name: "Save project", exact: true }).click();
  const preferences = await page.evaluate(
    () => (window as any).saved.entries[0].state.analysisPreferences,
  );
  expect(preferences).toMatchObject({
    "dispatch.search": "transpose",
    "dispatch.sort": "count",
    "counters.search": "SQ_INSTS",
    "counters.name": "Saved draft",
    "counters.formula": "C0 + 7",
    "tensor.scope": domain,
  });
  await page
    .getByRole("button", { name: "Open capture file", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Kernel dispatches", exact: true })
    .click();
  await expect(page.getByLabel("Filter kernels")).toHaveValue("");
  await page
    .getByRole("button", { name: bundle.capture.name + " DEMO", exact: true })
    .click();
  await expect(page.getByLabel("Filter kernels")).toHaveValue("transpose");
  await page.getByLabel("Filter kernels").fill("changed");
  await page.getByRole("button", { name: "Open project", exact: true }).click();
  await expect(page.getByLabel("Filter kernels")).toHaveValue("transpose");
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  await expect(page.getByLabel("Filter counters")).toHaveValue("SQ_INSTS");
  await expect(page.getByLabel("Counter expression")).toHaveValue("C0 + 7");
  await expect(page.getByLabel("Derived counter name")).toHaveValue(
    "Saved draft",
  );
  await expect(
    page.getByLabel("Counter tensor domain").locator("option:checked"),
  ).toHaveText(domain);
});

test("inline source hierarchy conserves weights, zooms and exports every level", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const bundle = await (await route.fetch()).json();
    bundle.capture.waves = [];
    bundle.capture.instructions = bundle.capture.instructions
      .slice(0, 3)
      .map((row: object, index: number) => ({
        ...row,
        source: index === 1 ? "outer.h:7" : "inner.h:3 -> outer.h:7",
        pcIssued: [8, 2, 4][index],
        pcStalled: 0,
      }));
    await route.fulfill({ json: bundle });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "PC sampling", exact: true }).click();
  await page.getByRole("button", { name: "Show sample flamegraph" }).click();
  await page.getByLabel("Sample flamegraph weight").selectOption("pcIssued");
  await page
    .getByLabel("File and inline source hierarchy", { exact: true })
    .check();
  const tree = page.locator(".source-hierarchy");
  await expect(
    tree.getByRole("button", { name: "outer.h", exact: true }),
  ).toHaveAttribute("title", "outer.h · 14 samples");
  await expect(
    tree.getByRole("button", { name: "inner.h:3", exact: true }),
  ).toHaveAttribute("title", "inner.h:3 · 12 samples");
  await tree.getByRole("button", { name: "inner.h:3", exact: true }).click();
  await expect(page.getByLabel("Flamegraph range end")).toHaveValue("12");
  const pending = page.waitForEvent("download");
  await page
    .getByRole("button", {
      name: "Export Source flamegraph as SVG",
      exact: true,
    })
    .click();
  const download = await pending;
  const path = await download.path();
  const { readFile } = await import("node:fs/promises");
  const svg = await readFile(path!, "utf8");
  await download.saveAs(test.info().outputPath("inline-samples.svg"));
  await page.screenshot({
    path: test.info().outputPath("hierarchy.png"),
    fullPage: true,
  });
  expect(svg).toContain("outer.h:7");
  expect(svg).toContain("inner.h:3");
  expect(svg).toContain("ISA ");
  expect(svg).toContain("visible cumulative weight 0–12");
  await page.getByRole("button", { name: "Full flamegraph range" }).click();
  await expect(page.getByLabel("Flamegraph range end")).toHaveValue("14");
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  await page.getByRole("button", { name: "PC sampling", exact: true }).click();
  await expect(
    page.getByLabel("File and inline source hierarchy", { exact: true }),
  ).toBeChecked();
});

test("reconstructed waits show only the selected loop token dependencies", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const bundle = await (await route.fetch()).json();
    for (const wave of Object.values(bundle.waves) as any[]) {
      const occurrences = [];
      for (let position = 0; position < wave.tokens.length; position++) {
        if (wave.tokens[position].instruction !== 4) continue;
        const source = wave.tokens
          .slice(0, position)
          .findLastIndex((token: any) => token.instruction === 3);
        if (source >= 0)
          occurrences.push({
            waitToken: position,
            sources: [
              {
                token: source,
                issueToWaitCycles: 10 + occurrences.length,
                issueToWaitEndCycles: 20 + occurrences.length,
              },
            ],
          });
      }
      wave.info.waitAnalysis = {
        basis: "named ISA counters in recorded token order",
        unmodeledCounters: ["expcnt"],
        conservativeImageOrdering: true,
        exportCounterModeled: true,
        conservativeExportOrdering: true,
        occurrences,
        unsupportedWaits: 0,
      };
    }
    await route.fulfill({ json: bundle });
  });
  await page.goto("/");
  await page
    .locator("tbody tr")
    .filter({ hasText: "s_waitcnt vmcnt(0)" })
    .click();
  await expect(
    page.getByText("issue to wait 10 cycles", { exact: false }),
  ).toBeVisible();
  await expect(
    page.getByText("Export-counter links track export or register hazards", {
      exact: false,
    }),
  ).toBeVisible();
  await expect(
    page.getByText(
      "Mixed export-counter event groups may complete out of order",
      { exact: false },
    ),
  ).toBeVisible();
  await page.getByLabel("Loop occurrence").fill("1");
  await expect(
    page.getByText("issue to wait 11 cycles", { exact: false }),
  ).toBeVisible();
  await expect(
    page.getByText("issue to wait 10 cycles", { exact: false }),
  ).toHaveCount(0);
  await expect(
    page.getByText("Reconstructed from named ISA counters", { exact: false }),
  ).toBeVisible();
  await expect(
    page.getByText("Counter dependencies not modeled: expcnt.", {
      exact: false,
    }),
  ).toBeVisible();
  await expect(
    page.getByText(
      "Image operations on this architecture are linked at zero-count waits only.",
      { exact: false },
    ),
  ).toBeVisible();
  await page.getByRole("button", { name: /ISA 3 → ISA 4 · token/ }).click();
  await expect(page.locator("tbody tr.selected")).toContainText(
    "global_load_dword",
  );
  await expect(page.getByLabel("Loop occurrence")).toHaveValue("1");
  await page.getByRole("button", { name: "Previous selection" }).click();
  await expect(page.locator("tbody tr.selected")).toContainText(
    "s_waitcnt vmcnt(0)",
  );
  await expect(page.getByLabel("Loop occurrence")).toHaveValue("1");
  await page.getByRole("button", { name: "Next selection" }).click();
  await expect(page.locator("tbody tr.selected")).toContainText(
    "global_load_dword",
  );
  await expect(page.getByLabel("Loop occurrence")).toHaveValue("1");
});

test("conflicting architecture metadata explains unavailable wait reconstruction", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const bundle = await (await route.fetch()).json();
    for (const wave of Object.values(bundle.waves) as any[]) {
      wave.waits = [];
      wave.info.waitAnalysis = {
        basis: "named ISA counters in recorded token order",
        occurrences: [],
        unavailableReason:
          "Architecture label and decoder gfxip disagree; wait reconstruction was skipped.",
      };
    }
    await route.fulfill({ json: bundle });
  });
  await page.goto("/");
  await page
    .locator("tbody tr")
    .filter({ hasText: "s_waitcnt vmcnt(0)" })
    .click();
  await expect(
    page.getByText("Architecture label and decoder gfxip disagree", {
      exact: false,
    }),
  ).toBeVisible();
  await expect(
    page.getByRole("img", {
      name: "Memory instructions linked to selected wait instruction",
    }),
  ).toHaveCount(0);
});

test("large wait dependency lists paginate without merging repeated source instructions", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const bundle = await (await route.fetch()).json();
    for (const wave of Object.values(bundle.waves) as any[]) {
      wave.tokens = Array.from({ length: 36 }, (_, index) => ({
        start: wave.start + index * 3,
        duration: 2,
        stall: 0,
        category: index === 35 ? 1 : 3,
        instruction: index === 35 ? 4 : 3,
      }));
      wave.info.waitAnalysis = {
        basis: "named ISA counters in recorded token order",
        occurrences: [
          {
            waitToken: 35,
            sources: Array.from({ length: 35 }, (_, token) => ({
              token,
              issueToWaitCycles: (35 - token) * 3,
              issueToWaitEndCycles: (35 - token) * 3 + 2,
            })),
          },
        ],
      };
    }
    await route.fulfill({ json: bundle });
  });
  await page.goto("/");
  await page
    .locator("tbody tr")
    .filter({ hasText: "s_waitcnt vmcnt(0)" })
    .click();
  await expect(
    page.getByText("1–20 of 35 dependencies", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: /ISA 3 → ISA 4 · token/ }),
  ).toHaveCount(20);
  expect(
    (await page.locator(".wait-diagram").boundingBox())!.height,
  ).toBeGreaterThan(600);
  await page
    .locator(".wait-links")
    .screenshot({ path: test.info().outputPath("dependencies.png") });
  await page
    .getByRole("button", { name: "Next dependencies", exact: true })
    .click();
  await expect(
    page.getByText("21–35 of 35 dependencies", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: /ISA 3 → ISA 4 · token/ }),
  ).toHaveCount(15);
  await page.getByRole("button", { name: /ISA 3 → ISA 4 · token 32 / }).click();
  await expect(page.getByLabel("Loop occurrence")).toHaveValue("32");
  await page.getByRole("button", { name: "Previous selection" }).click();
  await expect(
    page.getByText("1–20 of 35 dependencies", { exact: true }),
  ).toBeVisible();
});

test("inline source frames resolve snapshots and retain code-object scope on reverse navigation", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const bundle = await (await route.fetch()).json();
    Object.assign(bundle.capture.sources, {
      "inner.h": "inner header\ninner target\ninner next\n",
      "outer.h": "outer header\nouter second\nouter target\nouter next\n",
    });
    const rows = bundle.capture.instructions;
    rows.find((row: any) => row.index === 3).source = "inner.h:2 -> outer.h:3";
    rows.find((row: any) => row.index === 3).codeObject = "same";
    for (const id of [1, 5]) {
      rows.find((row: any) => row.index === id).source =
        "inner.h:3 -> outer.h:4";
      rows.find((row: any) => row.index === id).codeObject =
        id === 5 ? "same" : "different";
    }
    await route.fulfill({ json: bundle });
  });
  await page.goto("/");
  await page
    .locator("tbody tr")
    .filter({ hasText: "global_load_dword" })
    .click();
  await expect(page.locator(".source-selected")).toContainText("outer target");
  await page.getByLabel("Inline source location").selectOption("inner.h:2");
  await expect(page.locator(".source-selected")).toContainText("inner target");
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  await page
    .getByRole("button", { name: "Trace workspace", exact: true })
    .click();
  await expect(page.getByLabel("Inline source location")).toHaveValue(
    "inner.h:2",
  );
  await page
    .locator(".source button")
    .filter({ hasText: "inner next" })
    .click();
  await expect(page.locator("tbody tr.selected td").first()).toHaveText("5");
  await expect(page.getByLabel("Inline source location")).toHaveValue(
    "inner.h:3",
  );
  await expect(page.locator(".source-selected")).toContainText("inner next");
});

test("ATT counters display recorded SIMD selection and preserve separate identities", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const response = await route.fetch();
    const bundle = await response.json();
    bundle.capture.counters = [
      {
        name: "SQ_WAVES:5",
        unit: "raw count",
        scope: "SE 0 · CU 2 · bank 0",
        points: [
          [0, 1],
          [100, 4],
        ],
        att: {
          counter: "SQ_WAVES",
          simdMask: 5,
          simds: [0, 2],
          explicit: true,
        },
      },
      {
        name: "SQ_WAVES",
        unit: "raw count",
        scope: "SE 0 · CU 2 · bank 0",
        points: [
          [0, 2],
          [100, 8],
        ],
        att: {
          counter: "SQ_WAVES",
          simdMask: 15,
          simds: [0, 1, 2, 3],
          explicit: false,
        },
      },
    ];
    await route.fulfill({ json: bundle });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "C0 · SQ_WAVES:5", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "C1 · SQ_WAVES", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByText(
      "SE 0 · CU 2 · bank 0 · raw count · SIMDs 0, 2 · mask 0x5 (recorded suffix)",
      { exact: true },
    ),
  ).toBeVisible();
  await expect(
    page.getByText(
      "SE 0 · CU 2 · bank 0 · raw count · SIMDs 0, 1, 2, 3 · mask 0xf (SDK default; suffix absent)",
      { exact: true },
    ),
  ).toBeVisible();
});

test("SPM counter definitions show recorded type expression and unknown units", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const response = await route.fetch();
    const bundle = await response.json();
    bundle.capture.counters = [
      {
        name: "SAME_NAME",
        unit: "unspecified",
        scope: "process 0 · agent 42 · counter 1 · instance 7",
        points: [
          [0, 4],
          [100, 6],
        ],
        definition: {
          id: "1",
          description: "Recorded hardware events",
          block: "SQ",
          expression: "A / B",
          isDerived: true,
          isConstant: null,
        },
      },
    ];
    await route.fulfill({ json: bundle });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  await expect(
    page.getByText(
      "process 0 · agent 42 · counter 1 · instance 7 · unspecified",
      { exact: true },
    ),
  ).toBeVisible();
  await page.getByText("Recorded counter definition", { exact: true }).click();
  const definition = page.locator(".counter-definition");
  await expect(definition.getByText("Derived", { exact: true })).toBeVisible();
  await expect(
    definition.getByText("Recorded hardware events", { exact: true }),
  ).toBeVisible();
  await expect(definition.getByText("A / B", { exact: true })).toBeVisible();
  await expect(
    definition.getByText("Unspecified", { exact: true }),
  ).toBeVisible();
});

test("native SPM import feeds dimensioned worker expressions", async ({
  page,
}) => {
  test.skip(
    !process.env.ROCPROF_SPM_BUNDLE,
    "Requires a Rust-imported SPM bundle",
  );
  const { readFileSync } = await import("node:fs");
  const bundle = JSON.parse(
    readFileSync(process.env.ROCPROF_SPM_BUNDLE!, "utf8"),
  );
  await page.route("**/demo.json", (route) => route.fulfill({ json: bundle }));
  await page.goto("/");
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  await expect(
    page.getByLabel("Counter tensor domain").locator("option"),
  ).toHaveText([/^process 0 · agent 42(?: · collection 0)?$/]);
  await page
    .getByLabel("Counter definitions")
    .fill("R := sum[BUSY,axis=SE] / SQ_CYCLES\nTotal := sum[R]");
  await page
    .getByRole("button", { name: "Evaluate definitions", exact: true })
    .click();
  await expect(page.locator(".tensor-result output")).toHaveText("1.1");
  await expect(page.locator(".tensor-result").first()).toContainText(
    "2/2 valid values",
  );
});

test("counter expression aliases evaluate masked ATT and distinct SPM identities", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const response = await route.fetch();
    const bundle = await response.json();
    const spm = (id: number, value: number) => ({
      name: "BUSY",
      unit: "count",
      scope: `process 0 · agent 42 · counter ${id} · [{"dimension_name":"DIMENSION_XCC","index":0}]`,
      points: [[0, value]],
    });
    bundle.capture.counters = [
      {
        name: "SQ_WAVES:5",
        unit: "count",
        scope: "SE 0 · CU 0 · bank 0",
        points: [[0, 4]],
      },
      spm(1, 6),
      spm(2, 9),
    ];
    await route.fulfill({ json: bundle });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  await page.getByText("Counter expression names", { exact: true }).click();
  await expect(page.locator(".counter-bindings")).toContainText(
    "RAW_SQ_5f_WAVES_3a_5",
  );
  await expect(page.locator(".counter-bindings")).toContainText("SQ_WAVES:5");
  await page
    .getByLabel("Counter definitions")
    .fill("Total := sum[RAW_SQ_5f_WAVES_3a_5]");
  await page
    .getByRole("button", { name: "Evaluate definitions", exact: true })
    .click();
  await expect(page.locator(".tensor-result output")).toHaveText("4");
  await page
    .getByLabel("Counter tensor domain")
    .selectOption({ label: "process 0 · agent 42" });
  await expect(page.locator(".counter-bindings tbody tr")).toHaveCount(2);
  await page
    .getByLabel("Counter definitions")
    .fill("Difference := sum[RAW_BUSY_ID_2] - sum[RAW_BUSY_ID_1]");
  await page
    .getByRole("button", { name: "Evaluate definitions", exact: true })
    .click();
  await expect(page.locator(".tensor-result output")).toHaveText("3");
});

test("native SPM dispatch partitions select independent expression inputs", async ({
  page,
}) => {
  test.skip(
    !process.env.ROCPROF_SPM_PARTITIONS_BUNDLE,
    "Requires a Rust-imported partitioned SPM bundle",
  );
  const { readFileSync } = await import("node:fs");
  const bundle = JSON.parse(
    readFileSync(process.env.ROCPROF_SPM_PARTITIONS_BUNDLE!, "utf8"),
  );
  await page.route("**/demo.json", (route) => route.fulfill({ json: bundle }));
  await page.goto("/");
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  const domain = page.getByLabel("Counter tensor domain");
  await expect(domain.locator("option")).toHaveText([
    "process 0 · agent 42 · dispatch 10 · queue 7 · stream 0",
    "process 0 · agent 42 · dispatch 11 · queue 7 · stream 0",
  ]);
  await page.getByLabel("Counter definitions").fill("Total := sum[BUSY]");
  await page
    .getByRole("button", { name: "Evaluate definitions", exact: true })
    .click();
  await expect(page.locator(".tensor-result output")).toHaveText("18");
  await domain.selectOption("1");
  await expect(page.locator(".tensor-result")).toHaveCount(0);
  await page
    .getByRole("button", { name: "Evaluate definitions", exact: true })
    .click();
  await expect(page.locator(".tensor-result output")).toHaveText("36");
});

test("SPM raw plots retain isolated observations and export peer timestamp gaps", async ({
  page,
}, testInfo) => {
  await page.route("**/demo.json", async (route) => {
    const response = await route.fetch();
    const bundle = await response.json();
    const scope = (id: number, xcc = 0) =>
      `process 0 · agent 42 · dispatch 10 · counter ${id} · [{"dimension_name":"DIMENSION_XCC","index":${xcc}}]`;
    bundle.capture.start = 0;
    bundle.capture.end = 40;
    bundle.capture.counters = [
      {
        name: "A",
        scope: scope(1),
        unit: "count",
        points: [
          [10, 4],
          [30, 7],
        ],
      },
      {
        name: "PEER",
        scope: scope(2),
        unit: "count",
        points: [
          [10, 2],
          [20, 3],
          [30, 4],
        ],
      },
      {
        name: "OTHER_XCC",
        scope: scope(3, 1),
        unit: "count",
        points: [[15, 9]],
      },
    ];
    await route.fulfill({ json: bundle });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  const chart = page.getByRole("img", { name: "A", exact: true });
  await expect(chart.locator("circle")).toHaveCount(2);
  await chart.screenshot({ path: testInfo.outputPath("spm-gaps.png") });
  const path = await chart.locator("path").getAttribute("d");
  expect(path?.match(/M/g)).toHaveLength(2);
  expect(path).not.toContain("L");
  await expect(
    page.getByText(/1 unobserved peer timestamps between samples/),
  ).toBeVisible();
  await expect(
    page.getByRole("img", { name: "OTHER_XCC", exact: true }).locator("circle"),
  ).toHaveCount(1);
  const download = page.waitForEvent("download");
  await page
    .getByRole("button", { name: "Export A as SVG", exact: true })
    .click();
  const saved = await download;
  const file = testInfo.outputPath("spm-gaps.svg");
  await saved.saveAs(file);
  const { readFile } = await import("node:fs/promises");
  const svg = await readFile(file, "utf8");
  expect(svg.match(/<circle/g)).toHaveLength(2);
});

test("recorded code objects preserve scoped identities and paginate searchable provenance", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const response = await route.fetch();
    const bundle = await response.json();
    bundle.capture.codeObjects = Array.from({ length: 51 }, (_, i) => ({
      id: `partition-${i}`,
      objectId: "1",
      scope: `partition ${i} · GUID a · node 1 · process ${i}`,
      agent: "1",
      uri: i === 50 ? null : `file:///object-${i}`,
      storage: i === 50 ? "MEMORY" : "FILE",
      loadBase: "9007199254740993",
      loadSize: "4096",
      loadDelta: "-9007199254740993",
    }));
    await route.fulfill({ json: bundle });
  });
  await page.goto("/");
  await page.getByRole("button", { name: /^Capture evidence/ }).click();
  const panel = page.locator("section").filter({
    has: page.getByRole("heading", {
      name: "Recorded code objects",
      exact: true,
    }),
  });
  await expect(panel.locator("details")).toHaveCount(50);
  await page
    .getByRole("button", { name: "Next code objects", exact: true })
    .click();
  await expect(panel.locator("details")).toHaveCount(1);
  await panel.locator("summary").click();
  await expect(panel).toContainText("process 50");
  await expect(panel).toContainText("9007199254740993");
  await expect(panel).toContainText("MEMORY");
  await page.getByLabel("Search code objects").fill("object-12");
  await expect(panel.locator("details")).toHaveCount(1);
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  await page.getByRole("button", { name: /^Capture evidence/ }).click();
  await expect(page.getByLabel("Search code objects")).toHaveValue("object-12");
});

test("code-object extraction reports native receipts, errors and cancellation", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const bundle = await (await route.fetch()).json();
    bundle.capture.codeObjects = [
      {
        id: "scoped-file",
        objectId: "7",
        scope: "process 1",
        uri: "file:///host#offset=32&size=173",
        storage: "FILE",
      },
      {
        id: "scoped-memory",
        objectId: "8",
        scope: "process 1",
        uri: "memory://1#offset=4096&size=173",
        storage: "MEMORY",
      },
    ];
    await route.fulfill({ json: bundle });
  });
  await page.addInitScript(() => {
    const w = window as any;
    w.isTauri = true;
    w.extractions = [];
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, args: any) => {
        if (command === "save_project") {
          w.savedRelocations = structuredClone(args);
          return true;
        }
        if (command === "open_project") {
          const bundle = await (await fetch("/demo.json")).json();
          return {
            captures: [{ ...bundle.capture, id: "demo" }],
            project: {
              schemaVersion: 3,
              active: 0,
              definitionLibrary: w.savedRelocations.definitionLibrary,
              entries: w.savedRelocations.entries.map((entry: any) => ({
                input: null,
                state: entry.state,
                sources: entry.sources,
              })),
            },
          };
        }
        if (command !== "extract_code_object") throw Error(command);
        if (args.chooseSource && w.deferRelocation)
          return new Promise((resolve) => {
            w.finishRelocation = resolve;
          });
        if (args.sourcePath) {
          w.retainedExtraction = structuredClone(args);
          return {
            path: "/saved/reused.co",
            source: args.sourcePath,
            offset: "32",
            bytes: "173",
            sha256: "fresh-hash",
            recordedUri: "file:///host#offset=32&size=173",
            remapped: true,
          };
        }
        if (args.chooseSource)
          return {
            path: "/saved/remapped.co",
            source: "/relocated/host",
            offset: "32",
            bytes: "173",
            sha256: "def456",
            recordedUri: "file:///host#offset=32&size=173",
            remapped: true,
          };
        w.extractions.push(args);
        if (w.extractions.length === 2)
          throw Error(
            "Recorded range does not start with a little-endian ELF64 AMDGPU object",
          );
        if (w.extractions.length === 3) return null;
        return {
          path: "/saved/kernel.co",
          source: "/host",
          offset: "32",
          bytes: "173",
          sha256: "abc123",
        };
      },
    };
  });
  await page.goto("/");
  await page.getByRole("button", { name: /^Capture evidence/ }).click();
  await page.getByText("Object 7 ·", { exact: false }).click();
  const extract = page.getByRole("button", {
    name: "Extract recorded file range…",
    exact: true,
  });
  await extract.click();
  await expect(page.getByRole("status")).toContainText(
    "Saved 173 bytes to /saved/kernel.co",
  );
  await expect(page.getByRole("status")).toContainText("SHA-256: abc123");
  await expect(page.getByRole("status")).toContainText(
    "Identity at profiling time remains unverified",
  );
  expect(await page.evaluate(() => (window as any).extractions)).toEqual([
    { captureId: "demo", objectId: "scoped-file" },
  ]);
  await extract.click();
  await expect(page.getByRole("status")).toContainText("does not start with");
  await extract.click();
  await expect(page.getByRole("status")).toContainText("Extraction cancelled");
  await page.getByText("Object 8 ·", { exact: false }).click();
  await expect(
    page.getByText("Memory-only URI:", { exact: false }),
  ).toBeVisible();
  await expect(extract).toHaveCount(1);
  await page
    .getByRole("button", { name: "Select relocated container…", exact: true })
    .click();
  await expect(page.getByRole("status")).toContainText(
    "User-selected replacement container: /relocated/host",
  );
  await expect(page.getByRole("status")).toContainText(
    "Recorded URI: file:///host#offset=32&size=173",
  );
  await expect(
    page.getByText("Object 7 · file:///host#offset=32&size=173", {
      exact: true,
    }),
  ).toBeVisible();
  await expect(
    page.getByText("Retained container:", { exact: false }),
  ).toContainText("/relocated/host");
  await page.getByRole("button", { name: "Save project", exact: true }).click();
  await expect(
    page.getByRole("status").filter({ hasText: "Project saved" }),
  ).toBeVisible();
  const saved = await page.evaluate(
    () =>
      (window as any).savedRelocations.entries[0].state.analysisPreferences[
        "objects.relocations"
      ],
  );
  expect(JSON.parse(saved)).toEqual([
    ["scoped-file", "file:///host#offset=32&size=173", "/relocated/host"],
  ]);
  await page
    .getByRole("button", { name: "Forget retained container", exact: true })
    .click();
  await expect(
    page.getByRole("button", {
      name: "Extract from retained container…",
      exact: true,
    }),
  ).toHaveCount(0);
  await page.getByRole("button", { name: "Open project", exact: true }).click();
  await page.getByText("Object 7 ·", { exact: false }).click();
  await page
    .getByRole("button", {
      name: "Extract from retained container…",
      exact: true,
    })
    .click();
  await expect(
    page.getByRole("status").filter({ hasText: "SHA-256: fresh-hash" }),
  ).toBeVisible();
  expect(await page.evaluate(() => (window as any).retainedExtraction)).toEqual(
    {
      captureId: "demo",
      objectId: "scoped-file",
      sourcePath: "/relocated/host",
    },
  );
  await page.evaluate(() => {
    (window as any).deferRelocation = true;
  });
  await page
    .getByRole("button", { name: "Select relocated container…", exact: true })
    .click();
  await expect
    .poll(() => page.evaluate(() => typeof (window as any).finishRelocation))
    .toBe("function");
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  await page.evaluate(() =>
    (window as any).finishRelocation({
      path: "/saved/late.co",
      source: "/late/host",
      offset: "32",
      bytes: "173",
      sha256: "late",
      recordedUri: "file:///host#offset=32&size=173",
      remapped: true,
    }),
  );
  await page.getByRole("button", { name: /^Capture evidence/ }).click();
  await page.getByText("Object 7 ·", { exact: false }).click();
  await expect(
    page.getByText("Retained container:", { exact: false }),
  ).toContainText("/relocated/host");
  await expect(
    page.getByText("Retained container:", { exact: false }),
  ).not.toContainText("/late/host");
});

test("SPM expression clocks evaluate observed intervals in the worker", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const bundle = await (await route.fetch()).json();
    bundle.capture.counters = [
      {
        name: "BUSY",
        unit: "events",
        scope: "process 0 · agent 42 · dispatch 1 · counter 1 · []",
        points: [
          [10, 2],
          [30, 6],
          [60, 9],
        ],
      },
    ];
    await route.fulfill({ json: bundle });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  await expect(
    page.getByText("SCLOCK is the plotted", { exact: false }),
  ).toBeVisible();
  await page
    .getByLabel("Counter definitions")
    .fill(
      "Intervals := sum[SPM_CLOCK]\nClocks := sum[SCLOCK]\nRate := sum[BUSY / SPM_CLOCK]",
    );
  await page
    .getByRole("button", { name: "Evaluate definitions", exact: true })
    .click();
  const values = page.locator(".tensor-result output");
  await expect(values).toHaveCount(3);
  expect((await values.allTextContents()).map(Number)).toEqual([
    75,
    100,
    expect.closeTo(0.68, 12),
  ]);
});

test("capture evidence displays retained REALTIME anchor availability", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const bundle = await (await route.fetch()).json();
    bundle.capture.seTimeOffsets = { "0": 0, "2": 50 };
    bundle.capture.realtime = {
      frequencyHz: "9007199254740993",
      anchors: [
        {
          se: 2,
          shaderClock: "-9007199254740993",
          realtimeClock: "18446744073709551615",
        },
      ],
    };
    await route.fulfill({ json: bundle });
  });
  await page.goto("/");
  await page.getByRole("button", { name: /^Capture evidence/ }).click();
  const cells = page.locator(".evidence-grid > div");
  await expect(
    cells.filter({ hasText: "Cross-SE clock alignment" }),
  ).toContainText("Applied (2 engines)");
  await expect(cells.filter({ hasText: "REALTIME anchors" })).toContainText(
    "1",
  );
  await expect(
    cells.filter({ hasText: "Recorded realtime frequency (Hz)" }),
  ).toContainText("9007199254740993");
});

test("calibrated SPM worker uses retained realtime intervals", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const bundle = await (await route.fetch()).json();
    bundle.capture.counters = [
      {
        name: "BUSY",
        unit: "events",
        scope: "process 0 · agent 42 · counter 1 · []",
        points: [
          [500, 2],
          [530, 6],
          [580, 9],
        ],
        spmClock: {
          anchorSe: 2,
          realtimeTimestamps: [
            "18446744073709551515",
            "18446744073709551525",
            "18446744073709551535",
          ],
        },
      },
    ];
    await route.fulfill({ json: bundle });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  await page
    .getByLabel("Counter definitions")
    .fill("Shader := sum[SCLOCK]\nRealtime := sum[SPM_CLOCK]");
  await page
    .getByRole("button", { name: "Evaluate definitions", exact: true })
    .click();
  await expect(page.locator(".tensor-result output")).toHaveText([
    "1610",
    "30",
  ]);
});

test("SPM attachment opens a combined native capture and preserves failure state", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  const bundle = JSON.parse(await readFile("public/demo.json", "utf8"));
  await page.addInitScript((capture) => {
    const w = window as any;
    w.isTauri = true;
    w.attachMode = "cancel";
    w.native = {
      ...capture,
      id: "native-trace",
      name: "Original trace",
      architecture: "Unknown",
      waves: [],
      realtime: {
        frequencyHz: null,
        anchors: [{ se: 0, shaderClock: "0", realtimeClock: "1000" }],
      },
    };
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, args: any) => {
        if (command === "plugin:dialog|open") return "/trace";
        if (command === "open_capture") return w.native;
        if (command === "abort_catalog_transfer") return;
        if (command === "read_catalog_page") return {
          transferId: w.attachArgs.transferId, captureId: w.combined.id,
          offset: args.offset, total: w.combined.instructions.length,
          instructions: w.combined.instructions.slice(args.offset, args.offset + args.rows),
        };
        if (command === "finish_catalog_transfer") return w.combined.id;
        if (command === "attach_spm") {
          w.attachArgs = args;
          if (w.attachMode === "cancel") return null;
          if (w.attachMode === "error")
            throw Error("Conflicting REALTIME anchors");
          w.combined = {
            ...w.native,
            id: "combined",
            name: "Combined snapshot",
            counters: [
              {
                name: "BUSY",
                unit: "events",
                scope: "process 0 · agent 1 · counter 1 · []",
                points: [
                  [0, 1],
                  [10, 2],
                ],
                spmClock: { anchorSe: 0, realtimeTimestamps: ["1000", "1010"] },
              },
            ],
          };
          return { transport: "instruction-pages-v1", transferId: args.transferId,
            instructionCount: w.combined.instructions.length, maxPageRows: 4096,
            capture: { ...w.combined, instructions: [] } };
        }
        throw Error(command);
      },
    };
  }, bundle.capture);
  await page.goto("/");
  const attach = page.getByRole("button", { name: "Attach SPM…", exact: true });
  await expect(attach).toBeDisabled();
  await page
    .getByRole("button", { name: "Open capture file", exact: true })
    .click();
  await expect(attach).toBeEnabled();
  await attach.click();
  await expect(
    page.getByText("SPM attachment cancelled.", { exact: true }),
  ).toBeVisible();
  await page.evaluate(() => {
    (window as any).attachMode = "error";
  });
  await attach.click();
  await expect(
    page.getByText("Error: Conflicting REALTIME anchors", { exact: true }),
  ).toBeVisible();
  await expect(attach).toBeEnabled();
  await page.evaluate(() => {
    (window as any).attachMode = "success";
  });
  await attach.click();
  await expect(
    page.getByText(
      "Combined trace + SPM archive saved and opened. Save project to retain this tab.",
      { exact: true },
    ),
  ).toBeVisible();
  await expect(attach).toBeDisabled();
  await page.screenshot({
    path: test.info().outputPath("attached-workspace.png"),
    fullPage: true,
  });
  expect(await page.evaluate(() => (window as any).attachArgs)).toEqual({
    captureId: "native-trace",
    se: 0,
    instructionPages: true,
    transferId: expect.any(String),
  });
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  await page.getByLabel("Counter definitions").fill("Total := sum[SPM_CLOCK]");
  await page
    .getByRole("button", { name: "Evaluate definitions", exact: true })
    .click();
  await expect(page.locator(".tensor-result output")).toHaveText("20");
});

test("ISA optional columns distinguish missing sample values from zero", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const bundle = await (await route.fetch()).json();
    const row = bundle.capture.instructions.find((row: any) => row.index === 3);
    Object.assign(row, {
      pcIssued: 0,
      pcStalled: null,
      codeObject: "object-42",
      source: "/recorded/kernel.hip:17",
      idle: 23,
    });
    await route.fulfill({ json: bundle });
  });
  await page.goto("/");
  await page.getByText("ISA columns", { exact: true }).click();
  for (const name of [
    "Idle",
    "PC issued",
    "PC stalled",
    "Code object",
    "Source reference",
  ])
    await page.getByLabel(`Show ISA ${name}`, { exact: true }).check();
  const row = page
    .locator(".isa-table tbody tr")
    .filter({ hasText: "object-42" });
  await expect(row.locator("td").nth(7)).toHaveText("0");
  await expect(row.locator("td").nth(8)).toHaveText("0");
  await expect(row.locator("td").nth(9)).toHaveText("—");
  await expect(row.locator("td").nth(11)).toHaveAttribute(
    "title",
    "/recorded/kernel.hip:17",
  );
});

test("source cost bars isolate code objects and preserve missing and partial values", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const bundle = await (await route.fetch()).json();
    bundle.capture.sources["cost.h"] = "header\npartial cost\nzero cost\n";
    for (const [id, line, value, object] of [
      [3, 2, 7, "same"],
      [5, 2, null, "same"],
      [1, 2, 999, "other"],
      [4, 3, 0, "same"],
    ]) {
      Object.assign(
        bundle.capture.instructions.find((row: any) => row.index === id),
        {
          source: `cost.h:${line} -> cost.h:${line}`,
          pcIssued: value,
          codeObject: object,
        },
      );
    }
    Object.assign(
      bundle.capture.instructions.find((row: any) => row.index === 3),
      { latency: 100, stall: 40 },
    );
    Object.assign(
      bundle.capture.instructions.find((row: any) => row.index === 5),
      { latency: 80, stall: 20 },
    );
    await route.fulfill({ json: bundle });
  });
  await page.route("**/demo-hidden.json", async (route) => {
    const report = await (await route.fetch()).json();
    report.byInstruction[3] = { idle: 0, issue: 10, stall: 20 };
    report.byInstruction[5] = { idle: 0, issue: 5, stall: 10 };
    await route.fulfill({ json: report });
  });
  await page.goto("/");
  await page
    .locator(".isa-table tbody tr")
    .filter({ hasText: "global_load_dword" })
    .click();
  await page.getByLabel("Source cost metric").selectOption("pcIssued");
  const partial = page.getByRole("button", {
    name: "Source line 2: partial cost",
    exact: true,
  });
  await expect(partial.locator(".source-cost-value")).toHaveText("≥ 7");
  await expect(partial.locator(".source-cost-fill")).toHaveAttribute(
    "style",
    "width: 100%;",
  );
  await expect(
    page
      .getByRole("button", { name: "Source line 3: zero cost", exact: true })
      .locator(".source-cost-value"),
  ).toHaveText("0");
  await expect(
    page
      .getByRole("button", { name: "Source line 1: header", exact: true })
      .locator(".source-cost-value"),
  ).toHaveText("—");
  await page.getByLabel("Source cost metric").selectOption("latency");
  await expect(partial.locator(".source-cost-value")).toHaveText("180");
  await page
    .getByLabel("Include hidden source cost", { exact: true })
    .uncheck();
  await expect(partial.locator(".source-cost-value")).toHaveText("135");
  await expect(
    page.locator(".isa-table tbody tr.selected td").nth(4),
  ).toHaveText("100");
  await page.getByLabel("Include hidden source cost", { exact: true }).check();
  await expect(partial.locator(".source-cost-value")).toHaveText("180");
  await page.getByLabel("Source cost metric").selectOption("pcIssued");
  await page.getByLabel("Source line numbers", { exact: true }).uncheck();
  await expect(page.locator(".source-line-number")).toHaveCount(0);
  await page.getByLabel("Source cost bar width").focus();
  await page.getByLabel("Source cost bar width").press("Home");
  await expect(page.locator(".source-cost")).toHaveCount(0);
  await page.getByLabel("Source cost bar width").press("End");
  await expect(partial.locator(".source-cost")).toHaveAttribute(
    "style",
    "width: 400px;",
  );
  await page.getByRole("button", { name: "Counters", exact: true }).click();
  await page
    .getByRole("button", { name: "Trace workspace", exact: true })
    .click();
  await expect(page.getByLabel("Source cost metric")).toHaveValue("pcIssued");
  await expect(page.getByLabel("Source cost bar width")).toHaveValue("400");
  await expect(
    page.getByLabel("Source line numbers", { exact: true }),
  ).not.toBeChecked();
  await page.screenshot({
    path: test.info().outputPath("source-costs.png"),
    fullPage: true,
  });
});

test("code font controls size ISA and source rows and persist across reloads", async ({
  page,
}, testInfo) => {
  await page.goto("/");
  const control = page.getByLabel("Code font size");
  const isa = page.locator(".isa-table td code").first();
  const source = page.locator(".source code").first();
  await expect(control).toHaveValue("9");
  await expect(isa).toHaveCSS("font-size", "12px");
  await expect(source).toHaveCSS("font-size", "12px");
  await control.selectOption("19");
  const sizes = async () =>
    page
      .locator(".isa-table td code, .source code")
      .evaluateAll((nodes) =>
        nodes.map((node) => parseFloat(getComputedStyle(node).fontSize)),
      );
  await expect
    .poll(async () =>
      (await sizes()).every((size) => Math.abs(size - (19 * 4) / 3) < 0.01),
    )
    .toBe(true);
  const row = page.locator(".source > button").first();
  expect((await row.boundingBox())!.height).toBeGreaterThan(35);
  const lineNumber = await row.locator(".source-line-number").boundingBox();
  const cost = await row.locator(".source-cost").boundingBox();
  expect(lineNumber!.width).toBeGreaterThan(25);
  expect(cost!.x).toBeGreaterThan(lineNumber!.x + lineNumber!.width);
  await page.screenshot({
    path: testInfo.outputPath("code-font-19pt.png"),
    fullPage: true,
  });
  await page.reload();
  await expect(control).toHaveValue("19");
  await expect
    .poll(async () =>
      (await sizes()).every((size) => Math.abs(size - (19 * 4) / 3) < 0.01),
    )
    .toBe(true);
  await control.selectOption("5");
  await expect
    .poll(async () =>
      (await sizes()).every((size) => Math.abs(size - (5 * 4) / 3) < 0.01),
    )
    .toBe(true);
  await control.selectOption("9");
  await expect(source).toHaveCSS("font-size", "12px");
});

test("high-DPI display strokes preserve timeline picking and update on pixel-ratio changes", async ({
  browser,
}, testInfo) => {
  const context = await browser.newContext({
    deviceScaleFactor: 2,
    viewport: { width: 1500, height: 1050 },
  });
  try {
    const page = await context.newPage();
    await page.route("**/demo.json", async (route) => {
      const b = await (await route.fetch()).json();
      b.capture.start = 0;
      b.capture.end = 1000;
      const id = b.capture.waves[0].id;
      b.capture.waves = [{ ...b.capture.waves[0], start: 0, end: 1000 }];
      b.waves[id] = {
        ...b.waves[id],
        start: 0,
        end: 1000,
        tokens: [
          { start: 100, duration: 200, stall: 0, category: 6, instruction: 2 },
        ],
      };
      await route.fulfill({ json: b });
    });
    await page.goto("http://127.0.0.1:1421/");
    const canvas = page.locator("canvas");
    const physicalStroke = () =>
      canvas.evaluate((el: HTMLCanvasElement) => {
        const ctx = el.getContext("2d")!;
        return ctx.lineWidth * ctx.getTransform().a;
      });
    await expect.poll(physicalStroke).toBe(2);
    const box = (await canvas.boundingBox())!;
    const point = { x: 155 + 0.2 * (box.width - 169), y: 39 };
    await canvas.click({ position: point });
    await expect(page.locator(".isa-table tr.selected")).toContainText(
      "v_mad_u32_u24",
    );
    const selected = await page.locator(".isa-table tr.selected").innerText();
    await page.getByLabel("Plot display scaling").selectOption("device");
    await expect.poll(physicalStroke).toBe(1);
    expect((await canvas.boundingBox())!.width).toBe(box.width);
    await canvas.click({ position: point });
    await expect(page.locator(".isa-table tr.selected")).toHaveText(selected, {
      useInnerText: true,
    });
    await page.mouse.move(box.x + point.x, box.y + point.y);
    await page.mouse.wheel(0, -100);
    await expect
      .poll(async () => Number(await page.getByLabel("Range end").inputValue()))
      .toBeLessThan(1000);
    await page.getByRole("button", { name: "Occupancy", exact: true }).click();
    await expect(page.locator(".line-chart path").first()).toHaveAttribute(
      "stroke-width",
      "0.9",
    );
    await expect(page.locator(".line-chart path").first()).toHaveAttribute(
      "vector-effect",
      "non-scaling-stroke",
    );
    const pending = page.waitForEvent("download");
    await page
      .locator(".line-plot")
      .first()
      .getByRole("button", { name: /Export .* as SVG/ })
      .click();
    const exported = await (await pending).path();
    const { readFile } = await import("node:fs/promises");
    const svg = await readFile(exported!, "utf8");
    expect(svg).toContain("device strokes at DPR 2");
    expect(svg).toContain('stroke-width="0.9"');
    await page.screenshot({
      path: testInfo.outputPath("device-pixel-strokes.png"),
      fullPage: true,
    });
    await page.reload();
    await expect(page.getByLabel("Plot display scaling")).toHaveValue("device");
    await expect.poll(physicalStroke).toBe(1);
    await page.evaluate(() => {
      Object.defineProperty(window, "devicePixelRatio", {
        configurable: true,
        value: 1.5,
      });
      window.dispatchEvent(new Event("resize"));
    });
    await expect
      .poll(() =>
        canvas.evaluate(
          (el: HTMLCanvasElement) =>
            el.width / el.getBoundingClientRect().width,
        ),
      )
      .toBeCloseTo(1.5, 2);
    await expect.poll(physicalStroke).toBeCloseTo(1, 5);
    await page.getByLabel("Plot display scaling").selectOption("system");
    await expect.poll(physicalStroke).toBe(1.5);
  } finally {
    await context.close();
  }
});

test("combined marker source hierarchy conserves token costs, zooms exports and navigates ISA", async ({
  page,
}, testInfo) => {
  await page.route("**/demo.json", async (route) => {
    const b = await (await route.fetch()).json();
    const first = b.capture.waves[0];
    b.capture.start = 0;
    b.capture.end = 100;
    b.capture.waves = [{ ...first, start: 0, end: 100 }];
    b.waves[first.id] = {
      ...b.waves[first.id],
      start: 0,
      end: 100,
      cu: 5,
      tokens: [0, 10, 20, 30, 40].map((start) => ({
        start,
        duration: 4,
        stall: 1,
        category: 6,
        instruction: 2,
      })),
    };
    const instruction = b.capture.instructions.find((i: any) => i.index === 2);
    instruction.source = "inner.h:2 -> outer.h:8";
    instruction.text = "marker_test_op";
    b.capture.markers = [
      {
        id: 1,
        parent: null,
        markerId: 1,
        name: "Outer scope",
        depth: 0,
        start: 10,
        end: 40,
      },
      {
        id: 2,
        parent: 1,
        markerId: 2,
        name: "Inner scope",
        depth: 1,
        start: 10,
        end: 20,
      },
    ].map((m) => ({
      ...m,
      kind: "U",
      source: "outer.h:8",
      point: false,
      codeObject: instruction.codeObject,
      se: first.se,
      cu: 5,
      simd: first.simd,
      slot: first.slot,
    }));
    await route.fulfill({ json: b });
  });
  await page.goto("/");
  await page
    .getByRole("button", { name: "Marker scopes", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Build marker/source hierarchy", exact: true })
    .click();
  const plot = page.getByRole("img", {
    name: "Combined marker source and ISA hierarchy",
  });
  await expect(plot).toBeVisible();
  await expect(
    plot.getByRole("button", { name: "Outer scope: 12 cycles", exact: true }),
  ).toHaveCount(1);
  await expect(
    plot.getByRole("button", { name: "Inner scope: 4 cycles", exact: true }),
  ).toHaveCount(1);
  await expect(
    plot.getByRole("button", {
      name: "[No marker scope]: 8 cycles",
      exact: true,
    }),
  ).toHaveCount(1);
  await plot
    .getByRole("button", { name: "Inner scope: 4 cycles", exact: true })
    .click();
  await expect(page.getByLabel("Marker source cost range end")).toHaveValue(
    "12",
  );
  const pending = page.waitForEvent("download");
  await page
    .getByRole("button", {
      name: "Export Marker source hierarchy as SVG",
      exact: true,
    })
    .click();
  const file = await (await pending).path();
  const { readFile } = await import("node:fs/promises");
  const svg = await readFile(file!, "utf8");
  expect(svg).toContain("marker_test_op");
  expect(svg).toContain("Inner scope");
  expect(svg).toContain("outer.h:8");
  await page.screenshot({
    path: testInfo.outputPath("marker-source.png"),
    fullPage: true,
  });
  await plot
    .getByRole("button", { name: "marker_test_op: 4 cycles", exact: true })
    .click();
  await expect(page.locator(".isa-table tr.selected")).toContainText(
    "marker_test_op",
  );
});

test("marker hierarchy cancellation discards a late native wave and allows rebuilding", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  const bundle = JSON.parse(await readFile("public/demo.json", "utf8"));
  await page.addInitScript((bundle) => {
    const w = window as any;
    w.isTauri = true;
    w.holdHierarchy = false;
    const index = bundle.capture.waves[0];
    const wave = bundle.waves[index.id];
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, args: any) => {
        if (command === "plugin:dialog|open") return "/markers.json";
        if (command === "open_capture")
          return {
            ...bundle.capture,
            id: "marker-native",
            name: "Native markers",
            waves: [index],
          };
        if (command === "read_wave") {
          if (w.holdHierarchy)
            return new Promise((resolve) => {
              w.releaseHierarchy = () => resolve(wave);
            });
          return wave;
        }
        if (command === "begin_wave_query") return args.requestId;
        if (command === "cancel_wave_query") return null;
        if (command === "read_wave_window") return [wave];
        if (command === "close_capture") return;
        throw Error(command);
      },
    };
  }, bundle);
  await page.goto("/");
  await page
    .getByRole("button", { name: "Marker scopes", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Open capture file", exact: true })
    .click();
  await expect(
    page
      .locator(".capture-tabs")
      .getByRole("button", { name: /Native markers/ })
      .first(),
  ).toBeVisible();
  await expect(page.locator("canvas")).toBeVisible();
  await page
    .getByRole("button", { name: "Marker scopes", exact: true })
    .click();
  await page.evaluate(() => {
    (window as any).holdHierarchy = true;
  });
  await page
    .getByRole("button", { name: "Build marker/source hierarchy", exact: true })
    .click();
  await expect
    .poll(() => page.evaluate(() => typeof (window as any).releaseHierarchy))
    .toBe("function");
  await page
    .getByRole("button", { name: "Cancel hierarchy", exact: true })
    .click();
  await page.evaluate(() => {
    (window as any).holdHierarchy = false;
    (window as any).releaseHierarchy();
  });
  await expect(page.getByText("Cancelled", { exact: true })).toBeVisible();
  await expect(
    page.getByRole("img", { name: "Combined marker source and ISA hierarchy" }),
  ).toHaveCount(0);
  await page
    .getByRole("button", { name: "Build marker/source hierarchy", exact: true })
    .click();
  await expect(
    page.getByRole("img", { name: "Combined marker source and ISA hierarchy" }),
  ).toBeVisible();
});

test("marker hierarchy keeps stale slot scopes in an unverified ownership bucket", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const bundle = await (await route.fetch()).json();
    const index = bundle.capture.waves[0];
    const wave = bundle.waves[index.id];
    bundle.capture.waves = [index];
    bundle.capture.markers = [
      {
        id: 1,
        parent: null,
        markerId: 1,
        codeObject: "1",
        name: "Prior wave scope",
        kind: "U",
        source: "old.h:1",
        start: wave.start - 1,
        end: null,
        point: false,
        depth: 0,
        se: index.se,
        cu: wave.cu,
        simd: index.simd,
        slot: index.slot,
      },
    ];
    await route.fulfill({ json: bundle });
  });
  await page.goto("/");
  await page
    .getByRole("button", { name: "Marker scopes", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Build marker/source hierarchy", exact: true })
    .click();
  const plot = page.getByRole("img", {
    name: "Combined marker source and ISA hierarchy",
  });
  await expect(plot).toBeVisible();
  await expect(
    plot.getByRole("button", { name: /^\[Unverified marker ownership\]:/ }),
  ).toHaveCount(1);
  await expect(
    plot.getByRole("button", { name: /^Prior wave scope:/ }),
  ).toHaveCount(0);
  await expect(plot.locator("g")).not.toHaveCount(0);
});

test("explicit ATT associations persist ownership and open separate captures", async ({
  page,
}) => {
  const { readFile } = await import("node:fs/promises");
  const capture = JSON.parse(
    await readFile("public/demo.json", "utf8"),
  ).capture;
  capture.dispatches = [
    { ...capture.dispatches[0], id: "partition-a:guid:42", kernel: "first" },
    { ...capture.dispatches[0], id: "partition-b:guid:42", kernel: "second" },
  ];
  await page.route("**/demo.json", async (route) => {
    const bundle = await (await route.fetch()).json();
    bundle.capture = capture;
    await route.fulfill({ json: bundle });
  });
  await page.addInitScript((capture) => {
    const w = window as any;
    w.isTauri = true;
    w.associationPath = "/trace.rocprof-att";
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, args: any) => {
        if (command === "plugin:dialog|open") {
          if (w.delayAssociation) return new Promise((resolve, reject) => { w.resolveAssociation = resolve; w.rejectAssociation = reject; });
          return w.associationPath;
        }
        if (command === "save_project") {
          w.savedAssociation = structuredClone(args);
          return true;
        }
        if (command === "open_project")
          return {
            captures: [{ ...capture, id: "demo" }],
            project: {
              schemaVersion: 3,
              active: 0,
              definitionLibrary: w.savedAssociation.definitionLibrary,
              entries: w.savedAssociation.entries.map((e: any) => ({
                input: null,
                state: e.state,
                sources: e.sources,
              })),
            },
          };
        if (command === "open_capture") {
          w.associationOpened = args.path;
          if (w.failAssociation) throw Error("ATT manifest input is missing");
          return {
            ...capture,
            id: "associated",
            name: "Associated trace",
            waves: [],
            dispatches: [],
            metadata: {},
            instructions: [],
          };
        }
        throw Error(command);
      },
    };
  }, capture);
  await page.goto("/");
  await page.getByRole("button", { name: /^Capture evidence/ }).click();
  const panel = page.locator("section").filter({
    has: page.getByRole("heading", {
      name: "Associated ATT manifests",
      exact: true,
    }),
  });
  await expect(panel).toContainText("does not align or merge timestamps");
  await page
    .getByRole("button", { name: "Associate ATT manifest…", exact: true })
    .click();
  await expect(panel).toContainText("Unknown dispatch ownership");
  await page.getByLabel("Trace collection mode").selectOption("region");
  await page
    .getByRole("button", { name: "Associate ATT manifest…", exact: true })
    .click();
  await expect(panel).toContainText("no single-dispatch assignment");
  await page.getByRole("button", { name: "Save project", exact: true }).click();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as any).savedAssociation?.entries[0].state
            .analysisPreferences["att.associations"],
      ),
    )
    .toBe(
      JSON.stringify([
        { path: "/trace.rocprof-att", mode: "region", dispatch: "" },
      ]),
    );
  await panel
    .getByRole("button", { name: "Remove trace association", exact: true })
    .click();
  await page
    .getByLabel("Trace collection mode")
    .selectOption("single-dispatch");
  await page
    .getByRole("button", { name: "Associate ATT manifest…", exact: true })
    .click();
  await expect(panel.getByRole("alert")).toContainText(
    "uniquely identified dispatch",
  );
  await page
    .getByLabel("Associated dispatch", { exact: true })
    .selectOption("partition-b:guid:42");
  await page
    .getByRole("button", { name: "Associate ATT manifest…", exact: true })
    .click();
  await expect(panel).toContainText(
    "User-asserted dispatch: partition-b:guid:42",
  );
  await page.getByRole("button", { name: "Save project", exact: true }).click();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as any).savedAssociation?.entries[0].state
            .analysisPreferences["att.associations"],
      ),
    )
    .toBe(
      JSON.stringify([
        {
          path: "/trace.rocprof-att",
          mode: "single-dispatch",
          dispatch: "partition-b:guid:42",
        },
      ]),
    );
  await panel
    .getByRole("button", { name: "Remove trace association", exact: true })
    .click();
  await expect(
    panel.getByRole("button", { name: "Open associated trace", exact: true }),
  ).toHaveCount(0);
  await page.getByRole("button", { name: "Open project", exact: true }).click();
  await expect(panel).toContainText(
    "User-asserted dispatch: partition-b:guid:42",
  );
  await page.evaluate(() => { (window as any).associationPath = "/moved/trace.rocprof-att"; });
  await panel.getByRole("button", {name:"Locate moved manifest…", exact:true}).click();
  await expect(panel).toContainText("Recorded manifest: /trace.rocprof-att");
  await expect(panel).toContainText("Selected replacement: /moved/trace.rocprof-att");
  await page.getByRole("button", {name:"Save project",exact:true}).click();
  await panel.getByRole("button", {name:"Forget manifest replacement",exact:true}).click();
  await expect(panel).not.toContainText("Selected replacement:");
  await page.getByRole("button", {name:"Open project",exact:true}).click();
  await expect(panel).toContainText("Selected replacement: /moved/trace.rocprof-att");
  for (const outcome of ["success", "failure"]) {
    await page.evaluate(() => { (window as any).delayAssociation = true; (window as any).resolveAssociation = null; });
    await panel.getByRole("button", {name:"Locate moved manifest…",exact:true}).click();
    await expect.poll(() => page.evaluate(() => typeof (window as any).resolveAssociation)).toBe("function");
    await page.getByRole("button", {name:"Open project",exact:true}).click();
    await expect(panel.getByRole("button", {name:"Locate moved manifest…",exact:true})).toBeEnabled();
    await page.evaluate((outcome) => {
      const w=window as any; w.delayAssociation=false;
      if(outcome==="success") w.resolveAssociation("/stale.rocprof-att");
      else w.rejectAssociation(Error("stale picker failure"));
    }, outcome);
    await page.evaluate(() => new Promise(resolve => setTimeout(resolve, 0)));
    await expect(panel).toContainText("Selected replacement: /moved/trace.rocprof-att");
    await expect(panel).not.toContainText("/stale.rocprof-att");
    await expect(panel).not.toContainText("stale picker failure");
  }
  await page.evaluate(() => {
    (window as any).failAssociation = true;
  });
  await panel
    .getByRole("button", { name: "Open associated trace", exact: true })
    .click();
  await expect(
    page.getByText("ATT manifest input is missing", { exact: false }),
  ).toBeVisible();
  await expect(panel).toContainText(
    "User-asserted dispatch: partition-b:guid:42",
  );
  await page.evaluate(() => {
    (window as any).failAssociation = false;
  });
  await panel
    .getByRole("button", { name: "Open associated trace", exact: true })
    .click();
  await expect
    .poll(() => page.evaluate(() => (window as any).associationOpened))
    .toBe("/moved/trace.rocprof-att");
  await expect(
    page.getByRole("button", { name: /Associated trace/ }).first(),
  ).toBeVisible();
});

test("million-entry archived ISA catalog remains paginated and searchable", async ({
  page,
}) => {
  const fixture = process.env.ROCPROF_LARGE_CATALOG_BUNDLE;
  test.skip(!fixture, "Requires the archived million-entry catalog fixture");
  test.setTimeout(120_000);
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.route("**/demo.json", (route) =>
    route.continue({
      url: new URL(`/@fs${fixture}`, route.request().url()).href,
    }),
  );
  await page.goto("/");
  await expect(page.locator(".isa-table tbody tr")).toHaveCount(150, {
    timeout: 90_000,
  });
  await expect(page.getByText("Page 1 / 7642", { exact: false })).toBeVisible();
  await page
    .getByRole("button", { name: "Next ISA page", exact: true })
    .click();
  await expect(
    page.locator(".isa-table tbody tr").first().locator("td").first(),
  ).toHaveText("151");
  await page.locator(".isa-table tbody tr").nth(1).click();
  await expect(page.locator(".isa-table tr.selected td").first()).toHaveText(
    "152",
  );
  await page.getByLabel("Search ISA", { exact: true }).fill("s_endpgm");
  await expect(page.locator(".isa-table tbody tr")).toHaveCount(150);
  await expect
    .poll(async () =>
      (await page.locator(".isa-table tbody code").allTextContents()).every(
        (text) => text.includes("s_endpgm"),
      ),
    )
    .toBe(true);
  await page
    .getByLabel("Search ISA", { exact: true })
    .fill("__rocprof_absent_instruction_7642__");
  await expect(page.locator(".isa-table tbody tr")).toHaveCount(0);
  await page.getByLabel("Search ISA", { exact: true }).fill("");
  await expect(page.locator(".isa-table tbody tr")).toHaveCount(150);
  expect(errors).toEqual([]);
  const cdp = await page.context().newCDPSession(page);
  const heap = await cdp.send("Runtime.getHeapUsage");
  const { writeFile } = await import("node:fs/promises");
  await writeFile(
    test.info().outputPath("million-catalog-heap.json"),
    JSON.stringify(
      {
        instructions: 1146178,
        visibleRows: 150,
        heap,
        scope:
          "Post-interaction Chromium JavaScript heap; not process peak RSS or native WebKit",
      },
      null,
      2,
    ),
  );
  await test.info().attach("million-catalog-heap", {
    body: Buffer.from(
      JSON.stringify(
        {
          instructions: 1146178,
          visibleRows: 150,
          heap,
          scope:
            "Post-interaction Chromium JavaScript heap; not process peak RSS or native WebKit",
        },
        null,
        2,
      ),
    ),
    contentType: "application/json",
  });
  await page.screenshot({
    path: test.info().outputPath("million-catalog.png"),
  });
});


test("closing a capture discards its saved ISA state before same-ID reopen", async ({ page }) => {
  await page.goto("/");
  const search = page.getByPlaceholder("Search instructions, operands or source…");
  await search.fill("s_waitcnt");
  await expect(page.locator(".isa-table tbody tr")).toHaveCount(2);
  await page.getByRole("button", { name: "Close LDS transpose · example capture", exact: true }).click();
  await expect(page.locator(".capture-tab")).toHaveCount(0);
  await page.getByRole("button", { name: "Explore example", exact: true }).click();
  await expect(search).toHaveValue("");
  await expect(page.locator(".isa-table tbody tr")).toHaveCount(12);
});

test("raw stochastic sampling reports unresolved records without inventing ISA measurements", async ({ page }) => {
  let unresolvedOnly = false;
  await page.route("**/demo.json", async (route) => {
    const bundle = await (await route.fetch()).json();
    bundle.capture.waves = [];
    bundle.capture.metadata = {
      ...bundle.capture.metadata,
      pc_sampling_method: "stochastic",
      pc_sampling_records: "4",
      pc_sampling_unresolved: unresolvedOnly ? "4" : "1",
      pc_sampling_lock_errors: "1",
      instruction_costs: "incomplete",
    };
    bundle.capture.instructions = unresolvedOnly ? [] : bundle.capture.instructions.slice(0, 1).map((i: object) => ({
      ...i, pcIssued: 2, pcStalled: 1, pcSamples: 3, stallReasons: [0,0,0,1,0,0,0,0,0,0],
    }));
    await route.fulfill({ json: bundle });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "PC sampling", exact: true }).click();
  await expect(page.getByText("4 raw samples", { exact: false })).toContainText("1 unresolved samples excluded from ISA totals");
  await expect(page.getByText("4 raw samples", { exact: false })).toContainText("Lock-contention samples remain included");
  unresolvedOnly = true;
  await page.reload();
  await page.getByRole("button", { name: "PC sampling", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Samples have no resolved ISA locations" })).toBeVisible();
  await expect(page.getByText("4 stochastic samples were read", { exact: false })).toBeVisible();
  await expect(page.getByText("PC sampling was not collected", { exact: true })).toHaveCount(0);
});

test("host-trap sample totals, source weights and unavailable issue status stay distinct", async ({ page }) => {
  await page.route("**/demo.json", async (route) => {
    const bundle = await (await route.fetch()).json();
    bundle.capture.waves = [];
    bundle.capture.metadata = {
      ...bundle.capture.metadata,
      pc_sampling_method: "host-trap",
      pc_sampling_records: "10",
      pc_sampling_unresolved: "2",
      instruction_costs: "incomplete",
    };
    delete bundle.capture.metadata.pc_sampling_lock_errors;
    bundle.capture.instructions = bundle.capture.instructions.slice(0, 2).map((i: object, n: number) => ({
      ...i, pcIssued: null, pcStalled: null, pcSamples: n ? 5 : 3, stallReasons: null,
    }));
    await route.fulfill({ json: bundle });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "PC sampling", exact: true }).click();
  await expect(page.getByText("10 raw samples", { exact: false })).toContainText("2 unresolved samples excluded from ISA totals");
  await expect(page.getByText("10 raw samples", { exact: false })).not.toContainText("lock contention");
  await expect(page.getByText("Attributed samples", { exact: true }).locator("..")).toContainText("8");
  for (const label of ["Issued samples", "Stalled samples"]) {
    await expect(page.getByText(label, { exact: true }).locator("..")).toContainText("Unavailable");
  }
  await expect(page.getByRole("heading", { name: "Stall reasons unavailable" })).toBeVisible();
  await expect(page.getByLabel("Sort sampled instructions")).toHaveValue("samples");
  await expect(page.locator("tbody tr").first()).toContainText("5");
  await page.getByRole("button", { name: "Show sample flamegraph" }).click();
  await expect(page.getByLabel("Sample flamegraph weight")).toHaveValue("pcSamples");
  await expect(page.getByRole("heading", { name: "Attributed source samples" })).toBeVisible();
  await expect(page.locator(".flame-root")).toContainText("8 samples");
  await page.getByRole("button", { name: "Open ISA 1", exact: true }).click();
  await expect(page.locator(".isa-table tr.selected")).toBeVisible();
  await page.getByRole("button", { name: "PC sampling", exact: true }).click();
  await expect(page.getByLabel("Sample flamegraph weight")).toHaveValue("pcSamples");
});

test("mixed PC sampling chooser cancels cleanly and opens each distribution separately", async ({ page }) => {
  const { readFile } = await import("node:fs/promises");
  const { capture } = JSON.parse(await readFile("public/demo.json", "utf8"));
  await page.addInitScript((capture) => {
    const w = window as any;
    w.isTauri = true;
    w.selectedMethods = [];
    w.abortedSelections = 0;
    w.__TAURI_INTERNALS__ = { invoke: async (command: string, args: any) => {
      if (command === "plugin:dialog|open") return "/mixed.json";
      if (command === "abort_catalog_transfer") { w.abortedSelections++; return; }
      if (command === "open_capture") {
        if (!args.samplingMethod) throw Error("Mixed host-trap and stochastic sampling methods require an explicit method selection");
        w.selectedMethods.push(args.samplingMethod);
        const host = args.samplingMethod === "host-trap";
        return {...capture, id: args.samplingMethod, name: `mixed.json · ${args.samplingMethod}`, waves: [], sourceFiles: [], sources: {},
          metadata: {pc_sampling_method: args.samplingMethod, pc_sampling_selection: args.samplingMethod, pc_sampling_records: host ? "2" : "5", pc_sampling_unresolved: "0", pc_sampling_other_method_records: host ? "5" : "2", pc_sampling_lock_errors: "0"},
          instructions: [{...capture.instructions[0], pcSamples: host ? 2 : 5, pcIssued: host ? null : 3, pcStalled: host ? null : 2, stallReasons: host ? null : [0,0,0,2]}]};
      }
      throw Error(command);
    }};
  }, capture);
  await page.goto("/");
  await page.getByTitle("Open capture file").click();
  const chooser = page.getByRole("dialog", { name: "Choose a PC sampling method" });
  await expect(chooser).toBeVisible();
  await chooser.getByRole("button", { name: "Cancel method selection" }).click();
  expect(await page.evaluate(() => (window as any).selectedMethods)).toEqual([]);
  expect(await page.evaluate(() => (window as any).abortedSelections)).toBe(1);
  await page.getByTitle("Open capture file").click();
  await chooser.getByRole("button", { name: "Open host-trap samples" }).click();
  await expect(page.getByRole("button", { name: "mixed.json · host-trap", exact: true })).toBeVisible();
  await page.getByRole("button", { name: "PC sampling", exact: true }).click();
  await expect(page.getByText("5 observations from the other sampling method", { exact: false })).toBeVisible();
  await expect(page.getByText("Issued samples", { exact: true }).locator("..")).toContainText("Unavailable");
  await page.getByTitle("Open capture file").click();
  await chooser.getByRole("button", { name: "Open stochastic samples" }).click();
  await expect(page.getByRole("button", { name: "mixed.json · stochastic", exact: true })).toBeVisible();
  await page.getByRole("button", { name: "PC sampling", exact: true }).click();
  await expect(page.getByText("2 observations from the other sampling method", { exact: false })).toBeVisible();
  expect(await page.evaluate(() => (window as any).selectedMethods)).toEqual(["host-trap", "stochastic"]);
});

test("PC sampling CSV observations preserve exact fields, duplicates and searchable pagination", async ({ page }) => {
  await page.route("**/demo.json", async (route) => {
    const bundle = await (await route.fetch()).json();
    bundle.capture.instructions = [];
    bundle.capture.waves = [];
    bundle.capture.metadata = { pc_sampling_method: "stochastic", pc_sampling_records: "101" };
    bundle.capture.pcSamplingObservations = {
      columns: ["Sample_Timestamp", "Exec_Mask", "Dispatch_Id", "Instruction", "Instruction_Comment", "Correlation_Id", "Wave_Issued_Instruction", "Instruction_Type", "Stall_Reason", "Wave_Count", "Extra"],
      rows: Array.from({ length: 101 }, (_, i) => ["18446744073709551615", "18446744073709551615", "9007199254740993", "v_add v0, v1", "", "1", i === 100 ? "0" : "1", "TYPE", "REASON", "4", i === 100 ? "last observation" : "duplicate"]),
    };
    await route.fulfill({ json: bundle });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "PC sampling", exact: true }).click();
  const table = page.getByRole("table", { name: "PC sampling CSV observations" });
  await expect(table.locator("tbody tr")).toHaveCount(100);
  await expect(table.locator("tbody tr").first()).toContainText("9007199254740993");
  await expect(table.locator("tbody tr").first()).toContainText("18446744073709551615");
  await expect(page.getByText("101 stochastic samples.", { exact: false })).toContainText("100 issued · 1 not issued");
  await page.getByRole("button", { name: "Next samples" }).click();
  await expect(table.locator("tbody tr")).toHaveCount(1);
  await expect(table.locator("tbody tr").first()).toContainText("last observation");
  await page.getByLabel("Search PC sampling observations").fill("duplicate");
  await expect(table.locator("tbody tr")).toHaveCount(100);
  await expect(page.getByRole("button", { name: "Next samples" })).toBeDisabled();
  await page.getByLabel("Search PC sampling observations").fill("absent");
  await expect(page.getByText("No samples match the search.")).toBeVisible();
  await expect(page.getByRole("heading", { name: "Samples have no resolved ISA locations" })).toHaveCount(0);
});

test("CSV search yields, replaces stale results and resets immediately when cleared", async ({ page }) => {
  await page.route("**/demo.json", async (route) => {
    const bundle = await (await route.fetch()).json();
    bundle.capture.instructions = [];
    bundle.capture.waves = [];
    bundle.capture.metadata = { pc_sampling_method: "host-trap", pc_sampling_records: "100000" };
    bundle.capture.pcSamplingObservations = {
      columns: ["Instruction", "Comment"],
      rows: Array.from({ length: 100000 }, (_, i) => ["old observation", i === 99999 ? "fresh-final-match" : String(i)]),
    };
    await route.fulfill({ json: bundle });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "PC sampling", exact: true }).click();
  await page.evaluate(() => {
    const state = { pendingSeen: false, staleWhilePending: false };
    (window as any).csvSearchProbe = state;
    new MutationObserver(() => {
      const table = document.querySelector('[aria-label="PC sampling CSV observations"]');
      if (table?.parentElement?.getAttribute("aria-busy") === "true") {
        state.pendingSeen = true;
        if (table.querySelectorAll("tbody tr").length) state.staleWhilePending = true;
      }
    }).observe(document.body, { subtree: true, attributes: true, childList: true, characterData: true });
  });
  const search = page.getByLabel("Search PC sampling observations");
  await search.fill("old");
  await search.fill("fresh-final-match");
  const rows = page.getByRole("table", { name: "PC sampling CSV observations" }).locator("tbody tr");
  await expect(rows).toHaveCount(1);
  await expect(rows.first()).toContainText("100000");
  await expect(page.getByText("1 matching samples", { exact: true })).toBeVisible();
  expect(await page.evaluate(() => (window as any).csvSearchProbe)).toEqual({ pendingSeen: true, staleWhilePending: false });
  await search.fill("absent");
  await search.clear();
  await expect(rows).toHaveCount(100);
  await expect(page.getByText("100,000 matching samples", { exact: true })).toBeVisible();
  await expect(page.getByText("No samples match the search.")).toHaveCount(0);
});
