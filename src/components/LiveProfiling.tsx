import { useEffect, useRef, useState } from "react";
import {
  desktop,
  liveProfileStart,
  liveProfileTrigger,
  liveProfileStatus,
  liveProfileStop,
} from "../bridge";

type Json = Record<string, unknown>;

// Compact desktop surface for the four live-profiling commands. It drives the
// same core the MCP tools use (profile_start/trigger/status/stop); the logic is
// not forked. rocprofv3 windows can be timed (windowMs) or triggered (end on
// demand), and carry the same stream/counter/att/kernel-filter selection.
export default function LiveProfiling() {
  const [tracer, setTracer] = useState("rocprofv3");
  const [argv, setArgv] = useState("");
  const [cwd, setCwd] = useState("");
  const [profiler, setProfiler] = useState("");
  const [windowMs, setWindowMs] = useState("5000");
  const [triggered, setTriggered] = useState(false);
  const [traces, setTraces] = useState("");
  const [counters, setCounters] = useState("");
  const [att, setAtt] = useState(false);
  const [attTargetCu, setAttTargetCu] = useState("");
  const [attSimdSelect, setAttSimdSelect] = useState("");
  const [attBufferSize, setAttBufferSize] = useState("");
  const [attShaderEngineMask, setAttShaderEngineMask] = useState("");
  const [includeRegex, setIncludeRegex] = useState("");
  const [excludeRegex, setExcludeRegex] = useState("");
  const [iterationRange, setIterationRange] = useState("");
  const [profile, setProfile] = useState<Json | null>(null);
  const [status, setStatus] = useState<Json | null>(null);
  const [result, setResult] = useState<Json | null>(null);
  const [windows, setWindows] = useState<{ count: number; mode: string }[]>([]);
  const [openTriggered, setOpenTriggered] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  // Guard against setState after an in-flight invoke resolves post-unmount
  // (the panel unmounts when the user switches views), following McpControl.
  // Set true on each effect run so StrictMode's mount/unmount/mount in dev
  // leaves the ref true, not stuck false after the first cleanup.
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);

  const rocprofv3 = tracer === "rocprofv3";
  const list = (s: string) => s.split(/[\s,]+/).filter(Boolean);
  // Parses a nonnegative-integer option field; blank means "not set".
  const intOption = (label: string, raw: string): number | undefined => {
    const t = raw.trim();
    if (t === "") return undefined;
    const n = Number(t);
    if (!Number.isInteger(n) || n < 0) throw new Error(`${label} must be a nonnegative integer`);
    return n;
  };
  // A status refresh must not wipe a previous action's error, so poll passes
  // clearError=false; every other action clears before it runs.
  const run = async (fn: () => Promise<void>, clearError = true) => {
    setBusy(true);
    if (clearError) setError("");
    try {
      await fn();
    } catch (e) {
      if (mounted.current) setError(String(e));
    } finally {
      if (mounted.current) setBusy(false);
    }
  };

  const start = () =>
    run(async () => {
      const args: Json = { argv: list(argv), cwd, tracer };
      if (rocprofv3 && profiler.trim()) args.profiler = profiler.trim();
      const started = await liveProfileStart(args);
      if (!mounted.current) return;
      setProfile(started);
      setStatus(null);
      setResult(null);
      setWindows([]);
      setOpenTriggered(false);
    });

  const trigger = () =>
    run(async () => {
      if (!profile) return;
      const args: Json = { id: profile.id as string };
      if (rocprofv3) {
        if (triggered) args.triggered = true;
        else {
          // Validate here so a non-numeric/out-of-range value surfaces an error
          // instead of coercing to NaN -> null -> the silent 5000 ms default.
          const ms = Number(windowMs);
          if (!Number.isInteger(ms) || ms < 1 || ms > 600000)
            throw new Error("Window (ms) must be a whole number between 1 and 600000");
          args.windowMs = ms;
        }
        if (traces.trim()) args.traces = list(traces);
        if (counters.trim()) args.counters = list(counters);
        if (att) {
          const opts: Json = {};
          const targetCu = intOption("Target CU", attTargetCu);
          const simdSelect = intOption("SIMD select", attSimdSelect);
          const bufferSize = intOption("Buffer size", attBufferSize);
          const shaderEngineMask = intOption("Shader engine mask", attShaderEngineMask);
          if (targetCu !== undefined) opts.targetCu = targetCu;
          if (simdSelect !== undefined) opts.simdSelect = simdSelect;
          if (bufferSize !== undefined) opts.bufferSize = bufferSize;
          if (shaderEngineMask !== undefined) opts.shaderEngineMask = shaderEngineMask;
          args.att = opts;
        }
        if (includeRegex.trim()) args.kernelIncludeRegex = includeRegex.trim();
        if (excludeRegex.trim()) args.kernelExcludeRegex = excludeRegex.trim();
        if (iterationRange.trim()) args.kernelIterationRange = iterationRange.trim();
      }
      const res = await liveProfileTrigger(args);
      if (!mounted.current) return;
      if (typeof res.window === "number")
        setWindows((w) => [...w, { count: res.window as number, mode: String(res.mode ?? "timed") }]);
      if (res.mode === "triggered") setOpenTriggered(true);
      setStatus(res);
    });

  const endWindow = () =>
    run(async () => {
      if (!profile) return;
      const res = await liveProfileTrigger({ id: profile.id as string, end: true });
      if (!mounted.current) return;
      setOpenTriggered(false);
      setStatus(res);
    });

  const poll = () =>
    run(async () => {
      if (!profile) return;
      const res = await liveProfileStatus(profile.id as string);
      if (!mounted.current) return;
      setStatus(res);
    }, false);

  const stop = () =>
    run(async () => {
      if (!profile) return;
      const res = await liveProfileStop(profile.id as string);
      if (!mounted.current) return;
      setResult(res);
      setProfile(null);
      setStatus(null);
      setOpenTriggered(false);
    });

  const warnings = (value: Json | null): string[] =>
    Array.isArray(value?.warnings) ? (value!.warnings as string[]) : [];

  if (!desktop)
    return (
      <div className="content live-profiling">
        <h1>Live profiling</h1>
        <p>
          Live profiling launches an application and attaches a profiler on the
          local machine, so it runs only in the desktop app. Start it with{" "}
          <code>npm run tauri dev</code> or the packaged build; the browser
          preview has no profiling server.
        </p>
      </div>
    );

  return (
    <div className="content live-profiling">
      <h1>Live profiling</h1>
      <p>
        Launch an application and gate GPU capture through the same core the MCP
        profile tools use. RADV captures RGP frames on a file trigger; rocprofv3
        attaches for timed or triggered windows.
      </p>
      {!profile ? (
        <section className="live-form">
          <label>
            Tracer
            <select value={tracer} onChange={(e) => setTracer(e.target.value)} disabled={busy}>
              <option value="rocprofv3">rocprofv3 (HIP/ROCm attach)</option>
              <option value="radv">radv (Vulkan RGP)</option>
            </select>
          </label>
          <label>
            Command
            <textarea
              value={argv}
              onChange={(e) => setArgv(e.target.value)}
              placeholder="/path/to/llama-server --model model.gguf -ngl 99"
              disabled={busy}
            />
          </label>
          <label>
            Working directory
            <input value={cwd} onChange={(e) => setCwd(e.target.value)} placeholder="/path/to/cwd" disabled={busy} />
          </label>
          {rocprofv3 && (
            <label>
              rocprofv3 path (optional)
              <input
                value={profiler}
                onChange={(e) => setProfiler(e.target.value)}
                placeholder="/opt/rocm/bin/rocprofv3"
                disabled={busy}
              />
            </label>
          )}
          <button onClick={start} disabled={busy || !argv.trim() || !cwd.trim()}>
            Start profiling
          </button>
        </section>
      ) : (
        <section className="live-session">
          <p className="live-id">
            Running <strong>{String(profile.id)}</strong> · pid {String(profile.pid)}
          </p>
          {typeof profile.triggerFile === "string" && (
            <p>
              RADV trigger file: <code>{profile.triggerFile}</code>
            </p>
          )}
          {rocprofv3 ? (
            <>
              <label>
                Window mode
                <select
                  value={triggered ? "triggered" : "timed"}
                  onChange={(e) => setTriggered(e.target.value === "triggered")}
                  disabled={busy || openTriggered}
                >
                  <option value="timed">Timed (windowMs)</option>
                  <option value="triggered">Triggered (end on demand)</option>
                </select>
              </label>
              {!triggered && (
                <label>
                  Window (ms)
                  <input
                    type="number"
                    min={1}
                    max={600000}
                    value={windowMs}
                    onChange={(e) => setWindowMs(e.target.value)}
                    disabled={busy}
                  />
                </label>
              )}
              <label>
                Traces
                <input
                  value={traces}
                  onChange={(e) => setTraces(e.target.value)}
                  placeholder="kernel, hip"
                  disabled={busy}
                />
              </label>
              <label>
                Counters
                <input value={counters} onChange={(e) => setCounters(e.target.value)} disabled={busy} />
              </label>
              <label className="live-check">
                <input type="checkbox" checked={att} onChange={(e) => setAtt(e.target.checked)} disabled={busy} />
                ATT thread trace
              </label>
              {att && (
                <div className="live-att">
                  <label>
                    ATT target CU
                    <input type="number" min={0} value={attTargetCu} onChange={(e) => setAttTargetCu(e.target.value)} disabled={busy} />
                  </label>
                  <label>
                    ATT SIMD select
                    <input type="number" min={0} value={attSimdSelect} onChange={(e) => setAttSimdSelect(e.target.value)} disabled={busy} />
                  </label>
                  <label>
                    ATT buffer size (bytes)
                    <input type="number" min={0} value={attBufferSize} onChange={(e) => setAttBufferSize(e.target.value)} disabled={busy} />
                  </label>
                  <label>
                    ATT shader engine mask
                    <input type="number" min={0} value={attShaderEngineMask} onChange={(e) => setAttShaderEngineMask(e.target.value)} disabled={busy} />
                  </label>
                </div>
              )}
              <label>
                Kernel include regex
                <input value={includeRegex} onChange={(e) => setIncludeRegex(e.target.value)} disabled={busy} />
              </label>
              <label>
                Kernel exclude regex
                <input value={excludeRegex} onChange={(e) => setExcludeRegex(e.target.value)} disabled={busy} />
              </label>
              <label>
                Kernel iteration range
                <input
                  value={iterationRange}
                  onChange={(e) => setIterationRange(e.target.value)}
                  placeholder="64 or 0-3 (targets a later dispatch under a kernel name)"
                  disabled={busy}
                />
              </label>
              <div className="live-actions">
                <button onClick={trigger} disabled={busy || openTriggered}>
                  Trigger window
                </button>
                {openTriggered && (
                  <button onClick={endWindow} disabled={busy}>
                    End window
                  </button>
                )}
                <button onClick={poll} disabled={busy}>
                  Refresh status
                </button>
                <button onClick={stop} disabled={busy}>
                  Stop and collect
                </button>
              </div>
            </>
          ) : (
            <div className="live-actions">
              <button onClick={trigger} disabled={busy}>
                Arm RGP trigger
              </button>
              <button onClick={poll} disabled={busy}>
                Refresh status
              </button>
              <button onClick={stop} disabled={busy}>
                Stop and collect
              </button>
            </div>
          )}
          {windows.length > 0 && (
            <div className="live-windows">
              <strong>Windows</strong>
              <ul>
                {windows.map((w) => (
                  <li key={w.count}>
                    window-{w.count} · {w.mode}
                  </li>
                ))}
              </ul>
            </div>
          )}
          {status && (
            <p className="live-status">
              {status.windowRunning
                ? "Window running…"
                : status.triggerPending
                  ? "Trigger pending…"
                  : "Idle"}
              {typeof status.attachAttempts === "number" ? ` · attach attempts: ${status.attachAttempts}` : ""}
            </p>
          )}
          {warnings(status).map((w, i) => (
            <p key={i} role="alert" className="live-warning">
              {w}
            </p>
          ))}
        </section>
      )}
      {result && (
        <section className="live-result">
          <h2>Collected</h2>
          <p>
            {Array.isArray(result.files) ? (result.files as unknown[]).length : 0} file(s) in{" "}
            <code>{String(result.directory)}</code>
          </p>
          {warnings(result).map((w, i) => (
            <p key={i} role="alert" className="live-warning">
              {w}
            </p>
          ))}
        </section>
      )}
      {error && (
        <p role="alert" className="live-error">
          {error}
        </p>
      )}
    </div>
  );
}
