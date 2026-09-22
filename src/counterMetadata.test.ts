import { describe, expect, it } from "vitest";
import { counterDetail } from "./counterMetadata";
import type { Series } from "./types";
const series: Series = {
  name: "SQ_WAVES:5",
  scope: "SE 0 · CU 2 · bank 0",
  unit: "raw count",
  points: [[0, 4]],
};
describe("ATT counter selection descriptions", () => {
  it("shows explicit sparse and empty masks without scaling values", () => {
    expect(
      counterDetail({
        ...series,
        att: {
          counter: "SQ_WAVES",
          simdMask: 5,
          simds: [0, 2],
          explicit: true,
        },
      }),
    ).toContain("SIMDs 0, 2 · mask 0x5 (recorded suffix)");
    expect(
      counterDetail({
        ...series,
        att: { counter: "SQ_WAVES", simdMask: 0, simds: [], explicit: true },
      }),
    ).toContain("no SIMDs selected");
  });
  it("distinguishes SDK defaults from absent metadata in older archives", () => {
    expect(
      counterDetail({
        ...series,
        att: {
          counter: "SQ_WAVES",
          simdMask: 15,
          simds: [0, 1, 2, 3],
          explicit: false,
        },
      }),
    ).toContain("SDK default; suffix absent");
    expect(counterDetail(series)).toBe("SE 0 · CU 2 · bank 0 · raw count");
  });
});
