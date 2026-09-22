import { expect, it, vi } from "vitest";
import {
  parseTimelinePreferences,
  readTimelinePreferenceSet,
  restoreTimelinePreferenceSet,
  timelinePreferenceKey,
} from "./timelinePreferences";
import { timelineRect, insideRect } from "./timelineLayout";
it("accepts known preferences and defaults malformed or unsupported values", () => {
  expect(parseTimelinePreferences('{"stacked":false,"laneHeight":56}')).toEqual(
    { stacked: false, laneHeight: 56, detailPixels: 0, zoomMode: "continuous" },
  );
  for (const value of [
    null,
    "broken",
    "[]",
    '{"stacked":"false","laneHeight":1000000}',
  ])
    expect(parseTimelinePreferences(value)).toEqual({
      detailPixels: 0,
      zoomMode: "continuous",
      stacked: true,
      laneHeight: 28,
    });
  expect(parseTimelinePreferences('{"detailPixels":4}').detailPixels).toBe(4);
  expect(parseTimelinePreferences('{"detailPixels":999}').detailPixels).toBe(0);
  expect(parseTimelinePreferences('{"zoomMode":"mip"}').zoomMode).toBe("mip");
  expect(parseTimelinePreferences('{"zoomMode":"invalid"}').zoomMode).toBe(
    "continuous",
  );
  expect(timelinePreferenceKey("wave")).not.toBe(
    timelinePreferenceKey("global"),
  );
});
it("scales lane positions and stacked bar hit boxes together", () => {
  const first = timelineRect(0, 10, 0, 0, [0, 100], 1000, 56)!;
  const next = timelineRect(0, 10, 1, 0, [0, 100], 1000, 56)!;
  const upper = timelineRect(0, 10, 0, 2, [0, 100], 1000, 56)!;
  expect(first.y).toBe(37);
  expect(first.height).toBe(34);
  expect(next.y - first.y).toBe(56);
  expect(insideRect(first, 160, 65)).toBe(true);
  expect(insideRect(upper, 160, 65)).toBe(false);
  expect(insideRect(upper, 160, 40)).toBe(true);
});

it("snapshots defaults independently and restores projects without writing machine preferences", () => {
  const setItem = vi.fn();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) =>
      key.endsWith(".wave") ? '{"laneHeight":56}' : null,
    setItem,
  });
  try {
    const first = readTimelinePreferenceSet();
    const second = readTimelinePreferenceSet();
    second.wave.laneHeight = 20;
    expect(first.wave.laneHeight).toBe(56);
    const restored = restoreTimelinePreferenceSet(second);
    expect(restored.wave.laneHeight).toBe(20);
    expect(restored.global.laneHeight).toBe(28);
    expect(readTimelinePreferenceSet().wave.laneHeight).toBe(56);
    expect(setItem).not.toHaveBeenCalled();
    expect(restoreTimelinePreferenceSet().wave.laneHeight).toBe(56);
  } finally {
    vi.unstubAllGlobals();
  }
});
