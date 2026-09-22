import { test, expect } from "@playwright/test";
import { readFile } from "node:fs/promises";

test("delayed native close preserves a newer tab selection and chooses a live fallback", async ({ page }) => {
  const bundle = JSON.parse(await readFile("public/demo.json", "utf8"));
  await page.addInitScript(bundle => {
    const w = window as any;
    w.isTauri = true;
    w.closes = {};
    let sequence = 0;
    w.__TAURI_INTERNALS__ = { invoke: async (command: string, args: any) => {
      if (command === "plugin:dialog|open") return "/capture";
      if (command === "open_capture") {
        const name = ["First", "Second", "Third"][sequence++];
        return { ...bundle.capture, id: name, name, metadata: { ...bundle.capture.metadata, demo: false } };
      }
      if (command === "begin_wave_query") return args.requestId;
      if (command === "cancel_wave_query") return null;
      if (command === "read_wave") return bundle.waves[args.waveId];
      if (command === "close_capture") return new Promise(resolve => { w.closes[args.captureId] = () => resolve(null); });
      throw Error(command);
    }};
  }, bundle);
  await page.goto("/");
  for (const name of ["First", "Second", "Third"]) {
    await page.getByRole("button", { name: "Open capture file", exact: true }).click();
    await expect(page.locator(".capture-tab.active")).toContainText(name);
  }
  const tabs = page.locator(".capture-tabs");
  await tabs.getByRole("button", { name: "First", exact: true }).click();
  await page.getByRole("button", { name: "Close First", exact: true }).click();
  await expect.poll(() => page.evaluate(() => !!(window as any).closes.First)).toBe(true);
  await tabs.getByRole("button", { name: "Second", exact: true }).click();
  await page.evaluate(() => (window as any).closes.First());
  await expect(page.locator(".capture-tab")).toHaveCount(3);
  await expect(page.locator(".capture-tab.active")).toContainText("Second");

  await page.getByRole("button", { name: "Close Second", exact: true }).click();
  await expect.poll(() => page.evaluate(() => !!(window as any).closes.Second)).toBe(true);
  await page.getByRole("button", { name: "Close LDS transpose · example capture", exact: true }).click();
  await expect(page.locator(".capture-tab")).toHaveCount(2);
  await page.evaluate(() => (window as any).closes.Second());
  await expect(page.locator(".capture-tab")).toHaveCount(1);
  await expect(page.locator(".capture-tab.active")).toContainText("Third");
});

test("failed native close reports the error, preserves the capture and allows retry", async ({ page }) => {
  const bundle = JSON.parse(await readFile("public/demo.json", "utf8"));
  const unhandled: string[] = [];
  page.on("pageerror", error => unhandled.push(error.message));
  await page.addInitScript(bundle => {
    const w = window as any;
    w.isTauri = true;
    w.closeAttempts = 0;
    w.__TAURI_INTERNALS__ = { invoke: async (command: string, args: any) => {
      if (command === "plugin:dialog|open") return "/capture";
      if (command === "open_capture") return { ...bundle.capture, id: "retry", name: "Retry close", metadata: { ...bundle.capture.metadata, demo: false } };
      if (command === "begin_wave_query") return args.requestId;
      if (command === "cancel_wave_query") return null;
      if (command === "read_wave") return bundle.waves[args.waveId];
      if (command === "close_capture") {
        if (++w.closeAttempts === 1) throw Error("Native cleanup failed");
        return null;
      }
      throw Error(command);
    }};
  }, bundle);
  await page.goto("/");
  await page.getByRole("button", { name: "Open capture file", exact: true }).click();
  await expect(page.locator(".capture-tab.active")).toContainText("Retry close");
  await page.getByRole("button", { name: "Close Retry close", exact: true }).click();
  await expect(page.getByRole("alert").filter({ hasText: "Could not close capture" })).toContainText("Native cleanup failed");
  await expect(page.locator(".capture-tab")).toHaveCount(2);
  await expect(page.locator(".capture-tab.active")).toContainText("Retry close");
  expect(unhandled).toEqual([]);
  await page.getByRole("button", { name: "Close Retry close", exact: true }).click();
  await expect(page.locator(".capture-tab")).toHaveCount(1);
  await expect(page.locator(".capture-tab.active")).toContainText("LDS transpose");
  await expect(page.getByText("Could not close capture", { exact: false })).toHaveCount(0);
  expect(await page.evaluate(() => (window as any).closeAttempts)).toBe(2);
  expect(unhandled).toEqual([]);
});

