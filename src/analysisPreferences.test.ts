import { expect, it } from "vitest";
import { validAnalysisPreference as valid } from "./analysisPreferences";
it("validates persisted analysis controls without accepting unknown state", () => {
  for (const [key, value] of [
    ["sampling.search", "mfma"],
    ["plot.lodBias", -20],
    ["plot.lodBias", 20],
    ["tensor.scope", "agent 1"],
    ["source.location", "inline.h:4"],
    ["source.lineNumbers", false],
    ["source.includeHidden", false],
    ["counters.waveStates", false],
    ["source.costWidth", 240],
    ["source.costMetric", "pcIssued"],
    ["objects.search", "kernel.so"],
    ["objects.page", 2],
    ["host.page", 5],
    ["sampling.selected", null],
    ["summary.counts", true],
    ["hardware.cursor", 0.2],
    ["flame.pcIssued", "kernel.hip"],
    ["sampling.metric", "pcIssued"],
    ["sampling.metric", "pcSamples"],
    ["sampling.order", "samples"],
    ["flame.pcSamples", "kernel.hip"],
    ["dispatch.sort", "count"],
  ])
    expect(valid(key as string, value)).toBe(true);
  for (const [key, value] of [
    ["unknown", true],
    ["plot.lodBias", -21],
    ["plot.lodBias", 21],
    ["plot.lodBias", 0.5],
    ["plot.lodBias", "2"],
    ["source.lineNumbers", 1],
    ["counters.waveStates", "true"],
    ["source.costWidth", -1],
    ["source.costWidth", 401],
    ["source.costWidth", 1.5],
    ["source.costMetric", "unknown"],
    ["host.page", 1.5],
    ["host.page", -1],
    ["sampling.selected", Infinity],
    ["hardware.cursor", 1.1],
    ["summary.counts", "true"],
    ["sampling.order", "latency"],
    ["dispatch.sort", ["total"]],
    ["host.search", "é".repeat(4097)],
    ["host.search", "a\0b"],
  ])
    expect(valid(key as string, value)).toBe(false);
});
