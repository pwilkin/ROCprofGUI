import { expect, it } from "vitest";
import { exportWindows, timelineDocument } from "./timelineExport";
import type { Mark } from "./components/Plots";
const mark = (key: string, lane: string, start: number, end: number): Mark => ({
  key,
  lane,
  start,
  end,
  color: "#aabbcc",
  label: `event ${key}`,
});
const defaults = () => ({
  label: "Trace",
  unit: "cycles",
  bounds: [0, 100] as [number, number],
  timePages: 2,
  lanesPerPage: 1,
  stacked: true,
  signal: new AbortController().signal,
});
it("exports every time window and lane with escaped event identities", async () => {
  const windows: number[][] = [];
  const marks = [
    mark("early", "A", 1, 10),
    mark("cross", "B", 45, 55),
    mark("late<script>", "C<&", 90, 99),
  ];
  const blob = await timelineDocument({
    ...defaults(),
    label: '<img src=x onerror="bad()">',
    load: async (range) => {
      windows.push(range);
      return marks;
    },
  });
  const text = await blob.text();
  expect(windows).toEqual([
    [0, 50],
    [50, 100],
  ]);
  expect(text.match(/<section /g)).toHaveLength(4);
  for (const key of ["early", "cross", "late&lt;script&gt;"])
    expect(text).toContain(`data-key="${key}"`);
  expect(text).not.toContain("<img");
  expect(text).toContain("C&lt;&amp;");
  expect(text).toContain("Content-Security-Policy");
});
it("cancels after a pending segment without writing partial output", async () => {
  const controller = new AbortController();
  let calls = 0;
  await expect(
    timelineDocument({
      ...defaults(),
      signal: controller.signal,
      load: async () => {
        calls++;
        controller.abort();
        return [];
      },
    }),
  ).rejects.toThrow("cancelled");
  expect(calls).toBe(1);
});
it("rejects invalid bounds, timestamps and oversized document content", async () => {
  expect(() => exportWindows([0, 10], 0)).toThrow();
  expect(() => exportWindows([-1e308, 1e308], 1)).toThrow();
  await expect(
    timelineDocument({
      ...defaults(),
      load: async () => [mark("bad", "A", NaN, 1)],
    }),
  ).rejects.toThrow("timestamps");
  await expect(
    timelineDocument({
      ...defaults(),
      label: "x".repeat(16 * 1024 * 1024),
      load: async () => [],
    }),
  ).rejects.toThrow("16 MiB");
});

it("enforces segment record and document page limits", async () => {
  await expect(
    timelineDocument({
      ...defaults(),
      timePages: 1,
      load: async () => Array(500001).fill(mark("x", "A", 1, 2)),
    }),
  ).rejects.toThrow("500,000");
  await expect(
    timelineDocument({
      ...defaults(),
      timePages: 1,
      load: async () =>
        Array.from({ length: 513 }, (_, i) => mark(String(i), String(i), 1, 2)),
    }),
  ).rejects.toThrow("512 pages");
  await expect(
    timelineDocument({
      ...defaults(),
      timePages: 1,
      load: async () => Array(100001).fill(mark("dense", "A", 1, 2)),
    }),
  ).rejects.toThrow("100,000 drawn records");
  expect(() => exportWindows([1, 1 + Number.EPSILON], 64)).toThrow("precision");
});

it("streams exact pages and subdivides dense queries without using overview groups", async () => {
  const loaded: number[][] = [], output: string[] = [];
  await timelineDocument({ ...defaults(), timePages: 1, load: async range => {
    loaded.push(range);
    if (range[1]-range[0] > 50) throw Error("Visible wave query exceeds 500,000 tokens");
    return [mark("cross", "A", 45, 55)];
  }, writeChunk: async text => { output.push(text); } });
  expect(loaded).toEqual([[0,100],[0,50],[50,100]]);
  expect(output.join("").match(/data-key="cross"/g)).toHaveLength(2);
  expect(output.join("")).toMatch(/<\/body><\/html>$/);
  expect(output.every(text => new TextEncoder().encode(text).length <= 1024*1024)).toBe(true);
  await expect(timelineDocument({...defaults(), load: async () => [{...mark("group", "A", 0, 100), aggregateCount: 7}]})).rejects.toThrow("overview groups");
});
it("propagates sink failure and stops loading further segments", async () => {
  let loads = 0;
  await expect(timelineDocument({...defaults(), load: async () => {loads++;return [mark("x","A",1,2)];}, writeChunk: async () => {throw Error("disk full");}})).rejects.toThrow("disk full");
  expect(loads).toBe(1);
});

it("aborts a pending segment loader without emitting output or loading another segment", async () => {
  const controller = new AbortController();
  let entered!: () => void;
  const ready = new Promise<void>(resolve => { entered = resolve; });
  const output: string[] = [];
  let calls = 0;
  const pending = timelineDocument({
    ...defaults(), signal: controller.signal,
    writeChunk: async text => { output.push(text); },
    load: async (_range, signal) => {
      calls++;
      expect(signal).toBe(controller.signal);
      entered();
      return new Promise<Mark[]>((_resolve, reject) => {
        signal.addEventListener("abort", () => reject(Error("Wave query cancelled")), {once: true});
      });
    },
  });
  const failed = expect(pending).rejects.toThrow("Timeline export cancelled");
  await ready;
  controller.abort();
  await failed;
  expect(calls).toBe(1);
  expect(output).toEqual([]);
});

