import { expect, it } from "vitest";
import { sourceHierarchy, clipSourceHierarchy } from "./sourceHierarchy";
import type { Instruction } from "./types";
const row = (
  index: number,
  source: string,
  latency: number,
  pcIssued?: number,
): Instruction => ({
  index,
  source,
  latency,
  pcIssued,
  text: "v_add",
  address: "",
  codeObject: "0",
  hits: 1,
  stall: 0,
  idle: 0,
  pcSamples: 0,
});
it("reverses recorded inline chains and preserves shared prefix self weights", () => {
  const frames = sourceHierarchy(
    [
      row(0, "inner.h:3 -> outer.h:7", 8),
      row(1, "outer.h:7", 2),
      row(2, "inner.h:3 -> outer.h:7", 4),
      row(3, "", 6),
      row(4, "ignored.h:1", 0),
    ],
    "latency",
  );
  expect(
    frames.filter((f) => f.depth === 0).map((f) => [f.label, f.total]),
  ).toEqual([
    ["outer.h", 14],
    ["Unmapped ISA", 6],
  ]);
  const outer = frames.find((f) => f.label === "outer.h:7")!;
  const inner = frames.find((f) => f.label === "inner.h:3")!;
  expect([outer.total, inner.total, inner.depth]).toEqual([14, 12, 2]);
  const leaves = frames.filter((f) => f.instruction);
  expect(leaves.reduce((sum, f) => sum + f.total, 0)).toBe(20);
  expect(leaves.map((f) => f.instruction!.index)).toEqual([0, 2, 1, 3]);
  expect(leaves.map((f) => [f.start, f.end])).toEqual([
    [0, 8],
    [8, 12],
    [12, 14],
    [14, 20],
  ]);
  const clipped = clipSourceHierarchy(frames, [7, 15]);
  expect(clipped.every((f) => f.x >= 0 && f.x + f.width <= 1)).toBe(true);
  expect(
    clipped.filter((f) => f.instruction).reduce((s, f) => s + f.width, 0),
  ).toBeCloseTo(1);
});
it("keeps paths intact and only weights positive available samples", () => {
  const frames = sourceHierarchy(
    [
      row(0, "C:\\src\\kernel.h:12", 99, 3),
      row(1, "/x:9", 1),
      row(2, "/x:9", 1, 0),
    ],
    "pcIssued",
  );
  expect(frames[0].label).toBe("C:\\src\\kernel.h");
  expect(frames.filter((f) => f.instruction).map((f) => f.total)).toEqual([3]);
});
