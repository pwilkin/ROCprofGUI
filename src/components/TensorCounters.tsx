import { useAnalysisPreference } from "../analysisPreferences";
import { DefinitionWorkspace } from "../definitionWorkspace";
import { plotAxisKey } from "../plotAxes";
import { useEffect, useMemo, useRef, useState, useContext } from "react";
import type { Capture, Range } from "../types";
import { counterDomains } from "../counterTensors";
import { type Tensor } from "../tensor";
import { savePreference } from "../session";
import DefinitionLibrary from "./DefinitionLibrary";
import {
  editDefinition,
  type DefinitionLibrary as Library,
} from "../definitionLibrary";
import { startCounterJob, type CounterJob } from "../counterJob";
import { counterPlotPage, hydrateResults } from "../counterEvaluation";
import { LineChart } from "./Plots";
export default function TensorCounters({
  capture,
  range,
}: {
  capture: Capture;
  range: Range;
}) {
  const data = useMemo(() => {
    try {
      return { groups: counterDomains(capture.counters), error: "" };
    } catch (e) {
      return { groups: [], error: String(e) };
    }
  }, [capture]);
  const activeJob = useRef<CounterJob | null>(null);
  const generation = useRef(0);
  const [evaluating, setEvaluating] = useState(false);
  function stopEvaluation() {
    generation.current++;
    activeJob.current?.cancel();
    activeJob.current = null;
    setEvaluating(false);
  }
  useEffect(
    () => () => {
      generation.current++;
      activeJob.current?.cancel();
    },
    [],
  );
  useEffect(() => {
    stopEvaluation();
    setResults(new Map());
    setError("");
  }, [capture]);
  const [fileBusy, setFileBusy] = useState(false);
  const workspace = useContext(DefinitionWorkspace)!;
  const { library, storageError } = workspace;
  const definitions = library.files.find(
    (f) => f.id === library.selected,
  )!.text;
  function updateLibrary(next: Library) {
    stopEvaluation();
    workspace.update(next);
    setResults(new Map());
    setError("");
  }
  const [savedScope, setSavedScope] = useAnalysisPreference<string>(
    "tensor.scope",
    "",
  );
  const groupIndex = Math.max(
    0,
    data.groups.findIndex((group) => group.scope === savedScope),
  );
  const [results, setResults] = useState<Map<string, Tensor>>(new Map()),
    [error, setError] = useState(""),
    [evaluatedScope, setEvaluatedScope] = useState(""),
    [times, setTimes] = useState<number[]>([]),
    [page, setPage] = useState(0);
  const group = data.groups[groupIndex];
  async function apply() {
    stopEvaluation();
    const current = generation.current;
    setResults(new Map());
    setError("");
    try {
      if (!group) throw Error("No dimensioned counters in capture");
      setEvaluating(true);
      const job = startCounterJob({
        definitions,
        series: group.series,
        scope: group.scope,
      });
      activeJob.current = job;
      const output = await job.result;
      if (generation.current !== current) return;
      setResults(hydrateResults(output));
      setTimes(output.times);
      setEvaluatedScope(output.scope);
      setPage(0);
      if (!workspace.projectLocal)
        savePreference("rocprof:tensor-definitions", definitions);
    } catch (e) {
      if (generation.current === current) setError(String(e));
    } finally {
      if (generation.current === current) {
        activeJob.current = null;
        setEvaluating(false);
      }
    }
  }
  const {
    plots,
    total: plotCount,
    page: safePage,
  } = counterPlotPage(results, page);
  return (
    <section className="tensor-editor">
      <h3>Dimension-aware definitions</h3>
      {workspace.projectLocal && (
        <p className="muted small">
          Project definition library. Save the project to retain edits;
          application drafts are unchanged.
        </p>
      )}
      <p className="muted small">
        Upstream syntax: sum[COUNTER, axis=[SE,CU]], select[COUNTER, 0:4,
        axis=Time], delta[COUNTER, axis=Time]. One name := expression per line;
        definitions can reference each other. Missing samples remain gaps.
        INSTANCE/WGP counters use the CU axis convention.
      </p>
      {data.error && <p role="alert">{data.error}</p>}
      <label>
        Counter domain{" "}
        <select
          aria-label="Counter tensor domain"
          value={groupIndex}
          onChange={(e) => {
            stopEvaluation();
            setError("");
            setSavedScope(data.groups[Number(e.target.value)]?.scope || "");
            setResults(new Map());
          }}
        >
          {data.groups.map((g, i) => (
            <option key={g.scope} value={i}>
              {g.scope}
            </option>
          ))}
        </select>
      </label>
      <p className="small mono">
        {group?.names.join(" · ") || "No recognized counter dimensions"}
      </p>
      {group?.scope.startsWith("process ") && (
        <p className="small">
          SCLOCK is the plotted sample timestamp (shader cycles after
          calibration). SPM_CLOCK is the interval between original realtime
          samples on that XCC, in source SPM time units. The first interval uses
          the mean observed interval (zero for one sample), matching upstream.
          Calibration does not establish lost samples or measured completion
          latency.
        </p>
      )}
      {group?.bindings.some((binding) => binding.symbol !== binding.name) && (
        <details className="counter-bindings">
          <summary>Counter expression names</summary>
          <p>
            Use these names in definitions. Recorded counter names and values
            are unchanged.
          </p>
          <table>
            <thead>
              <tr>
                <th>Expression name</th>
                <th>Recorded counter</th>
                <th>Counter ID</th>
              </tr>
            </thead>
            <tbody>
              {group.bindings.map((binding) => (
                <tr key={binding.key}>
                  <td>
                    <code>{binding.symbol}</code>
                  </td>
                  <td>{binding.name}</td>
                  <td>{binding.counterId ?? "Not recorded"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </details>
      )}
      <DefinitionLibrary
        library={library}
        onChange={updateLibrary}
        storageError={storageError}
        onBusy={setFileBusy}
      />
      <textarea
        aria-label="Counter definitions"
        disabled={fileBusy}
        rows={5}
        spellCheck={false}
        className="mono"
        value={definitions}
        onChange={(e) => {
          try {
            updateLibrary(editDefinition(library, { text: e.target.value }));
          } catch (error) {
            setError(String(error));
          }
        }}
      />
      <button
        onClick={apply}
        className="primary"
        disabled={!group || fileBusy || evaluating}
      >
        Evaluate definitions
      </button>
      {evaluating && (
        <div role="status">
          Evaluating counters in background…{" "}
          <button
            onClick={() => {
              stopEvaluation();
              setError("Counter evaluation cancelled");
            }}
          >
            Cancel evaluation
          </button>
        </div>
      )}
      {error && (
        <p role="alert" className="error-text">
          {error}
        </p>
      )}
      {plots.map(({ name, tensor, n, coords }) => {
        const values = tensor.values.slice(
          n * tensor.shape[3],
          (n + 1) * tensor.shape[3],
        );
        const timeIndices = tensor.axisIndices[3];
        const clockAligned =
          !!timeIndices && timeIndices.every((i) => i < times.length);
        const points = values.map(
          (v, i) =>
            [clockAligned ? times[timeIndices![i]] : i, v] as [number, number],
        );
        const locations = coords.map(
          (index, axis) => tensor.axisIndices[axis]?.[index] ?? "reduced",
        );
        const label = `${name} · XCC ${locations[0]}, SE ${locations[1]}, CU/INSTANCE ${locations[2]}`;
        return (
          <div className="tensor-result" key={`${name}:${n}`}>
            <b>{label}</b>
            <p className="muted small">
              {evaluatedScope} · user-defined units ·{" "}
              {clockAligned
                ? "capture timestamps"
                : "transformed Time axis: sample index"}{" "}
              · {values.filter(Number.isFinite).length}/{values.length} valid
              values
            </p>
            {values.length === 1 ? (
              <output aria-label={label}>
                {Number.isFinite(values[0]) ? values[0] : "No valid samples"}
              </output>
            ) : (
              <LineChart
                xBounds={
                  clockAligned
                    ? [capture.start, capture.end]
                    : [0, Math.max(1, values.length - 1)]
                }
                axisKey={plotAxisKey(
                  "tensor",
                  evaluatedScope,
                  label,
                  clockAligned ? "clock" : "index",
                )}
                points={points}
                range={clockAligned ? range : [0, values.length - 1]}
                label={label}
              />
            )}
          </div>
        );
      })}
      {!!plotCount && (
        <div className="toolbar-inline">
          <button disabled={!safePage} onClick={() => setPage(safePage - 1)}>
            Previous results
          </button>
          <span>
            {safePage * 12 + 1}–{Math.min((safePage + 1) * 12, plotCount)} of{" "}
            {plotCount} series
          </span>
          <button
            disabled={(safePage + 1) * 12 >= plotCount}
            onClick={() => setPage(safePage + 1)}
          >
            Next results
          </button>
        </div>
      )}
    </section>
  );
}
