import { expect, it } from "vitest";
import {
  cuWaveLane,
  utilizationLane,
  waveLocation,
  otherSimdMarks,
} from "./waveTopology";
import type { Wave, WaveIndex } from "./types";
it("hardware lanes separate known CUs and never merge unknown CU identities", () => {
  const index = { se: 2, simd: 1, slot: 3, wid: 7 } as WaveIndex;
  const a = { id: "a", cu: 0 } as Wave,
    b = { id: "b", cu: 4 } as Wave;
  expect(cuWaveLane(a, index)).toBe("CU 0 · S1 slot3");
  expect(cuWaveLane(a, index)).not.toBe(cuWaveLane(b, index));
  expect(utilizationLane(a, index, true, "IMMED")).not.toBe(
    utilizationLane(b, index, true, "IMMED"),
  );
  expect(cuWaveLane({ ...a, cu: null }, index)).not.toBe(
    cuWaveLane({ ...b, cu: null }, index),
  );
  expect(waveLocation(a, index)).toBe("SE 2 · CU 0 · SIMD 1 · slot 3 · wave 7");
  expect(waveLocation({ ...a, cu: null })).toContain("unknown [a]");
});

it("supplemental activity retains provenance and filters shader engines without inventing ISA links", () => {
  const capture = {
    otherSimd: [
      {
        se: 2,
        wgp: 7,
        source: "other.json",
        start: 30,
        duration: 12,
        category: 4,
      },
      {
        se: 3,
        wgp: 0,
        source: "elsewhere.json",
        start: 0,
        duration: 99,
        category: 1,
      },
      {
        se: 2,
        wgp: null,
        source: "unknown-a.json",
        start: 10,
        duration: 0,
        category: null,
      },
      {
        se: 2,
        wgp: null,
        source: "unknown-b.json",
        start: 10,
        duration: 1,
        category: null,
      },
    ],
  } as import("./types").Capture;
  const marks = otherSimdMarks(capture, 2, 1);
  expect(marks).toHaveLength(3);
  expect(marks[0]).toMatchObject({
    start: 30,
    end: 42,
    lane: "Other S3 · WGP 7 · cat 4",
    recordIndex: 0,
    recordKind: "activity",
  });
  expect(marks[0].label).toContain("recorded WGP scope");
  expect(marks[1].label).toContain("CU association unverified");
  expect(
    marks.every(
      (mark) => mark.waveId === undefined && mark.tokenIndex === undefined,
    ),
  ).toBe(true);
  expect(marks[1].lane).not.toBe(marks[2].lane);
  expect(marks[1].key).toBe("other-simd:2");
});
