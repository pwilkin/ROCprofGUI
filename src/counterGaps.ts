import type { Series } from "./types";
import { counterClockScope } from "./counterTensors";
export function counterGaps(
  series: Series[],
): Map<Series, { points: [number, number][]; missing: number }> {
  const domains = new Map<string, Series[]>();
  for (const item of series) {
    const clock = counterClockScope(item);
    if (clock) {
      const items = domains.get(clock) ?? [];
      items.push(item);
      domains.set(clock, items);
    }
  }
  const result = new Map<
    Series,
    { points: [number, number][]; missing: number }
  >();
  for (const items of domains.values()) {
    const observed = new Set<number>();
    for (const item of items)
      for (const [time] of item.points) observed.add(time);
    const times = [...observed].sort((a, b) => a - b);
    const ordinal = new Map(times.map((time, index) => [time, index]));
    for (const item of items) {
      const points: [number, number][] = [];
      let missing = 0;
      for (let i = 0; i < item.points.length; i++) {
        const point = item.points[i];
        if (i > 0) {
          const before = ordinal.get(item.points[i - 1][0])!;
          const after = ordinal.get(point[0])!;
          if (after > before + 1) {
            points.push([times[before + 1], NaN]);
            missing += after - before - 1;
          }
        }
        points.push(point);
      }
      if (missing) result.set(item, { points, missing });
    }
  }
  return result;
}
