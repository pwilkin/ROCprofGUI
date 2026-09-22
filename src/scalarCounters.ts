import { evaluate } from "./analysis";
import type { Series } from "./types";
export interface ScalarDefinition {
  name: string;
  formula: string;
  bindings: { index: number; name: string; scope: string; unit: string }[];
}
export function scalarDefinition(
  name: string,
  formula: string,
  counters: Series[],
): ScalarDefinition {
  if (!name.trim() || name.length > 256 || formula.length > 4096)
    throw Error(
      "Enter a counter name (up to 256 characters) and expression (up to 4096 characters)",
    );
  const refs = [...new Set(formula.match(/\bC\d+\b/g) || [])];
  if (!refs.length)
    throw Error("Reference at least one raw series (C0, C1, …)");
  const bindings = refs.map((ref) => {
    const index = Number(ref.slice(1)),
      series = counters[index];
    if (!series) throw Error(`Unknown series ${ref}`);
    return { index, name: series.name, scope: series.scope, unit: series.unit };
  });
  return { name, formula, bindings };
}
export function scalarSeries(
  definition: ScalarDefinition,
  counters: Series[],
  budget = { remaining: 2_000_000 },
): Series {
  const expected = scalarDefinition(
    definition.name,
    definition.formula,
    counters,
  );
  if (
    expected.bindings.length !== definition.bindings.length ||
    expected.bindings.some((b, i) => {
      const saved = definition.bindings[i];
      return (
        b.index !== saved.index ||
        b.name !== saved.name ||
        b.scope !== saved.scope ||
        b.unit !== saved.unit
      );
    })
  )
    throw Error(
      "Referenced counter identities changed; remove and recreate this definition after checking the inputs",
    );
  const refs = [...new Set(definition.formula.match(/\bC\d+\b/g) || [])];
  const series = expected.bindings.map((b) => counters[b.index]);
  if (series.some((s) => s.scope !== series[0].scope))
    throw Error(
      "Choose counters with identical scopes. Cross-CU and cross-agent arithmetic requires an explicit reduction.",
    );
  if (series.reduce((n, s) => n + s.points.length, 0) > 500000)
    throw Error(
      "Scalar evaluation exceeds 500,000 input samples; use dimension-aware definitions for bounded worker evaluation",
    );
  const cost =
    series.reduce((n, s) => n + s.points.length, 0) * definition.formula.length;
  if (cost > budget.remaining)
    throw Error(
      "Scalar expression budget exceeded; use dimension-aware worker evaluation for larger definitions",
    );
  budget.remaining -= cost;
  const maps = series.map((s) => new Map(s.points));
  const points: [number, number][] = [];
  for (const [time] of series[0].points) {
    if (maps.some((m) => !m.has(time))) continue;
    const vars = Object.fromEntries(
      refs.map((ref, n) => [ref, maps[n].get(time)!]),
    );
    points.push([time, evaluate(definition.formula, vars)]);
  }
  if (!points.length)
    throw Error("No exact timestamp overlap; series were not aligned");
  return {
    name: definition.name,
    scope: `${series[0].scope} · ${definition.formula}`,
    unit: "user-defined",
    points,
  };
}
