import { test, expect } from "@playwright/test";

for (const architecture of ["gfx942", "gfx1151"]) {
  test(`wave-state display preference for ${architecture} preserves counter identities`, async ({ page }) => {
    await page.route("**/demo.json", async (route) => {
      const response = await route.fetch();
      const bundle = await response.json();
      bundle.capture.architecture = architecture;
      bundle.capture.counters = [
        { name: "Wave state · WAIT", unit: "waves", scope: "SE 0 · precomputed wave states", points: [[0, 2], [100, 3]] },
        { name: "Retained counter", unit: "count", scope: "SE 0", points: [[0, 4], [100, 6]] },
      ];
      await route.fulfill({ json: bundle });
    });
    await page.goto("/");
    await page.getByRole("button", { name: "Counters", exact: true }).click();
    const toggle = page.getByLabel("Show precomputed wave states");
    const wave = page.getByRole("heading", { name: "C0 · Wave state · WAIT", exact: true });
    const other = page.getByRole("heading", { name: "C1 · Retained counter", exact: true });
    if (architecture === "gfx942") {
      await expect(toggle).toBeChecked();
      await expect(wave).toBeVisible();
    } else {
      await expect(toggle).not.toBeChecked();
      await expect(wave).toHaveCount(0);
    }
    await toggle.check();
    await expect(wave).toBeVisible();
    await toggle.uncheck();
    await expect(wave).toHaveCount(0);
    await expect(other).toBeVisible();
    await page.getByLabel("Derived counter name").fill("Hidden state doubled");
    await page.getByLabel("Counter expression", { exact: true }).fill("C0 * 2");
    await page.getByRole("button", { name: "Add counter", exact: true }).click();
    await expect(page.getByRole("heading", { name: "Hidden state doubled", exact: true })).toBeVisible();
    await page.getByRole("button", { name: "Trace workspace", exact: true }).click();
    await page.getByRole("button", { name: "Counters", exact: true }).click();
    await expect(toggle).not.toBeChecked();
    await expect(other).toBeVisible();
  });
}
