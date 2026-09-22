import { sourceLocations } from "./sourceLocations";
import type { Instruction } from "./types";

export type SourceCostMetric =
  "latency" | "pcSamples" | "pcIssued" | "pcStalled";
export interface SourceLineCost {
  value: number;
  available: number;
  missing: number;
}
export function sourceLineCosts(
  instructions: Instruction[],
  path: string,
  codeObject: string,
  metric: SourceCostMetric,
): Map<number, SourceLineCost> {
  const result = new Map<number, SourceLineCost>();
  for (const instruction of instructions) {
    if (instruction.codeObject !== codeObject) continue;
    const lines = new Set(
      sourceLocations(instruction.source)
        .filter((location) => location.path === path)
        .map((location) => location.line),
    );
    for (const line of lines) {
      const value = result.get(line) || { value: 0, available: 0, missing: 0 };
      const cost = instruction[metric];
      if (cost == null || !Number.isFinite(cost) || cost < 0) value.missing++;
      else {
        value.value += cost;
        value.available++;
      }
      result.set(line, value);
    }
  }
  return result;
}
