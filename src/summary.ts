import type { Instruction, Series } from "./types";

export type SummarySeries = Pick<Series, "name" | "scope" | "unit"> & {
  points: Iterable<[number, number]> & {readonly length: number};
};

export interface ActivityCost {
  idle: number;
  stall: number;
  issue: number;
  total: number;
}
function* activityCostSteps(
  instructions: readonly Instruction[],
): Generator<void, ActivityCost | null> {
  let idle = 0,
    stall = 0,
    issue = 0;
  for (let index = 0; index < instructions.length; index++) {
    if (index % 4096 === 0) yield;
    const i = instructions[index];
    if (
      ![i.idle, i.stall, i.latency].every(Number.isFinite) ||
      i.idle < 0 ||
      i.stall < 0 ||
      i.latency < i.stall
    )
      return null;
    idle += i.idle;
    stall += i.stall;
    issue += i.latency - i.stall;
  }
  const total = idle + stall + issue;
  return total > 0 && Number.isFinite(total)
    ? { idle, stall, issue, total }
    : null;
}


export function activityCost(instructions: readonly Instruction[]): ActivityCost | null {
  const steps = activityCostSteps(instructions);
  for (;;) {
    const step = steps.next();
    if (step.done) return step.value;
  }
}

export async function requestActivityCost(
  instructions: readonly Instruction[],
  signal: AbortSignal,
  yieldTask: () => Promise<void> = () =>
    new Promise((resolve) => setTimeout(resolve, 0)),
): Promise<ActivityCost | null> {
  const steps = activityCostSteps(instructions);
  for (;;) {
    if (signal.aborted)
      throw new DOMException("Instruction activity cancelled", "AbortError");
    const step = steps.next();
    if (step.done) return step.value;
    await yieldTask();
  }
}

interface SampledCounter {
  name: string;
  bank: number;
  byCu: Map<string, SummarySeries>;
  sums: Map<string, number>;
  sum: number;
  peak: number | null;
  error?: string;
}
export interface SummaryColumn {
  name: string;
  counter: string;
  scale: number;
  average: number | null;
  peak: number | null;
  reason?: string;
}
export interface SummaryRow {
  coordinate: string;
  utilization: (number | null)[];
  accumulated: (number | null)[];
}
export interface HardwareSummary {
  unavailable?: string;
  columns: SummaryColumn[];
  counters: string[];
  rows: SummaryRow[];
  peakCounts: (number | null)[];
  warnings: string[];
  delta: number;
}
const scaleFor = (name: string) =>
  /^ACTIVE_INST_(LDS|VMEM|FLAT)$/.test(name) ? 2 : 1;
const skew = (a: number, b: number) => {
  const lo = Math.min(a, b),
    hi = Math.max(a, b);
  return hi <= (8 * lo) / 7 ? hi : lo;
};
const coordinateOrder = (a: string, b: string) => {
  const aa = a.split(":"),
    bb = b.split(":");
  return +aa[0] - +bb[0] || +aa[1] - +bb[1];
};
const ratio = (
  value: number | undefined | null,
  busy: number | undefined | null,
) =>
  value != null && busy != null && busy > 0 && Number.isFinite(value / busy)
    ? (100 * value) / busy
    : null;

// RCV keeps each CU's latest sample, inserts gap zeros, and coalesces within four clocks.
function aggregatePeak(counter: SampledCounter, delta: number): number | null {
  if (counter.error) return null;
  type Event = {time: number; cu: string; value: number};
  function* events(cu: string, series: SummarySeries): Generator<Event> {
    let next: number | undefined;
    for (const [time, value] of series.points) {
      if (next !== undefined && time > next) yield {time: next, cu, value: 0};
      yield {time, cu, value};
      next = time + delta;
    }
  }
  const heap: {stream: Generator<Event>; event: Event}[] = [];
  const compare = (a: typeof heap[number], b: typeof heap[number]) =>
    a.event.time - b.event.time || coordinateOrder(a.event.cu, b.event.cu);
  for (const [cu, series] of counter.byCu) {
    const stream = events(cu, series), first = stream.next();
    if (!first.done) heap.push({stream, event:first.value});
  }
  heap.sort(compare);
  const last = new Map<string, number>();
  let sum = 0,
    time = 0,
    peak = 0;
  while (heap.length) {
    const event = heap[0].event;
    if (time + 4 < event.time) peak = Math.max(peak, sum);
    sum += event.value - (last.get(event.cu) ?? 0);
    last.set(event.cu, event.value);
    time = event.time;
    const next = heap[0].stream.next();
    if (!next.done) heap[0].event = next.value;
    else {
      const last = heap.pop()!;
      if (heap.length) heap[0] = last;
    }
    let root = 0;
    while (root * 2 + 1 < heap.length) {
      let child = root * 2 + 1;
      if (child + 1 < heap.length && compare(heap[child + 1], heap[child]) < 0) child++;
      if (compare(heap[root], heap[child]) <= 0) break;
      [heap[root], heap[child]] = [heap[child], heap[root]];
      root = child;
    }
  }
  return Math.max(peak, sum) * scaleFor(counter.name);
}

export function hardwareSummaryUnavailable(architecture: string, series: SummarySeries[]): string | undefined {
  if (!/^gfx(90a|94[012])(?::|$)/i.test(architecture)) {
    return `The upstream activity model applies to MI200 (gfx90a) and MI300 (gfx940/941/942). ${architecture || "Unknown architecture"} has no validated summary model.`;
  }
  const input = series.filter((s) =>
    /^SE \d+ · CU \d+ · bank \d+$/.test(s.scope),
  );
  if (!input.length) {
    return "ATT activity counters were not collected. SPM and dispatch durations are not substitutes for ATT activity measurements.";
  }
  if (input.reduce((n, s) => n + s.points.length, 0) > 2_000_000) {
    return "This summary exceeds the current 2,000,000-sample limit. Raw counters remain available in Counters.";
  }
  return undefined;
}

