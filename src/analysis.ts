import type { Capture, Instruction, Range, Token, Wave } from "./types";
export function union(ranges: Range[]): number {
  const sorted = ranges
    .filter(([a, b]) => Number.isFinite(a) && Number.isFinite(b) && b > a)
    .sort((a, b) => a[0] - b[0]);
  let total = 0,
    start = 0,
    end = 0;
  for (const [a, b] of sorted) {
    if (a > end) {
      total += end - start;
      start = a;
      end = b;
    } else end = Math.max(end, b);
  }
  return total + end - start;
}
export function occupancy(
  c: Capture,
  se?: number,
  kernel?: string,
): [number, number][] {
  const deltas = new Map<number, number>();
  for (const i of c.intervals) {
    if ((se !== undefined && i.se !== se) || (kernel && i.kernel !== kernel))
      continue;
    deltas.set(i.start, (deltas.get(i.start) || 0) + 1);
    deltas.set(i.end, (deltas.get(i.end) || 0) - 1);
  }
  let active = 0;
  return [...deltas]
    .sort((a, b) => a[0] - b[0])
    .map(([t, d]) => [t, (active += d)]);
}
/**
 * Per-kernel dispatch durations, plus what occupancy measured for the same kernel:
 * `waves` resident, `waveTime` (summed wave lifetimes) and `activeTime` (the union
 * of those lifetimes, the time the kernel had any wave resident). SQTT dispatch
 * records are points, so for thread traces only the occupancy figures are nonzero.
 */
export type KernelSample = { kernel: string; start: number; end: number };
export type KernelRow = { name: string; count: number; total: number; min: number; max: number; waves: number; waveTime: number; activeTime: number };
export function createKernelStatsAccumulator(guard?: {
  beforeKernel(name: string, nextGroups: number): void;
  beforeInterval(nextPairs: number): void;
}) {
  const m = new Map<string, KernelRow>();
  const lifetimes = new Map<string, Range[]>();
  let pairs = 0;
  return {
    dispatch(d: KernelSample) {
      let v = m.get(d.kernel);
      if (!v) {
        guard?.beforeKernel(d.kernel, m.size + 1);
        v = { name: d.kernel, count: 0, total: 0, min: Infinity, max: 0, waves: 0, waveTime: 0, activeTime: 0 };
      }
      const duration = d.end - d.start;
      v.count++;
      v.total += duration;
      v.min = Math.min(v.min, duration);
      v.max = Math.max(v.max, duration);
      m.set(d.kernel, v);
    },
    interval(i: KernelSample) {
      const v = m.get(i.kernel);
      if (!v) return;
      guard?.beforeInterval(pairs + 1);
      pairs++;
      v.waves++;
      v.waveTime += i.end - i.start;
      const ranges = lifetimes.get(i.kernel) ?? [];
      ranges.push([i.start, i.end]);
      lifetimes.set(i.kernel, ranges);
    },
    finish(): KernelRow[] {
      for (const [kernel, ranges] of lifetimes) m.get(kernel)!.activeTime = union(ranges);
      return [...m.values()].sort((a, b) => b.total - a.total || b.activeTime - a.activeTime);
    },
  };
}
export function kernelStats(c: Capture): KernelRow[] {
  const accumulator = createKernelStatsAccumulator();
  for (const d of c.dispatches) accumulator.dispatch(d);
  for (const i of c.intervals) accumulator.interval(i);
  return accumulator.finish();
}
export function instructionStats(
  code: Instruction[],
  wave: Wave | null,
  range: Range,
  mode: string,
  iteration = 0,
) {
  if (mode === "capture") return code;
  if (mode === "capture-mean")
    return code.map((i) => ({
      ...i,
      latency: i.hits ? i.latency / i.hits : 0,
    }));
  const stats = new Map<
    number,
    { hits: number; latency: number; stall: number }
  >();
  const occurrences = new Map<number, number>();
  for (const t of wave?.tokens || []) {
    const occurrence = occurrences.get(t.instruction) || 0;
    if (mode === "iteration") occurrences.set(t.instruction, occurrence + 1);
    if (
      mode === "iteration"
        ? occurrence !== iteration
        : mode !== "max" && (t.start < range[0] || t.start >= range[1])
    )
      continue;
    const v = stats.get(t.instruction) || { hits: 0, latency: 0, stall: 0 };
    v.hits++;
    if (mode === "max") {
      if (v.hits === 1 || t.duration > v.latency) {
        v.latency = t.duration;
        v.stall = t.stall;
      }
    } else {
      v.latency += t.duration;
      v.stall += t.stall;
    }
    stats.set(t.instruction, v);
  }
  return code.map((i) => {
    const s = stats.get(i.index) || { hits: 0, latency: 0, stall: 0 };
    return {
      ...i,
      ...s,
      latency: mode === "mean" && s.hits ? s.latency / s.hits : s.latency,
      idle: 0,
    };
  });
}
export function tokenIteration(tokens: Token[], index: number) {
  return tokens
    .slice(0, index)
    .filter((t) => t.instruction === tokens[index]?.instruction).length;
}
export function boundedRange(next: Range, bounds: Range): Range {
  const span = Math.min(bounds[1] - bounds[0], Math.max(1, next[1] - next[0]));
  const a = Math.max(bounds[0], Math.min(bounds[1] - span, next[0]));
  return [a, a + span];
}

