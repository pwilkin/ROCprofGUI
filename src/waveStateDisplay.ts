import type { Capture, Series } from "./types";
import { clockModel } from "./utilization";

export function defaultWaveStateDisplay(capture: Pick<Capture, "architecture" | "metadata">) {
  return clockModel(capture)?.family !== "rdna";
}

export function isWaveStateSeries(series: Series) {
  return series.unit === "waves" &&
    /^Wave state · (EXEC|WAIT|STALL)$/.test(series.name) &&
    /^SE \d+ · precomputed wave states$/.test(series.scope);
}
