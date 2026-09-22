import type { Mark } from "./components/Plots";
import { categories, colors } from "./types";

export interface MipToken {
  start: number;
  duration: number;
  category: number;
  slot: number;
  count: number;
  source?: number;
  rawStart?: number;
  rawEnd?: number;
}
interface PartialToken {
  token: MipToken;
  cycles: number[];
}
const order = (a: MipToken, b: MipToken) =>
  a.start - b.start || a.slot - b.slot || b.duration - a.duration;
function dominant(cycles: number[]): number {
  let best = -1,
    type = 0;
  cycles.forEach((value, index) => {
    if (value >= best) {
      best = value;
      type = index;
    }
  });
  return type;
}
function finalize(current: PartialToken, resolution: number): MipToken {
  const round = (value: number) =>
    Math.floor((value + Math.floor(resolution / 2)) / resolution) * resolution;
  current.token.category = dominant(current.cycles);
  current.token.start = round(current.token.start);
  current.token.duration = round(current.token.duration);
  return { ...current.token };
}

// Port of RCV TokenGroup::SetMipN; attribution is retained in third_party/rcv/LICENSE.
export function mipTokens(
  input: MipToken[],
  base: 1 | 4,
  level: number,
): MipToken[] {
  const tokens = input
    .filter(
      (t) => Number.isInteger(t.category) && t.category >= 0 && t.category < 16,
    )
    .map((t) => ({ ...t }))
    .sort(order);
  if (level <= 1) return tokens;
  let previous: PartialToken[] = tokens.map((token) => {
    const cycles = Array<number>(16).fill(0);
    cycles[token.category] = token.duration;
    return { token, cycles };
  });
  let result: MipToken[] = [];
  for (let m = 0; m <= Math.min(8, level - 2); m++) {
    const resolution = 2 ** m * base;
    result = [];
    if (previous.length <= 16) {
      for (const entry of previous) {
        const token = finalize(
          { token: { ...entry.token }, cycles: entry.cycles },
          resolution,
        );
        if (token.category > 0) result.push(token);
      }
    } else {
      const fresh = (start: number): PartialToken => ({
        token: { start, duration: 0, category: 0, slot: 0, count: 0 },
        cycles: Array<number>(16).fill(0),
      });
      const next: PartialToken[] = [];
      let current = fresh(previous[0].token.start),
        cycle = current.token.start;
      for (const entry of previous) {
        const token = entry.token;
        const prevEnd =
          token.duration + token.start - current.token.start > 2 * resolution;
        const thisEnd =
          prevEnd &&
          current.token.duration > resolution &&
          token.category !== dominant(current.cycles);
        if (cycle + Math.floor(resolution / 2) <= token.start || thisEnd) {
          if (current.token.duration > 0) {
            const final = finalize(current, resolution);
            next.push(current);
            if (final.category > 0) result.push(final);
          }
          current = fresh(token.start);
          cycle = token.start;
        } else if (cycle !== token.start) {
          current.cycles[0] += token.start - cycle;
          current.token.duration += token.start - cycle;
        }
        current.token.duration = Math.max(
          current.token.duration,
          token.start + token.duration - current.token.start,
        );
        current.token.count += token.count;
        current.token.rawStart = Math.min(
          current.token.rawStart ?? Infinity,
          token.rawStart ?? token.start,
        );
        current.token.rawEnd = Math.max(
          current.token.rawEnd ?? -Infinity,
          token.rawEnd ?? token.start + token.duration,
        );
        for (let n = 0; n < 16; n++) current.cycles[n] += entry.cycles[n];
        cycle = Math.max(cycle, token.start + token.duration);
      }
      const final = finalize(current, resolution);
      if (final.category > 0) result.push(final);
      next.push(current);
      previous = next;
    }
    result.sort(order);
  }
  return result;
}

export function mipLevel(
  range: [number, number],
  width: number,
  base: 1 | 4,
): number {
  return Math.max(
    -2,
    Math.min(
      10,
      Math.round(
        Math.log2(
          ((range[1] - range[0]) / Math.max(1, width - 169)) * (6 / base),
        ),
      ),
    ),
  );
}
export function mipMarks(marks: Mark[], base: 1 | 4, level: number): Mark[] {
  if (level <= 1) return marks;
  const groups = new Map<string, Mark[]>(),
    untouched: Mark[] = [];
  for (const mark of marks) {
    if (mark.mipGroup === undefined || mark.tokenCategory === undefined) {
      untouched.push(mark);
      continue;
    }
    const group = groups.get(mark.mipGroup) || [];
    group.push(mark);
    groups.set(mark.mipGroup, group);
  }
  const output = [...untouched];
  for (const [key, group] of groups) {
    const templates = new Map(group.map((mark) => [mark.tokenCategory, mark]));
    const tokens = group.map((mark, index) => ({
      start: mark.start,
      duration: mark.end - mark.start,
      category: mark.tokenCategory!,
      slot: mark.displaySlot ?? 0,
      count: 1,
      source: index,
    }));
    for (const [index, token] of mipTokens(tokens, base, level).entries()) {
      if (token.source !== undefined) {
        const original = group[token.source];
        output.push({
          ...original,
          start: token.start,
          end: token.start + token.duration,
          color: colors[token.category] || colors[0],
        });
      } else {
        const template = templates.get(token.category) || group[0];
        output.push({
          start: token.start,
          end: token.start + token.duration,
          lane: template.lane,
          color: colors[token.category] || colors[0],
          key: `mip:${key}:${index}`,
          displaySlot: token.slot,
          aggregateCount: Math.max(1, token.count),
          zoomRange: [
            token.rawStart ?? token.start,
            token.rawEnd ?? token.start + token.duration,
          ],
          label: `Upstream mip ${level} · ${token.count} records · dominant ${categories[token.category] || token.category}; click to zoom`,
        });
      }
    }
  }
  return output;
}
