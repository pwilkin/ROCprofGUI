import { readPreference } from "./session";
export const isaColumns = [
  { key: "line", label: "Line", width: 48 },
  { key: "address", label: "Address", width: 85 },
  { key: "instruction", label: "Instruction", width: 320 },
  { key: "hits", label: "Hits", width: 70 },
  { key: "cycles", label: "Cycles", width: 85 },
  { key: "stall", label: "Stall", width: 75 },
  { key: "samples", label: "PC samples", width: 95 },
  { key: "idle", label: "Idle", width: 85 },
  { key: "issued", label: "PC issued", width: 95 },
  { key: "stalled", label: "PC stalled", width: 95 },
  { key: "codeObject", label: "Code object", width: 120 },
  { key: "source", label: "Source reference", width: 240 },
] as const;
export const defaultHiddenColumns = isaColumns
  .slice(7)
  .map((column) => column.key);
export function isaWidths(stored: unknown): number[] {
  const valid =
    Array.isArray(stored) &&
    [7, isaColumns.length].includes(stored.length) &&
    stored.every((n) => Number.isFinite(n) && n >= 40 && n <= 1000);
  return isaColumns.map((column, i) =>
    valid ? (stored[i] ?? column.width) : column.width,
  );
}
export function isaHiddenColumns(stored: unknown): string[] {
  return Array.isArray(stored) &&
    new Set(stored).size === stored.length &&
    stored.every(
      (key) =>
        key !== "instruction" &&
        isaColumns.some((column) => column.key === key),
    )
    ? [...stored]
    : [...defaultHiddenColumns];
}
export interface IsaState {
  search: string;
  fold: boolean;
  collapsed: number[];
  widths: number[];
  hiddenColumns?: string[];
  sort: boolean;
  page: number;
}
export function initialIsaState(): IsaState {
  const widths = isaWidths(
    readPreference<unknown>("rocprof:isa-columns", null),
  );
  return {
    search: "",
    fold: true,
    collapsed: [],
    widths: [...widths],
    hiddenColumns: isaHiddenColumns(
      readPreference<unknown>("rocprof:isa-hidden-columns", null),
    ),
    sort: false,
    page: 0,
  };
}
