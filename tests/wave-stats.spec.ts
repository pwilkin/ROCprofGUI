import { test, expect } from "@playwright/test";
import { readFile } from "node:fs/promises";
test("exact all-wave statistics coexist with oversized CU overview", async ({ page }) => {
  const bundle = JSON.parse(await readFile("public/demo.json", "utf8"));
  await page.addInitScript((bundle) => {
    const w = window as any; w.isTauri = true;
    const index = { ...bundle.capture.waves[0], start: 0, end: 1000 };
    const wave = { ...bundle.waves[index.id], tokens: [{ start: 110, duration: 50, stall: 0, category: 6, instruction: 2 }] };
    w.__TAURI_INTERNALS__ = { invoke: async (command: string, args: any) => {
      if (command === "plugin:dialog|open") return "/overview";
      if (command === "open_capture") return { ...bundle.capture, id: "overview", start: 0, end: 1000, architecture: "gfx942", metadata: {}, waves: [index] };
      if (command === "begin_wave_query") return args.requestId;
      if (command === "cancel_wave_query") return null;
      if (command === "read_wave_stats") { w.statsQueries = (w.statsQueries || 0) + 1; return [{instruction: 2, hits: 600001, latency: 1200002, stall: 7}]; }
      if (command === "read_wave") return wave;
      if (command === "read_wave_window") {
        if (args.end-args.start > 200) throw Error("Visible wave query exceeds 500,000 tokens");
        return [wave];
      }
      if (command === "read_wave_overview") return { range: [args.start,args.end],representedTokens:600001,waves:[{wave:index,cu:0,groups:[{category:6,start:100,end:800,count:600001,zoomRange:[100,200]}]}] };
      throw Error(command);
    }};
  }, bundle);
  await page.goto("/");
  await page.getByRole("button", { name: "Open capture file", exact: true }).click();
  await expect(page.locator(".capture-tab")).toHaveCount(2);
  await page.getByRole("button", { name: "CU waves", exact: true }).click();
  await expect(page.getByText(/CU overview: 600,001 recorded events/)).toBeVisible();
  await expect(page.getByLabel("Timeline detail summary")).toContainText("600,001 events");
  await expect(page.getByRole("option", { name: "Overview groups", exact: true })).toHaveCount(1);
  await page.getByLabel("Latency scope").selectOption("all");
  await expect.poll(() => page.evaluate(() => (window as any).statsQueries)).toBe(1);
  await expect(page.getByText("Calculating exact visible-wave statistics…")).toHaveCount(0);
  await expect(page.getByText(/CU overview: 600,001 recorded events/)).toBeVisible();
  await page.getByLabel("Latency scope").selectOption("all-mean");
  await expect.poll(() => page.evaluate(() => (window as any).statsQueries)).toBe(2);
  await expect(page.getByText("Calculating exact visible-wave statistics…")).toHaveCount(0);
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("pending, stale and failed statistics never display fabricated measurements", async ({ page }) => {
  const bundle = JSON.parse(await readFile("public/demo.json", "utf8"));
  await page.addInitScript(bundle => {
    const w = window as any;
    w.isTauri = true;
    w.statsRequests = [];
    w.statsCancelled = [];
    w.__TAURI_INTERNALS__ = { invoke: async (command: string, args: any) => {
      if (command === "plugin:dialog|open") return "/stats";
      if (command === "open_capture") return { ...bundle.capture, id: "stats", start: 0, end: 1000, architecture: "gfx942", metadata: {} };
      if (command === "read_wave") return bundle.waves[args.waveId];
      if (command === "begin_wave_query") return args.requestId;
      if (command === "cancel_wave_query") { w.statsCancelled.push(args.requestId); return null; }
      if (command === "read_wave_stats") return new Promise((resolve, reject) => w.statsRequests.push({args, resolve, reject}));
      throw Error(command);
    }};
  }, bundle);
  await page.goto("/");
  await page.getByRole("button", {name: "Open capture file", exact: true}).click();
  await expect(page.locator(".capture-tab")).toHaveCount(2);
  await page.getByLabel("Latency scope").selectOption("all");
  await expect.poll(() => page.evaluate(() => (window as any).statsRequests.length)).toBe(1);
  await expect(page.getByText("Calculating exact visible-wave statistics…")).toBeVisible();
  await expect(page.locator(".isa-table tbody tr")).toHaveCount(0);
  await page.getByLabel("Range end", {exact: true}).fill("500");
  await page.getByLabel("Range end", {exact: true}).press("Enter");
  await expect.poll(() => page.evaluate(() => (window as any).statsRequests.length)).toBe(2);
  await expect.poll(() => page.evaluate(() => {
    const w = window as any;
    return w.statsCancelled.includes(w.statsRequests[0].args.requestId) && !w.statsCancelled.includes(w.statsRequests[1].args.requestId);
  })).toBe(true);
  await page.evaluate(() => (window as any).statsRequests[1].resolve([{instruction: 2, hits: 3, latency: 30, stall: 4}]));
  const row = page.locator(".isa-table tbody tr").filter({has: page.locator("td:first-child", {hasText: /^2$/})});
  await expect(row.locator("td").nth(3)).toHaveText("3");
  await expect(row.locator("td").nth(4)).toHaveText("30");
  await page.evaluate(() => (window as any).statsRequests[0].resolve([{instruction: 2, hits: 999, latency: 999, stall: 999}]));
  await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
  await expect(row.locator("td").nth(3)).toHaveText("3");
  await expect(row.locator("td").nth(4)).toHaveText("30");
  await page.getByLabel("Latency scope").selectOption("all-mean");
  await expect.poll(() => page.evaluate(() => (window as any).statsRequests.length)).toBe(3);
  await expect(page.locator(".isa-table tbody tr")).toHaveCount(0);
  await page.evaluate(() => (window as any).statsRequests[2].reject(Error("Statistics test failure")));
  await expect(page.getByRole("alert")).toContainText("Statistics test failure");
  await expect(page.getByText("Calculating exact visible-wave statistics…")).toHaveCount(0);
  await expect(page.locator(".isa-table tbody tr")).toHaveCount(0);
});
