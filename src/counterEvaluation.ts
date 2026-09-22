import { counterGroups } from "./counterTensors";
import {
  Tensor,
  tensorDefinitions,
  withTensorBudget,
  type Shape,
} from "./tensor";
import type { Series } from "./types";
export const counterLimits = {
  points: 1_000_000,
  rawCells: 4_000_000,
  allocationCells: 8_000_000,
  resultCells: 2_000_000,
  definitions: 256 * 1024,
  timeoutMs: 30_000,
};
export interface EvaluationRequest {
  series: Series[];
  scope: string;
  definitions: string;
}
export interface TensorData {
  shape: Shape;
  values: number[];
  axisIndices: (number[] | null)[];
}
export interface EvaluationResult {
  results: [string, TensorData][];
  times: number[];
  scope: string;
}
export function validateEvaluation(request: EvaluationRequest) {
  if (
    new TextEncoder().encode(request.definitions).length >
    counterLimits.definitions
  )
    throw Error("Definition file exceeds 256 KiB");
  if (
    request.series.reduce((n, s) => n + s.points.length, 0) >
    counterLimits.points
  )
    throw Error("Counter domain exceeds one million input samples");
}
export function evaluateCounters(request: EvaluationRequest): EvaluationResult {
  validateEvaluation(request);
  const group = counterGroups(request.series, counterLimits.rawCells).find(
    (g) => g.scope === request.scope,
  );
  if (!group) throw Error("No dimensioned counters in selected domain");
  const results = withTensorBudget(counterLimits.allocationCells, () =>
    tensorDefinitions(request.definitions, group.raw),
  );
  if (!results.size) throw Error("Enter at least one definition");
  if (
    [...results.values()].reduce((n, t) => n + t.values.length, 0) >
    counterLimits.resultCells
  )
    throw Error(
      "Counter results exceed two million cells; reduce axes or evaluate fewer definitions",
    );
  return { results: [...results], times: group.times, scope: group.scope };
}
export function hydrateResults(result: EvaluationResult): Map<string, Tensor> {
  return new Map(
    result.results.map(([name, data]) => [
      name,
      Object.assign(Object.create(Tensor.prototype) as Tensor, data),
    ]),
  );
}
export function counterPlotPage(
  results: Map<string, Tensor>,
  page: number,
  size = 12,
) {
  let total = 0;
  for (const tensor of results.values())
    total += tensor.values.length / tensor.shape[3];
  const safePage = Math.max(0, Math.min(page, Math.ceil(total / size) - 1));
  const first = safePage * size,
    last = first + size;
  const plots: { name: string; tensor: Tensor; n: number; coords: number[] }[] =
    [];
  let offset = 0;
  for (const [name, tensor] of results) {
    const count = tensor.values.length / tensor.shape[3];
    for (
      let n = Math.max(0, first - offset);
      n < Math.min(count, last - offset);
      n++
    )
      plots.push({
        name,
        tensor,
        n,
        coords: tensor.coords(n * tensor.shape[3]).slice(0, 3),
      });
    offset += count;
    if (offset >= last) break;
  }
  return { plots, total, page: safePage };
}
