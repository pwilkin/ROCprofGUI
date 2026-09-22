import { expect, it } from "vitest";
import { topHotspots } from "./hotspots";

it("matches stable full sorting across large catalogs, ties and zero weights", () => {
  let seed = 42;
  const rows = Array.from({ length: 1_000_000 }, (_, index) => {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    return { index, latency: seed % 10000 };
  });
  const expected = rows.filter((row) => row.latency > 0)
    .sort((a, b) => b.latency - a.latency).slice(0, 20);
  expect(topHotspots(rows)).toEqual(expected);
  expect(rows[0].index).toBe(0);
  expect(rows.at(-1)?.index).toBe(999999);
});

it("retains input order for ties and handles empty and short inputs", () => {
  expect(topHotspots([])).toEqual([]);
  const rows = [{ latency: 0 }, { latency: 2 }, { latency: -1 }, { latency: 2 }];
  expect(topHotspots(rows)).toEqual([rows[1], rows[3]]);
  const ties = Array.from({ length: 25 }, (_, index) => ({ index, latency: 4 }));
  expect(topHotspots(ties)).toEqual(ties.slice(0, 20));
});