export function evaluate(
  expression: string,
  vars: Record<string, number>,
): number {
  const parts =
    expression.match(
      /\s*(?:([A-Za-z_][A-Za-z_0-9]*)|((?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?)|(.))/g,
    ) || [];
  const tokens = parts.map((s) => s.trim());
  let pos = 0,
    depth = 0;
  const peek = () => tokens[pos];
  const take = () => tokens[pos++];
  function atom(): number {
    if (++depth > 64) throw Error("Expression is too deeply nested");
    let result: number;
    const token = take();
    if (token === "(") {
      result = expr();
      if (take() !== ")") throw Error("Missing closing parenthesis");
    } else if (token === "-") result = -atom();
    else if (token === "+") result = atom();
    else if (token && /^\d|^\./.test(token)) {
      result = Number(token);
    } else if (
      token &&
      ["min", "max", "abs"].includes(token) &&
      peek() === "("
    ) {
      take();
      const a = expr();
      if (token === "abs") {
        if (take() !== ")") throw Error("abs takes one argument");
        result = Math.abs(a);
      } else {
        if (take() !== ",") throw Error("Expected comma");
        const b = expr();
        if (take() !== ")") throw Error("Missing closing parenthesis");
        result = token === "min" ? Math.min(a, b) : Math.max(a, b);
      }
    } else if (token && Object.hasOwn(vars, token)) result = vars[token];
    else
      throw Error(
        `Unknown counter or function: ${token || "end of expression"}`,
      );
    depth--;
    return result;
  }
  function term(): number {
    let v = atom();
    while (peek() === "*" || peek() === "/") {
      const op = take(),
        r = atom();
      if (op === "/" && r === 0) throw Error("Division by zero");
      v = op === "*" ? v * r : v / r;
    }
    return v;
  }
  function expr(): number {
    let v = term();
    while (peek() === "+" || peek() === "-") {
      const op = take(),
        r = term();
      v = op === "+" ? v + r : v - r;
    }
    return v;
  }
  const value = expr();
  if (pos !== tokens.length || !Number.isFinite(value))
    throw Error("Invalid expression");
  return value;
}

export function hiddenLatencyStats(
  capture: Capture,
  analysis: import("./types").HiddenAnalysis,
  mode: string,
  waveId: string,
  se: number,
  range: Range,
  nonhidden: boolean,
  includeIdle: boolean,
  iteration = 0,
): Instruction[] {
  const cost = (
    latency: number,
    stall: number,
    idle: number,
    hidden: import("./types").HiddenCost,
  ) => {
    const active = Math.max(0, latency);
    const stalled = Math.max(0, Math.min(active, stall));
    const gap = includeIdle ? Math.max(0, idle) : 0;
    return (
      active +
      gap -
      (nonhidden
        ? Math.min(gap, Math.max(0, hidden.idle)) +
          Math.min(stalled, Math.max(0, hidden.stall)) +
          Math.min(active - stalled, Math.max(0, hidden.issue))
        : 0)
    );
  };
  const stats = new Map<
    number,
    {
      hits: number;
      latency: number;
      stall: number;
      idle: number;
      hidden: import("./types").HiddenCost;
    }
  >();
  const add = (
    index: number,
    hits: number,
    latency: number,
    stall: number,
    idle: number,
    hidden: import("./types").HiddenCost,
  ) => {
    const value = stats.get(index) || {
      hits: 0,
      latency: 0,
      stall: 0,
      idle: 0,
      hidden: { idle: 0, stall: 0, issue: 0 },
    };
    if (mode === "max") {
      if (
        !value.hits ||
        cost(latency, stall, idle, hidden) >
          cost(value.latency, value.stall, value.idle, value.hidden)
      ) {
        value.latency = latency;
        value.stall = stall;
        value.idle = idle;
        value.hidden = { ...hidden };
      }
      value.hits += hits;
      stats.set(index, value);
      return;
    }
    value.hits += hits;
    value.latency += latency;
    value.stall += stall;
    value.idle += idle;
    value.hidden.idle += hidden.idle;
    value.hidden.stall += hidden.stall;
    value.hidden.issue += hidden.issue;
    stats.set(index, value);
  };
  if (mode === "capture" || mode === "capture-mean")
    for (const i of capture.instructions)
      add(
        i.index,
        i.hits,
        i.latency,
        i.stall,
        i.idle,
        analysis.byInstruction[i.index] || { idle: 0, stall: 0, issue: 0 },
      );
  else
    for (const index of capture.waves) {
      if (mode.startsWith("all") ? index.se !== se : index.id !== waveId)
        continue;
      const occurrences = new Map<number, number>();
      for (const t of analysis.byWave[index.id] || []) {
        const occurrence = occurrences.get(t.instruction) || 0;
        if (mode === "iteration")
          occurrences.set(t.instruction, occurrence + 1);
        if (
          mode === "iteration"
            ? occurrence !== iteration
            : mode !== "max" && (t.start < range[0] || t.start >= range[1])
        )
          continue;
        add(t.instruction, 1, t.duration, t.stall, t.idle, t.hidden);
      }
    }
  return capture.instructions.map((i) => {
    const s = stats.get(i.index) || {
      hits: 0,
      latency: 0,
      stall: 0,
      idle: 0,
      hidden: { idle: 0, stall: 0, issue: 0 },
    };
    const active = Math.max(0, s.latency),
      stall = Math.max(0, Math.min(active, s.stall)),
      idle = includeIdle ? Math.max(0, s.idle) : 0;
    const hidden =
      Math.min(idle, Math.max(0, s.hidden.idle)) +
      Math.min(stall, Math.max(0, s.hidden.stall)) +
      Math.min(active - stall, Math.max(0, s.hidden.issue));
    const latency = active + idle - (nonhidden ? hidden : 0);
    return {
      ...i,
      hits: s.hits,
      latency:
        (mode === "mean" || mode === "all-mean" || mode === "capture-mean") &&
        s.hits
          ? latency / s.hits
          : latency,
      stall:
        stall - (nonhidden ? Math.min(stall, Math.max(0, s.hidden.stall)) : 0),
      idle,
    };
  });
}
