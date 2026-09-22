import { expect, it } from "vitest";
import {
  markerInstruction,
  markerSourceObservations as iterateObservations,
} from "./markerSource";
import { weightedSourceHierarchy } from "./sourceHierarchy";
import type { Instruction, MarkerScope, Wave, WaveIndex } from "./types";
const markerSourceObservations = (
  ...args: Parameters<typeof iterateObservations>
) => [...iterateObservations(...args)];
const instruction: Instruction = {
  index: 1,
  text: "v_add",
  source: "inner.h:2 -> outer.h:8",
  codeObject: "A",
  address: "0",
  hits: 0,
  latency: 0,
  stall: 0,
  idle: 0,
  pcSamples: 0,
};
const index: WaveIndex = {
  id: "wave",
  se: 1,
  simd: 2,
  slot: 3,
  wid: 4,
  start: 0,
  end: 100,
};
const wave: Wave = {
  id: "wave",
  start: 0,
  end: 100,
  cu: 5,
  tokens: [0, 10, 20, 30, 40].map((start) => ({
    start,
    duration: 4,
    stall: 1,
    category: 6,
    instruction: 1,
  })),
  states: [],
  waits: null,
  info: {},
};
const marker = (
  id: number,
  start: number,
  end: number | null,
  parent: number | null = null,
): MarkerScope => ({
  id,
  markerId: id,
  codeObject: "A",
  name: `scope${id}`,
  kind: "U",
  source: "outer.h:8",
  start,
  end,
  parent,
  depth: parent === null ? 0 : 1,
  point: false,
  se: 1,
  cu: 5,
  simd: 2,
  slot: 3,
});
const rows = new Map([[1, instruction]]);
it("assigns half-open scopes at issue time and conserves ISA weights through source frames", () => {
  const observations = markerSourceObservations(
    index,
    wave,
    [
      marker(1, 10, 40),
      marker(2, 10, 20, 1),
      { ...marker(3, 20, 20, 1), point: true },
    ],
    rows,
    [0, 100],
    false,
  );
  expect(observations.map((o) => o.prefix.map((p) => p.label))).toEqual([
    ["[No marker scope]"],
    ["scope1", "scope2"],
    ["scope1"],
    ["scope1"],
    ["[No marker scope]"],
  ]);
  const frames = weightedSourceHierarchy(observations);
  expect(
    frames.filter((f) => f.depth === 0).reduce((s, f) => s + f.total, 0),
  ).toBe(20);
  expect(
    frames.filter((f) => f.instruction).reduce((s, f) => s + f.total, 0),
  ).toBe(20);
  expect(frames.find((f) => f.label === "scope2")?.total).toBe(4);
});
it("isolates hardware, flags conflicting chains and preserves open scopes", () => {
  const others = ["se", "cu", "simd", "slot"].map((field, n) => ({
    ...marker(n + 10, 0, 100),
    [field]: 99,
  }));
  const scoped = markerSourceObservations(
    index,
    wave,
    [...others, marker(1, 10, null), marker(2, 20, 30)],
    rows,
    [0, 100],
    false,
  );
  expect(scoped[0].prefix[0].label).toBe("[No marker scope]");
  expect(scoped[2].prefix[0].label).toBe("[Ambiguous marker scopes]");
  expect(scoped[4].prefix[0].label).toBe("scope1");
  expect(() =>
    markerSourceObservations(
      index,
      { ...wave, cu: null },
      [],
      rows,
      [0, 100],
      false,
    ),
  ).toThrow("no recorded CU");
});
it("uses start-time selection, preceding idle, and exact occurrence hidden costs", () => {
  const selected = markerSourceObservations(
    index,
    wave,
    [],
    rows,
    [10, 30],
    true,
  );
  expect(selected.map((o) => o.weight)).toEqual([10, 10]);
  const hidden = {
    model: "test",
    byInstruction: {},
    byWave: {
      wave: wave.tokens.map((t) => ({
        ...t,
        idle: 0,
        hidden: { issue: 1, stall: 1, idle: 2 },
      })),
    },
  };
  expect(
    markerSourceObservations(index, wave, [], rows, [10, 30], true, hidden).map(
      (o) => o.weight,
    ),
  ).toEqual([6, 6]);
  hidden.byWave.wave[1].instruction = 99;
  expect(() =>
    markerSourceObservations(index, wave, [], rows, [10, 30], true, hidden),
  ).toThrow("do not match");
});
it("marker source navigation matches an inline location only in its code object", () => {
  const m = marker(1, 0, 10);
  expect(
    markerInstruction(m, [
      { ...instruction, index: 2, codeObject: "B" },
      instruction,
    ])?.index,
  ).toBe(1);
  expect(
    markerInstruction({ ...m, codeObject: "missing" }, [instruction]),
  ).toBeUndefined();
});
it("rejects unordered occurrences and stops hierarchy growth at its explicit budget", () => {
  expect(() =>
    markerSourceObservations(
      index,
      { ...wave, tokens: [wave.tokens[2], wave.tokens[0]] },
      [],
      rows,
      [0, 100],
      false,
    ),
  ).toThrow("unordered");
  const observations = markerSourceObservations(
    index,
    wave,
    [],
    rows,
    [0, 100],
    false,
  );
  expect(() => weightedSourceHierarchy(observations, 2)).toThrow(
    "hierarchy frames",
  );
});
it("does not attribute a reused slot to a prior wave's open scope", () => {
  const scopes = [marker(1, 10, null)];
  const earlier = markerSourceObservations(
    index,
    wave,
    scopes,
    rows,
    [0, 100],
    false,
  );
  expect(earlier[4].prefix[0].label).toBe("scope1");
  const later = {
    ...wave,
    start: 100,
    end: 200,
    tokens: wave.tokens.map((t) => ({ ...t, start: t.start + 100 })),
  };
  const observations = markerSourceObservations(
    { ...index, start: 100, end: 200 },
    later,
    scopes,
    rows,
    [100, 200],
    false,
  );
  expect(
    observations.every(
      (o) => o.prefix[0].label === "[Unverified marker ownership]",
    ),
  ).toBe(true);
  expect(
    weightedSourceHierarchy(observations)
      .filter((f) => f.depth === 0)
      .reduce((s, f) => s + f.total, 0),
  ).toBe(20);
  expect(
    markerSourceObservations(
      index,
      wave,
      [marker(1, 10, 110)],
      rows,
      [0, 100],
      false,
    )[1].prefix[0].label,
  ).toBe("[Unverified marker ownership]");
});
