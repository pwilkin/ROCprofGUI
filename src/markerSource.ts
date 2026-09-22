import type {
  HiddenAnalysis,
  Instruction,
  MarkerScope,
  Range,
  Wave,
  WaveIndex,
} from "./types";
import { sourceLocations } from "./sourceLocations";
import type { SourceObservation } from "./sourceHierarchy";

export function markerInstruction(
  marker: MarkerScope,
  instructions: Instruction[],
) {
  const references = new Set(
    sourceLocations(marker.source).map((location) => location.reference),
  );
  return instructions.find(
    (instruction) =>
      instruction.codeObject === marker.codeObject &&
      sourceLocations(instruction.source).some((location) =>
        references.has(location.reference),
      ),
  );
}
export function* markerSourceObservations(
  index: WaveIndex,
  wave: Wave,
  markers: MarkerScope[],
  instructions: Map<number, Instruction>,
  range: Range,
  includeIdle: boolean,
  hidden?: HiddenAnalysis,
): Generator<SourceObservation> {
  if (wave.cu === null)
    throw Error(
      `Wave ${wave.id} has no recorded CU; marker association is unavailable`,
    );
  const spans = markers
    .filter(
      (m) =>
        !m.point &&
        m.se === index.se &&
        m.cu === wave.cu &&
        m.simd === index.simd &&
        m.slot === index.slot,
    )
    .sort((a, b) => a.start - b.start || a.depth - b.depth);
  let cursor = 0,
    previousEnd = wave.start;
  let active: MarkerScope[] = [];
  let previousStart = -Infinity;
  for (const [tokenIndex, token] of wave.tokens.entries()) {
    if (
      !Number.isFinite(token.start) ||
      !Number.isFinite(token.duration) ||
      token.duration < 0 ||
      token.start < previousStart
    )
      throw Error(
        `Invalid or unordered instruction occurrences in wave ${wave.id}`,
      );
    previousStart = token.start;
    const idle = Math.max(0, token.start - previousEnd);
    previousEnd = token.start + token.duration;
    if (token.start < range[0] || token.start >= range[1]) continue;
    active = active.filter((m) => m.end === null || m.end > token.start);
    while (cursor < spans.length && spans[cursor].start <= token.start) {
      const span = spans[cursor++];
      if (span.end === null || span.end > token.start) active.push(span);
    }
    if (active.length > 128) throw Error("Marker nesting exceeds 128 scopes");
    const ordered = [...active].sort(
      (a, b) => a.depth - b.depth || a.start - b.start,
    );
    const coherent = ordered.every((m, n) =>
      n === 0 ? m.parent === null : m.parent === ordered[n - 1].id,
    );
    const instruction = instructions.get(token.instruction);
    if (!instruction) continue;
    let weight = Math.max(1, token.duration + (includeIdle ? idle : 0));
    if (hidden) {
      const cost = hidden.byWave[wave.id]?.[tokenIndex];
      if (
        !cost ||
        cost.start !== token.start ||
        cost.instruction !== token.instruction ||
        cost.duration !== token.duration
      )
        throw Error(`Hidden-cost occurrences do not match wave ${wave.id}`);
      if (
        Object.values(cost.hidden).some(
          (value) => !Number.isFinite(value) || value < 0,
        )
      )
        throw Error(`Invalid hidden cost in wave ${wave.id}`);
      weight = Math.max(
        0,
        weight -
          cost.hidden.issue -
          cost.hidden.stall -
          (includeIdle ? cost.hidden.idle : 0),
      );
    }
    if (!Number.isFinite(weight) || weight < 0)
      throw Error("Invalid instruction cost in marker hierarchy");
    const ownershipUnverified = ordered.some(
      (marker) =>
        marker.start < wave.start ||
        marker.start >= wave.end ||
        (marker.end !== null && marker.end > wave.end),
    );
    const prefix = ownershipUnverified
      ? [{ key: "ownership", label: "[Unverified marker ownership]" }]
      : !coherent
        ? [{ key: "ambiguous", label: "[Ambiguous marker scopes]" }]
        : ordered.length
          ? ordered.map((m) => ({
              key: JSON.stringify([m.codeObject, m.markerId, m.kind]),
              label: m.name || `[Marker ${m.markerId}]`,
            }))
          : [{ key: "unscoped", label: "[No marker scope]" }];
    yield { instruction, weight, prefix };
  }
}
