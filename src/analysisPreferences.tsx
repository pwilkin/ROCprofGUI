import {
  createContext,
  useContext,
  useState,
  type Dispatch,
  type SetStateAction,
} from "react";
import { readCodeObjectRelocations } from "./codeObjectRelocations";
import { readTraceAssociations } from "./traceAssociations";
export type PreferenceValue = string | number | boolean | null;
export type AnalysisPreferences = Record<string, PreferenceValue>;
const strings = [
  "hardware.selected",
  "sampling.search",
  "markers.scope",
  "dispatch.search",
  "host.search",
  "host.selected",
  "counters.search",
  "objects.search",
  "counters.formula",
  "counters.name",
  "events.search",
  "monitor.search",
  "tensor.scope",
  "source.location",
];
const pages = [
  "sampling.page",
  "markers.page",
  "summary.page",
  "host.page",
  "objects.page",
  "host.linkPage",
  "events.page",
  "monitor.page",
];
export function validAnalysisPreference(key: string, value: unknown): boolean {
  if (key === "att.associations") return readTraceAssociations(value) !== null;
  if (key === "objects.relocations")
    return readCodeObjectRelocations(value) !== null;
  if (key === "plot.lodBias")
    return (
      typeof value === "number" &&
      Number.isInteger(value) &&
      value >= -20 &&
      value <= 20
    );
  if (strings.includes(key))
    return (
      typeof value === "string" &&
      new TextEncoder().encode(value).length <= 8192 &&
      !value.includes("\0")
    );
  if (pages.includes(key))
    return (
      typeof value === "number" &&
      Number.isInteger(value) &&
      value >= 0 &&
      value <= 1000000
    );
  if (["sampling.selected", "markers.selected"].includes(key))
    return (
      value === null ||
      (typeof value === "number" && Number.isSafeInteger(value))
    );
  if (
    [
      "sampling.flame",
      "summary.counts",
      "flame.hierarchy",
      "source.lineNumbers",
      "source.includeHidden",
      "counters.waveStates",
    ].includes(key)
  )
    return typeof value === "boolean";
  if (key === "source.costWidth")
    return (
      typeof value === "number" &&
      Number.isInteger(value) &&
      value >= 0 &&
      value <= 400
    );
  if (key === "source.costMetric")
    return ["latency", "pcSamples", "pcIssued", "pcStalled"].includes(
      value as string,
    );
  if (key === "flame.groupBy")
    return value === "source" || value === "codeObject";
  if (key === "hardware.cursor")
    return (
      typeof value === "number" &&
      Number.isFinite(value) &&
      value >= 0 &&
      value <= 1
    );
  if (key === "sampling.metric")
    return value === "pcSamples" || value === "pcIssued" || value === "pcStalled";
  if (key === "sampling.order")
    return value === "samples" || value === "issued" || value === "stalled";
  if (key === "dispatch.sort")
    return (
      typeof value === "string" && ["total", "count", "name"].includes(value)
    );
  if (
    [
      "flame.latency",
      "flame.pcSamples",
      "flame.pcIssued",
      "flame.pcStalled",
    ].includes(key)
  )
    return (
      value === null ||
      (typeof value === "string" &&
        new TextEncoder().encode(value).length <= 8192 &&
        !value.includes("\0"))
    );
  return false;
}
export const AnalysisPreferencesContext = createContext<{
  values: AnalysisPreferences;
  set: Dispatch<SetStateAction<AnalysisPreferences>>;
} | null>(null);
export function useAnalysisPreference<T extends PreferenceValue>(
  key: string,
  initial: T,
): [T, Dispatch<SetStateAction<T>>] {
  const context = useContext(AnalysisPreferencesContext);
  const [local, setLocal] = useState<T>(initial);
  const stored = context?.values[key];
  const value =
    context && validAnalysisPreference(key, stored)
      ? (stored as T)
      : context
        ? initial
        : local;
  const set: Dispatch<SetStateAction<T>> = (update) => {
    if (!context) {
      setLocal(update);
      return;
    }
    context.set((previous) => {
      const old = validAnalysisPreference(key, previous[key])
        ? (previous[key] as T)
        : initial;
      const next = typeof update === "function" ? update(old) : update;
      return validAnalysisPreference(key, next)
        ? { ...previous, [key]: next }
        : previous;
    });
  };
  return [value, set];
}
