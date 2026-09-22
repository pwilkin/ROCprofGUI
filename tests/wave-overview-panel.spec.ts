import { test, expect } from "@playwright/test";
import { readFile } from "node:fs/promises";
test("independent CU panel overviews zoom without changing the primary range", async ({ page }) => {
  const bundle = JSON.parse(await readFile("public/demo.json", "utf8"));
  await page.addInitScript((bundle) => {
    const w = window as any; w.isTauri = true;
    const index = { ...bundle.capture.waves[0], start: 0, end: 1000 };
    const wave = { ...bundle.waves[index.id], tokens: [{ start: 110, duration: 50, stall: 0, category: 6, instruction: 2 }] };
    w.__TAURI_INTERNALS__ = { invoke: async (command: string, args: any) => {
      if (command === "plugin:dialog|open") return "/overview";
      if (command === "open_capture") return { ...bundle.capture, id: "overview", start: 0, end: 1000, architecture: "gfx942", metadata: {}, waves: [index] };
      if (command === "read_wave") return wave;
      if (command === "read_wave_window") {
        if (args.end-args.start > 200) throw Error("Visible wave query exceeds 500,000 tokens");
        return [wave];
      }
      if (command === "begin_wave_query") return args.requestId;
      if (command === "cancel_wave_query") return null;
      if (command === "read_wave_overview") return { range: [args.start,args.end],representedTokens:600001,waves:[{wave:index,cu:0,groups:[{category:6,start:100,end:800,count:600001,zoomRange:[100,200]}]}] };
      throw Error(command);
    }};
  }, bundle);
  await page.goto("/");
  await page.getByRole("button", { name: "Open capture file", exact: true }).click();
  await expect(page.locator(".capture-tab")).toHaveCount(2);
  await page.getByLabel("Add docked panel").selectOption("trace-cu");
  const panel = page.getByRole("region", { name: "CU waves panel", exact: true });
  await panel.getByLabel("Link timeline ranges", { exact: true }).uncheck();
  await expect(panel.getByText(/CU overview: 600,001 recorded events/)).toBeVisible();
  await expect(panel.getByLabel("Timeline detail summary")).toContainText("600,001 events");
  await expect(panel.getByRole("option", { name: "Overview groups", exact: true })).toHaveCount(1);
  const canvas = panel.locator("canvas"); const box = await canvas.boundingBox();
  if (!box) throw Error("Missing CU plot");
  await canvas.click({ position: { x:155+0.4*(box.width-169), y:40 } });
  await expect(page.getByLabel("Range start",{exact:true})).toHaveValue("0");
  await expect(page.getByLabel("Range end",{exact:true})).toHaveValue("1000");
  await expect(panel.getByText(/CU overview:/)).toHaveCount(0);
  await expect(panel.getByLabel("Timeline detail summary")).toContainText("1 events");
});
