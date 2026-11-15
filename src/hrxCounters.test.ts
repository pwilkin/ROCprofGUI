import { expect, it } from "vitest";
import { diagnosticManifest } from "./diagnostics";
import { hrxCounterKind } from "./liveProfileHrx";
import { scalarDefinition, scalarSeries } from "./scalarCounters";
import { initialTabState } from "./session";
import type { Capture } from "./types";
// The Rust importer's output for fixtures/irpf/synthetic-hrx-counters.irpf; a
// Rust test (frontend_counter_capture_fixture_is_the_importer_output) keeps it
// equal to what the importer produces.
import stored from "../tests/fixtures/hrx-counters-capture.json";

const capture = stored as unknown as Capture;

it("joins every hrx counter row to one dispatch of the capture", () => {
  const dispatches = new Map(capture.dispatches.map((d) => [d.id, d]));
  const rows = capture.eventCounters ?? [];
  expect(rows).toHaveLength(10);
  for (const row of rows) {
    expect(row.dispatches).toHaveLength(1);
    const dispatch = dispatches.get(row.dispatches[0])!;
    // The row names the kernel and geometry it was measured around.
    expect(row.scope).toContain(`${dispatch.kernel} · grid ${dispatch.grid} · workgroup ${dispatch.workgroup}`);
    expect(row.unit).toBe(hrxCounterKind(row.name) === "cycles" ? "cycles" : "count");
    // A cycle counter says in its scope that it is not isolated to the dispatch.
    expect(row.scope.includes("hardware-wide during the window")).toBe(row.unit === "cycles");
  }
  // Wave64 kernel: SQG_WAVES is workgroups × ceil(64 / 64).
  expect(rows.filter((r) => r.name === "SQG_WAVES").map((r) => r.value)).toEqual([4, 8]);
  // The third dispatch was past the sample bound and has no row.
  expect(new Set(rows.map((r) => r.dispatches[0])).size).toBe(2);
});

it("evaluates a scalar definition over the per-dispatch counter series", () => {
  const names = capture.counters.map((s) => s.name);
  expect(names).toEqual(["SQG_WAVES", "SQG_WAVES_32", "SQG_WAVES_64", "SQG_ITEMS", "SQG_BUSY_CYCLES"]);
  // What capture_analyze analysis=scalar runs: work items per wave.
  const formula = `C${names.indexOf("SQG_ITEMS")} / C${names.indexOf("SQG_WAVES")}`;
  const series = scalarSeries(scalarDefinition("items per wave", formula, capture.counters), capture.counters);
  // One point per sampled dispatch, at the dispatch midpoint.
  expect(series.points).toEqual([
    [1000, 64],
    [6000, 64],
  ]);
});

it("lists the counters and their dispatch links in the diagnostics inventory", async () => {
  const manifest = (await diagnosticManifest({
    capture,
    generatedAt: "2026-11-15T00:00:00Z",
    runtime: {},
    workspace: initialTabState(capture),
    sources: {},
    definitions: { version: 1, selected: "", files: [] },
    hiddenPending: false,
    wave: null,
  })) as any;
  expect(manifest.inventory.sampledCounters.map((c: any) => [c.name, c.unit, c.sampleCount])).toEqual([
    ["SQG_WAVES", "count", 2],
    ["SQG_WAVES_32", "count", 2],
    ["SQG_WAVES_64", "count", 2],
    ["SQG_ITEMS", "count", 2],
    ["SQG_BUSY_CYCLES", "cycles", 2],
  ]);
  expect(manifest.inventory.eventCounters).toEqual({
    records: 10,
    definitions: 5,
    missingValues: 0,
    ambiguousDispatchAssociations: 0,
    withoutDispatch: 0,
  });
  expect(manifest.capture.metadata.hrxCounterSampledDispatches).toBe("2 of 3");
  expect(manifest.capture.warnings.join(" ")).toContain("are window sums");
});
