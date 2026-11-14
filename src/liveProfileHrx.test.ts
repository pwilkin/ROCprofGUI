import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  HRX_DEFAULT_FAMILIES,
  HRX_DEFAULT_FLUSH_MS,
  HRX_FAMILIES,
  HRX_LIMITS,
  emptyHrxForm,
  hrxBundleLines,
  hrxOptions,
  hrxStatusLine,
  isHrxResult,
} from "./liveProfileHrx";

const core = readFileSync(new URL("../src-tauri/src/mcp.rs", import.meta.url), "utf8");
// The string literals of one `const NAME: [&str; N] = [...]` table in mcp.rs.
const rustTable = (name: string): string[] => {
  const body = core.match(new RegExp(`const ${name}: \\[&str; \\d+\\] = \\[([^;]*)\\];`));
  if (!body) throw new Error(`${name} not found in mcp.rs`);
  return [...body[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]);
};

describe("hrx tracer request", () => {
  it("keeps its family tables and default flush equal to the Rust core's", () => {
    expect([...HRX_FAMILIES]).toEqual(rustTable("HRX_FAMILIES"));
    expect([...HRX_DEFAULT_FAMILIES]).toEqual(rustTable("HRX_DEFAULT_FAMILIES"));
    expect(core).toContain(`const HRX_DEFAULT_FLUSH_MS: u64 = ${HRX_DEFAULT_FLUSH_MS};`);
    expect(rustTable("HRX_OPTIONS").sort()).toEqual(
      ["allowPartial", "families", "filterFunction", "flushIntervalMs"].sort(),
    );
  });

  it("sends an empty object for the defaults, so the core chooses families and flush", () => {
    expect(hrxOptions(emptyHrxForm())).toEqual({});
  });

  it("maps every field to the MCP hrx object", () => {
    expect(
      hrxOptions({
        familyMode: "custom",
        families: ["queue-events", "executable-metadata"],
        extraFamilies: "host-execution, counters",
        flushIntervalMs: "0",
        filterFunction: " g_gemm_* ",
        allowPartial: true,
      }),
    ).toEqual({
      families: ["queue-events", "executable-metadata", "host-execution", "counters"],
      flushIntervalMs: 0,
      filterFunction: "g_gemm_*",
      allowPartial: true,
    });
    // Custom ticks are ignored in default mode: the default is the absence of families.
    expect(hrxOptions({ ...emptyHrxForm(), families: ["queue-events"], flushIntervalMs: "10" })).toEqual({
      flushIntervalMs: 10,
    });
  });

  it("rejects what the core rejects, with the core's messages", () => {
    const custom = (patch: Partial<ReturnType<typeof emptyHrxForm>>) => () =>
      hrxOptions({ ...emptyHrxForm(), familyMode: "custom", ...patch });
    expect(custom({ families: [] })).toThrow("hrx families cannot be empty");
    expect(custom({ extraFamilies: "waves" })).toThrow("Unknown hrx data family waves");
    expect(custom({ extraFamilies: "queue-events" })).toThrow("hrx families must be unique");
    for (const flushIntervalMs of ["60001", "-1", "2.5", "soon"])
      expect(() => hrxOptions({ ...emptyHrxForm(), flushIntervalMs })).toThrow(
        "hrx flushIntervalMs must be an integer between 0 and 60000",
      );
    expect(() => hrxOptions({ ...emptyHrxForm(), filterFunction: "g".repeat(257) })).toThrow(
      "hrx filterFunction must be a nonempty glob of at most 256 characters",
    );
    for (const message of [
      "hrx families cannot be empty",
      "Unknown hrx data family ",
      "hrx families must be unique",
      "hrx flushIntervalMs must be an integer between 0 and 60000",
      "hrx filterFunction must be a nonempty glob of at most 256 characters",
    ])
      expect(core).toContain(message);
  });
});

describe("hrx tracer display", () => {
  it("states the three limits before a run", () => {
    const text = HRX_LIMITS.join(" ");
    expect(text).toContain("no window to trigger");
    expect(text).toMatch(/No waves, no ISA, no hardware counters/);
    expect(text).toContain("XDNA device may record no dispatches");
  });

  it("describes a running and an exited application", () => {
    expect(hrxStatusLine({ running: true, bundleBytes: 4917850 })).toBe(
      "Recording the whole run · bundle 4,917,850 bytes so far",
    );
    expect(hrxStatusLine({ running: false, exitCode: 1, bundleBytes: 1578 })).toBe(
      "The application has exited (code 1) · bundle 1,578 bytes so far · stop and collect to read it",
    );
  });

  it("tells an hrx result from a rocprofv3 or radv one", () => {
    expect(isHrxResult({ status: "stopped", files: [], bundle: null })).toBe(true);
    expect(isHrxResult({ status: "stopped", files: [], triggerCount: 1 })).toBe(false);
  });

  it("summarises a GPU + NPU bundle per device session", () => {
    const lines = hrxBundleLines({
      files: ["/jobs/x/hrx-profile.irpf"],
      bundle: {
        bytes: 4917850,
        finalized: true,
        sessions: 2,
        producers: ["amd-gpu", "xdna"],
        functions: 117,
        dispatchEvents: 21490,
        queueEvents: 16406,
        truncated: [],
        failedSessions: [],
        perProducer: [
          { producer: "amd-gpu", sessions: 1, dispatchEvents: 21490, queueEvents: 16277 },
          { producer: "xdna", sessions: 1, dispatchEvents: 0, queueEvents: 129 },
        ],
      },
    });
    expect(lines).toEqual([
      "Profile bundle: /jobs/x/hrx-profile.irpf · 4,917,850 bytes · finalized",
      "Device sessions: 2 (amd-gpu, xdna)",
      "Dispatch events: 21,490 · queue events: 16,406 · functions: 117",
      "amd-gpu: 21,490 dispatch events, 16,277 queue events",
      "xdna: no dispatch events, 129 queue events",
      "Waves, ISA and hardware counters: none (the bundle is dispatch-level).",
    ]);
  });

  it("never renders a missing, unfinalized or truncated bundle as a clean result", () => {
    expect(hrxBundleLines({ files: [], bundle: null })).toEqual(["No profile bundle was written."]);
    expect(hrxBundleLines({ files: ["/jobs/x/hrx-profile.irpf"], bundle: null })).toEqual([
      "Profile bundle /jobs/x/hrx-profile.irpf could not be read.",
    ]);
    const open = hrxBundleLines({
      files: ["/jobs/x/hrx-profile.irpf"],
      bundle: { bytes: 24, finalized: false, sessions: 0, producers: [], functions: 0, dispatchEvents: 0, queueEvents: 0, truncated: [], perProducer: [] },
    });
    expect(open[0]).toContain("NOT finalized (cannot be imported)");
    const truncated = hrxBundleLines({
      files: ["/jobs/x/hrx-profile.irpf"],
      bundle: { bytes: 1340000, finalized: true, sessions: 1, producers: ["amd-gpu"], functions: 3, dispatchEvents: 0, queueEvents: 4096, truncated: [{ family: "queue-events", dropped: 12216 }], perProducer: [{ producer: "amd-gpu", sessions: 1, dispatchEvents: 0, queueEvents: 4096 }] },
    });
    expect(truncated).toContain("TRUNCATED queue-events: 12,216 record(s) dropped by the producer");
    expect(truncated).toContain("Dispatch events: 0 · queue events: 4,096 · functions: 3");
  });
});
