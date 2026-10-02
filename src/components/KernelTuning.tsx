import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { Play, Square, Search, Save, Code2 } from "lucide-react";

type Config = {
  hipcc?: string;
  sourceTree: string;
  cwd: string;
  argv: string[];
  occurrence: number;
  captureScope?: "reachable" | "all";
  captureLibrary: string;
  replay: string;
  profiler: string;
  objdump: string;
  readobj: string;
  compile?: {
    argv: string[];
    cwd: string;
    translationUnit?: string;
    alternatives?: { argv: string[]; cwd: string; translationUnit: string }[];
  };
};
type Metric = {
  value: number | null;
  unit: string;
  reason?: string;
  method?: string;
  basis?: unknown;
  delta?: number;
  deltaPercent?: number | null;
};
type Calibration = {
  createdAt: number;
  fingerprint: string;
  reused: boolean;
  path: string;
  device: { name: string; arch: string };
  peaks: Record<
    string,
    {
      rate: number;
      unit: string;
      samples: number;
      coefficientOfVariation: number;
      quality: string;
      method: string;
    }
  >;
  errors: Record<string, string>;
};
type Run = {
  calibration?: Calibration;
  characterization?: {
    families?: string[];
    errors?: Record<string, string>;
    error?: string;
  };
  number: number;
  variant: boolean;
  iterations: number;
  conditions?: KernelConditions;
  codeHash: string;
  metrics: Record<string, Metric>;
  directory: string;
  validation?: { status: string; mismatchedRegions?: string[]; reason?: string };
};
type Session = {
  phase?: string;
  calibration?: Calibration;
  calibrationError?: string;
  kernel: string;
  directory: string;
  traceDispatch: { id: string; grid: string; workgroup: string };
  config?: Config;
  candidates?: { path: string; line: number; code: string }[];
  source?: { path: string; line: number };
  code?: string;
  isa?: string;
  captured?: string;
  captureBytes?: number;
  captureScope?: string;
  captureRegions?: { kept: number; keptBytes: number; tracked: number; trackedBytes: number };
  captureScopeNote?: string;
  argumentsHex?: string;
  parameters?: {
    declaration: string;
    value: string;
    offset: number;
    size: number;
  }[];
  dispatch?: unknown;
  allocations?: unknown;
  lastSave?: { path: string; backup: string };
  runs: Run[];
};
type Status = {
  session: Session;
  running: boolean;
  error: string | null;
  log: string;
};
// The replay conditions the HIP (Kerncap) path honours. Memory placement and
// cache flush are Vulkan-only; see docs/kernel-tuning-parity.md.
type KernelConditions = { mode: "stateless" | "sustained"; repeats: number };
const BEST_CASE: KernelConditions = { mode: "stateless", repeats: 1 };
function describeConditions(c: KernelConditions = BEST_CASE) {
  return [
    c.mode === "sustained"
      ? "dispatched back to back after one restore"
      : "inputs restored before every iteration",
    c.repeats > 1 ? `${c.repeats} replays with fresh allocations` : "",
  ]
    .filter(Boolean)
    .join(" · ");
}
const defaults: Config = {
  sourceTree: "",
  cwd: "",
  argv: [],
  occurrence: 0,
  captureLibrary: "libkerncap.so",
  replay: "kerncap-replay",
  profiler: "/opt/rocm/bin/rocprofv3",
  objdump: "/opt/rocm/llvm/bin/llvm-objdump",
  readobj: "/opt/rocm/llvm/bin/llvm-readobj",
};
const number = (v: number) =>
  new Intl.NumberFormat("en-US", { maximumSignificantDigits: 6 }).format(v);
const metricOrder = [
  "GPU duration",
  "Launch throughput",
  "Occupancy",
  "Scratch",
  "LDS",
  "VGPRs",
  "LDS bank conflicts",
  "WMMA / MFMA instructions",
  "WMMA issued",
  "Compute throughput",
  "Memory throughput",
  "Compute speed of light",
  "Memory speed of light",
];
const metricRank = (name: string) =>
  metricOrder.includes(name) ? metricOrder.indexOf(name) : metricOrder.length;
