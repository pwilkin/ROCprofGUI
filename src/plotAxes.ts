import { createContext } from "react";
import type { Range } from "./types";
export type PlotAxes = Record<string, Range>;
export const PlotAxesContext = createContext<{
  axes: PlotAxes;
  ranges: PlotAxes;
  changeRange: (key: string, range: Range | null) => void;
  change: (key: string, bounds: Range | null) => void;
} | null>(null);
export function plotAxisKey(...identity: (string | number)[]): string {
  return JSON.stringify(identity);
}
