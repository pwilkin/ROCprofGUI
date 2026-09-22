import { describe, expect, it } from "vitest";
import {
  counterBindings,
  counterDomains,
  counterGroups,
} from "./counterTensors";
import { tensorExpression } from "./tensor";
import type { Series } from "./types";
const counter = (name: string, value = 1): Series => ({
  name,
  unit: "count",
  scope: "SE 0 · CU 0 · bank 0",
  points: [[10, value]],
});
describe("counter expression bindings", () => {
  it("makes masked ATT names usable without changing raw identities", () => {
    const input = [
      counter("SQ_WAVES:5", 4),
      counter("SQ_WAVES:a", 7),
      counter("NORMAL", 2),
    ];
    const before = JSON.stringify(input);
    const domain = counterDomains(input)[0];
    expect(domain.names).toEqual([
      "RAW_SQ_5f_WAVES_3a_5",
      "RAW_SQ_5f_WAVES_3a_a",
      "NORMAL",
    ]);
    const group = counterGroups(input)[0];
    expect(group.names).toEqual(domain.names);
    expect(
      tensorExpression(
        "sum[RAW_SQ_5f_WAVES_3a_5] + sum[RAW_SQ_5f_WAVES_3a_a]",
        (n) => group.raw[n],
      ).scalar(),
    ).toBe(11);
    expect(JSON.stringify(input)).toBe(before);
  });
  it("keeps aliases unique and stable across order and unrelated additions", () => {
    const input = [
      "a:b",
      "a_3a_b",
      "RAW_a_3a_b",
      "α rate",
      "",
      "__proto__",
      "A:1_ID_2",
    ].map((name) => counter(name));
    const a = counterBindings(input);
    expect(new Set(a.map((b) => b.symbol)).size).toBe(a.length);
    expect(a.every((b) => /^[A-Za-z_]\w*$/.test(b.symbol))).toBe(true);
    const b = counterBindings([counter("new counter"), ...input.toReversed()]);
    for (const binding of a)
      expect(b.find((x) => x.key === binding.key)?.symbol).toBe(binding.symbol);
    const group = counterGroups(input)[0];
    expect(group.raw.__proto__.scalar()).toBe(1);
  });
  it("merges coordinates only within one recorded counter identity", () => {
    const scope = (id: number, se: number) =>
      `process 0 · agent 42 · counter ${id} · [{"dimension_name":"DIMENSION_SHADER_ENGINE","index":${se}}]`;
    const input = [
      { ...counter("BUSY", 4), scope: scope(1, 0) },
      { ...counter("BUSY", 6), scope: scope(1, 1) },
      { ...counter("BUSY", 9), scope: scope(2, 0) },
    ];
    expect(counterDomains(input)[0].bindings).toHaveLength(2);
    const group = counterGroups(input)[0];
    expect(group.raw.RAW_BUSY_ID_1.reduce("sum", [1]).scalar()).toBe(10);
    expect(group.raw.RAW_BUSY_ID_2.scalar()).toBe(9);
  });
});
