import { describe, expect, it, vi } from "vitest";
import { tokenLabeler } from "./tokenLabels";
import { categories, fmt, type Token } from "./types";

vi.mock("./types", async (original) => {
  const actual = await original<typeof import("./types")>();
  return { ...actual, fmt: vi.fn(actual.fmt) };
});
const token = (instruction: number, stall = 0, category = 6): Token => ({
  start: 0, duration: 1, instruction, stall, category,
});

describe("token labels", () => {
  it("preserves the original text, including signed zero and unknown categories", () => {
    const label = tokenLabeler("SE 2 · CU 4 · wave 3");
    for (const category of [0, 6, 99]) {
      for (const instruction of [-1, 0, 300, Number.MAX_SAFE_INTEGER]) {
        for (const stall of [0, -0, 0.15, 999999.9, 1000000, NaN, Infinity]) {
          const t = token(instruction, stall, category);
          expect(label(t)).toBe(`SE 2 · CU 4 · wave 3 · ${categories[category] || "OTHER"} · ISA ${instruction} · stall ${fmt(stall)}`);
        }
      }
    }
  });

  it("reuses repeated labels independently per wave and evicts old entries at the bound", () => {
    const label = tokenLabeler("wave A");
    vi.mocked(fmt).mockClear();
    for (let i = 0; i < 10000; i++) label(token(i % 3));
    expect(fmt).toHaveBeenCalledTimes(3);
    for (let i = 3; i < 4097; i++) label(token(i));
    vi.mocked(fmt).mockClear();
    label(token(4096));
    expect(fmt).not.toHaveBeenCalled();
    label(token(0));
    expect(fmt).toHaveBeenCalledTimes(1);
    expect(tokenLabeler("wave B")(token(0))).toMatch(/^wave B/);
  });
});
