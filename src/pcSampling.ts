import type { Instruction } from "./types";
export const reasonNames = [
  "None",
  "Instruction fetch",
  "ALU dependency",
  "Waitcnt",
  "Internal",
  "Barrier",
  "Arbitration loss",
  "Pipe stall",
  "Other wait",
  "Sleeping",
];
const valid = (n: unknown): n is number =>
  typeof n === "number" && Number.isSafeInteger(n) && n >= 0;
export function sampleCount(i: Instruction): number | null {
  return valid(i.pcSamples) &&
    (i.pcSamples > 0 || valid(i.pcIssued) || valid(i.pcStalled))
    ? i.pcSamples
    : null;
}
export function samplingTotals(instructions: Instruction[]) {
  let samples = 0,
    sampleRows = 0,
    issued = 0,
    stalled = 0,
    issuedRows = 0,
    stalledRows = 0,
    reasonRows = 0;
  const reasons: number[] = [];
  for (const i of instructions) {
    const count = sampleCount(i);
    if (count !== null) {
      samples += count;
      sampleRows++;
    }
    if (valid(i.pcIssued)) {
      issued += i.pcIssued;
      issuedRows++;
    }
    if (valid(i.pcStalled)) {
      stalled += i.pcStalled;
      stalledRows++;
    }
    if (i.stallReasons?.every(valid)) {
      reasonRows++;
      i.stallReasons.forEach(
        (n, index) => (reasons[index] = (reasons[index] ?? 0) + n),
      );
    }
  }
  return {
    samples: Number.isSafeInteger(samples) && sampleRows ? samples : null,
    sampleRows,
    issued: Number.isSafeInteger(issued) && issuedRows ? issued : null,
    stalled: Number.isSafeInteger(stalled) && stalledRows ? stalled : null,
    issuedRows,
    stalledRows,
    reasonRows,
    rows: instructions.length,
    reasons: reasons.map((count, index) => ({
      index,
      name: reasonNames[index] ?? `Unknown category ${index}`,
      count: Number.isSafeInteger(count) ? count : null,
    })),
  };
}