it("subdivides the native wave-count budget and preserves every exported identity", async () => {
  const records = Array.from({length: 300}, (_, i) => mark(`wave-${i}:0`, "CU 0", i + .2, i + .8));
  const loaded: number[][] = [], output: string[] = [];
  await timelineDocument({...defaults(), bounds:[0,300], timePages:1,
    load: async range => {
      loaded.push(range);
      const visible = records.filter(m => m.end >= range[0] && m.start <= range[1]);
      if (visible.length > 128) throw Error("This window contains more than 128 waves. Narrow the range; no waves were silently dropped.");
      return visible;
    }, writeChunk: async text => { output.push(text); },
  });
  expect(loaded).toEqual([[0,300],[0,150],[0,75],[75,150],[150,300],[150,225],[225,300]]);
  const html = output.join("");
  expect([...html.matchAll(/data-key="([^"]+)"/g)].map(m => m[1])).toEqual(records.map(m => m.key));
  expect(html.match(/<section /g)).toHaveLength(4);
  expect(html).toMatch(/<\/body><\/html>$/);
});
it("bounds subdivision when concurrent waves never fit and does not publish a footer", async () => {
  const output: string[] = [];
  let loads = 0;
  await expect(timelineDocument({...defaults(), timePages:1,
    load: async () => { loads++; throw Error("Wave query exceeds 128 waves"); },
    writeChunk: async text => { output.push(text); },
  })).rejects.toThrow("cannot subdivide further");
  expect(loads).toBe(256);
  expect(output).toEqual([]);
});
it("does not subdivide unrelated query failures", async () => {
  for (const message of ["Malformed instruction token", "Wave query exceeds 4096 waves"]) {
    let loads = 0;
    await expect(timelineDocument({...defaults(),timePages:1,
      load:async () => { loads++; throw Error(message); }, writeChunk:async () => {},
    })).rejects.toThrow(message);
    expect(loads).toBe(1);
  }
});

it("bisects typed window budgets while assigning boundary points once", async () => {
  const { TimelineWindowBudgetError } = await import("./timelineExportBudget");
  const { overlaps } = await import("../scripts/mcp-analysis-transport");
  const marks = [0, 5, 10].map((start, i) => ({ start, end: start, lane: "events", color: "#abc", label: "point", key: `point:${i}` }));
  let html = "", complete: any;
  await timelineDocument({ label: "points", unit: "ns", bounds: [0, 10], timePages: 1, lanesPerPage: 12, stacked: true, signal: new AbortController().signal,
    load: async range => { if (range[1] - range[0] > 5) throw new TimelineWindowBudgetError("density"); return marks.filter(m => overlaps(m.start, m.end, range)); },
    writeChunk: async text => { html += text; }, onComplete: stats => { complete = stats; },
  });
  expect(html.match(/data-key="point:0"/g)).toHaveLength(1);
  expect(html.match(/data-key="point:1"/g)).toHaveLength(1);
  expect(html).not.toContain('data-key="point:2"'); expect(complete.segments).toBe(2);
});
it("streams more than 16 MiB with bounded chunks and refuses lowered or inline budgets", async () => {
  const marks = Array.from({ length: 10000 }, (_, i) => ({ start: i, end: i + 0.5, lane: "Q0", color: "#abc", label: "λ".repeat(900), key: `d:${i}` }));
  let bytes = 0, maximum = 0, tail = "", complete: any;
  const options = { label: "large", unit: "ns", bounds: [0, 10000] as [number, number], timePages: 1, lanesPerPage: 12, stacked: true, signal: new AbortController().signal, load: async () => marks };
  await timelineDocument({ ...options, writeChunk: async text => { const n = new TextEncoder().encode(text).length; bytes += n; maximum = Math.max(maximum, n); tail = text.slice(-14); }, onComplete: stats => { complete = stats; } });
  expect(bytes).toBeGreaterThan(16 * 1024 * 1024); expect(maximum).toBeLessThanOrEqual(1024 * 1024); expect(tail).toBe("</body></html>"); expect(complete.bytes).toBe(bytes); expect(complete.glyphs).toBe(10000);
  await expect(timelineDocument({ ...options, limits: { maxBytes: 16 * 1024 * 1024 }, writeChunk: async () => {} })).rejects.toThrow("16777216 bytes");
  let accepted = 0;
  await expect(timelineDocument({ ...options, limits: { maxBytes: 1000 }, writeChunk: async text => { accepted += text.length; } })).rejects.toThrow("1000 bytes"); expect(accepted).toBeLessThanOrEqual(1000);
});
it("glyph admission bounds overlay expansion and selected/bucket insertion", async () => {
  const { timelineGlyphs } = await import("./timelineDetail"), { timelineOverlays } = await import("./timelineOverlays");
  const lanes = Array.from({ length: 24 }, (_, i) => `SE${i % 2}-${i}`);
  const marks = lanes.map((lane, i) => ({ start: 0, end: 5, lane, color: "#abc", label: "r", key: lane, recordKind: "residency" as const, shaderEngine: i % 2 }));
  const event = { start: 2, end: 2, lane: "event", color: "#abc", label: "e", key: "event", recordKind: "event" as const, shaderEngine: 0, overlayWidth: 5 };
  expect(timelineOverlays([...marks, event], lanes, [0, 5], 1100, 28, 12)).toHaveLength(12);
  expect(() => timelineOverlays([...marks, event, event], lanes, [0, 5], 1100, 28, 12)).toThrow("drawn records");
  for (const detail of [0, 2]) expect(() => timelineGlyphs(marks.map((m, i) => ({ ...m, selected: i % 2 === 0 })), lanes, [0, 5], 1100, 28, true, detail, undefined, 3)).toThrow("drawn records");
});
