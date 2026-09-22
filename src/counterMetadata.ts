import type { Series } from "./types";

export function counterDetail(series: Series): string {
  const detail = `${series.scope} · ${series.unit}`;
  const selection = series.att;
  if (!selection) return detail;
  const simds = selection.simds.length
    ? `SIMDs ${selection.simds.join(", ")}`
    : "no SIMDs selected";
  return `${detail} · ${simds} · mask 0x${selection.simdMask.toString(16)} (${selection.explicit ? "recorded suffix" : "SDK default; suffix absent"})`;
}
