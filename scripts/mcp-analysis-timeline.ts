import { open } from "node:fs/promises";
import { join } from "node:path";
import {
  dispatchMark,
  residencyMark,
  markerMark,
  decoderMark,
  defaultGlobalLayers,
} from "../src/globalTimeline";
import {
  timelineDocument,
  type TimelineExportStats,
} from "../src/timelineExport";
import { TimelineWindowBudgetError } from "../src/timelineExportBudget";
import type { Range } from "../src/types";
import type { Mark } from "../src/components/Plots";
import protocol from "../src/mcpAnalysisProtocol.json";
import {
  budgets,
  overlaps,
  readRecords,
  timelineRecord,
  type Manifest,
} from "./mcp-analysis-transport";
export function loadTimelineWindow(
  folder: string,
  manifest: Manifest,
  range: Range,
  limits = { marks: budgets.windowMarks, bytes: budgets.windowBytes },
  stats?: { marks: number; bytes: number },
) {
  const marks: Mark[] = [];
  let bytes = 2;
  for (const raw of readRecords(folder, manifest.streams![0])) {
    const row = timelineRecord(raw),
      r = row.record;
    const start =
      row.kind === "event" ? row.record.time : (r as { start: number }).start;
    const end =
      row.kind === "event"
        ? start
        : row.kind === "marker"
          ? row.record.point
            ? start
            : (row.record.end ?? manifest.capture.end)
          : (r as { end: number }).end;
    if (!overlaps(start, end, range)) continue;
    let mark: Mark | null;
    switch (row.kind) {
      case "dispatch":
        mark = dispatchMark(row.record, row.index);
        break;
      case "residency":
        mark = residencyMark(row.record, row.index);
        break;
      case "marker":
        mark = markerMark(row.record, row.index, manifest.capture.end);
        break;
      case "event":
        mark = decoderMark(row.record, row.index, defaultGlobalLayers);
        break;
    }
    if (!mark) continue;
    if (manifest.args.timeline === "global") mark.selected = false;
    const size = Buffer.byteLength(JSON.stringify(mark));
    if (size + 2 > limits.bytes)
      throw Error(
        `MCP_ANALYSIS_RECORD_BUDGET: single generated mark exceeds ${limits.bytes} bytes; use offline inspection. No events were sampled.`,
      );
    if (
      marks.length >= limits.marks ||
      bytes + size + (marks.length ? 1 : 0) > limits.bytes
    )
      throw new TimelineWindowBudgetError(
        `MCP_TIMELINE_WINDOW_BUDGET: window exceeds ${limits.marks} marks or ${limits.bytes} encoded bytes`,
      );
    bytes += size + (marks.length ? 1 : 0);
    marks.push(mark);
    if (stats) {
      stats.marks = Math.max(stats.marks, marks.length);
      stats.bytes = Math.max(stats.bytes, bytes);
    }
  }
  return marks;
}
export async function timelineAnalysis(folder: string, manifest: Manifest) {
  const a = manifest.args;
  const file = await open(join(folder, protocol.files.timelineResult), "wx");
  let bytes = 0,
    complete: TimelineExportStats | undefined;
  try {
    await timelineDocument({
      label: manifest.capture.name,
      unit: manifest.capture.unit,
      bounds: a.range as Range,
      timePages: a.timePages,
      lanesPerPage: a.lanesPerPage,
      stacked: true,
      load: async (range) => loadTimelineWindow(folder, manifest, range),
      signal: new AbortController().signal,
      limits: { maxBytes: a.maxOutputBytes, maxGlyphs: a.maxGlyphs },
      contextNote: `MCP export: recorded-data-only; replay completeness unknown. Clock ${manifest.capture.unit}, origin ${manifest.capture.origin}. Effective output budgets: ${a.maxOutputBytes} bytes and ${a.maxGlyphs} drawn glyphs; window ${budgets.windowMarks} marks / ${budgets.windowBytes} encoded bytes. Glyphs duplicated across pages are not distinct dispatches.`,
      writeChunk: async (text) => {
        const size = Buffer.byteLength(text);
        if (size > budgets.chunkBytes || bytes + size > a.maxOutputBytes)
          throw Error(
            "MCP_TIMELINE_BUDGET: output write exceeds effective byte/chunk limit",
          );
        await file.writeFile(text, "utf8");
        bytes += size;
      },
      onComplete: (stats) => {
        complete = stats;
      },
    });
    if (!complete || complete.bytes !== bytes)
      throw Error("Timeline footer/receipt byte mismatch");
  } catch (error) {
    if (
      error instanceof Error &&
      /cannot subdivide|Timeline export exceeds/.test(error.message)
    ) {
      const remedy =
        a.outputMode === "inline" && /bytes|drawn records/.test(error.message)
          ? ", or set output to a new HTML path for the file-output budget"
          : "";
      throw Error(
        `MCP_TIMELINE_BUDGET: ${error.message} for range ${JSON.stringify(a.range)}. Retry capture_analyze with a narrower range [start,end]${remedy}. limit does not limit timeline events. No events were sampled and no output file was published.`,
      );
    }
    throw error;
  } finally {
    await file.close();
  }
  return {
    protocol: protocol.protocol,
    version: protocol.version,
    kind: "timeline-html",
    file: protocol.files.timelineResult,
    bytes,
    pages: complete.pages,
    segments: complete.segments,
    glyphs: complete.glyphs,
    budgets: {
      ...complete.budgets,
      windowMarks: budgets.windowMarks,
      windowBytes: budgets.windowBytes,
      timePages: a.timePages,
      lanesPerPage: a.lanesPerPage,
    },
    range: a.range,
  };
}
