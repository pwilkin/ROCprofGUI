import { test, expect } from "@playwright/test";
import { readFile } from "node:fs/promises";

test("CU loading remains explicit until the current native request settles", async ({ page }) => {
  const bundle = JSON.parse(await readFile("public/demo.json", "utf8"));
  await page.addInitScript((bundle) => {
    const w = window as any;
    w.isTauri = true;
    w.pendingWaves = [];
    w.cancelledWaves = [];
    w.__TAURI_INTERNALS__ = { invoke: async (command: string, args: any) => {
      if (command === "plugin:dialog|open") return "/waves";
      if (command === "open_capture") return { ...bundle.capture, id: "native-waves", architecture: "gfx942" };
      if (command === "read_wave") return bundle.waves[args.waveId];
      if (command === "begin_wave_query") return args.requestId;
      if (command === "cancel_wave_query") { w.cancelledWaves.push(args.requestId); return null; }
      if (command === "read_wave_window") return new Promise((resolve, reject) => {
        w.pendingWaves.push({ requestId: args.requestId, resolve: () => resolve(Object.values(bundle.waves)), reject });
      });
      throw Error(command);
    }};
  }, bundle);
  await page.goto("/");
  await page.getByRole("button", { name: "Open capture file", exact: true }).click();
  await expect(page.locator(".capture-tab")).toHaveCount(2);
  await page.getByRole("button", { name: "CU waves", exact: true }).click();
  const loading = page.getByText("Loading detailed waves for the selected range…", { exact: true });
  await expect(loading).toBeVisible();
  await expect.poll(() => page.evaluate(() => (window as any).pendingWaves.length)).toBe(1);
  await page.getByRole("button", { name: "Target wave", exact: true }).click();
  await expect(loading).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => (window as any).cancelledWaves.includes((window as any).pendingWaves[0].requestId))).toBe(true);
  await page.getByRole("button", { name: "CU waves", exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).pendingWaves.length)).toBe(2);
  await page.evaluate(() => (window as any).pendingWaves[0].resolve());
  await expect(loading).toBeVisible();
  await page.evaluate(() => (window as any).pendingWaves[1].reject("fixture wave read failure"));
  await expect(loading).toHaveCount(0);
  await expect(page.getByRole("alert").filter({ hasText: "fixture wave read failure" })).toBeVisible();
});
