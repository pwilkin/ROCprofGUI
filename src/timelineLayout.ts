import type { Token } from "./types";

// RCV appendTokenWithSlotBump assigns four display slots for equal-clock collisions.
export function tokenSlots(tokens: Token[]): number[] {
  const previous = [0, 0, 0, 0],
    end = [0, 0, 0, 0];
  return tokens.map((token) => {
    let slot = 0;
    while (
      previous[slot] === token.start &&
      end[slot] > token.start &&
      slot < 3
    )
      slot++;
    previous[slot] = token.start;
    end[slot] = Math.max(
      end[slot],
      token.start + Math.max(token.duration, token.stall),
    );
    return slot;
  });
}
export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}
export interface MipGeometry {
  level: number;
  base: 1 | 4;
}
export function mipPixel(value: number, { level, base }: MipGeometry): number {
  const rounding = level > 0 ? 2 ** (level - 1) : 0;
  return Math.floor(
    (Math.trunc((Math.trunc(value) * (base === 1 ? 12 : 3)) / 2) + rounding) /
      2 ** level,
  );
}
export function timelineRect(
  start: number,
  end: number,
  row: number,
  slot: number,
  range: [number, number],
  width: number,
  laneHeight = 28,
  mip?: MipGeometry,
): Rect | null {
  if (
    row < 0 ||
    end < range[0] ||
    (end === range[0] && start < end) ||
    start > range[1] ||
    width <= 169 ||
    range[1] <= range[0]
  )
    return null;
  const scale = (width - 169) / (range[1] - range[0]);
  const project = (time: number) =>
    mip
      ? 155 +
        (mipPixel(time - range[0], mip) * scale) /
          (6 / mip.base / 2 ** mip.level)
      : 155 + (time - range[0]) * scale;
  const projectedX = Math.max(155, project(start));
  const x = start === end ? Math.min(width - 16, projectedX) : projectedX;
  const right = Math.min(width - 14, project(end));
  return {
    x,
    y: 27 + (5 - Math.min(1, slot)) * (laneHeight / 28) + row * laneHeight,
    width: Math.min(Math.max(2, right - x), width - 14 - x),
    height: Math.max(
      2,
      Math.min(
        laneHeight - (5 * laneHeight) / 28,
        (17 -
          (mip && mip.level >= 1 ? 2 - mip.level : 3) *
            Math.max(0, 2 * slot - 1)) *
          (laneHeight / 28),
      ),
    ),
  };
}
export const insideRect = (r: Rect, x: number, y: number) =>
  x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height;
