import type { Capture, DecoderEvent, Dispatch, Interval, MarkerScope } from "./types";
import { colors } from "./types";
import type { Mark } from "./components/Plots";
const decoderEventNames = [
  "None",
  "CS partial flush",
  "Bottom-of-pipe timestamp",
  "Save context",
  "Dispatch end",
  "Cache flush",
  "Packet loss",
  "Code object load",
  "Code object unload",
  "Trace stall begin",
  "Trace stall end",
  "Trace flush",
  "DIDT stall begin",
  "DIDT stall end",
  "Cluster barrier",
  "Reserved",
  "GC rinse",
  "SPM sample",
];
export function decoderEventLabel(event: DecoderEvent): string {
  if (event.kind === "dispatch") return "Dispatch start";
  if (event.kind !== "event") return event.kind;
  const type = event.fields.type;
  const index = type === undefined ? -1 : Number(type);
  return Number.isInteger(index) &&
    index >= 0 &&
    index < decoderEventNames.length
    ? decoderEventNames[index]
    : `Decoder event type ${type ?? "unknown"}`;
}
export const decoderEventGroups = [
  [1, "Dispatch starts"],
  [2, "Flush / timestamps"],
  [4, "Code objects"],
  [8, "SQTT stalls / flush"],
  [16, "GC rinse"],
  [32, "Other events"],
] as const;
export const defaultEventGroups = 47;
export function decoderEventGroup(event: DecoderEvent): number {
  if (event.kind === "dispatch") return 1;
  if (event.kind !== "event") return 32;
  switch (Number(event.fields.type)) {
    case 1:
    case 2:
    case 5:
      return 2;
    case 7:
    case 8:
      return 4;
    case 9:
    case 10:
    case 11:
      return 8;
    case 16:
      return 16;
    default:
      return 32;
  }
}
export interface GlobalLayers {
  residency: boolean;
  dispatches: boolean;
  markers: boolean;
  events?: boolean;
  eventGroups?: number;
}
export const defaultGlobalLayers: GlobalLayers = {
  residency: true,
  dispatches: true,
  markers: true,
  events: true,
  eventGroups: defaultEventGroups,
};
export type DispatchRecord = Pick<Dispatch, "id" | "kernel" | "start" | "end" | "agent" | "queue" | "grid" | "workgroup">;
export type ResidencyRecord = Pick<Interval, "kernel" | "start" | "end" | "se" | "cu" | "simd" | "slot">;
export type MarkerRecord = Pick<MarkerScope, "start" | "end" | "point" | "se" | "cu" | "simd" | "slot" | "depth" | "name" | "markerId" | "codeObject" | "source">;
export function dispatchMark(d: DispatchRecord, index: number): Mark | null {
  return d.end > d.start ? {
    start: d.start, end: d.end, lane: `Agent ${d.agent} Q${d.queue}`,
    color: colors[3 + (index % 7)],
    label: `Dispatch ${d.id} · ${d.kernel} · agent ${d.agent} · queue ${d.queue} · grid ${d.grid || "unknown"} · workgroup ${d.workgroup || "unknown"}`,
    key: `dispatch:${index}`, recordKind: "dispatch", recordIndex: index,
  } : null;
}
export function residencyMark(i: ResidencyRecord, index: number): Mark {
  return { start: i.start, end: i.end, lane: `SE${i.se} CU${i.cu} S${i.simd} slot${i.slot}`,
    color: colors[5 + (i.se % 3)], label: `${i.kernel} · SE ${i.se} · CU ${i.cu} · SIMD ${i.simd} · slot ${i.slot}`,
    key: `residency:${index}`, recordKind: "residency", shaderEngine: i.se, recordIndex: index };
}
export function markerMark(m: MarkerRecord, index: number, captureEnd: number): Mark {
  return { start: m.start, end: m.point ? m.start : (m.end ?? captureEnd),
    lane: `SE${m.se} CU${m.cu} S${m.simd} slot${m.slot} · marker depth ${m.depth}`,
    color: colors[(m.depth + 3) % colors.length],
    label: `${m.name} · marker ${m.markerId} · ${m.point ? "point" : m.end === null ? "unclosed scope; displayed to capture end" : "scope"} · code object ${m.codeObject} · ${m.source || "source unavailable"}`,
    key: `marker:${index}`, recordKind: "marker", shaderEngine: m.se, recordIndex: index };
}
export function decoderMark(e: DecoderEvent, index: number, layers: GlobalLayers, surface: "global" | "cu" = "global", se?: number): Mark | null {
  if (layers.events === false ||
    (surface === "cu" && (e.se !== se || (e.kind === "event" && [1, 7, 8, 17].includes(Number(e.fields.type))))) ||
    !(decoderEventGroup(e) & (layers.eventGroups ?? defaultEventGroups))) return null;
  return { start: e.time, end: e.time, lane: `Events SE${e.se} ME${e.fields.me_id ?? "?"} P${e.fields.pipe_id ?? "?"}`,
    color: colors[e.kind === "dispatch" ? 5 : 3],
    label: `${decoderEventLabel(e)} ${e.id} · SE ${e.se} · ${Object.entries(e.fields).map(([key, value]) => `${key}=${value}`).join(" · ")}`,
    key: `event:${index}`, recordKind: "event", shaderEngine: e.se,
    overlayWidth: e.kind === "dispatch" ? 5 : 4, recordIndex: index };
}
export function dispatchMarks(capture: Capture): Mark[] {
  return capture.dispatches.flatMap((d, index) => { const mark = dispatchMark(d, index); return mark ? [mark] : []; });
}
export function globalMarks(capture: Capture, layers: GlobalLayers, selectedKey?: string): Mark[] {
  const marks: Mark[] = [];
  if (layers.residency) capture.intervals.forEach((i, index) => marks.push(residencyMark(i, index)));
  if (layers.markers) (capture.markers || []).forEach((m, index) => marks.push(markerMark(m, index, capture.end)));
  if (layers.dispatches) marks.push(...dispatchMarks(capture));
  marks.push(...decoderMarks(capture, layers));
  for (const mark of marks) mark.selected = mark.key === selectedKey;
  return marks;
}
export function decoderMarks(capture: Capture, layers: GlobalLayers, surface: "global" | "cu" = "global", se?: number): Mark[] {
  return (capture.decoderEvents || []).flatMap((e, index) => { const mark = decoderMark(e, index, layers, surface, se); return mark ? [mark] : []; });
}
