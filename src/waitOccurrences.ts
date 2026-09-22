import type { Wave } from "./types";
interface Source {
  token: number;
  issueToWaitCycles: number;
  issueToWaitEndCycles: number;
}
function readAnalysis(wave: Wave | null | undefined) {
  const raw = wave?.info?.waitAnalysis as unknown as
    | {
        basis?: string;
        occurrences?: { waitToken: number; sources: Source[] }[];
        unsupportedWaits?: number;
        missingInstructionTokens?: number;
        unorderedTokens?: boolean;
        unmodeledCounters?: string[];
        conservativeImageOrdering?: boolean;
        conservativeExportOrdering?: boolean;
        exportCounterModeled?: boolean;
        unavailableReason?: string;
      }
    | undefined;
  if (
    !raw ||
    raw.basis !== "named ISA counters in recorded token order" ||
    !Array.isArray(raw.occurrences)
  )
    return null;
  if (
    raw.occurrences.some(
      (row) =>
        !row ||
        !Number.isInteger(row.waitToken) ||
        row.waitToken < 0 ||
        row.waitToken >= wave!.tokens.length ||
        !Array.isArray(row.sources) ||
        row.sources.some(
          (source) =>
            !source ||
            !Number.isInteger(source.token) ||
            source.token < 0 ||
            source.token >= row.waitToken ||
            !Number.isFinite(source.issueToWaitCycles) ||
            !Number.isFinite(source.issueToWaitEndCycles),
        ),
    )
  )
    return null;
  return {
    occurrences: raw.occurrences,
    conservativeImageOrdering: raw.conservativeImageOrdering === true,
    conservativeExportOrdering: raw.conservativeExportOrdering === true,
    exportCounterModeled: raw.exportCounterModeled === true,
    reason:
      typeof raw.unavailableReason === "string" ? raw.unavailableReason : "",
    unmodeled: Array.isArray(raw.unmodeledCounters)
      ? raw.unmodeledCounters
          .filter((name) => typeof name === "string")
          .join(", ")
      : "",
    unsupported:
      (raw.unsupportedWaits || 0) +
      (raw.missingInstructionTokens || 0) +
      (raw.unorderedTokens ? 1 : 0),
  };
}

export function buildWaitIndex(wave: Wave | null | undefined) {
  const positions = new Map<number, number[]>();
  const iterations = new Uint32Array(wave?.tokens.length || 0);
  wave?.tokens.forEach((token, index) => {
    let entries = positions.get(token.instruction);
    if (!entries) positions.set(token.instruction, (entries = []));
    iterations[index] = entries.length;
    entries.push(index);
  });
  const analysis = readAnalysis(wave);
  const occurrences = new Map<number, Source[]>();
  let valid = analysis !== null;
  for (const row of analysis?.occurrences || []) {
    if (occurrences.has(row.waitToken)) valid = false;
    occurrences.set(row.waitToken, row.sources);
  }
  return {
    count(instruction: number) {
      return positions.get(instruction)?.length || 0;
    },
    tokenAt(instruction: number, iteration: number) {
      const index = positions.get(instruction)?.[iteration];
      return index === undefined ? undefined : wave?.tokens[index];
    },
    location(index: number) {
      if (
        !wave ||
        !Number.isInteger(index) ||
        index < 0 ||
        index >= wave.tokens.length
      )
        return null;
      return {
        instruction: wave.tokens[index].instruction,
        iteration: iterations[index],
        waveId: wave.id,
      };
    },
    wait(instruction: number | null, iteration: number) {
      if (!valid || !analysis) return null;
      const index =
        instruction === null
          ? undefined
          : positions.get(instruction)?.[iteration];
      return {
        sources: index === undefined ? [] : occurrences.get(index) || [],
        reason: analysis.reason,
        unmodeled: analysis.unmodeled,
        conservativeExportOrdering: analysis.conservativeExportOrdering,
        exportCounterModeled: analysis.exportCounterModeled,
        conservativeImageOrdering: analysis.conservativeImageOrdering,
        unsupported: analysis.unsupported,
      };
    },
  };
}
export type WaitIndex = ReturnType<typeof buildWaitIndex>;
export function waitOccurrence(
  wave: Wave | null | undefined,
  instruction: number | null,
  iteration: number,
) {
  return buildWaitIndex(wave).wait(instruction, iteration);
}
export function tokenLocation(wave: Wave | null, index: number) {
  return buildWaitIndex(wave).location(index);
}
