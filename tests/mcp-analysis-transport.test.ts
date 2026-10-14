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
