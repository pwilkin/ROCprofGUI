import { test, expect } from "@playwright/test";
import { readFileSync } from "node:fs";

// Produced by the Rust importer from fixtures/radv-rgp/*.rgp with --bundle.
const fixture = (name: string) =>
  JSON.parse(readFileSync(new URL(`../fixtures/radv-rgp/${name}.bundle.json`, import.meta.url), "utf8"));
const bundle = fixture("compute-spm");

test("RADV capture renders counters, waves and its attribution warning", async ({ page }) => {
  await page.route("**/demo.json", (route) => route.fulfill({ json: bundle }));
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto("/");

  await page.getByRole("button", { name: "Counters", exact: true }).click();
  await expect(page.getByText("14 raw series")).toBeVisible();

  await page.getByRole("button", { name: "Trace workspace", exact: true }).click();
  await expect(page.getByRole("button", { name: "CU waves", exact: true })).toBeVisible();

  await page.getByRole("button", { name: /^Capture evidence/ }).click();
  await expect(page.getByText(/must not be used for hotspot analysis/)).toBeVisible();
  await expect(page.getByText(/device-wide/)).toBeVisible();

  expect(errors).toEqual([]);
});

test("rocprof-compatible RADV capture shows exact per-instruction hits", async ({ page }) => {
  await page.route("**/demo.json", (route) => route.fulfill({ json: fixture("compute-rocprof-compat") }));
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto("/");

  // Two traced waves × 64 loop iterations.
  const row = page.locator(".isa-table tr", { hasText: "v_mul_lo_u32" });
  await expect(row.getByRole("cell", { name: "128", exact: true })).toBeVisible();

  await page.getByRole("button", { name: /^Capture evidence/ }).click();
  await expect(page.getByText(/device-wide/)).toBeVisible();
  await expect(page.getByText(/must not be used for hotspot analysis/)).toHaveCount(0);

  expect(errors).toEqual([]);
});
