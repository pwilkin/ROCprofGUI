import { closeSync, openSync, readSync, statSync } from "node:fs";
import { join } from "node:path";
import protocol from "../src/mcpAnalysisProtocol.json";
import type { KernelSample } from "../src/analysis";
import type {
  DispatchRecord,
  ResidencyRecord,
  MarkerRecord,
} from "../src/globalTimeline";
import type { DecoderEvent, Range } from "../src/types";
export const budgets = protocol.budgets;
export const mismatch =
  "Analysis worker protocol mismatch; rebuild the rocprof-studio binary and bundled analysis worker together.";
export function object(value: unknown): Record<string, any> {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw Error("Malformed analysis object");
  return value as Record<string, any>;
}
export function keys(
  value: unknown,
  required: string[],
  optional: string[] = [],
) {
  const o = object(value);
  if (
    required.some((k) => !Object.hasOwn(o, k)) ||
    Object.keys(o).some((k) => ![...required, ...optional].includes(k))
  )
    throw Error("Malformed analysis fields");
  return o;
}
function integer(v: unknown) {
  if (!Number.isSafeInteger(v) || (v as number) < 0)
    throw Error("Malformed nonnegative analysis integer");
}
export function finite(v: unknown) {
  if (typeof v !== "number" || !Number.isFinite(v))
    throw Error("Malformed finite analysis coordinate");
}
export function string(v: unknown, max = budgets.stringBytes) {
  if (typeof v !== "string" || Buffer.byteLength(v) > max)
    throw Error(
      `MCP_ANALYSIS_RECORD_BUDGET: string exceeds ${max} UTF-8 bytes; use offline inspection`,
    );
}
export type Descriptor = {
  kind: string;
  file: string;
  rows: number;
  bytes: number;
};
export interface Manifest {
  protocol: string;
  version: number;
  budgetVersion: number;
  analysis: string;
  mode: string;
  capture: {
    id: string;
    name: string;
    format: string;
    architecture: string;
    unit: string;
    origin: string;
    start: number;
    end: number;
  };
  args: Record<string, any>;
  streams?: Descriptor[];
  payload?: { kind: string; file: string; bytes: number };
}
export function readBoundedJson(path: string, max: number) {
  if (statSync(path).size > max)
    throw Error(`MCP_ANALYSIS_INPUT_BUDGET: input exceeds ${max} bytes`);
  const fd = openSync(path, "r");
  try {
    const bytes = Buffer.alloc(statSync(path).size);
    let offset = 0;
    while (offset < bytes.length) {
      const n = readSync(
        fd,
        bytes,
        offset,
        Math.min(budgets.readBufferBytes, bytes.length - offset),
        null,
      );
      if (!n) throw Error("Truncated analysis JSON");
      offset += n;
    }
    return JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
  } finally {
    closeSync(fd);
  }
}
export function validateManifest(input: unknown): Manifest {
  const m = object(input);
  if (
    m.protocol !== protocol.protocol ||
    m.version !== protocol.version ||
    m.budgetVersion !== protocol.budgetVersion
  )
    throw Error(mismatch);
  keys(
    m,
    [
      "protocol",
      "version",
      "budgetVersion",
      "analysis",
      "mode",
      "capture",
      "args",
    ],
    ["streams", "payload"],
  );
  const c = keys(m.capture, [
    "id",
    "name",
    "format",
    "architecture",
    "unit",
    "origin",
    "start",
    "end",
  ]);
  for (const k of ["id", "name", "format", "architecture", "unit", "origin"])
    string(c[k]);
  finite(c.start);
  finite(c.end);
  if (
    !protocol.analyses.includes(m.analysis) ||
    !protocol.modes.includes(m.mode)
  )
    throw Error(mismatch);
  if (m.mode === "kernel-stream" || m.mode === "timeline-stream") {
    if (m.payload !== undefined || !Array.isArray(m.streams))
      throw Error("Malformed analysis stream manifest");
    const expected =
      m.mode === "kernel-stream"
        ? [
            ["kernel-dispatch", protocol.files.dispatches],
            ["kernel-interval", protocol.files.intervals],
          ]
        : [["timeline-record", protocol.files.timeline]];
    if (
      m.analysis !== (m.mode === "kernel-stream" ? "kernels" : "timeline") ||
      m.streams.length !== expected.length
    )
      throw Error(mismatch);
    m.streams.forEach((s: any, i: number) => {
      keys(s, ["kind", "file", "rows", "bytes"]);
      integer(s.rows);
      integer(s.bytes);
      if (s.kind !== expected[i][0] || s.file !== expected[i][1])
        throw Error("Invalid analysis stream kind/path");
    });
    if (
      m.streams.reduce((sum: number, s: any) => sum + s.bytes, 0) >
      budgets.spoolBytes
    )
      throw Error("MCP_ANALYSIS_SPOOL_BUDGET: spool too large");
    if (m.mode === "kernel-stream") {
      const a = keys(m.args, ["query", "offset", "limit"]);
      string(a.query, budgets.manifestBytes);
      // Paging is authoritative in Rust; no worker arithmetic uses offset.
      if (
        !Number.isInteger(a.offset) ||
        a.offset < 0 ||
        a.offset > Number("18446744073709551615")
      )
        throw Error("Malformed page offset");
      integer(a.limit);
      if (a.limit > 1000) throw Error("Malformed effective page limit");
    } else {
      const a = keys(m.args, [
        "timeline",
        "range",
        "timePages",
        "lanesPerPage",
        "outputMode",
        "maxOutputBytes",
        "maxGlyphs",
      ]);
      if (
        !["dispatch", "global"].includes(a.timeline) ||
        !["inline", "file"].includes(a.outputMode)
      )
        throw Error("Malformed timeline mode");
      if (!Array.isArray(a.range) || a.range.length !== 2)
        throw Error("Malformed timeline range");
      a.range.forEach(finite);
      if (a.range[1] <= a.range[0] || !Number.isFinite(a.range[1] - a.range[0]))
        throw Error("Malformed timeline range");
      for (const k of [
        "timePages",
        "lanesPerPage",
        "maxOutputBytes",
        "maxGlyphs",
      ])
        integer(a[k]);
      if (
        !a.timePages ||
        a.timePages > budgets.timePages ||
        !a.lanesPerPage ||
        a.lanesPerPage > budgets.lanesPerPage ||
        !a.maxOutputBytes ||
        a.maxOutputBytes >
          (a.outputMode === "file" ? budgets.fileBytes : budgets.inlineBytes) ||
        !a.maxGlyphs ||
        a.maxGlyphs >
          (a.outputMode === "file" ? budgets.fileGlyphs : budgets.inlineGlyphs)
      )
        throw Error("Malformed timeline budgets");
    }
  } else {
    if (
      m.streams !== undefined ||
      ["kernels", "timeline"].includes(m.analysis) ||
      (m.analysis === "diagnostics") !==
        (m.mode === "diagnostics-compatibility")
    )
      throw Error(mismatch);
    keys(m.args, []);
    const p = keys(m.payload, ["kind", "file", "bytes"]);
    integer(p.bytes);
    if (
      p.kind !== "payload" ||
      p.file !== protocol.files.payload ||
      p.bytes > budgets.projectedBytes
    )
      throw Error("Invalid analysis payload kind/path/bytes");
  }
  return m as Manifest;
}
export function* readRecords(
  folder: string,
  descriptor: Descriptor,
): Generator<unknown> {
  const path = join(folder, descriptor.file);
  if (statSync(path).size !== descriptor.bytes)
    throw Error("Analysis stream byte count mismatch");
  const fd = openSync(path, "r"),
    buffer = Buffer.alloc(budgets.readBufferBytes),
    line = Buffer.alloc(budgets.recordBytes);
  let used = 0,
    rows = 0,
    bytes = 0;
  const decoder = new TextDecoder("utf-8", { fatal: true });
  try {
    for (;;) {
      const n = readSync(fd, buffer, 0, buffer.length, null);
      if (!n) break;
      bytes += n;
      let start = 0;
      for (let i = 0; i < n; i++) {
        if (buffer[i] !== 10) continue;
        const len = i - start;
        if (used + len + 1 > budgets.recordBytes)
          throw Error("MCP_ANALYSIS_RECORD_BUDGET: NDJSON record too large");
        buffer.copy(line, used, start, i);
        used += len;
        rows++;
        yield JSON.parse(decoder.decode(line.subarray(0, used)));
        used = 0;
        start = i + 1;
      }
      if (used + n - start >= budgets.recordBytes)
        throw Error("MCP_ANALYSIS_RECORD_BUDGET: NDJSON record too large");
      buffer.copy(line, used, start, n);
      used += n - start;
    }
    if (used)
      throw Error("Truncated analysis record; trailing newline required");
    if (rows !== descriptor.rows || bytes !== descriptor.bytes)
      throw Error("Analysis stream count/byte mismatch");
  } finally {
    closeSync(fd);
  }
}
export function kernelSample(v: unknown): KernelSample {
  const o = keys(v, ["kernel", "start", "end"]);
  string(o.kernel, budgets.recordBytes);
  finite(o.start);
  finite(o.end);
  return o as KernelSample;
}
export type TimelineRecord =
  | { kind: "dispatch"; index: number; record: DispatchRecord }
  | { kind: "residency"; index: number; record: ResidencyRecord }
  | { kind: "marker"; index: number; record: MarkerRecord }
  | { kind: "event"; index: number; record: DecoderEvent };
