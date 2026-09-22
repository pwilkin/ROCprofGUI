import { useAnalysisPreference } from "../analysisPreferences";
import { useEffect, useState } from "react";
import type { Capture } from "../types";
import { fmt } from "../types";
import { requestActivityCost, type ActivityCost, type HardwareSummary } from "../summary";
import { requestHardwareSummary } from "../summaryWorker";
import { Empty, Panel } from "./AnalysisViews";
const percentage = (value: number | null) =>
  value == null ? "Unavailable" : `${fmt(value)}%`;
export default function SummaryView({ capture }: { capture: Capture }) {
  const [activityState, setActivityState] = useState<{
    capture: Capture; result?: ActivityCost | null; error?: string;
  } | null>(null);
  useEffect(() => {
    const controller = new AbortController();
    setActivityState(null);
    if (capture.metadata.instruction_costs === "incomplete") {
      setActivityState({capture, result: null});
    } else {
      requestActivityCost(capture.instructions, controller.signal).then(
        result => { if (!controller.signal.aborted) setActivityState({capture, result}); },
        error => { if (!controller.signal.aborted) setActivityState({capture, error: String(error)}); },
      );
    }
    return () => controller.abort();
  }, [capture]);
  const currentActivity = activityState?.capture === capture ? activityState : null;
  const activity = currentActivity?.result;
  const [state, setState] = useState<{capture: Capture; result?: HardwareSummary; error?: string} | null>(null);
  useEffect(() => {
    const controller = new AbortController();
    setState(null);
    requestHardwareSummary(capture.architecture, capture.counters, controller.signal).then(
      result => { if (!controller.signal.aborted) setState({capture, result}); },
      error => { if (!controller.signal.aborted) setState({capture, error:String(error)}); },
    );
    return () => controller.abort();
  }, [capture]);
  const currentState = state?.capture === capture ? state : null;
  const summary = currentState?.result ?? {columns:[], counters:[], rows:[], peakCounts:[], warnings:[], delta:4096};
  const [page, setPage] = useAnalysisPreference<number>("summary.page", 0);
  const [counts, setCounts] = useAnalysisPreference<boolean>(
    "summary.counts",
    false,
  );
  const current = Math.min(
    page,
    Math.max(0, Math.ceil(summary.rows.length / 32) - 1),
  );
  const costs = activity
    ? [
        { name: "Idle", value: activity.idle, color: "#7f95a8" },
        { name: "Stall / wait", value: activity.stall, color: "#eeb669" },
        { name: "Issue", value: activity.issue, color: "#78c4a4" },
      ]
    : [];
  const maximum = Math.max(
    100,
    ...summary.columns.flatMap((c) => [c.average ?? 0, c.peak ?? 0]),
  );
  return (
    <>
      <Panel
        title="Instruction activity distribution"
        detail="Whole-capture imported ISA costs · unaffected by timeline selection"
      >
        {!currentActivity ? (
          <p role="status">Calculating instruction activity…</p>
        ) : currentActivity.error ? (
          <p role="alert">Instruction activity failed: {currentActivity.error}</p>
        ) : activity ? (
          <div className="summary-activity">
            <div
              className="summary-stack"
              role="img"
              aria-label="Accumulated instruction cost distribution"
            >
              {costs.map((c) => (
                <div
                  key={c.name}
                  style={{
                    width: `${(100 * c.value) / activity.total}%`,
                    background: c.color,
                  }}
                  title={`${c.name}: ${percentage((100 * c.value) / activity.total)}`}
                />
              ))}
            </div>
            <div className="summary-legend">
              {costs.map((c) => (
                <div key={c.name}>
                  <span style={{ color: c.color }}>{c.name}</span>
                  <strong>
                    {percentage((100 * c.value) / activity.total)}
                  </strong>
                  <small>
                    {fmt(c.value)} {capture.unit}
                  </small>
                </div>
              ))}
            </div>
            <p>
              Issue = latency − stall. Total = idle + stall + issue. These are
              accumulated instruction costs, not elapsed-time GPU utilization;
              overlapping instruction costs can exceed wall time.
            </p>
          </div>
        ) : (
          <Empty
            title="Instruction costs unavailable"
            detail="Latency, stall and idle must all be collected, finite and nonnegative, with a positive total and no stall exceeding instruction latency."
          />
        )}
      </Panel>
      <Panel
        title="Hardware activity"
        detail="MI200 / MI300 ATT activity model · whole capture"
      >
        {!currentState ? (
          <p role="status">Calculating hardware activity…</p>
        ) : currentState.error ? (
          <p role="alert">Hardware activity failed: {currentState.error}</p>
        ) : summary.unavailable ? (
          <Empty
            title="Hardware summary unavailable"
            detail={summary.unavailable}
          />
        ) : (
          <>
            {summary.warnings.map((warning) => (
              <p className="summary-note" role="status" key={warning}>
                {warning}
              </p>
            ))}
            {summary.columns.length ? (
              <div className="summary-bars">
                {summary.columns.map((column) => (
                  <div className="summary-category" key={column.counter}>
                    <h4
                      title={`${column.counter} × ${column.scale} / BUSY_CU_CYCLES × 100`}
                    >
                      {column.name}
                    </h4>
                    {(["average", "peak"] as const).map((kind) => (
                      <div
                        className="summary-bar-row"
                        key={kind}
                        title={column.reason}
                      >
                        <span>
                          {kind === "average" ? "Average" : "Peak ratio"}
                        </span>
                        <div className="summary-bar-track">
                          <div
                            style={{
                              width: `${(100 * (column[kind] ?? 0)) / maximum}%`,
                            }}
                            className={kind}
                          />
                        </div>
                        <strong>{percentage(column[kind])}</strong>
                      </div>
                    ))}
                  </div>
                ))}
                <p className="summary-note">
                  Shared scale: 0–{fmt(maximum)}%. Average divides accumulated
                  activity by accumulated busy quadcycles. Peak ratio divides
                  each counter’s aggregate peak by the busy counter’s aggregate
                  peak; these peaks need not coincide. This is not the maximum
                  instantaneous percentage.
                </p>
              </div>
            ) : (
              <Empty
                title="Activity counters unavailable"
                detail="No ACTIVE_INST_* or VALU_MFMA_BUSY_CYCLES counters were collected."
              />
            )}
            <details className="summary-method">
              <summary>Counter formulas and collection</summary>
              <p>
                VMEM, LDS and FLAT activity counts are multiplied by 2. MFMA
                busy cycles are divided by 4 to match busy quadcycles. Other
                activity counts use a factor of 1. Values are not capped at
                100%: MI300 VALU can reach 200% with dual issue.
              </p>
              <p>
                Peaks use upstream’s most-recent-sample aggregation, gap-zero
                insertion and four-clock coalescing; inferred sample interval:{" "}
                {fmt(summary.delta)} clocks. No terminal zero is inserted.
                Different counter banks may sample different periods. No time
                alignment or simultaneous activity is inferred across banks.
              </p>
              <p>
                Collect using{" "}
                <code>rocprofv3 --att-activity 10 -- &lt;application&gt;</code>,
                or select SQ_BUSY_CU_CYCLES, SQ_VALU_MFMA_BUSY_CYCLES and
                SQ_ACTIVE_INST_* counters with ATT counter controls. The
                profiler and hardware must support these options.
              </p>
            </details>
          </>
        )}
      </Panel>
      {!summary.unavailable && summary.rows.length > 0 && (
        <Panel
          title="Per-CU activity and counters"
          detail="Only recorded SE / CU coordinates · missing values remain unavailable"
          tools={
            <button className="button" onClick={() => setCounts(!counts)}>
              {counts ? "Show utilization" : "Show accumulated counters"}
            </button>
          }
        >
          <div className="summary-table-scroll">
            <table className="summary-table">
              <thead>
                <tr>
                  <th scope="col">Coordinate</th>
                  {(counts
                    ? summary.counters
                    : summary.columns.map((c) => `${c.name} utilization`)
                  ).map((name) => (
                    <th scope="col" key={name}>
                      {name}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                <tr>
                  <th scope="row">Aggregate peak</th>
                  {(counts
                    ? summary.peakCounts
                    : summary.columns.map((c) => c.peak)
                  ).map((value, n) => (
                    <td key={n}>
                      {counts
                        ? value == null
                          ? "Unavailable"
                          : fmt(value)
                        : percentage(value)}
                    </td>
                  ))}
                </tr>
                {summary.rows
                  .slice(current * 32, (current + 1) * 32)
                  .map((row) => (
                    <tr key={row.coordinate}>
                      <th scope="row">
                        SE {row.coordinate.split(":")[0]} · CU{" "}
                        {row.coordinate.split(":")[1]}
                      </th>
                      {(counts ? row.accumulated : row.utilization).map(
                        (value, n) => (
                          <td key={n}>
                            {counts
                              ? value == null
                                ? "Unavailable"
                                : fmt(value)
                              : percentage(value)}
                          </td>
                        ),
                      )}
                    </tr>
                  ))}
              </tbody>
            </table>
          </div>
          {counts && (
            <p className="summary-note">
              Upstream-compatible accumulated counters: ACTIVE_INST_LDS / VMEM /
              FLAT are scaled ×2; MFMA busy remains in cycles here. The Counters
              view retains original unscaled samples.
            </p>
          )}
          <div className="summary-pagination">
            <button
              className="button"
              disabled={!current}
              onClick={() => setPage(current - 1)}
            >
              Previous CUs
            </button>
            <span>
              {current * 32 + 1}–
              {Math.min((current + 1) * 32, summary.rows.length)} of{" "}
              {summary.rows.length} CUs
            </span>
            <button
              className="button"
              disabled={(current + 1) * 32 >= summary.rows.length}
              onClick={() => setPage(current + 1)}
            >
              Next CUs
            </button>
          </div>
        </Panel>
      )}
    </>
  );
}