export function KernelTuning({ id }: { id: string }) {
  const [status, setStatus] = useState<Status | null>(null),
    [config, setConfig] = useState(defaults),
    [argv, setArgv] = useState(JSON.stringify(defaults.argv)),
    [compile, setCompile] = useState("[]"),
    [code, setCode] = useState(""),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false),
    [iterations, setIterations] = useState(20),
    [conditions, setConditions] = useState<KernelConditions>(BEST_CASE),
    [work, setWork] = useState<Record<string, string>>({}),
    [counters, setCounters] = useState("[]"),
    [runIndex, setRunIndex] = useState(-1),
    [validate, setValidate] = useState(true),
    [autoCounters, setAutoCounters] = useState(true),
    [autoCalibrate, setAutoCalibrate] = useState(true);
  const mcpRevision = useRef(-1);
  useEffect(() => {
    const control = (event: Event) => { const s = (event as CustomEvent).detail; if (s.workspace?.tuningId === id && mcpRevision.current !== s.workspace.tuningRevision) { mcpRevision.current = s.workspace.tuningRevision; initialized.current = false; codeLoaded.current = false; setRunIndex(-1); } };
    window.addEventListener("mcp-control", control);
    return () => window.removeEventListener("mcp-control", control);
  }, [id]);
  const initialized = useRef(false),
    codeLoaded = useRef(false),
    stopped = useRef(false);
  useEffect(() => {
    stopped.current = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const next = await invoke<Status>("tuning_status", { id });
        if (stopped.current) return;
        setStatus(next);
        setBusy(next.running);
        if (!initialized.current && next.session.config) {
          setConfig(next.session.config);
          setArgv(JSON.stringify(next.session.config.argv));
          setCompile(JSON.stringify(next.session.config.compile?.argv || []));
          initialized.current = true;
        }
        if (next.session.code !== undefined && !codeLoaded.current) {
          setCode(next.session.code);
          codeLoaded.current = true;
        }
        if (next.session.config?.compile && !initialized.current) {
          setCompile(JSON.stringify(next.session.config.compile.argv));
        }
      } catch (e) {
        if (!stopped.current) setError(String(e));
      }
      if (!stopped.current) timer = setTimeout(poll, 750);
    };
    void poll();
    return () => {
      stopped.current = true;
      clearTimeout(timer);
    };
  }, [id]);
  async function action(op: string, extra: Record<string, unknown> = {}) {
    setError("");
    try {
      if (op === "run") {
        const parsed = JSON.parse(counters);
        if (!Array.isArray(parsed))
          throw Error("Counter mappings must be an array");
        extra = {
          ...extra,
          counters: parsed,
          validate,
          autoCounters,
          autoCalibrate,
        };
      }
      const cfg = {
        ...config,
        argv: JSON.parse(argv),
        compile: {
          ...config.compile,
          argv: JSON.parse(compile),
          cwd: config.compile?.cwd || config.cwd,
        },
      };
      if (!Array.isArray(cfg.argv) || !Array.isArray(cfg.compile.argv))
        throw Error("Commands must be JSON arrays of arguments");
      await invoke("tuning_start", {
        id,
        request: { op, config: cfg, ...extra },
      });
      setBusy(true);
    } catch (e) {
      setError(String(e));
    }
  }
  async function select(index: number) {
    try {
      codeLoaded.current = false;
      await action("select", { index });
      const next = await new Promise<Status>((resolve, reject) => {
        let attempts = 0;
        const check = async () => {
          try {
            const s = await invoke<Status>("tuning_status", { id });
            if (s.running && ++attempts < 100) {
              setTimeout(check, 100);
              return;
            }
            resolve(s);
          } catch (e) {
            reject(e);
          }
        };
        void check();
      });
      if (next.session.config) {
        setConfig(next.session.config);
        setCompile(JSON.stringify(next.session.config.compile?.argv || []));
      }
    } catch (e) {
      setError(String(e));
    }
  }
  const session = status?.session,
    runs = session?.runs || [],
    selected = runIndex < 0 ? runs.at(-1) : runs[runIndex];
  const field = (key: keyof Config, label: string) => (
    <label>
      {label}
      <input
        value={String(config[key] ?? "")}
        onChange={(e) =>
          setConfig({
            ...config,
            [key]:
              key === "occurrence" ? Number(e.target.value) : e.target.value,
          })
        }
      />
    </label>
  );
  return (
    <div className="tuning-window">
      <header className="tuning-header">
        <Code2 />
        <div>
          <strong>Kernel laboratory</strong>
          <p>{session?.kernel || "Opening session…"}</p>
        </div>
        <span className="badge">
          {busy ? "RUNNING" : session?.captured ? "INPUTS CAPTURED" : "SETUP"}
        </span>
      </header>
      <div className="tuning-content">
        {busy && (
          <button
            onClick={() =>
              void invoke("tuning_cancel", { id }).catch((e) =>
                setError(String(e)),
              )
            }
          >
            <Square size={15} />
            Cancel operation
          </button>
        )}
        {(error || status?.error) && (
          <div className="error-notice" role="alert">
            {error || status?.error}
          </div>
        )}
        {busy && session?.phase && <p role="status">{session.phase}…</p>}
        <section className="calibration-panel" aria-label="GPU calibration">
          <div className="tuning-section-title">
            <h2>GPU calibration</h2>
            <label>
              <input
                type="checkbox"
                checked={autoCalibrate}
                onChange={(e) => setAutoCalibrate(e.target.checked)}
              />{" "}
              Calibrate and characterize automatically
            </label>
            <button disabled={busy} onClick={() => void action("calibrate")}>
              {session?.calibration ? "Recalibrate GPU" : "Calibrate GPU now"}
            </button>
          </div>
          <p>
            {session?.calibration
              ? `${session.calibration.device.name} · ${session.calibration.device.arch} · ${session.calibration.reused ? "cached" : "measured"} ${new Date(session.calibration.createdAt * 1000).toLocaleString()}`
              : "The first isolated run measures this GPU’s attainable compute and memory rates. Later runs reuse matching calibration and recollect kernel counters."}
          </p>
          {session?.calibrationError && (
            <p role="alert">
              Calibration unavailable: {session.calibrationError}. GPU timing
              remains available.
            </p>
          )}
          {session?.calibration && (
            <details>
              <summary>Calibrated peaks, variability and coverage</summary>
              <table>
                <thead>
                  <tr>
                    <th>Instruction family / memory scope</th>
                    <th>Attainable rate</th>
                    <th>Samples</th>
                    <th>Variation</th>
                    <th>Quality</th>
                  </tr>
                </thead>
                <tbody>
                  {Object.entries(session.calibration.peaks).map(
                    ([family, peak]) => (
                      <tr key={family}>
                        <td>{family.replaceAll("_", " ")}</td>
                        <td>
                          {number(peak.rate)} {peak.unit}
                        </td>
                        <td>{peak.samples}</td>
                        <td>{number(peak.coefficientOfVariation * 100)}%</td>
                        <td>
                          {peak.method} · {peak.quality}
                        </td>
                      </tr>
                    ),
                  )}
                </tbody>
              </table>
              <p>
                Percentages use the matching calibrated instruction family or
                memory scope. Values can exceed 100% when this kernel
                outperforms the calibration microbenchmark. Calibration expires
                after seven days or a detected
                device/toolchain/power-configuration change.
              </p>
              {Object.entries(session.calibration.errors).map(
                ([family, reason]) => (
                  <p key={family}>
                    {family}: {reason}
                  </p>
                ),
              )}
              <p>Calibration record: {session.calibration.path}</p>
            </details>
          )}
        </section>
        <details open={!session?.captured}>
          <summary>1 · Source and capture setup</summary>
          <div className="tuning-form">
            {field("sourceTree", "Source tree")}
            <button
              disabled={busy}
              onClick={async () => {
                const path = await open({ directory: true, multiple: false });
                if (typeof path === "string")
                  setConfig({ ...config, sourceTree: path });
              }}
            >
              Choose source tree
            </button>
            {field("cwd", "Application working directory")}
            <label>
              Application command · JSON argument array
              <textarea
                value={argv}
                onChange={(e) => setArgv(e.target.value)}
                rows={3}
              />
            </label>
            {field(
              "occurrence",
              "Matching kernel occurrence · zero-based (not trace dispatch ID)",
            )}
            <label>
              Memory to capture
              <select
                aria-label="Capture scope"
                value={config.captureScope || "reachable"}
                onChange={(e) => setConfig({ ...config, captureScope: e.target.value as Config["captureScope"] })}
              >
                <option value="reachable">Allocations the kernel's arguments reach (follows pointer tables)</option>
                <option value="all">Every allocation in the process (kernels that reach memory another way)</option>
              </select>
            </label>
            <p>
              Selected trace dispatch: {session?.traceDispatch.id} · Grid{" "}
              {session?.traceDispatch.grid} · Workgroup{" "}
              {session?.traceDispatch.workgroup}. Re-running the application can
              change dispatch order; inspect the captured launch below.
            </p>
            <details>
              <summary>Capture and profiler tools</summary>
              {field(
                "captureLibrary",
                "Kerncap capture library · absolute path",
              )}
              {field("replay", "Kerncap replay executable")}
              {field("profiler", "rocprofv3")}
              {field("objdump", "llvm-objdump")}
              {field("readobj", "llvm-readobj")}
              {field(
                "hipcc",
                "Calibration HIP compiler (default /opt/rocm/bin/hipcc)",
              )}
            </details>
            <div className="tuning-actions">
              <button disabled={busy} onClick={() => void action("search")}>
                <Search size={15} />
                Find kernel source
              </button>
              <button
                disabled={busy || !session?.source || !!session?.captured}
                onClick={() => void action("capture")}
              >
                <Play size={15} />
                Replay application & capture inputs
              </button>
            </div>
            {session?.candidates && (
              <ul>
                {session.candidates.map((c, i) => (
                  <li key={`${c.path}:${c.line}`}>
                    <button
                      disabled={busy || !!session.captured}
                      onClick={() => void select(i)}
                    >
                      {c.path}:{c.line}
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </details>
        {session?.source && (
          <>
            <div className="tuning-section-title">
              <h2>2 · Edit & replay</h2>
              <span>
                {session.source.path}:{session.source.line}
              </span>
            </div>
            <div className="tuning-editors">
              <label>
                Kernel source
                <textarea
                  aria-label="Kernel source editor"
                  spellCheck={false}
                  value={code}
                  onChange={(e) => setCode(e.target.value)}
                />
              </label>
              <label>
                ISA · last compiled run
                <pre aria-label="Kernel ISA disassembly">
                  {session.isa ||
                    "Capture the kernel to inspect its compiled ISA."}
                </pre>
              </label>
            </div>
            <details>
              <summary>
                Build context, manual overrides & counter mappings
              </summary>
              <div className="tuning-form">
                {config.compile?.alternatives && (
                  <label>
                    Translation unit that instantiates this kernel
                    <select
                      aria-label="Kernel translation unit"
                      value={config.compile.translationUnit || ""}
                      onChange={(e) => {
                        const context = config.compile?.alternatives?.find(
                          (c) => c.translationUnit === e.target.value,
                        );
                        if (context) {
                          setCompile(JSON.stringify(context.argv));
                          setConfig({
                            ...config,
                            compile: {
                              ...context,
                              alternatives: config.compile?.alternatives,
                            },
                          });
                        }
                      }}
                    >
                      {config.compile.alternatives.map((c, i) => (
                        <option key={i} value={c.translationUnit}>
                          {c.translationUnit}
                        </option>
                      ))}
                    </select>
                  </label>
                )}
                <label>
                  Original compiler command · JSON argument array
                  <textarea
                    value={compile}
                    onChange={(e) => setCompile(e.target.value)}
                    rows={3}
                  />
                </label>
                <label>
                  Compiler working directory
                  <input
                    value={config.compile?.cwd || config.cwd}
                    onChange={(e) =>
                      setConfig({
                        ...config,
                        compile: { argv: [], cwd: e.target.value },
                      })
                    }
                  />
                </label>
                <p>
                  The kernel signature must stay compatible with captured
                  arguments. The original translation unit compiles through a
                  private source overlay; the source tree is changed only by
                  Save to original.
                </p>
                {Object.entries({
                  flops: "FLOPs per dispatch",
                  bytes: "Bytes transferred per dispatch",
                  peakTflops: "Hardware peak TFLOP/s",
                  peakGbps: "Hardware peak GB/s",
                }).map(([key, label]) => (
                  <label key={key}>
                    {label}
                    <input
                      type="number"
                      min="0"
                      value={work[key] || ""}
                      onChange={(e) =>
                        setWork({ ...work, [key]: e.target.value })
                      }
                    />
                  </label>
                ))}
                <label>
                  Measured counters · JSON mappings
                  <textarea
                    rows={4}
                    value={counters}
                    onChange={(e) => setCounters(e.target.value)}
                    placeholder={
                      '[{"label":"Occupancy","counter":"SUPPORTED_COUNTER","unit":"%"}]'
                    }
                  />
                </label>
                <p>
                  Use counters supported by your GPU. Counter values retain
                  their native units; no percentage conversion is inferred.
                  Timing is measured separately from counter passes.
                </p>
              </div>
            </details>
            <div className="tuning-actions">
              <label>
                <input
                  type="checkbox"
                  checked={validate}
                  onChange={(e) => setValidate(e.target.checked)}
                />{" "}
                Check outputs against captured baseline
              </label>
              <label>
                <input
                  type="checkbox"
                  checked={autoCounters}
                  onChange={(e) => setAutoCounters(e.target.checked)}
                />{" "}
                Collect supported counters
              </label>
              <label>
                Iterations
                <input
                  aria-label="Replay iterations"
                  type="number"
                  min="1"
                  max="10000"
                  value={iterations}
                  onChange={(e) => setIterations(Number(e.target.value))}
                />
              </label>
              <fieldset className="tuning-conditions">
                <legend>Replay conditions</legend>
                <label>
                  Mode
                  <select
                    aria-label="Replay mode"
                    value={conditions.mode}
                    onChange={(e) =>
                      setConditions((c) => ({
                        ...c,
                        mode: e.target.value as KernelConditions["mode"],
                      }))
                    }
                  >
                    <option value="stateless">
                      Stateless (restore inputs each iteration)
                    </option>
                    <option value="sustained">
                      Sustained (back to back after one restore)
                    </option>
                  </select>
                </label>
                <label>
                  Replays
                  <input
                    aria-label="Replays with fresh allocations"
                    type="number"
                    min="1"
                    max="10"
                    value={conditions.repeats}
                    onChange={(e) =>
                      setConditions((c) => ({
                        ...c,
                        repeats: Number(e.target.value),
                      }))
                    }
                  />
                </label>
              </fieldset>
              <button
                disabled={busy || !session.captured}
                onClick={() =>
                  void action("run", {
                    variant: false,
                    iterations,
                    conditions,
                    work,
                    counters: [],
                  })
                }
              >
                Run captured baseline
              </button>
              <button
                disabled={busy || !session.captured}
                onClick={() =>
                  void action("run", {
                    variant: true,
                    code,
                    iterations,
                    conditions,
                    work,
                    counters: [],
                  })
                }
              >
                <Play size={15} />
                Build & run edited kernel
              </button>
              <button
                disabled={busy}
                onClick={() => void action("save", { code })}
              >
                <Save size={15} />
                Save to original
              </button>
              {busy && (
                <button
                  onClick={() =>
                    void invoke("tuning_cancel", { id }).catch((e) =>
                      setError(String(e)),
                    )
                  }
                >
                  <Square size={15} />
                  Cancel
                </button>
              )}
            </div>
            {session.lastSave && (
              <p role="status">
                Saved {session.lastSave.path}. Backup: {session.lastSave.backup}
              </p>
            )}
          </>
        )}
        <div className="tuning-section-title">
          <h2>3 · Run comparison</h2>
          <select
            aria-label="Tuning run"
            value={runIndex}
            onChange={(e) => setRunIndex(Number(e.target.value))}
          >
            <option value={-1}>Latest run</option>
            {runs.map((r, i) => (
              <option key={r.number} value={i}>
                Run {r.number} · {r.variant ? "edited" : "baseline"}
              </option>
            ))}
          </select>
        </div>
        {selected ? (
          <>
            <p>
              Run {selected.number} · {selected.iterations} iterations ·{" "}
              {describeConditions(selected.conditions)} · deltas against the last
              earlier run under the same conditions
            </p>
            <p role="status">
              Output validation: {selected.validation?.status || "not checked"}
              {selected.validation?.mismatchedRegions?.length
                ? ` · ${selected.validation.mismatchedRegions.length} memory regions differ`
                : "."}
              {selected.validation?.reason && ` ${selected.validation.reason}.`}
            </p>
            {selected.characterization?.families && (
              <p>
                Detected instruction families:{" "}
                {selected.characterization.families.join(", ") ||
                  "no supported floating-point family"}
                . Counter collection uses separate replay passes.
              </p>
            )}
            {selected.characterization?.error && (
              <p role="alert">
                Characterization: {selected.characterization.error}
              </p>
            )}
            {Object.entries(selected.characterization?.errors || {}).map(
              ([family, reason]) => (
                <p key={family}>
                  {family}: {reason}
                </p>
              ),
            )}
            <div className="tuning-metrics">
              {Object.entries(selected.metrics)
                .sort(([a], [b]) => metricRank(a) - metricRank(b))
                .map(([name, m]) => (
                  <article
                    key={name}
                    title={m.basis ? JSON.stringify(m.basis) : m.reason}
                  >
                    <h3>
                      {m.basis &&
                      typeof m.basis === "object" &&
                      "calibration" in m.basis
                        ? name.replace(
                            "speed of light",
                            "· % of calibrated peak",
                          )
                        : name}
                    </h3>
                    {m.method && <span className="badge">{m.method}</span>}
                    <strong>
                      {m.value === null ? "Not collected" : number(m.value)}{" "}
                      <small>{m.value === null ? "" : m.unit}</small>
                    </strong>
                    {m.delta !== undefined && (
                      <p className="metric-delta">
                        Δ {m.delta > 0 ? "+" : ""}
                        {number(m.delta)} {m.unit}
                        {m.deltaPercent != null
                          ? ` (${m.deltaPercent > 0 ? "+" : ""}${number(m.deltaPercent)}%)`
                          : ""}
                      </p>
                    )}
                    {m.reason && <p>{m.reason}</p>}
                  </article>
                ))}
            </div>
          </>
        ) : (
          <p>No completed runs yet. Capture inputs, then run the baseline.</p>
        )}
        {session?.captureBytes !== undefined && (
          <p>
            Captured inputs: {number(session.captureBytes / 2 ** 30)} GiB on disk
            {session.captureRegions &&
              (session.captureScope === "reachable"
                ? `, the ${session.captureRegions.kept} of ${session.captureRegions.tracked} allocations (${number(session.captureRegions.trackedBytes / 2 ** 30)} GiB in all) that the kernel's arguments reach`
                : `, all ${session.captureRegions.kept} allocations in the process`)}
            . They are deleted when this session closes; results, logs and source backups stay.
          </p>
        )}
        {session?.captureScopeNote && <p role="note">{session.captureScopeNote}.</p>}
        <details>
          <summary>Captured launch, arguments and allocations</summary>
          {session?.parameters && (
            <table>
              <thead>
                <tr>
                  <th>Parameter</th>
                  <th>Captured value</th>
                  <th>Offset</th>
                  <th>Bytes</th>
                </tr>
              </thead>
              <tbody>
                {session.parameters.map((p) => (
                  <tr key={p.offset}>
                    <td>{p.declaration}</td>
                    <td>{p.value}</td>
                    <td>{p.offset}</td>
                    <td>{p.size}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          <pre>
            {JSON.stringify(
              {
                dispatch: session?.dispatch,
                argumentsHex: session?.argumentsHex,
                allocations: session?.allocations,
              },
              null,
              2,
            )}
          </pre>
        </details>
        <details>
          <summary>Job log & artifacts</summary>
          <p>{session?.directory}</p>
          <pre>{status?.log || "No commands have run yet."}</pre>
        </details>
      </div>
    </div>
  );
}
