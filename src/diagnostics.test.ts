import { expect, it } from "vitest";
import { diagnosticManifest, diagnosticBytes } from "./diagnostics";
import { initialTabState } from "./session";
import demo from "../public/demo.json";
import type { Capture } from "./types";
it("diagnostics preserve zero/missing distinctions, clock origins and hashed source replacements", async () => {
  const capture = structuredClone(demo.capture) as unknown as Capture;
  capture.origin = "9223372036854000000";
  capture.pcSamplingObservations = { columns: ["Sample_Timestamp", "Instruction"], rows: [["9223372036854000000", "private instruction text"]] };
  capture.dispatches = [
    { ...capture.dispatches[0], vgpr: 0, lds: null, scratch: null, sgpr: 12 },
  ];
  capture.instructions = [
    {
      ...capture.instructions[0],
      pcIssued: 0,
      pcStalled: null,
      stallReasons: null,
    },
  ];
  capture.eventCounters = [
    {
      id: "zero",
      counterId: "c1",
      event: "e",
      dispatches: ["a", "b"],
      name: "counter",
      unit: "count",
      scope: "s",
      value: 0,
    },
    {
      id: "missing",
      counterId: "c1",
      event: null,
      dispatches: [],
      name: "counter",
      unit: "count",
      scope: "s",
      value: null,
    },
  ];
  const report = await diagnosticManifest({
    capture,
    workspace: initialTabState(capture),
    sources: {
      "kernel.hip": { name: "replacement.hip", text: "abc" },
      "large.hip": {
        name: "large.hip",
        text: "",
        reference: {
          path: "/local/large.hip",
          size: 3000000,
          sha256: "a".repeat(64),
        },
      },
    },
    definitions: {
      version: 1,
      selected: "one",
      files: [{ id: "one", name: "test.def", text: "X := 1" }],
    },
    hidden: {
      byInstruction: {},
      byWave: {},
      model: "test",
      warnings: ["CU assumption"],
    },
    hiddenPending: false,
    wave: null,
    runtime: { environment: "test" },
    generatedAt: "2026-01-01T00:00:00.000Z",
  });
  expect(report.inventory.pcSamplingObservations).toEqual({ records: 1, columns: ["Sample_Timestamp", "Instruction"], attribution: "CSV observations without resolved PC/code-object identities" });
  expect(JSON.stringify(report.inventory)).not.toContain("private instruction text");
  expect(report.analysis.hidden.warnings).toEqual(["CU assumption"]);
  expect(report.capture.clock.origin).toBe(capture.origin);
  expect(report.measurementAvailability.dispatchResources.vgpr).toEqual({
    known: 1,
    missing: 0,
  });
  expect(report.measurementAvailability.dispatchResources.lds).toEqual({
    known: 0,
    missing: 1,
  });
  expect(report.measurementAvailability.pcIssuedRows).toBe(1);
  expect(report.measurementAvailability.pcStalledRows).toBe(0);
  expect(report.inventory.eventCounters).toEqual({
    records: 2,
    definitions: 1,
    missingValues: 1,
    ambiguousDispatchAssociations: 1,
    withoutDispatch: 1,
  });
  expect(report.sourceReplacements[0].sha256).toBe(
    "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
  );
  expect(report.sourceReplacements[0]).not.toHaveProperty("text");
  expect(report.sourceReplacements[1]).toMatchObject({
    referencePath: "/local/large.hip",
    bytes: 3000000,
    sha256: "a".repeat(64),
    lines: null,
    revisionVerified: false,
    hashStatus:
      "saved reference fingerprint; disk state not checked by this export",
  });
  expect(report.sourceReplacements[0].revisionVerified).toBe(false);
  expect(report.qualification.replayCoverageVerified).toBe(false);
  expect(report.definitions.files[0].text).toBe("X := 1");
  expect(JSON.parse(new TextDecoder().decode(diagnosticBytes(report)))).toEqual(
    report,
  );
});
it("diagnostic byte budget accounts for UTF-8 rather than character count", () => {
  expect(() => diagnosticBytes({ text: "Ω".repeat(9 * 1024 * 1024) })).toThrow(
    "16 MiB",
  );
});
