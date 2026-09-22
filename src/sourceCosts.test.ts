import { expect, it } from "vitest";
import { sourceLineCosts } from "./sourceCosts";
import type { Instruction } from "./types";
function row(
  source: string,
  latency: number,
  codeObject = "a",
  pcIssued?: number | null,
): Instruction {
  return {
    index: 1,
    text: "v_add",
    source,
    codeObject,
    address: "0",
    hits: 1,
    latency,
    stall: 0,
    idle: 0,
    pcSamples: 0,
    pcIssued,
  };
}
it("attributes inclusive inline costs once per line and isolates code objects", () => {
  const rows = [
    row("/a.hip:3 -> /a.hip:3 -> /a.hip:9", 12),
    row("/a.hip:3", 8),
    row("/a.hip:3", 100, "b"),
    row("/other.hip:3", 900),
  ];
  expect(sourceLineCosts(rows, "/a.hip", "a", "latency")).toEqual(
    new Map([
      [3, { value: 20, available: 2, missing: 0 }],
      [9, { value: 12, available: 1, missing: 0 }],
    ]),
  );
});
it("distinguishes unobserved, zero and partial sample sums", () => {
  const rows = [
    row("C:\\a.hip:2", 1, "a", 0),
    row("C:\\a.hip:3", 1),
    row("C:\\a.hip:4", 1, "a", 7),
    row("C:\\a.hip:4", 1, "a", null),
  ];
  const costs = sourceLineCosts(rows, "C:\\a.hip", "a", "pcIssued");
  expect(costs.get(2)).toEqual({ value: 0, available: 1, missing: 0 });
  expect(costs.get(3)).toEqual({ value: 0, available: 0, missing: 1 });
  expect(costs.get(4)).toEqual({ value: 7, available: 1, missing: 1 });
  expect(costs.has(5)).toBe(false);
});
