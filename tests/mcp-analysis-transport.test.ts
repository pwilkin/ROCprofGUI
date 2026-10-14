import { afterEach, expect, it } from "vitest";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { readRecords, kernelSample, timelineRecord, overlaps, validateManifest } from "../scripts/mcp-analysis-transport";
import protocol from "../src/mcpAnalysisProtocol.json";
const dirs: string[] = [];
function directory() { const d = mkdtempSync(join(tmpdir(), "a1-transport-")); dirs.push(d); return d; }
afterEach(() => { for (const d of dirs.splice(0)) rmSync(d, { recursive: true }); });
it("validates bounded lines across UTF-8 and 64 KiB read boundaries", () => {
  const d = directory(); const sample = { kernel: "λ".repeat(40000), start: -5, end: -1 };
  const text = JSON.stringify(sample) + "\n"; writeFileSync(join(d, "dispatches.ndjson"), text);
  const descriptor = { kind: "kernel-dispatch", file: "dispatches.ndjson", rows: 1, bytes: Buffer.byteLength(text) };
  expect([...readRecords(d, descriptor)].map(kernelSample)).toEqual([sample]);
  expect(() => [...readRecords(d, { ...descriptor, rows: 2 })]).toThrow("count/byte mismatch");
  expect(() => [...readRecords(d, { ...descriptor, bytes: 1 })]).toThrow("byte count mismatch");
  writeFileSync(join(d, descriptor.file), text.trimEnd());
  expect(() => [...readRecords(d, { ...descriptor, bytes: Buffer.byteLength(text) - 1 })]).toThrow("trailing newline");
  const large = "x".repeat(protocol.budgets.recordBytes) + "\n";
  writeFileSync(join(d, descriptor.file), large);
  expect(() => [...readRecords(d, { ...descriptor, bytes: large.length })]).toThrow("RECORD_BUDGET");
});
it("rejects malformed records and protocol fields without subdivision", () => {
  for (const v of [{ kernel: "a", start: NaN, end: 1 }, { kernel: "a", start: null, end: 1 }, { kernel: "a", start: 0, end: 1, extra: true }]) expect(() => kernelSample(v)).toThrow();
  for (const v of [{ kind: "dispatch", index: -1, record: {} }, { kind: "unknown", index: 0, record: {} }]) expect(() => timelineRecord(v)).toThrow();
  expect(() => validateManifest({ protocol: protocol.protocol, version: 99 })).toThrow("rebuild");
  expect(overlaps(0, 0, [0, 5])).toBe(true);
  expect(overlaps(5, 5, [0, 5])).toBe(false);
  expect(overlaps(2, 8, [0, 5])).toBe(true);
  expect(overlaps(2, 8, [5, 10])).toBe(true);
});

it("timeline loading bounds retention, preserves ordinals and never splits malformed data", async () => {
  const { loadTimelineWindow } = await import("../scripts/mcp-analysis-timeline");
  const { TimelineWindowBudgetError } = await import("../src/timelineExportBudget");
  const d = directory();
  const record = (index: number, start: number) => ({ kind: "dispatch", index, record: { id: String(index), kernel: "λ", start, end: start + 0.5, agent: "0", queue: "1", grid: "64", workgroup: "32" } });
  const records = [record(2, -1), record(17, 5), record(100, 6)];
  const text = records.map(r => JSON.stringify(r) + "\n").join(""); writeFileSync(join(d, "timeline.ndjson"), text);
  const m = validateManifest({ protocol: protocol.protocol, version: 2, budgetVersion: 2, analysis: "timeline", mode: "timeline-stream", capture: { id: "c", name: "c", format: "CSV", architecture: "gfx1151", unit: "ns", origin: "0", start: -2, end: 10 }, args: { timeline: "dispatch", range: [-2, 10], timePages: 1, lanesPerPage: 12, outputMode: "inline", maxOutputBytes: protocol.budgets.inlineBytes, maxGlyphs: protocol.budgets.inlineGlyphs }, streams: [{ kind: "timeline-record", file: "timeline.ndjson", rows: 3, bytes: Buffer.byteLength(text) }] });
  const stats = { marks: 0, bytes: 0 };
  const marks = loadTimelineWindow(d, m, [5, 7], { marks: 2, bytes: 10000 }, stats);
  expect(marks.map(m => m.key)).toEqual(["dispatch:17", "dispatch:100"]); expect(stats.marks).toBe(2);
  expect(() => loadTimelineWindow(d, m, [-2, 10], { marks: 2, bytes: 10000 }, stats)).toThrow(TimelineWindowBudgetError); expect(stats.marks).toBe(2);
  expect(() => loadTimelineWindow(d, m, [-2, 10], { marks: 2, bytes: 5 })).toThrow("single generated mark");
  records[0].index = -1; const invalid = records.map(r => JSON.stringify(r) + "\n").join(""); writeFileSync(join(d, "timeline.ndjson"), invalid); m.streams![0].bytes = Buffer.byteLength(invalid);
  expect(() => loadTimelineWindow(d, m, [5, 7])).toThrow("nonnegative");
});
