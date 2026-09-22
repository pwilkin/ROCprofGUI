import { expect, it } from "vitest";
import { timelineGlyphs } from "./timelineDetail";
import type { Mark } from "./components/Plots";
const mark = (key: string, start: number, patch: Partial<Mark> = {}): Mark => ({
  key,
  start,
  end: start + 0.1,
  lane: "a",
  color: "green",
  label: key,
  ...patch,
});
it("detail buckets preserve counts and separate lanes, slots, colors and selected events", () => {
  const marks = [
    mark("1", 10),
    mark("2", 11),
    mark("slot", 10, { displaySlot: 1 }),
    mark("color", 10, { color: "blue" }),
    mark("lane", 10, { lane: "b" }),
    mark("selected", 10, { selected: true }),
    mark("long", 10, { end: 30 }),
    mark("outside", -10),
  ];
  const glyphs = timelineGlyphs(
    marks,
    ["a", "b"],
    [0, 1000],
    1169,
    28,
    true,
    4,
  );
  expect(glyphs).toHaveLength(6);
  expect(glyphs[0].mark.aggregateCount).toBe(2);
  expect(glyphs[0].mark.start).toBe(10);
  expect(glyphs[0].mark.end).toBe(11.1);
  expect(glyphs.at(-1)?.mark).toBe(marks[5]);
  expect(glyphs.reduce((n, g) => n + (g.mark.aggregateCount || 1), 0)).toBe(7);
  expect(marks[0].aggregateCount).toBeUndefined();
});
it("exact mode keeps paint order and zoom restores separately resolved events", () => {
  const marks = [
    mark("selected", 10, { selected: true }),
    mark("1", 10),
    mark("2", 11),
  ];
  expect(
    timelineGlyphs(marks, ["a"], [0, 1000], 1169, 28, true, 0).map(
      (g) => g.mark,
    ),
  ).toEqual(marks);
  expect(
    timelineGlyphs(marks.slice(1), ["a"], [9, 12], 1169, 28, true, 4),
  ).toHaveLength(2);
  expect(timelineGlyphs(marks, [], [0, 1000], 1169, 28, true, 8)).toEqual([]);
  const clipped = [mark("long", -100, { end: 0.1 }), mark("short", 0)];
  expect(
    timelineGlyphs(clipped, ["a"], [0, 1000], 1169, 28, true, 8),
  ).toHaveLength(2);
});
it("dense overview bounds paint work without discarding underlying records", () => {
  const marks = Array.from({ length: 10000 }, (_, n) =>
    mark(String(n), 100 + n / 10000, {
      end: 100 + n / 10000 + 0.00001,
      selected: n === 5000,
    }),
  );
  const glyphs = timelineGlyphs(marks, ["a"], [0, 1000], 1169, 28, true, 8);
  expect(glyphs).toHaveLength(2);
  expect(glyphs[0].mark.aggregateCount).toBe(9999);
  expect(glyphs[1].mark.key).toBe("5000");
  expect(marks).toHaveLength(10000);
});
it("finalized groups preserve source marks, group counts, and selected paint order", () => {
  const marks = [
    Object.freeze(mark("first", 11, { aggregateCount: 3 })),
    Object.freeze(mark("earlier", 10, { end: 12 })),
    Object.freeze(mark("selected-a", 10, { selected: true })),
    Object.freeze(mark("selected-b", 11, { selected: true })),
  ];
  const glyphs = timelineGlyphs(marks, ["a"], [0, 1000], 1169, 42, true, 8);
  expect(glyphs.map((g) => g.mark.key)).toEqual(["first", "selected-a", "selected-b"]);
  expect(glyphs[0].mark).toMatchObject({ start: 10, end: 12, aggregateCount: 4, label: "4 short events · aggregated span; click to zoom" });
  expect(marks[0]).toMatchObject({ start: 11, end: 11.1, aggregateCount: 3, label: "first" });
  expect(glyphs[1].mark).toBe(marks[2]);
  expect(glyphs[2].mark).toBe(marks[3]);
});