for (const removal of ["close", "same-ID project replacement"] as const) {
test(`timeline record selection releases captures on ${removal} but survives tab switches`, async ({ page }) => {
  const bundle = JSON.parse(await readFile("public/demo.json", "utf8"));
  bundle.capture.start = 0;
  bundle.capture.end = 1000;
  bundle.capture.dispatches = [{ ...bundle.capture.dispatches[0], id: "selected-dispatch", start: 100, end: 900 }];
  await page.route("**/demo.json", route => route.fulfill({ json: bundle }));
  await page.addInitScript(bundle => {
    const w = window as any;
    w.isTauri = true;
    w.__TAURI_INTERNALS__ = { invoke: async (command: string, args: any) => {
      if (command === "plugin:dialog|open") return "/other";
      if (command === "save_project") { w.saved = structuredClone(args); return true; }
      if (command === "open_tuning") { w.tuning = structuredClone(args); return "test"; }
      if (command === "open_project") return {
        captures: [{ ...bundle.capture, id: "demo", name: "Replacement capture" }],
        project: { schemaVersion: 1, active: 0, entries: [
          { input: null, state: w.saved.entries[0].state, sources: w.saved.entries[0].sources }
        ] }
      };
      if (command === "open_capture") return { ...bundle.capture, id: "other", name: "Other capture", metadata: { ...bundle.capture.metadata, demo: false } };
      if (command === "begin_wave_query") return args.requestId;
      if (command === "cancel_wave_query" || command === "close_capture") return null;
      if (command === "read_wave") return bundle.waves[args.waveId];
      throw Error(command);
    }};
  }, bundle);
  const selectedOwners = () => page.evaluate(() => {
    const root = document.getElementById("root") as any;
    const key = Object.keys(root).find(k => k.startsWith("__reactContainer"))!;
    const pending = [root[key].stateNode.current], seen = new Set(), owners: string[] = [];
    while (pending.length) {
      const fiber = pending.pop();
      if (!fiber || seen.has(fiber)) continue;
      seen.add(fiber);
      if (typeof fiber.type === "function") {
        let hook = fiber.memoizedState;
        while (hook) {
          const state = hook.memoizedState;
          if (state?.capture && state?.mark?.recordKind) owners.push(state.capture.id);
          hook = hook.next;
        }
      }
      pending.push(fiber.child, fiber.sibling);
    }
    return owners;
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Global", exact: true }).click();
  await page.getByLabel("Show global residency").uncheck();
  await page.getByLabel("Show global markers").uncheck();
  const canvas = page.locator("canvas");
  const box = await canvas.boundingBox();
  if (!box) throw Error("No timeline canvas");
  await canvas.click({ position: { x: 155 + .5 * (box.width - 169), y: 40 } });
  await expect(page.getByLabel("Timeline record details")).toContainText("selected-dispatch");
  await expect.poll(selectedOwners).toEqual(["demo"]);
  await page.getByRole("button", { name: "Tune this kernel", exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).tuning)).toEqual({captureId:"demo",dispatchId:"selected-dispatch"});
  await page.getByRole("button", { name: "Open capture file", exact: true }).click();
  await expect(page.locator(".capture-tab.active")).toContainText("Other capture");
  await expect.poll(selectedOwners).toEqual(["demo"]);
  await page.locator(".capture-tab").first().getByRole("button").first().click();
  await expect(page.getByLabel("Timeline record details")).toContainText("selected-dispatch");
  if (removal === "close") {
    await page.getByRole("button", { name: `Close ${bundle.capture.name}`, exact: true }).click();
  } else {
    await page.getByRole("button", { name: "Save project", exact: true }).click();
    await expect.poll(() => page.evaluate(() => !!(window as any).saved)).toBe(true);
    await page.getByRole("button", { name: "Open project", exact: true }).click();
    await expect(page.locator(".capture-tab.active")).toContainText("Replacement capture");
  }
  await expect(page.locator(".capture-tab")).toHaveCount(1);
  await expect.poll(selectedOwners).toEqual([]);
});
}
