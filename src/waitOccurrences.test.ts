import { expect, it } from "vitest";
import {
  waitOccurrence,
  tokenLocation,
  buildWaitIndex,
} from "./waitOccurrences";
import type { Wave } from "./types";
it("keeps repeated wait occurrences distinct and rejects invalid token references", () => {
  const wave = {
    tokens: [
      { instruction: 1 },
      { instruction: 2 },
      { instruction: 1 },
      { instruction: 2 },
    ],
    info: {
      waitAnalysis: {
        basis: "named ISA counters in recorded token order",
        occurrences: [
          {
            waitToken: 1,
            sources: [
              { token: 0, issueToWaitCycles: 4, issueToWaitEndCycles: 6 },
            ],
          },
          {
            waitToken: 3,
            sources: [
              { token: 2, issueToWaitCycles: 8, issueToWaitEndCycles: 9 },
            ],
          },
        ],
      },
    },
  } as unknown as Wave;
  expect(waitOccurrence(wave, 2, 0)?.sources[0].token).toBe(0);
  expect(waitOccurrence(wave, 2, 1)?.sources[0].token).toBe(2);
  expect(waitOccurrence(wave, 1, 0)?.sources).toEqual([]);
  expect(waitOccurrence(wave, 2, 0)?.conservativeImageOrdering).toBe(false);
  (wave.info as any).waitAnalysis.conservativeImageOrdering = true;
  expect(waitOccurrence(wave, 2, 0)?.conservativeImageOrdering).toBe(true);
  (wave.info as any).waitAnalysis.occurrences[1].sources[0].token = 99;
  expect(waitOccurrence(wave, 2, 1)).toBeNull();
});

it("resolves exact token positions across interleaved repeated instructions", () => {
  const wave = {
    id: "w1",
    tokens: [
      { instruction: 7 },
      { instruction: 2 },
      { instruction: 7 },
      { instruction: 7 },
      { instruction: 2 },
    ],
  } as Wave;
  expect(tokenLocation(wave, 3)).toEqual({
    instruction: 7,
    iteration: 2,
    waveId: "w1",
  });
  expect(tokenLocation(wave, 4)).toEqual({
    instruction: 2,
    iteration: 1,
    waveId: "w1",
  });
  for (const index of [-1, 5, 0.5, NaN])
    expect(tokenLocation(wave, index)).toBeNull();
  expect(tokenLocation(null, 0)).toBeNull();
});

it("indexes large occurrence reports once and isolates wave instances sharing an ID", () => {
  const length = 100000;
  let reads = 0;
  const occurrences = new Proxy(
    Array.from({ length: length / 2 }, (_, index) => ({
      waitToken: index * 2 + 1,
      sources: [
        { token: index * 2, issueToWaitCycles: 1, issueToWaitEndCycles: 2 },
      ],
    })),
    {
      get(target, key, receiver) {
        if (typeof key === "string" && /^\d+$/.test(key)) reads++;
        return Reflect.get(target, key, receiver);
      },
    },
  );
  const wave = {
    id: "same",
    tokens: Array.from({ length }, (_, index) => ({ instruction: index % 2 })),
    info: {
      waitAnalysis: {
        basis: "named ISA counters in recorded token order",
        occurrences,
      },
    },
  } as unknown as Wave;
  const index = buildWaitIndex(wave);
  const initialReads = reads;
  expect(index.count(1)).toBe(50000);
  for (let iteration = 0; iteration < 50000; iteration += 97) {
    expect(index.wait(1, iteration)!.sources[0].token).toBe(iteration * 2);
    expect(index.location(iteration * 2 + 1)?.iteration).toBe(iteration);
    expect(index.tokenAt(1, iteration)?.instruction).toBe(1);
  }
  expect(reads).toBe(initialReads);
  const replacement = buildWaitIndex({
    ...wave,
    tokens: [wave.tokens[0]],
    info: {},
  });
  expect(replacement.count(1)).toBe(0);
  expect(replacement.wait(1, 0)).toBeNull();
  expect(index.count(1)).toBe(50000);
});
it("refuses ambiguous duplicate wait-token records", () => {
  const row = {
    waitToken: 1,
    sources: [{ token: 0, issueToWaitCycles: 1, issueToWaitEndCycles: 2 }],
  };
  const wave = {
    tokens: [{ instruction: 0 }, { instruction: 1 }],
    info: {
      waitAnalysis: {
        basis: "named ISA counters in recorded token order",
        occurrences: [row, row],
      },
    },
  } as unknown as Wave;
  expect(buildWaitIndex(wave).wait(1, 0)).toBeNull();
});
