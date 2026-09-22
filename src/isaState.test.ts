import { it, expect, vi } from "vitest";
import {
  initialIsaState,
  isaColumns,
  isaWidths,
  isaHiddenColumns,
  defaultHiddenColumns,
} from "./isaState";
it("isolates ISA defaults and rejects malformed machine column widths", () => {
  vi.stubGlobal("localStorage", { getItem: () => "[48,85,400,70,85,75,95]" });
  try {
    const a = initialIsaState(),
      b = initialIsaState();
    expect(a.widths[2]).toBe(400);
    a.widths[2] = 200;
    expect(b.widths[2]).toBe(400);
    vi.stubGlobal("localStorage", { getItem: () => '["bad"]' });
    expect(initialIsaState().widths).toEqual(
      isaColumns.map((column) => column.width),
    );
  } finally {
    vi.unstubAllGlobals();
  }
});
it("migrates legacy widths and validates stable visibility keys", () => {
  const legacy = [48, 85, 400, 70, 85, 75, 95];
  expect(isaWidths(legacy).slice(0, 7)).toEqual(legacy);
  expect(isaWidths(legacy)).toHaveLength(12);
  expect(isaHiddenColumns(undefined)).toEqual(defaultHiddenColumns);
  expect(isaHiddenColumns([])).toEqual([]);
  expect(isaHiddenColumns(["line", "hits"])).toEqual(["line", "hits"]);
  for (const invalid of [
    ["instruction"],
    ["hits", "hits"],
    ["unknown"],
    [null],
  ])
    expect(isaHiddenColumns(invalid)).toEqual(defaultHiddenColumns);
});
