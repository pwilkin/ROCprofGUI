import { version } from "../package.json";
import type { Capture, HiddenAnalysis, Wave } from "./types";
import type { TabState } from "./session";
import type { SourceMappings } from "./sourceFiles";
import type { DefinitionLibrary } from "./definitionLibrary";
function count<T>(rows: T[], predicate: (row: T) => boolean): number {
  return rows.reduce((n, row) => n + Number(predicate(row)), 0);
}
export async function diagnosticManifest(input: {
  capture: Capture;
  workspace: TabState;
  sources: SourceMappings;
  definitions: DefinitionLibrary;
  hidden?: HiddenAnalysis;
  hiddenPending: boolean;
  wave: Wave | null;
  runtime: unknown;
  generatedAt?: string;
}) {
  const { capture: c } = input;
  const sourceReplacements = [];
  for (const [path, file] of Object.entries(input.sources)) {
    if (file.reference) {
      sourceReplacements.push({
        path,
        name: file.name,
        referencePath: file.reference.path,
        bytes: file.reference.size,
        lines: null,
        sha256: file.reference.sha256,
        hashStatus:
          "saved reference fingerprint; disk state not checked by this export",
        revisionVerified: false,
      });
      continue;
    }
    const bytes = new TextEncoder().encode(file.text);
    const digest = globalThis.crypto?.subtle
      ? await crypto.subtle.digest("SHA-256", bytes)
      : null;
    sourceReplacements.push({
      path,
      name: file.name,
      bytes: bytes.length,
      lines: file.text.split("\n").length,
      sha256: digest
        ? Array.from(new Uint8Array(digest), (b) =>
            b.toString(16).padStart(2, "0"),
          ).join("")
        : null,
      hashStatus: digest ? "computed from replacement text" : "unavailable",
      revisionVerified: false,
    });
  }
  const dispatchMeasurements = Object.fromEntries(
    (["vgpr", "sgpr", "lds", "scratch"] as const).map((field) => [
      field,
      {
        known: count(c.dispatches, (d) => Number.isFinite(d[field])),
        missing: count(c.dispatches, (d) => !Number.isFinite(d[field])),
      },
    ]),
  );
  const counters = c.counters.map((counter, index) => ({
    index,
    name: counter.name,
    scope: counter.scope,
    unit: counter.unit,
    sampleCount: counter.points.length,
    firstTimestamp: counter.points[0]?.[0] ?? null,
    lastTimestamp: counter.points.at(-1)?.[0] ?? null,
  }));
  const events = c.eventCounters || [];
  const markers = c.markers || [];
  const counterIdentities = new Set<string>();
  for (const event of events) counterIdentities.add(event.counterId);
  return {
    schemaVersion: 1,
    kind: "rocprof-studio-diagnostics",
    generatedAt: input.generatedAt || new Date().toISOString(),
    application: { name: "rocprof studio", version },
    runtime: input.runtime,
    capture: {
      name: c.name,
      format: c.format,
      architecture: c.architecture,
      clock: { unit: c.unit, origin: c.origin, start: c.start, end: c.end },
      metadata: c.metadata,
      warnings: c.warnings,
    },
    inventory: {
      waveIndexes: c.waves.length,
      residencyIntervals: c.intervals.length,
      instructions: c.instructions.length,
      pcSamplingObservations: c.pcSamplingObservations
        ? {
            records: c.pcSamplingObservations.rows.length,
            columns: c.pcSamplingObservations.columns,
            attribution: "CSV observations without resolved PC/code-object identities",
          }
        : null,
      dispatches: c.dispatches.length,
      sampledCounters: counters,
      eventCounters: {
        records: events.length,
        definitions: counterIdentities.size,
        missingValues: count(events, (e) => e.value === null),
        ambiguousDispatchAssociations: count(
          events,
          (e) => e.dispatches.length > 1,
        ),
        withoutDispatch: count(events, (e) => !e.dispatches.length),
      },
      decoderEvents: (c.decoderEvents || []).length,
      monitorIntervals: (c.monitorIntervals || []).length,
      hostCalls: (c.hostCalls || []).length,
      codeObjects: (c.codeObjects || []).length,
      realtimeAnchors: c.realtime?.anchors.length ?? 0,
      otherSimdRecords: (c.otherSimd || []).length,
      markers: {
        records: markers.length,
        points: count(markers, (m) => m.point),
        incompleteScopes: count(markers, (m) => !m.point && m.end === null),
      },
      registeredSources: new Set([
        ...(c.sourceFiles || []),
        ...Object.keys(c.sources),
      ]).size,
      inlineSourceSnapshots: Object.keys(c.sources).length,
    },
    measurementAvailability: {
      dispatchResources: dispatchMeasurements,
      pcIssuedRows: count(c.instructions, (i) => i.pcIssued != null),
      pcStalledRows: count(c.instructions, (i) => i.pcStalled != null),
      stallReasonRows: count(c.instructions, (i) => i.stallReasons != null),
      coverageMeaning:
        "Presence counts describe imported fields; they do not prove collection completeness or replay coverage.",
    },
    workspace: input.workspace,
    definitions: input.definitions,
    definitionState:
      "Current editor drafts; may differ from the last successful tensor evaluation.",
    sourceRegistry: [
      ...new Set([...(c.sourceFiles || []), ...Object.keys(c.sources)]),
    ],
    sourceReplacements,
    analysis: {
      hidden: {
        available: !!input.hidden,
        pending: input.hiddenPending,
        model: input.hidden?.model ?? null,
        warnings: input.hidden?.warnings || [],
      },
      loadedWave: input.wave
        ? {
            id: input.wave.id,
            tokens: input.wave.tokens.length,
            waitRelationships: input.wave.waits?.length ?? null,
          }
        : null,
    },
    qualification: {
      replayCoverageVerified: false,
      nativeTimingControlAttached: false,
      correctnessGateAttached: false,
      runtimeLibraryMappingVerified: false,
      thermalContentionControlAttached: false,
    },
    limitations: [
      "This manifest is diagnostic context, not a portable capture or a performance qualification.",
      "Raw inputs, trace samples and source text are not embedded. Input hashes are not computed by this export.",
      "Definition text, capture metadata and local paths are included; source replacement hashes identify text without proving its revision.",
    ],
  };
}
export function diagnosticBytes(report: unknown) {
  const bytes = new TextEncoder().encode(JSON.stringify(report, null, 2));
  if (bytes.length > 16 * 1024 * 1024)
    throw Error("Diagnostic manifest exceeds 16 MiB");
  return bytes;
}
