import { boundedRange } from "./analysis";
import { mipLevel } from "./timelineMip";
import type { Range } from "./types";
export const waveMipLevels = Array.from({ length: 13 }, (_, n) => n - 2);
export function discreteZoomRange(
  range: Range,
  bounds: Range,
  width: number,
  base: 1 | 4,
  level: number,
  anchor = 0.5,
): Range {
  const clampedLevel = Math.max(-2, Math.min(10, Math.round(level)));
  const span = Math.max(
    1,
    Math.floor(
      Math.floor((Math.max(1, width - 169) * base) / 6) * 2 ** clampedLevel,
    ),
  );
  const fraction = Math.max(0, Math.min(1, anchor));
  const time = range[0] + fraction * (range[1] - range[0]);
  return boundedRange(
    [time - fraction * span, time + (1 - fraction) * span],
    bounds,
  );
}
export function stepZoomRange(
  range: Range,
  bounds: Range,
  width: number,
  base: 1 | 4,
  direction: -1 | 1,
  anchor = 0.5,
): Range {
  const level = mipLevel(range, width, base);
  if ((direction < 0 && level <= -2) || (direction > 0 && level >= 10))
    return range;
  return discreteZoomRange(
    range,
    bounds,
    width,
    base,
    level + direction,
    anchor,
  );
}
