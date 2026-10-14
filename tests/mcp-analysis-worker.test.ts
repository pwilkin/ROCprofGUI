import { afterAll, expect, it } from "vitest";
import { execFileSync, spawnSync } from "node:child_process";
import * as fs from "node:fs";
import {
  mkdtempSync,
  rmSync,
  writeFileSync,
  readFileSync,
  statSync,
  appendFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import protocol from "../src/mcpAnalysisProtocol.json";
import { boundedKernelAccumulator } from "../scripts/mcp-analysis-kernels";
import { goldenKernelStats } from "./fixtures/a1-golden";
import type { Capture } from "../src/types";
const root = mkdtempSync(join(tmpdir(), "a1-worker-")),
  bundle = join(root, "analysis.cjs");
execFileSync(process.execPath, ["scripts/build-mcp-analysis.mjs", bundle]);
afterAll(() => rmSync(root, { recursive: true }));
function manifest(folder: string, rows: number, intervals = 0) {
  const streams = [
    ["kernel-dispatch", "dispatches.ndjson", rows],
    ["kernel-interval", "intervals.ndjson", intervals],
  ].map(([kind, file, count]) => ({
    kind,
    file,
    rows: count,
    bytes: statSync(join(folder, String(file))).size,
  }));
  return {
    protocol: protocol.protocol,
    version: 2,
    budgetVersion: 2,
    analysis: "kernels",
    mode: "kernel-stream",
    capture: {
      id: "c",
      name: "synthetic",
      format: "CSV",
      architecture: "gfx1151",
      unit: "ns",
      origin: "0",
      start: -10,
      end: 527878,
    },
    args: {},
    streams,
  };
}
function run(folder: string, input: unknown, heap = 256) {
  writeFileSync(join(folder, "input.json"), JSON.stringify(input));
  return spawnSync(
    process.execPath,
    [`--max-old-space-size=${heap}`, bundle, join(folder, "input.json")],
    { encoding: "utf8", timeout: 120000 },
  );
}
it("bundled worker aggregates 527878 dispatches exactly with a test-only 256 MiB heap", () => {
  const folder = mkdtempSync(join(root, "large-"));
  const counts = [0, 0, 0];
  let buffer = "";
  writeFileSync(join(folder, "dispatches.ndjson"), "");
  writeFileSync(join(folder, "intervals.ndjson"), "");
  for (let i = 0; i < 527878; i++) {
    counts[i % 3]++;
    buffer +=
      JSON.stringify({ kernel: String(i % 3), start: i, end: i + 1 }) + "\n";
    if (buffer.length > 65536) {
      appendFileSync(join(folder, "dispatches.ndjson"), buffer);
      buffer = "";
    }
  }
  appendFileSync(join(folder, "dispatches.ndjson"), buffer);
  const result = run(folder, manifest(folder, 527878));
  expect(result.status, result.stderr).toBe(0);
  const receipt = JSON.parse(result.stdout);
  expect(receipt.rows).toBe(3);
  expect(receipt.bytes).toBe(statSync(join(folder, "result.ndjson")).size);
  const rows = readFileSync(join(folder, "result.ndjson"), "utf8")
    .trim()
    .split("\n")
    .map((l) => JSON.parse(l));
  expect(rows).toEqual(
    counts.map((count, name) => ({
      name: String(name),
      count,
      total: count,
      min: 1,
      max: 1,
      waves: 0,
      waveTime: 0,
      activeTime: 0,
    })),
  );
}, 120000);
it("kernel guard charges once per group and refuses before retaining state", () => {
  const { accumulator: a, stats } = boundedKernelAccumulator({
    groups: 2,
    names: 4,
    pairs: 3,
  });
  const sample = { kernel: "λλ", start: 0, end: 1 };
  a.dispatch(sample);
  a.dispatch(sample);
  expect(stats).toEqual({
    groups: 1,
    nameBytes: 4,
    pairs: 0,
    retainedDispatchRecords: 0,
  });
  expect(() => a.dispatch({ ...sample, kernel: "b" })).toThrow("name");
  expect(stats.groups).toBe(1);
  a.interval({ ...sample, kernel: "ignored" });
  expect(stats.pairs).toBe(0);
  for (let i = 0; i < 3; i++) a.interval(sample);
  expect(() => a.interval(sample)).toThrow("intervals");
  expect(stats.pairs).toBe(3);
  expect(a.finish()).toEqual(
    goldenKernelStats({
      dispatches: [sample, sample],
      intervals: [sample, sample, sample],
    } as unknown as Capture),
  );
  const g = boundedKernelAccumulator({ groups: 2, names: 100, pairs: 3 });
  for (const kernel of ["a", "b"])
    g.accumulator.dispatch({ ...sample, kernel });
  expect(() => g.accumulator.dispatch({ ...sample, kernel: "c" })).toThrow(
    "groups",
  );
  expect(g.stats.groups).toBe(2);
});
it("actual bundle rejects record/version/path/count defects without a success receipt", () => {
  for (const defect of [
    "version",
    "path",
    "count",
    "fields",
    "truncated",
    "nonfinite",
    "kind",
  ]) {
    const folder = mkdtempSync(join(root, "bad-"));
    let line = JSON.stringify({ kernel: "a", start: -1, end: 0 }) + "\n";
    if (defect === "fields")
      line = '{"kernel":"a","start":0,"end":1,"extra":true}\n';
    if (defect === "truncated") line = line.trimEnd();
    if (defect === "nonfinite") line = '{"kernel":"a","start":1e999,"end":1}\n';
    writeFileSync(join(folder, "dispatches.ndjson"), line);
    writeFileSync(join(folder, "intervals.ndjson"), "");
    const m = manifest(folder, defect === "count" ? 2 : 1);
    if (defect === "version") m.version = 99;
    if (defect === "path") m.streams[0].file = "../dispatches.ndjson";
    if (defect === "kind") m.streams[0].kind = "unknown";
    const result = run(folder, m);
    expect(result.status).not.toBe(0);
    expect(result.stdout).toBe("");
  }
});

it("maximal retained interval state succeeds with the unchanged production 1024 MiB setting", () => {
  const folder = mkdtempSync(join(root, "intervals-"));
  writeFileSync(
    join(folder, "dispatches.ndjson"),
    '{"kernel":"a","start":0,"end":1}\n',
  );
  writeFileSync(join(folder, "intervals.ndjson"), "");
  const line = '{"kernel":"a","start":0,"end":1}\n',
    block = line.repeat(4096);
  let count = 0;
  while (count + 4096 <= protocol.budgets.kernelIntervals) {
    appendFileSync(join(folder, "intervals.ndjson"), block);
    count += 4096;
  }
  appendFileSync(
    join(folder, "intervals.ndjson"),
    line.repeat(protocol.budgets.kernelIntervals - count),
  );
  const result = run(
    folder,
    manifest(folder, 1, protocol.budgets.kernelIntervals),
    1024,
  );
  expect(result.status, result.stderr).toBe(0);
  expect(
    JSON.parse(readFileSync(join(folder, "result.ndjson"), "utf8")),
  ).toEqual({
    name: "a",
    count: 1,
    total: 1,
    min: 1,
    max: 1,
    waves: 2000000,
    waveTime: 2000000,
    activeTime: 1,
  });
}, 120000);

it("bundled timeline subdivides full ranges, completes large file output and gives the inline remedy", () => {
  const folder = mkdtempSync(join(root, "timeline-")),
    count = 100001;
  let buffer = "";
  writeFileSync(join(folder, "timeline.ndjson"), "");
  for (let i = 0; i < count; i++) {
    buffer +=
      JSON.stringify({
        kind: "dispatch",
        index: i,
        record: {
          id: String(i),
          kernel: "a",
          start: i,
          end: i + 0.5,
          agent: "0",
          queue: "0",
          grid: "64",
          workgroup: "32",
        },
      }) + "\n";
    if (buffer.length > 65536) {
      appendFileSync(join(folder, "timeline.ndjson"), buffer);
      buffer = "";
    }
  }
  appendFileSync(join(folder, "timeline.ndjson"), buffer);
  const m = {
    protocol: protocol.protocol,
    version: 2,
    budgetVersion: 2,
    analysis: "timeline",
    mode: "timeline-stream",
    capture: {
      id: "c",
      name: "<synthetic>",
      format: "CSV",
      architecture: "gfx1151",
      unit: "ns",
      origin: "0",
      start: 0,
      end: count,
    },
    args: {
      timeline: "dispatch",
      range: [0, count],
      timePages: 1,
      lanesPerPage: 12,
      outputMode: "file",
      maxOutputBytes: protocol.budgets.fileBytes,
      maxGlyphs: protocol.budgets.fileGlyphs,
    },
    streams: [
      {
        kind: "timeline-record",
        file: "timeline.ndjson",
        rows: count,
        bytes: statSync(join(folder, "timeline.ndjson")).size,
      },
    ],
  };
  const result = run(folder, m);
  expect(result.status, result.stderr).toBe(0);
  const receipt = JSON.parse(result.stdout);
  expect(receipt.glyphs).toBe(count);
  expect(receipt.segments).toBeGreaterThan(1);
  expect(receipt.bytes).toBeGreaterThan(protocol.budgets.inlineBytes);
  expect(receipt.bytes).toBe(statSync(join(folder, "result.html")).size);
  // Drain the file in bounded reads and check every original identity once.
  const fd = fs.openSync(join(folder, "result.html"), "r"),
    chunk = Buffer.alloc(65536),
    identities = new Uint8Array(count);
  let carry = "",
    total = 0,
    tail = "";
  for (;;) {
    const n = fs.readSync(fd, chunk, 0, chunk.length, null);
    if (!n) break;
    const text = carry + chunk.subarray(0, n).toString("utf8");
    const safe = text.length - 64;
    for (const match of text.matchAll(/data-key="dispatch:(\d+)"/g)) {
      if (match.index! >= safe) continue;
      identities[Number(match[1])]++;
      total++;
    }
    carry = text.slice(Math.max(0, safe));
    tail = text.slice(-14);
  }
  for (const match of carry.matchAll(/data-key="dispatch:(\d+)"/g)) {
    identities[Number(match[1])]++;
    total++;
  }
  fs.closeSync(fd);
  expect(total).toBe(count);
  expect(identities.every((n) => n === 1)).toBe(true);
  expect(tail).toBe("</body></html>");
  rmSync(join(folder, "result.html"));
  m.args.outputMode = "inline";
  m.args.maxOutputBytes = protocol.budgets.inlineBytes;
  m.args.maxGlyphs = protocol.budgets.inlineGlyphs;
  const inline = run(folder, m);
  expect(inline.status).not.toBe(0);
  expect(inline.stdout).toBe("");
  expect(inline.stderr).toContain("set output to a new HTML path");
}, 120000);
