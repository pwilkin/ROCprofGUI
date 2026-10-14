import type { Capture, Range } from "../../src/types";
export function union(ranges: Range[]): number {
  const sorted = ranges
    .filter(([a, b]) => Number.isFinite(a) && Number.isFinite(b) && b > a)
    .sort((a, b) => a[0] - b[0]);
  let total = 0,
    start = 0,
    end = 0;
  for (const [a, b] of sorted) {
    if (a > end) {
      total += end - start;
      start = a;
      end = b;
    } else end = Math.max(end, b);
  }
  return total + end - start;
}
export function goldenKernelStats(c: Capture) {
  const m = new Map<
    string,
    {
      name: string;
      count: number;
      total: number;
      min: number;
      max: number;
      waves: number;
      waveTime: number;
      activeTime: number;
    }
  >();
  for (const d of c.dispatches) {
    const v = m.get(d.kernel) || {
      name: d.kernel,
      count: 0,
      total: 0,
      min: Infinity,
      max: 0,
      waves: 0,
      waveTime: 0,
      activeTime: 0,
    };
    const duration = d.end - d.start;
    v.count++;
    v.total += duration;
    v.min = Math.min(v.min, duration);
    v.max = Math.max(v.max, duration);
    m.set(d.kernel, v);
  }
  const lifetimes = new Map<string, Range[]>();
  for (const i of c.intervals) {
    const v = m.get(i.kernel);
    if (!v) continue;
    v.waves++;
    v.waveTime += i.end - i.start;
    const ranges = lifetimes.get(i.kernel) ?? [];
    ranges.push([i.start, i.end]);
    lifetimes.set(i.kernel, ranges);
  }
  for (const [kernel, ranges] of lifetimes) m.get(kernel)!.activeTime = union(ranges);
  return [...m.values()].sort(
    (a, b) => b.total - a.total || b.activeTime - a.activeTime,
  );
}
import type { DecoderEvent } from "../../src/types";
import { colors } from "../../src/types";
import type { Mark } from "../../src/components/Plots";
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
export function goldenDispatchMarks(capture: Capture): Mark[] {
  return capture.dispatches.flatMap((d, index) =>
    d.end > d.start
      ? [
          {
            start: d.start,
            end: d.end,
            lane: `Agent ${d.agent} Q${d.queue}`,
            color: colors[3 + (index % 7)],
            label: `Dispatch ${d.id} · ${d.kernel} · agent ${d.agent} · queue ${d.queue} · grid ${d.grid || "unknown"} · workgroup ${d.workgroup || "unknown"}`,
            key: `dispatch:${index}`,
            recordKind: "dispatch" as const,
            recordIndex: index,
          },
        ]
      : [],
  );
}
export function goldenGlobalMarks(
  capture: Capture,
  layers: GlobalLayers,
  selectedKey?: string,
): Mark[] {
  const marks: Mark[] = [];
  if (layers.residency)
    capture.intervals.forEach((i, index) =>
      marks.push({
        start: i.start,
        end: i.end,
        lane: `SE${i.se} CU${i.cu} S${i.simd} slot${i.slot}`,
        color: colors[5 + (i.se % 3)],
        label: `${i.kernel} · SE ${i.se} · CU ${i.cu} · SIMD ${i.simd} · slot ${i.slot}`,
        key: `residency:${index}`,
        recordKind: "residency",
        shaderEngine: i.se,
        recordIndex: index,
      }),
    );
  if (layers.markers)
    (capture.markers || []).forEach((m, index) =>
      marks.push({
        start: m.start,
        end: m.point ? m.start : (m.end ?? capture.end),
        lane: `SE${m.se} CU${m.cu} S${m.simd} slot${m.slot} · marker depth ${m.depth}`,
        color: colors[(m.depth + 3) % colors.length],
        label: `${m.name} · marker ${m.markerId} · ${m.point ? "point" : m.end === null ? "unclosed scope; displayed to capture end" : "scope"} · code object ${m.codeObject} · ${m.source || "source unavailable"}`,
        key: `marker:${index}`,
        recordKind: "marker",
        shaderEngine: m.se,
        recordIndex: index,
      }),
    );
  if (layers.dispatches) marks.push(...goldenDispatchMarks(capture));
  marks.push(...goldenDecoderMarks(capture, layers));
  for (const mark of marks) mark.selected = mark.key === selectedKey;
  return marks;
}

export function goldenDecoderMarks(
  capture: Capture,
  layers: GlobalLayers,
  surface: "global" | "cu" = "global",
  se?: number,
): Mark[] {
  const marks: Mark[] = [];
  if (layers.events !== false)
    (capture.decoderEvents || []).forEach((e, index) => {
      if (
        surface === "cu" &&
        (e.se !== se ||
          (e.kind === "event" && [1, 7, 8, 17].includes(Number(e.fields.type))))
      )
        return;
      if (!(decoderEventGroup(e) & (layers.eventGroups ?? defaultEventGroups)))
        return;
      marks.push({
        start: e.time,
        end: e.time,
        lane: `Events SE${e.se} ME${e.fields.me_id ?? "?"} P${e.fields.pipe_id ?? "?"}`,
        color: colors[e.kind === "dispatch" ? 5 : 3],
        label: `${decoderEventLabel(e)} ${e.id} · SE ${e.se} · ${Object.entries(
          e.fields,
        )
          .map(([key, value]) => `${key}=${value}`)
          .join(" · ")}`,
        key: `event:${index}`,
        recordKind: "event",
        shaderEngine: e.se,
        overlayWidth: e.kind === "dispatch" ? 5 : 4,
        recordIndex: index,
      });
    });
  return marks;
}
