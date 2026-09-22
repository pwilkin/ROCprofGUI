import { expect, it } from "vitest";
import { clockModel, utilizationToken } from "./utilization";
import type { Token } from "./types";
const gcn = { base: 4, family: "gcn" } as const;
const rdna = { base: 1, family: "rdna" } as const;
const token: Token = {
  start: 100,
  duration: 20,
  stall: 20,
  category: 6,
  instruction: 1,
};
it("recognizes supported families and rejects unknown clock models", () => {
  for (const architecture of ["gfx908", "gfx90a", "gfx942", "gfx950", "vega"])
    expect(clockModel({ architecture, metadata: {} })).toEqual(gcn);
  for (const architecture of [
    "gfx1010",
    "gfx1030",
    "gfx1100",
    "gfx1151",
    "gfx1200",
    "navi",
  ])
    expect(clockModel({ architecture, metadata: {} })).toEqual(rdna);
  expect(
    clockModel({ architecture: "unknown", metadata: { gfxip: "11" } }),
  ).toEqual(rdna);
  for (const architecture of ["unknown", "gfx1300", "gfx942invalid"])
    expect(clockModel({ architecture, metadata: {} })).toBeNull();
  expect(
    clockModel({ architecture: "unknown", metadata: { gfxip: "9.5" } }),
  ).toBeNull();
});
it("retains an issue quantum for full stalls without changing original clocks", () => {
  expect(utilizationToken(token, gcn, "")?.start).toBe(116);
  expect(utilizationToken(token, rdna, "")?.start).toBe(119);
  expect(
    utilizationToken({ ...token, duration: 2, stall: 2 }, gcn, ""),
  ).toMatchObject({ start: 100, end: 102 });
  expect(
    utilizationToken({ ...token, duration: 0, stall: 0 }, gcn, ""),
  ).toMatchObject({ start: 100, end: 100 });
  expect(
    utilizationToken({ ...token, duration: 2, stall: 20 }, gcn, ""),
  ).toMatchObject({ start: 116, end: 120 });
  expect(token).toEqual({
    start: 100,
    duration: 20,
    stall: 20,
    category: 6,
    instruction: 1,
  });
});
it("routes every upstream category and handles special shared issue pipes", () => {
  const short = [
    "SCAL",
    "SCAL",
    "SCAL",
    "VMEM",
    "VMEM",
    "VMEM",
    "VALU",
    "SCAL",
    "SCAL",
    "IMMED",
    "IMMED",
    "MSG",
    "VMEM",
    "VALU",
  ];
  const expanded = [
    "SCAL",
    "SCAL",
    "SCAL",
    "VMEM",
    "VMEM",
    "LDS",
    "VALU",
    "JUMP",
    "JUMP",
    "IMMED",
    "IMMED",
    "MSG",
    "RAY",
    "MATRIX",
  ];
  for (let category = 0; category < 14; category++) {
    const t = { ...token, category, stall: 0 };
    expect(utilizationToken(t, gcn, "")?.pipe).toBe(short[category]);
    expect(utilizationToken(t, rdna, "")?.pipe).toBe(expanded[category]);
    expect(utilizationToken(t, rdna, "")?.shared).toBe(
      [7, 8, 9, 10, 11].includes(category),
    );
    expect(utilizationToken(t, gcn, "")?.start).toBe(
      [9, 10, 11].includes(category) ? 116 : 100,
    );
  }
  expect(utilizationToken({ ...token, category: 5 }, gcn, "", true)?.pipe).toBe(
    "LDS",
  );
  expect(utilizationToken(token, rdna, "v_wmma_f32_16x16x16_f16")?.pipe).toBe(
    "MATRIX",
  );
  expect(utilizationToken(token, gcn, "v_mfma_f32_32x32x8f16")?.pipe).toBe(
    "VALU",
  );
  expect(utilizationToken({ ...token, category: 99 }, gcn, "")).toBeNull();
});
