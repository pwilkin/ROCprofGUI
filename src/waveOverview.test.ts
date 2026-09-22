import { expect, it } from "vitest";
import { overviewMarks, isWaveDetailBudgetError, type WaveOverview } from "./waveOverview";
it("creates counted zoom targets without inventing token identities", () => {
  const overview: WaveOverview = {range:[0,100],representedTokens:600001,waves:[{wave:{id:"w",se:2,simd:1,slot:3,wid:9,start:0,end:100},cu:4,groups:[{category:6,start:1,end:99,count:600001,zoomRange:[0,10]}]}]};
  const [mark] = overviewMarks(overview);
  expect(mark.aggregateCount).toBe(600001);
  expect(mark.zoomRange).toEqual([0,10]);
  expect(mark.lane).toBe("CU 4 · S1 slot3");
  expect(mark.tokenIndex).toBeUndefined();
  expect(mark.instruction).toBeUndefined();
});
it("preserves utilization pipe scope without claiming individual token identity", () => {
  const overview: WaveOverview = {range:[0,100],utilization:true,representedTokens:3,waves:[{wave:{id:"w",se:2,simd:1,slot:3,wid:9,start:0,end:100},cu:4,groups:[{category:13,pipe:"MATRIX",shared:false,start:1,end:99,count:2,zoomRange:[0,10]},{category:9,pipe:"IMMED",shared:true,start:2,end:98,count:1,zoomRange:[0,10]}]}]};
  const marks = overviewMarks(overview);
  expect(marks.map(mark => mark.lane)).toEqual(["CU 4 · S1 · MATRIX", "CU 4 · Shared · IMMED"]);
  expect(marks.map(mark => mark.aggregateCount)).toEqual([2, 1]);
  expect(marks.every(mark => mark.tokenIndex === undefined)).toBe(true);
});

it("uses bounded overview for exact wave and token budgets, not malformed data", () => {
  for (const message of ["This window contains more than 128 waves", "This window contains more than 128 detailed waves", "Wave query exceeds 128 waves", "Visible wave query exceeds 500,000 tokens"]) {
    expect(isWaveDetailBudgetError(message)).toBe(true);
  }
  for (const message of ["This window contains more than 4096 waves", "Invalid wave interval", "Malformed instruction token"]) {
    expect(isWaveDetailBudgetError(message)).toBe(false);
  }
});
