import { expect, it } from "vitest";
import { mipTokens, mipMarks, mipLevel } from "./timelineMip";
import { mipPixel, timelineRect } from "./timelineLayout";
import type { Mark } from "./components/Plots";
const token = (start: number, category = 6) => ({
  start,
  duration: 2,
  category,
  slot: 0,
  count: 1,
});
it("small groups round independently, retain slots, and omit idle categories", () => {
  expect(
    mipTokens([{ ...token(3), slot: 2, source: 0 }, token(5, 0)], 4, 2),
  ).toEqual([{ ...token(4), duration: 4, slot: 2, source: 0 }]);
  expect(mipTokens([token(0, 16)], 1, 2)).toEqual([]);
  expect(mipTokens([token(1, 0)], 1, 1)).toEqual([token(1, 0)]);
});
it("coarse groups retain their original zoom extent and cannot masquerade as an ISA token", () => {
  const marks: Mark[] = Array.from({ length: 30 }, (_, n) => ({
    start: n,
    end: n + 2,
    lane: "VALU",
    color: "green",
    label: "instruction",
    key: `${n}`,
    waveId: "wave",
    tokenIndex: n,
    tokenCategory: 6,
    mipGroup: "wave",
  }));
  const result = mipMarks(marks, 1, 10);
  expect(result).toHaveLength(1);
  expect(result[0].aggregateCount).toBe(30);
  expect(result[0].zoomRange).toEqual([0, 31]);
  expect(result[0].waveId).toBeUndefined();
  expect(result[0].tokenIndex).toBeUndefined();
  expect(mipMarks(marks, 1, 1)).toBe(marks);
  expect(mipMarks([{ ...marks[0], mipGroup: undefined }], 1, 10)[0]).toEqual({
    ...marks[0],
    mipGroup: undefined,
  });
});
it("mip grouping keeps CUs and shader engine lanes independent", () => {
  const marks: Mark[] = ["CU0", "CU4"].flatMap((group) =>
    Array.from({ length: 30 }, (_, n) => ({
      start: n,
      end: n + 2,
      lane: group,
      key: `${group}:${n}`,
      color: "green",
      label: "x",
      mipGroup: group,
      tokenCategory: 6,
    })),
  );
  expect(mipMarks(marks, 1, 10).map((m) => m.lane)).toEqual(["CU0", "CU4"]);
});
it("architecture mip level and integer geometry remain bounded and hit geometry follows slot reduction", () => {
  expect(mipLevel([0, 1000], 1169, 1)).toBe(3);
  expect(mipLevel([0, 1000], 1169, 4)).toBe(1);
  expect(mipLevel([0, 1e12], 1169, 1)).toBe(10);
  expect(mipPixel(3, { level: 1, base: 4 })).toBe(2);
  expect(mipPixel(-3, { level: -1, base: 4 })).toBe(-8);
  const fine = timelineRect(0, 10, 0, 2, [0, 100], 1000, 28, {
    level: 0,
    base: 1,
  })!;
  const coarse = timelineRect(0, 10, 0, 2, [0, 100], 1000, 28, {
    level: 2,
    base: 1,
  })!;
  expect(coarse.height).toBeGreaterThan(fine.height);
  expect(coarse.y + coarse.height).toBeLessThan(55);
});
