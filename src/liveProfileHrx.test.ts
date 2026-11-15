import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  HRX_DEFAULT_FAMILIES,
  HRX_DEFAULT_FLUSH_MS,
  HRX_FAMILIES,
  HRX_LIMITS,
  HRX_TRACE_FAMILY,
  HRX_TRACE_LIMITS,
  HRX_TRACE_OPTIONS,
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
      ["allowPartial", "families", "filterFunction", "flushIntervalMs", "trace"].sort(),
    );
  });

  it("keeps the thread-trace family, option names and ranges equal to the Rust core's", () => {
    expect(core).toContain(`const HRX_TRACE_FAMILY: &str = "${HRX_TRACE_FAMILY}";`);
    const body = core.match(/const HRX_TRACE_OPTIONS: \[\(&str, &str, u64, u64\); \d+\] = \[([^;]*)\];/);
    if (!body) throw new Error("HRX_TRACE_OPTIONS not found in mcp.rs");
    // Bounds are written as products and differences of integers.
    const number = (text: string) =>
      text
        .split("-")
        .map((term) => term.split("*").reduce((product, factor) => product * Number(factor.trim()), 1))
        .reduce((total, term, index) => (index === 0 ? term : total - term));
    const rust = [...body[1].matchAll(/\("([^"]+)", "([^"]+)", ([^,]+), ([^)]+)\)/g)].map((m) => ({
      name: m[1],
      flag: m[2],
      low: number(m[3]),
      high: number(m[4]),
    }));
    expect(rust.map(({ name, low, high }) => ({ name, low, high }))).toEqual(HRX_TRACE_OPTIONS.map((o) => ({ ...o })));
    for (const option of rust) expect(option.flag).toMatch(/^--device_profiling_trace_[a-z_]+$/);
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
        trace: false,
        traceTargetCu: "4",
        traceSimdSelect: "",
        traceShaderEngineMask: "",
        traceBufferMib: "",
        traceMaxTraces: "",
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

  it("adds the thread-trace family and sends only the trace fields that were filled", () => {
    // Ticked with nothing else: the family joins the defaults, the driver chooses the rest.
    expect(hrxOptions({ ...emptyHrxForm(), trace: true })).toEqual({
      families: ["queue-events", "dispatch-events", "executable-metadata", "executable-traces"],
    });
    expect(
      hrxOptions({
        ...emptyHrxForm(),
        trace: true,
        traceTargetCu: "4",
        traceSimdSelect: "1",
        traceShaderEngineMask: "1",
        traceBufferMib: "64",
        traceMaxTraces: "3",
      }),
    ).toEqual({
      families: ["queue-events", "dispatch-events", "executable-metadata", "executable-traces"],
      trace: { targetCu: 4, simdSelect: 1, shaderEngineMask: 1, bufferSize: 67108864, maxTraces: 3 },
    });
    // Custom families keep their order; the family is not added twice.
    expect(
      hrxOptions({ ...emptyHrxForm(), familyMode: "custom", families: ["dispatch-events"], extraFamilies: "executable-traces", trace: true, traceBufferMib: "0.5" }),
    ).toEqual({ families: ["dispatch-events", "executable-traces"], trace: { bufferSize: 524288 } });
    // Unticked: the trace fields are not sent, so the core cannot reject them.
    expect(hrxOptions({ ...emptyHrxForm(), traceBufferMib: "64", traceMaxTraces: "9" })).toEqual({});
  });

  it("rejects trace values the core rejects, with the core's messages", () => {
    const traced = (patch: Partial<ReturnType<typeof emptyHrxForm>>) => () =>
      hrxOptions({ ...emptyHrxForm(), trace: true, ...patch });
    expect(traced({ traceTargetCu: "16" })).toThrow("hrx trace targetCu must be an integer between 0 and 15");
    expect(traced({ traceSimdSelect: "4" })).toThrow("hrx trace simdSelect must be an integer between 0 and 3");
    expect(traced({ traceShaderEngineMask: "0" })).toThrow("hrx trace shaderEngineMask must be an integer between 1 and 255");
    expect(traced({ traceBufferMib: "0.1" })).toThrow("hrx trace bufferSize must be an integer between 131072 and 536866816");
    expect(traced({ traceBufferMib: "512" })).toThrow("hrx trace bufferSize must be an integer between 131072 and 536866816");
    expect(traced({ traceBufferMib: "0.126" })).toThrow("hrx trace bufferSize must be an integer between 131072 and 536866816");
    expect(traced({ traceMaxTraces: "0" })).toThrow("hrx trace maxTraces must be an integer between 1 and 4096");
    expect(traced({ traceMaxTraces: "two" })).toThrow("hrx trace maxTraces must be an integer between 1 and 4096");
    for (const message of [
      "hrx trace {name} must be an integer between {low} and {high}",
      "hrx trace bufferSize must be a multiple of 4096 bytes",
    ])
      expect(core).toContain(message);
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
    expect(text).toMatch(/Dispatch-level by default/);
    expect(text).toContain("hardware counters are not available");
    expect(text).toContain("XDNA device may record no dispatches");
  });

  it("says a thread trace is a sample of whole recorded executions", () => {
    const text = HRX_TRACE_LIMITS.join(" ");
    expect(text).toContain("A sample, not a census");
    expect(text).toContain("about 2% of a dispatch's waves");
    expect(text).toContain("one whole recorded execution");
    expect(text).toContain("A full buffer truncates it");
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

  it("reports thread-trace partitions as a sample to decode, and their loss", () => {
    const lines = hrxBundleLines({
      files: ["/jobs/x/hrx-profile.irpf"],
      bundle: {
        bytes: 11390925,
        finalized: true,
        sessions: 2,
        producers: ["amd-gpu", "xdna"],
        functions: 78,
        dispatchEvents: 4285,
        queueEvents: 16286,
        executableTraces: 2,
        executableTraceBytes: 8388544,
        counterSamples: 0,
        truncated: [{ family: "executable-traces", dropped: 149 }],
        perProducer: [{ producer: "amd-gpu", sessions: 1, dispatchEvents: 4285, queueEvents: 16286 }],
      },
    });
    expect(lines).toContain(
      "TRUNCATED thread trace: a buffer filled, tokens were dropped (149 counted) or the stop did not finish",
    );
    expect(lines.at(-1)).toBe(
      "Thread trace: 2 partition(s), 8,388,544 bytes of SQTT. Open the capture to decode waves and ISA rows: a sample of one SIMD of one workgroup processor per shader engine, not a census. Hardware counters: none.",
    );
    expect(lines.join(" ")).not.toContain("the bundle is dispatch-level");
  });
});
