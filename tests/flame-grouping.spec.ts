import { test, expect } from "@playwright/test";

// The flamegraph "Group by" control (flame.groupBy) had zero browser coverage.
// The shipped demo.json gives every instruction codeObject "1", collapsing to one
// group, so this injects a capture with two distinct code objects and two source
// files — the only way the source vs code-object groupings are distinguishable —
// and checks the grouping drives the on-screen detail and the SVG export caption.
test("flamegraph Group by switches between source and code-object grouping", async ({
  page,
}) => {
  await page.route("**/demo.json", async (route) => {
    const response = await route.fetch();
    const bundle = await response.json();
    bundle.capture.instructions = bundle.capture.instructions.map(
      (ins: Record<string, unknown>, i: number) => ({
        ...ins,
        codeObject: i % 2 === 0 ? "1" : "2",
        source: i % 2 === 0 ? "alpha.hip:10" : "beta.hip:20",
        latency: i + 1,
        pcSamples: i + 1,
      }),
    );
    await route.fulfill({ json: bundle });
  });

  const exportCaption = async () => {
    const { readFile } = await import("node:fs/promises");
    const pending = page.waitForEvent("download");
    await page
      .getByRole("button", { name: "Export Source flamegraph as SVG", exact: true })
      .click();
    const file = await (await pending).path();
    if (!file) throw Error("Missing export");
    return readFile(file, "utf8");
  };

  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "Trace workspace", exact: true }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Source flamegraph", exact: true })
    .click();

  // Default grouping is by source location.
  const grouping = page.getByLabel("Flamegraph grouping");
  await expect(grouping).toHaveValue("source");
  await expect(page.getByText("Grouped by source location and ISA")).toBeVisible();
  const sourceXml = await exportCaption();
  expect(sourceXml).toContain("source-to-ISA aggregation");
  expect(sourceXml).not.toContain("grouped by recorded code object");

  // Switch to code-object grouping: the detail text and the two code-object root
  // frames change, and the export caption records the grouping.
  await grouping.selectOption("codeObject");
  await expect(page.getByText("Grouped by the recorded code object")).toBeVisible();
  await expect(page.locator(".flames > div > button").first()).toBeVisible();
  const codeObjectXml = await exportCaption();
  expect(codeObjectXml).toContain("grouped by recorded code object");
  expect(codeObjectXml).not.toEqual(sourceXml);
});
