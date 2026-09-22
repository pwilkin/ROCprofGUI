import { expect, it } from "vitest";
import { defaultWaveStateDisplay, isWaveStateSeries } from "./waveStateDisplay";

it("uses architecture defaults without treating them as capability rejection", () => {
  for (const architecture of ["navi", "gfx1010", "gfx1030", "gfx1100", "gfx1151", "gfx1200"])
    expect(defaultWaveStateDisplay({ architecture, metadata: {} })).toBe(false);
  for (const architecture of ["vega", "gfx900", "gfx942", "gfx950", "unknown"])
    expect(defaultWaveStateDisplay({ architecture, metadata: {} })).toBe(true);
  expect(defaultWaveStateDisplay({ architecture: "unknown", metadata: { gfxip: "11" } })).toBe(false);
});

it("recognizes only imported wave-state series, preserving similarly named counters", () => {
  const series = { name: "Wave state · WAIT", scope: "SE 3 · precomputed wave states", unit: "waves", points: [] };
  expect(isWaveStateSeries(series)).toBe(true);
  expect(isWaveStateSeries({ ...series, scope: "SE 3 · CU 0" })).toBe(false);
  expect(isWaveStateSeries({ ...series, unit: "raw count" })).toBe(false);
  expect(isWaveStateSeries({ ...series, name: "Wave state · CUSTOM" })).toBe(false);
});
