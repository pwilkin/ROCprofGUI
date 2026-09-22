import { test, expect } from "@playwright/test";
import { readFile } from "node:fs/promises";

test("viewport CU marks retain original token and loop occurrence navigation", async ({ page }) => {
  const bundle = JSON.parse(await readFile("public/demo.json", "utf8"));
  await page.addInitScript((bundle) => {
    const w = window as any; w.isTauri = true;
    const first = { ...bundle.capture.waves[0], start: 0, end: 1000 };
    const tokens = Array.from({ length: 8 }, (_, n) => ({ start: n < 7 ? n * 10 : 100, duration: n < 7 ? 1 : 700, stall: 0, category: 6, instruction: 2 }));
    const wave = { ...bundle.waves[first.id], start: 0, end: 1000, tokens, waits: null };
    w.__TAURI_INTERNALS__ = { invoke: async (command: string, args: any) => {
      if (command === "plugin:dialog|open") return "/viewport";
      if (command === "open_capture") return { ...bundle.capture, id: "viewport", start: 0, end: 1000, architecture: "gfx942", metadata: {}, waves: [first] };
      if (command === "read_wave") return wave;
      if (command === "begin_wave_query") return args.requestId;
      if (command === "cancel_wave_query") return null;
      if (command === "read_wave_window") return [{ ...wave, tokens: [tokens[7]], tokenIdentities: [{ index: 7, occurrence: 7, displaySlot: 3 }], viewport: [80, 1000] }];
      throw Error(command);
    }};
  }, bundle);
  await page.goto("/");
  await page.getByRole("button", { name: "Open capture file", exact: true }).click();
  await expect(page.locator(".capture-tab")).toHaveCount(2);
  await page.getByRole("button", { name: "CU waves", exact: true }).click();
  await expect(page.getByLabel("Timeline detail summary")).toContainText("1 events");
  await page.getByLabel("Token layout", { exact: true }).selectOption("flat");
  const canvas = page.locator("canvas");
  const box = await canvas.boundingBox();
  if (!box) throw Error("Missing CU canvas");
  await canvas.click({ position: { x: 155 + 0.4 * (box.width - 169), y: 40 }, modifiers: ["Alt"] });
  const chooser = page.getByRole("region", { name: "Overlapping timeline records" });
  await chooser.getByRole("button", { name: /token 7$/ }).click();
  await expect(page.locator(".iteration input")).toHaveValue("7");
  await expect(page.locator(".isa-table tr.selected")).toContainText(
    bundle.capture.instructions.find((i: { index: number }) => i.index === 2).text,
  );
});