export function hardwareSummary(
  architecture: string,
  series: SummarySeries[],
): HardwareSummary {
  const result: HardwareSummary = {
    columns: [],
    counters: [],
    rows: [],
    peakCounts: [],
    warnings: [],
    delta: 4096,
  };
  const unavailable = hardwareSummaryUnavailable(architecture, series);
  if (unavailable) { result.unavailable = unavailable; return result; }
  const input = series.filter(s => /^SE \d+ · CU \d+ · bank \d+$/.test(s.scope));
  const counters = new Map<string, SampledCounter>();
  const bankDeltas = new Map<number, Map<number, number>>();
  for (const s of input) {
    const [, seText, cuText, bankText] = s.scope.match(
      /^SE (\d+) · CU (\d+) · bank (\d+)$/,
    )!;
    const se = +seText,
      bank = +bankText,
      cu = `${se}:${+cuText}`;
    const name = s.name.replace(/^SQ_/, "");
    let counter = counters.get(name);
    if (!counter) {
      counter = {
        name,
        bank,
        byCu: new Map(),
        sums: new Map(),
        sum: 0,
        peak: null,
      };
      counters.set(name, counter);
    }
    if (counter.bank !== bank || counter.byCu.has(cu))
      counter.error = "Ambiguous repeated counter or counter bank";
    if (+cuText >= 16)
      counter.error = "CU identity exceeds the upstream ATT counter model";
    if (s.unit !== "raw count") counter.error = "Counter unit is not raw count";
    counter.byCu.set(cu, s);
    let previous = -Infinity,
      sum = 0,
      delta = 4096;
    if (!s.points.length) counter.error = "Counter has no samples";
    for (const [time, value] of s.points) {
      if (
        !Number.isFinite(time) ||
        !Number.isFinite(value) ||
        value < 0 ||
        time < 0 ||
        time <= previous
      )
        counter.error = "Nonfinite, negative, duplicate or unordered samples";
      if (time - previous >= 32) delta = Math.min(delta, time - previous);
      previous = time;
      sum += value;
    }
    if (!Number.isFinite(sum)) counter.error = "Accumulated counter overflow";
    counter.sums.set(cu, sum * scaleFor(name));
    counter.sum += sum * scaleFor(name);
    const ses = bankDeltas.get(bank) ?? new Map<number, number>();
    ses.set(se, Math.min(ses.get(se) ?? 4096, delta));
    bankDeltas.set(bank, ses);
  }
  let delta = Infinity;
  for (const [, ses] of [...bankDeltas].sort((a, b) => a[0] - b[0])) {
    const candidates = [
      ...new Set([...ses.values()].filter((d) => d !== 4096)),
    ].sort((a, b) => a - b);
    const bankDelta =
      candidates.length > 1
        ? skew(candidates[0], candidates[1])
        : (candidates[0] ?? 4096);
    delta = skew(delta, bankDelta);
  }
  result.delta = delta;
  for (const counter of counters.values()) {
    counter.peak = aggregatePeak(counter, delta);
    if (counter.error)
      result.warnings.push(`${counter.name}: ${counter.error}.`);
  }
  const busy = counters.get("BUSY_CU_CYCLES");
  const busyValid = busy && !busy.error;
  if (!busyValid)
    result.warnings.push(
      "BUSY_CU_CYCLES is missing or invalid; utilization is unavailable.",
    );
  const all = [...counters.values()].sort((a, b) =>
    a.name.localeCompare(b.name),
  );
  result.counters = all.map((c) => c.name);
  result.peakCounts = all.map((c) => c.peak);
  for (const counter of all) {
    if (
      !counter.name.startsWith("ACTIVE_INST_") &&
      counter.name !== "VALU_MFMA_BUSY_CYCLES"
    )
      continue;
    const name =
      counter.name === "VALU_MFMA_BUSY_CYCLES"
        ? "MFMA"
        : counter.name.slice(12);
    const scale = name === "MFMA" ? 0.25 : 1;
    const matched =
      busyValid &&
      busy.byCu.size === counter.byCu.size &&
      [...counter.byCu.keys()].every((cu) => busy.byCu.has(cu));
    const reason =
      counter.error ||
      (!busyValid
        ? "Missing or invalid busy baseline"
        : !matched
          ? "Counter and baseline cover different CUs"
          : busy.sum === 0
            ? "Zero busy baseline"
            : undefined);
    result.columns.push({
      name,
      counter: counter.name,
      scale: scale * scaleFor(counter.name),
      average: reason ? null : ratio(counter.sum * scale, busy!.sum),
      peak: reason
        ? null
        : ratio(counter.peak == null ? null : counter.peak * scale, busy!.peak),
      reason,
    });
  }
  const coordinates = [...new Set(all.flatMap((c) => [...c.byCu.keys()]))].sort(
    coordinateOrder,
  );
  result.rows = coordinates.map((coordinate) => ({
    coordinate,
    utilization: result.columns.map((column) => {
      const counter = counters.get(column.counter)!;
      return counter.error || !busyValid
        ? null
        : ratio(
            counter.sums.has(coordinate)
              ? counter.sums.get(coordinate)! *
                  (column.name === "MFMA" ? 0.25 : 1)
              : null,
            busy!.sums.get(coordinate),
          );
    }),
    accumulated: all.map((c) =>
      c.error ? null : (c.sums.get(coordinate) ?? null),
    ),
  }));
  return result;
}