export function timelineRecord(v: unknown): TimelineRecord {
  const o = keys(v, ["kind", "index", "record"]);
  integer(o.index);
  const r = object(o.record);
  switch (o.kind) {
    case "dispatch":
      keys(r, [
        "id",
        "kernel",
        "start",
        "end",
        "agent",
        "queue",
        "grid",
        "workgroup",
      ]);
      break;
    case "residency":
      keys(r, ["kernel", "start", "end", "se", "cu", "simd", "slot"]);
      break;
    case "marker":
      keys(r, [
        "start",
        "end",
        "point",
        "se",
        "cu",
        "simd",
        "slot",
        "depth",
        "name",
        "markerId",
        "codeObject",
        "source",
      ]);
      if (typeof r.point !== "boolean") throw Error("Malformed marker point");
      break;
    case "event":
      keys(r, ["id", "se", "time", "kind", "fields"]);
      finite(r.time);
      string(r.kind);
      for (const [k, v] of Object.entries(object(r.fields))) {
        string(k);
        string(v);
      }
      break;
    default:
      throw Error("Unknown timeline record kind");
  }
  for (const k of ["start", "end"])
    if (
      Object.hasOwn(r, k) &&
      !(o.kind === "marker" && k === "end" && r.end === null)
    )
      finite(r[k]);
  for (const k of [
    "id",
    "kernel",
    "agent",
    "queue",
    "grid",
    "workgroup",
    "name",
    "codeObject",
    "source",
  ])
    if (Object.hasOwn(r, k)) string(r[k]);
  for (const k of ["se", "cu", "simd", "slot", "depth", "markerId"])
    if (Object.hasOwn(r, k)) integer(r[k]);
  return o as TimelineRecord;
}
export function overlaps(start: number, end: number, range: Range) {
  return start === end
    ? range[0] <= start && start < range[1]
    : start < range[1] && end > range[0];
}

export function validatePayload(input: unknown, manifest: Manifest) {
  const p = keys(input, ["capture", "args"]);
  const args = {
    occupancy: ["se", "kernel"],
    hotspots: ["metric"],
    source: ["metric"],
    markers: ["range"],
    scalar: ["name", "formula"],
    tensor: ["raw", "definitions"],
    diagnostics: ["workspace"],
  };
  keys(
    p.args,
    ["analysis"],
    args[manifest.analysis as keyof typeof args] ?? [],
  );
  if (p.args.analysis !== manifest.analysis)
    throw Error("Analysis payload kind mismatch");
  if (manifest.analysis !== "diagnostics") {
    const fields = {
      occupancy: ["intervals"],
      activity: ["instructions"],
      hardware: ["architecture", "counters"],
      hotspots: ["instructions"],
      source: ["instructions"],
      markers: ["markers", "start", "end"],
      scalar: ["counters"],
      tensor: [],
    };
    keys(p.capture, fields[manifest.analysis as keyof typeof fields]);
  }
  return p;
}
