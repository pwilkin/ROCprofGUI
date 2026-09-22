import { expect, it } from "vitest";
import { waveOccurrence } from "./waveViewport";
import type { Wave } from "./types";
it("resolves original occurrences in filtered waves and retains full-wave behavior", () => {
  const tokens = [0, 1].map((n) => ({ start: n, duration: 1, stall: 0, category: 6, instruction: 7 }));
  const wave: Wave = { id: "w", start: 0, end: 10, cu: null, tokens, states: [], waits: null, info: {} };
  expect(waveOccurrence(wave, 7, 1)).toBe(tokens[1]);
  wave.tokenIdentities = [{ index: 10, occurrence: 4, displaySlot: 1 }, { index: 14, occurrence: 6, displaySlot: 3 }];
  expect(waveOccurrence(wave, 7, 6)).toBe(tokens[1]);
  expect(waveOccurrence(wave, 7, 1)).toBeUndefined();
});
