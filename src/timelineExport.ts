import type { Mark } from "./components/Plots";
import type { Range } from "./types";
import { timelineGlyphs } from "./timelineDetail";
import { timelineOverlays } from "./timelineOverlays";
import { isWaveDetailBudgetError } from "./waveOverview";
import { isTimelineWindowBudgetError, drawnRecordBudgetError } from "./timelineExportBudget";
import protocol from "./mcpAnalysisProtocol.json";
export const timelineExportMaxima = protocol.budgets;
export interface TimelineExportStats { bytes: number; pages: number; segments: number; glyphs: number; budgets: { maxBytes: number; maxGlyphs: number; chunkBytes: number; pages: number; segments: number } }
const escape = (value: unknown) =>
  String(value).replace(
    /[&<>"']/g,
    (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        c
      ]!,
  );
export function exportWindows(bounds: Range, count: number): Range[] {
  if (
    !bounds.every(Number.isFinite) ||
    !Number.isFinite(bounds[1] - bounds[0]) ||
    bounds[1] <= bounds[0] ||
    !Number.isInteger(count) ||
    count < 1 ||
    count > 64
  )
    throw Error("Choose 1–64 time pages and a finite positive range");
  const windows = Array.from(
    { length: count },
    (_, i) =>
      [
        bounds[0] + ((bounds[1] - bounds[0]) * i) / count,
        i + 1 === count
          ? bounds[1]
          : bounds[0] + ((bounds[1] - bounds[0]) * (i + 1)) / count,
      ] as Range,
  );
  if (windows.some(([start, end]) => end <= start))
    throw Error("Time segments exceed timestamp precision; reduce time pages");
  return windows;
}
export async function timelineDocument(options: {
  label: string;
  unit: string;
  bounds: Range;
  timePages: number;
  lanesPerPage: number;
  stacked: boolean;
  load: (range: Range, signal: AbortSignal) => Promise<Mark[]>;
  signal: AbortSignal;
  progress?: (message: string) => void;
  writeChunk?: (text: string) => Promise<void>;
  limits?: { maxBytes?: number; maxGlyphs?: number };
  onComplete?: (stats: TimelineExportStats) => void;
  contextNote?: string;
}): Promise<Blob> {
  const {
    label,
    unit,
    bounds,
    timePages,
    lanesPerPage,
    stacked,
    load,
    signal,
    progress,
    writeChunk,
  } = options;
  if (!Number.isInteger(lanesPerPage) || lanesPerPage < 1 || lanesPerPage > 24)
    throw Error("Choose 1–24 lanes per page");
  const chunks: string[] = [];
  const encoder = new TextEncoder();
  let bufferedBytes = 0;
  const maxima = protocol.budgets;
  const byteCeiling = writeChunk ? maxima.fileBytes : maxima.inlineBytes;
  const glyphCeiling = writeChunk ? maxima.fileGlyphs : maxima.inlineGlyphs;
  const maxBytes = options.limits?.maxBytes ?? byteCeiling;
  const maxGlyphs = options.limits?.maxGlyphs ?? glyphCeiling;
  const byteDescription = options.limits?.maxBytes === undefined ? `${writeChunk ? 512 : 16} MiB` : `${maxBytes} bytes`;
  if (!Number.isSafeInteger(maxBytes) || maxBytes < 1 || maxBytes > byteCeiling || !Number.isSafeInteger(maxGlyphs) || maxGlyphs < 1 || maxGlyphs > glyphCeiling) throw Error("Timeline limits may only lower the output mode maxima");
  const flush = async () => {
    if (writeChunk && chunks.length) {
      check();
      await writeChunk(chunks.join(""));
      chunks.length = 0;
      bufferedBytes = 0;
      check();
    }
  };
  let bytes = 0,
    pages = 0,
    glyphCount = 0;
  const append = (text: string) => {
    const size = encoder.encode(text).length;
    if (bytes + size > maxBytes) throw Error(`Timeline export exceeds ${byteDescription}; export a smaller time range`);
    bytes += size;
    bufferedBytes += size;
    if (writeChunk && bufferedBytes > 1024 * 1024) throw Error("Timeline export chunk exceeds 1 MiB; shorten event labels");
    if (bytes > maxBytes)
      throw Error(
        `Timeline export exceeds ${byteDescription}; export a smaller time range`,
      );
    chunks.push(text);
  };
  const check = () => {
    if (signal.aborted) throw Error("Timeline export cancelled");
  };
  append(
    `<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'"><title>${escape(label)}</title><style>@page{size:A4 landscape;margin:10mm}body{font:12px sans-serif;color:#18212f;background:white;margin:20px}section{break-after:page;page-break-after:always;margin-bottom:24px}section:last-child{break-after:auto}h1{font-size:18px}svg{width:100%;height:auto}p{overflow-wrap:anywhere}@media print{body{margin:0}section{margin:0}svg{max-height:150mm}h1{font-size:16px}}</style></head><body>`,
  );
  const windows = exportWindows(bounds, timePages);
  for (let t = 0; t < windows.length; t++) {
    check();
    progress?.(`Loading time segment ${t + 1}/${windows.length}`);
    const window = windows[t];
    const split = () => {
      const mid = window[0] + (window[1] - window[0]) / 2;
      if (windows.length >= 256 || mid <= window[0] || mid >= window[1]) throw Error("Exact timeline export cannot subdivide further; select a smaller range");
      windows.splice(t, 1, [window[0], mid], [mid, window[1]]);
      t--;
    };
    let input: Mark[];
    try { input = await load(window, signal); }
    catch (error) {
      check();
      if (writeChunk && (isWaveDetailBudgetError(error) || isTimelineWindowBudgetError(error))) { split(); continue; }
      throw error;
    }
    check();
    if (
      input.some(
        (mark) =>
          !Number.isFinite(mark.start) ||
          !Number.isFinite(mark.end) ||
          mark.end < mark.start,
      )
    )
      throw Error("Timeline contains invalid event timestamps");
    const marks = input.filter(
      (m) => m.end >= window[0] && m.start <= window[1],
    );
    if (marks.some(mark => mark.aggregateCount !== undefined)) throw Error("Exact timeline export requires event records, not overview groups");
    if (marks.length > 500000 && writeChunk) { split(); continue; }
    if (marks.length > 500000)
      throw Error(
        "Timeline export exceeds 500,000 records in one time segment; increase time pages",
      );
    const lanes = [...new Set(marks.map((m) => m.lane))];
    for (let row = 0; row < Math.max(1, lanes.length); row += lanesPerPage) {
      check();
      if (++pages > 512)
        throw Error(
          "Timeline export exceeds 512 pages; reduce the range or increase lanes per page",
        );
      const group = lanes.slice(row, row + lanesPerPage),
        height = 42 + Math.max(1, group.length) * 28;
      let glyphs, overlays;
      try {
        glyphs = timelineGlyphs(marks, group, window, 1100, 28, stacked, 0, undefined, maxGlyphs - glyphCount);
        overlays = timelineOverlays(marks, group, window, 1100, 28, maxGlyphs - glyphCount - glyphs.length);
      } catch (error) {
        if (error instanceof Error && error.message.includes("drawn records")) throw drawnRecordBudgetError(maxGlyphs);
        throw error;
      }
      glyphCount += glyphs.length + overlays.length;
      append(
        `<section data-page="${pages}" data-start="${window[0]}" data-end="${window[1]}"><h1>${escape(label)}</h1><p>Page ${pages} · time segment ${t + 1}${writeChunk ? "" : `/${timePages}`} · ${window[0]}–${window[1]} ${escape(unit)} · lanes ${lanes.length ? row + 1 : 0}–${Math.min(row + lanesPerPage, lanes.length)} of ${lanes.length} · exact geometry, no aggregation · ${stacked ? "stacked" : "flat"}</p><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1100 ${height}" role="img" aria-label="Timeline page ${pages}">`,
      );
      for (const [i, lane] of group.entries()) {
        append(
          `<rect x="0" y="${27 + i * 28}" width="1100" height="28" fill="${i % 2 ? "#f0f3f6" : "#ffffff"}"/><text x="4" y="${45 + i * 28}" font-size="10" fill="#18212f"${lane.length > 24 ? ' textLength="145" lengthAdjust="spacingAndGlyphs"' : ""}><title>${escape(lane)}</title>${escape(lane)}</text>`,
        );
        if (writeChunk) await flush();
      }
      for (let tick = 0; tick <= 5; tick++) {
        const x = 155 + (tick * 931) / 5;
        append(
          `<line x1="${x}" x2="${x}" y1="25" y2="${height}" stroke="#dce1e8"/><text x="${Math.min(x, 1020)}" y="16" font-size="10">${escape(Number((window[0] + ((window[1] - window[0]) * tick) / 5).toPrecision(8)))}</text>`,
        );
      }
      await flush();
      for (const array of [glyphs, overlays]) for (const { mark, rect } of array) {
        const color = /^#[0-9a-f]{3,8}$/i.test(mark.color)
          ? mark.color
          : "#538aad";
        append(
          `<rect data-key="${escape(mark.key)}" x="${rect.x}" y="${rect.y}" width="${rect.width}" height="${rect.height}" fill="${color}"><title>${escape(mark.label)} · ${mark.start}–${mark.end} ${escape(unit)}</title></rect>`,
        );
        if (writeChunk && bufferedBytes >= 512 * 1024) await flush();
      }
      append(
        "</svg><p>Export covers this timeline’s selected hardware scope and filters. It does not establish unrecorded trace coverage. Events crossing page boundaries may appear on adjacent pages.</p></section>",
      );
      await flush();
      progress?.(`Rendered ${pages} pages`);
      await new Promise((resolve) => setTimeout(resolve, 0));
    }
  }
  check();
  if (options.contextNote) append(`<p>${escape(options.contextNote)}</p>`);
  append("</body></html>");
  await flush();
  options.onComplete?.({ bytes, pages, segments: windows.length, glyphs: glyphCount, budgets: { maxBytes, maxGlyphs, chunkBytes: maxima.chunkBytes, pages: maxima.pages, segments: maxima.segments } });
  return new Blob(chunks, { type: "text/html;charset=utf-8" });
}
