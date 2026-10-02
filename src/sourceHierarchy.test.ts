import { expect, it } from "vitest";
import {
  sourceHierarchy,
  clipSourceHierarchy,
  codeObjectLabel,
} from "./sourceHierarchy";
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
it("defaults to source grouping with no code-object root frame", () => {
  const frames = sourceHierarchy(
    [{ ...row(0, "x.cl:1", 2), codeObject: "pipeline A" }],
    "latency",
  );
  expect(frames[0].label).toBe("x.cl");
});
it("groups by code object with the source hierarchy nested beneath", () => {
  const frames = sourceHierarchy(
    [
      { ...row(0, "a.cl:1", 5), codeObject: "pipeline A" },
      { ...row(1, "a.cl:2", 3), codeObject: "pipeline A" },
      { ...row(2, "b.cl:1", 4), codeObject: "code object 2" },
    ],
    "latency",
    "codeObject",
  );
  const roots = frames.filter((f) => f.depth === 0);
  expect(roots.map((f) => [f.label, f.total])).toEqual([
    ["pipeline A", 8],
    ["code object 2", 4],
  ]);
  // The source file of the first code object nests beneath it, carrying only
  // that code object's weight.
  const file = frames.find((f) => f.label === "a.cl" && f.depth === 1);
  expect(file?.total).toBe(8);
  // Every ISA leaf keeps its own weight; nothing is double counted.
  const leaves = frames.filter((f) => f.instruction);
  expect(leaves.reduce((sum, f) => sum + f.total, 0)).toBe(12);
});
it("keeps same-source instructions in distinct code objects apart", () => {
  const frames = sourceHierarchy(
    [
      { ...row(0, "shared.cl:7", 6), codeObject: "pipeline A" },
      { ...row(1, "shared.cl:7", 2), codeObject: "pipeline B" },
    ],
    "latency",
    "codeObject",
  );
  expect(frames.filter((f) => f.depth === 0).map((f) => f.label)).toEqual([
    "pipeline A",
    "pipeline B",
  ]);
  expect(frames.filter((f) => f.instruction).map((f) => f.total)).toEqual([
    6, 2,
  ]);
});
it("labels an empty code object explicitly instead of guessing", () => {
  expect(codeObjectLabel("")).toBe("Unknown code object");
  expect(codeObjectLabel("pipeline deadbeef")).toBe("pipeline deadbeef");
  const frames = sourceHierarchy(
    [{ ...row(0, "x:1", 2), codeObject: "" }],
    "latency",
    "codeObject",
  );
  expect(frames[0].label).toBe("Unknown code object");
  expect(frames[0].depth).toBe(0);
});
