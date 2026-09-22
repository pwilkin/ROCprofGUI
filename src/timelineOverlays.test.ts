import { expect, it } from "vitest";
import { timelineOverlays } from "./timelineOverlays";
import type { Mark } from "./components/Plots";
const mark = (
  lane: string,
  shaderEngine: number,
  recordKind: Mark["recordKind"] = "residency",
): Mark => ({
  lane,
  shaderEngine,
  recordKind,
  start: 0,
  end: 100,
  color: "red",
  label: lane,
  key: lane,
});
it("overlays cover full matching hardware rows, merge adjacent rows and skip unrelated scopes", () => {
  const marks = [
    mark("a", 0),
    mark("b", 0, "marker"),
    mark("c", 1),
    mark("d", 0),
    mark("queue", 0, "dispatch"),
    { ...mark("event", 0, "event"), start: 50, end: 50, overlayWidth: 5 },
  ];
  const result = timelineOverlays(
    marks,
    ["a", "b", "c", "d", "queue"],
    [0, 100],
    1000,
    28,
  );
  expect(result.map((g) => [g.rect.y, g.rect.height, g.rect.width])).toEqual([
    [27, 56, 5],
    [111, 28, 5],
  ]);
  expect(result[0].mark).toBe(marks[5]);
  expect(timelineOverlays(marks, ["c", "queue"], [0, 100], 1000, 28)).toEqual(
    [],
  );
  expect(timelineOverlays(marks, ["a"], [60, 100], 1000, 28)).toEqual([]);
  expect(timelineOverlays(marks, ["a"], [0, 100], 169, 28)).toEqual([]);
});
it("endpoint overlays stay inside the plot and lane height controls their full extent", () => {
  const marks = [
    mark("a", 0),
    ...[0, 100].map((start) => ({
      ...mark(`e${start}`, 0, "event"),
      start,
      end: start,
      overlayWidth: 4,
    })),
  ];
  const result = timelineOverlays(marks, ["a"], [0, 100], 1000, 56);
  expect(result.map((g) => [g.rect.x, g.rect.width, g.rect.height])).toEqual([
    [155, 4, 56],
    [982, 4, 56],
  ]);
  expect(marks[2].start).toBe(100);
});
