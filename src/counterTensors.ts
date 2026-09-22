import type { Series } from "./types";
import { Tensor, type Shape } from "./tensor";
export interface CounterGroup {
  scope: string;
  raw: Record<string, Tensor>;
  times: number[];
  names: string[];
}
function identity(scope: string): { group: string; coords: number[] } | null {
  const att = scope.match(/^SE (\d+) · CU (\d+) · bank (\d+)$/);
  if (att)
    return { group: `ATT bank ${att[3]}`, coords: [0, +att[1], +att[2]] };
  const spm = scope.match(
    /^(process \d+ · agent \S+(?: · (?:dispatch|queue|stream|collection) \d+)*) · (?:counter \d+ · )?(\[.*\])$/,
  );
  if (!spm) return null;
  const dims: unknown = JSON.parse(spm[2]);
  if (!Array.isArray(dims)) throw Error("Invalid captured counter dimensions");
  const seen = new Set<string>();
  const coords = [0, 0, 0],
    rest: string[] = [];
  for (const d of dims) {
    if (
      !d ||
      typeof d.dimension_name !== "string" ||
      !d.dimension_name ||
      !Number.isSafeInteger(d.index) ||
      d.index < 0 ||
      seen.has(d.dimension_name)
    )
      throw Error("Invalid captured counter dimension");
    seen.add(d.dimension_name);
    const axis = [
      "DIMENSION_XCC",
      "DIMENSION_SHADER_ENGINE",
      "DIMENSION_INSTANCE",
    ].indexOf(d.dimension_name);
    if (axis < 0) rest.push(`${d.dimension_name}=${d.index}`);
    else coords[axis] = d.index;
  }
  return {
    group: `${spm[1]}${rest.length ? " · " + rest.sort().join(" · ") : ""}`,
    coords,
  };
}
export function counterClockScope(series: Series): string | null {
  if (!series.scope.startsWith("process ")) return null;
  const parsed = identity(series.scope);
  return parsed ? `${parsed.group} · clock XCC ${parsed.coords[0]}` : null;
}
export interface CounterBinding {
  key: string;
  name: string;
  counterId: string | null;
  symbol: string;
}
function counterId(series: Series): string | null {
  return (
    series.definition?.id ??
    series.scope.match(/ · counter (\d+) · /)?.[1] ??
    null
  );
}
function bindingKey(series: Series): string {
  return JSON.stringify([series.name, counterId(series)]);
}
function encodeSymbol(value: string): string {
  return [...value]
    .map((char) =>
      /^[A-Za-z0-9]$/.test(char)
        ? char
        : `_${char.codePointAt(0)!.toString(16)}_`,
    )
    .join("");
}
export function counterBindings(series: Series[]): CounterBinding[] {
  const sources = new Map<string, Series>();
  const identities = new Map<string, Set<string>>();
  for (const item of series) {
    const key = bindingKey(item);
    sources.set(key, item);
    const keys = identities.get(item.name) ?? new Set<string>();
    keys.add(key);
    identities.set(item.name, keys);
  }
  return [...sources].map(([key, item]) => {
    const id = counterId(item);
    const alias =
      !/^[A-Za-z_]\w*$/.test(item.name) ||
      item.name.startsWith("RAW_") ||
      item.name === "SCLOCK" ||
      item.name === "SPM_CLOCK" ||
      identities.get(item.name)!.size > 1;
    return {
      key,
      name: item.name,
      counterId: id,
      symbol: alias
        ? `RAW_${encodeSymbol(item.name)}${id === null ? "" : `_ID_${encodeSymbol(id)}`}`
        : item.name,
    };
  });
}
export interface CounterDomain {
  scope: string;
  series: Series[];
  names: string[];
  bindings: CounterBinding[];
}
export function counterDomains(series: Series[]): CounterDomain[] {
  const groups = new Map<string, CounterDomain>();
  for (const item of series) {
    const id = identity(item.scope);
    if (!id) continue;
    let group = groups.get(id.group);
    if (!group) {
      group = { scope: id.group, series: [], names: [], bindings: [] };
      groups.set(id.group, group);
    }
    group.series.push(item);
  }
  return [...groups.values()].map((group) => {
    const bindings = counterBindings(group.series);
    return {
      ...group,
      bindings,
      names: [
        ...bindings.map((binding) => binding.symbol),
        ...(group.scope.startsWith("process ") ? ["SCLOCK", "SPM_CLOCK"] : []),
      ],
    };
  });
}
export function counterGroups(
  series: Series[],
  maxCells = 8_000_000,
): CounterGroup[] {
  let allocated = 0;
  const groups = new Map<string, { series: Series; coords: number[] }[]>();
  for (const item of series) {
    const id = identity(item.scope);
    if (!id) continue;
    if (!groups.has(id.group)) groups.set(id.group, []);
    groups.get(id.group)!.push({ series: item, coords: id.coords });
  }
  return [...groups].map(([scope, items]) => {
    const bindings = new Map(
      counterBindings(items.map((item) => item.series)).map((binding) => [
        binding.key,
        binding.symbol,
      ]),
    );
    const symbols = items.map((item) => ({
      ...item,
      symbol: bindings.get(bindingKey(item.series))!,
    }));
    const timestamps = new Set<number>();
    for (const item of items)
      for (const [time] of item.series.points) timestamps.add(time);
    const times = [...timestamps].sort((a, b) => a - b);
    const timeIndex = new Map(times.map((t, n) => [t, n]));
    const raw: Record<string, Tensor> = Object.create(null),
      names: string[] = [];
    for (const { series, coords, symbol } of symbols) {
      if (!raw[symbol]) {
        const shape: Shape = [1, 1, 1, times.length];
        for (const item of symbols.filter((i) => i.symbol === symbol))
          item.coords.forEach((v, n) => (shape[n] = Math.max(shape[n], v + 1)));
        const count = shape.reduce((a, b) => a * b, 1);
        if (count < 1 || count > 2_000_000)
          throw Error(`Counter ${series.name} exceeds two million cells`);
        allocated += count;
        if (allocated > maxCells)
          throw Error("Counter domain allocation budget exceeded");
        raw[symbol] = new Tensor(shape, Array(count).fill(NaN));
        names.push(symbol);
      }
      const tensor = raw[symbol];
      for (const [t, value] of series.points) {
        const index = tensor.index([...coords, timeIndex.get(t)!]);
        if (Number.isFinite(tensor.values[index]))
          throw Error(`Duplicate counter coordinate in ${series.name}`);
        tensor.values[index] = value;
      }
    }
    if (scope.startsWith("process ")) {
      const modes = new Set(
        items.map(({ series }) =>
          series.spmClock ? `SE ${series.spmClock.anchorSe}` : "raw",
        ),
      );
      if (modes.size !== 1)
        throw Error(
          "SPM domain mixes calibrated and uncalibrated clocks or anchor engines",
        );
      const xccTimes = new Map<number, Map<number, bigint>>();
      for (const { series, coords } of items) {
        const observed = xccTimes.get(coords[0]) ?? new Map<number, bigint>();
        if (
          series.spmClock &&
          series.spmClock.realtimeTimestamps.length !== series.points.length
        )
          throw Error("SPM realtime metadata does not match the sample count");
        for (const [sample, [time]] of series.points.entries()) {
          if (!Number.isFinite(time))
            throw Error("Invalid SPM sample timestamp");
          const recorded = series.spmClock?.realtimeTimestamps[sample];
          if (
            recorded !== undefined &&
            (!/^\d+$/.test(recorded) || recorded.length > 20)
          )
            throw Error("Invalid recorded SPM realtime timestamp");
          if (
            recorded === undefined &&
            (!Number.isSafeInteger(time) || time < 0)
          )
            throw Error("Invalid raw SPM timestamp");
          const realtime =
            recorded === undefined ? BigInt(time) : BigInt(recorded);
          if (realtime > 18446744073709551615n)
            throw Error("SPM realtime timestamp exceeds u64");
          if (observed.has(time) && observed.get(time) !== realtime)
            throw Error(
              "Conflicting SPM realtime timestamps at one shader clock",
            );
          observed.set(time, realtime);
        }
        xccTimes.set(coords[0], observed);
      }
      let xccCount = 0;
      for (const xcc of xccTimes.keys()) xccCount = Math.max(xccCount, xcc + 1);
      const shape: Shape = [xccCount, 1, 1, times.length];
      const cells = xccCount * times.length;
      if (cells < 1 || cells > 2_000_000 || allocated + 2 * cells > maxCells)
        throw Error(
          "SPM clock tensors exceed counter domain allocation budget",
        );
      allocated += 2 * cells;
      const clock = new Tensor(shape, Array(cells).fill(NaN));
      const deltas = new Tensor(shape, Array(cells).fill(NaN));
      for (const [xcc, observed] of xccTimes) {
        const ordered = [...observed.keys()].sort((a, b) => a - b);
        const duration = (end: number, start: number) => {
          const delta = observed.get(end)! - observed.get(start)!;
          if (delta < 0n || delta > 9007199254740991n)
            throw Error(
              "SPM realtime interval is unordered or exceeds exact precision",
            );
          if (end !== start && delta === 0n)
            throw Error(
              "Distinct shader clocks share one SPM realtime timestamp",
            );
          return Number(delta);
        };
        const average =
          ordered.length > 1
            ? duration(ordered[ordered.length - 1], ordered[0]) /
              (ordered.length - 1)
            : 0;
        ordered.forEach((time, n) => {
          const index = clock.index([xcc, 0, 0, timeIndex.get(time)!]);
          clock.values[index] = time;
          deltas.values[index] =
            n === 0 ? average : duration(time, ordered[n - 1]);
        });
      }
      raw.SCLOCK = clock;
      raw.SPM_CLOCK = deltas;
      names.push("SCLOCK", "SPM_CLOCK");
    }
    return { scope, raw, times, names };
  });
}
