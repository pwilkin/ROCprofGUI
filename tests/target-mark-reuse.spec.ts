import { test, expect } from "@playwright/test";
import { readFile } from "node:fs/promises";

test("target range changes reuse token labels while selection still refreshes", async ({ page }) => {
  const bundle = JSON.parse(await readFile("public/demo.json", "utf8"));
  await page.addInitScript((bundle) => {
    const w = window as any;
    w.isTauri = true;
    w.numberFormats = 0;
    const descriptor = Object.getOwnPropertyDescriptor(Intl.NumberFormat.prototype, "format")!;
    Object.defineProperty(Intl.NumberFormat.prototype, "format", {
      ...descriptor,
      get() {
        const format = descriptor.get!.call(this);
        return (value: number) => { w.numberFormats++; return format(value); };
      },
    });
    const first = { ...bundle.capture.waves[0], start: 0, end: 40000 };
    const tokens = Array.from({ length: 20000 }, (_, n) => ({ start: n * 2, duration: 1, stall: 0, category: 6, instruction: n + 2 }));
    const wave = { ...bundle.waves[first.id], start: 0, end: 40000, tokens, waits: null };
    w.__TAURI_INTERNALS__ = { invoke: async (command: string, args: any) => {
      if (command === "plugin:dialog|open") return "/mark-reuse";
      if (command === "open_capture") return { ...bundle.capture, id: "mark-reuse", start: 0, end: 40000, architecture: "gfx942", metadata: {}, waves: [first] };
      if (command === "read_wave") return wave;
      if (command === "begin_wave_query") return args.requestId;
      if (command === "cancel_wave_query") return null;
      throw Error(command);
    }};
  }, bundle);
  await page.goto("/");
  await page.getByRole("button", { name: "Open capture file", exact: true }).click();
  await expect(page.getByLabel("Timeline detail summary")).toContainText("20,000 events");
  await page.evaluate(() => { (window as any).numberFormats = 0; });
  await page.getByLabel("Range end", { exact: true }).fill("21");
  await page.getByLabel("Range end", { exact: true }).press("Enter");
  await expect(page.getByLabel("Timeline detail summary")).toContainText("11 events");
  expect(await page.evaluate(() => (window as any).numberFormats)).toBeLessThan(1000);
  await page.getByLabel("Token layout", { exact: true }).selectOption("flat");
  const canvas = page.locator("canvas");
  await canvas.click({ position: { x: 156, y: 40 } });
  await expect(page.locator(".isa-table tr.selected")).toContainText(bundle.capture.instructions.find((i: { index: number }) => i.index === 2).text);
});
