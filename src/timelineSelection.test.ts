import { expect, it } from "vitest";
import { timelineHits } from "./timelineSelection";
import type { TimelineGlyph } from "./timelineDetail";
function glyph(key: string, x = 10, y = 20): TimelineGlyph {
  return {
    mark: { key, start: 0, end: 10, lane: "lane", color: "green", label: key },
    rect: { x, y, width: 30, height: 10 },
  };
}
it("overlap picking follows paint order, deduplicates overlays and retains covered identities", () => {
  const first = glyph("first"),
    top = glyph("top"),
    event = glyph("event");
  expect(
    timelineHits([first, top, event], [event, event], 15, 25).map((m) => m.key),
  ).toEqual(["event", "top", "first"]);
  expect(timelineHits([first], [], 40, 25)).toEqual([]);
  expect(timelineHits([first], [], 15, 30)).toEqual([]);
  expect(
    timelineHits([first, glyph("another row", 10, 40)], [], 15, 25),
  ).toEqual([first.mark]);
});
it("aggregated marks remain groups rather than pretending to identify an underlying token", () => {
  const group = glyph("group");
  group.mark.aggregateCount = 12;
  const result = timelineHits([group], [], 15, 25);
  expect(result).toEqual([group.mark]);
  expect(result[0].aggregateCount).toBe(12);
});
