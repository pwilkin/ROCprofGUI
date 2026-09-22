import PcSamplingObservations from "./PcSamplingObservations";
import { useAnalysisPreference } from "../analysisPreferences";
import { useMemo } from "react";
import type { Capture } from "../types";
import { fmt } from "../types";
import { sampleCount, samplingTotals } from "../pcSampling";
import { Empty, Panel, FlameView } from "./AnalysisViews";
const value = (n: number | null | undefined) =>
  n == null ? "Unavailable" : fmt(n);
export default function PcSampling({
  capture,
  onSelect,
}: {
  capture: Capture;
  onSelect: (id: number) => void;
}) {
  const hostTrap = capture.metadata.pc_sampling_method === "host-trap";
  const [flame, setFlame] = useAnalysisPreference<boolean>(
    "sampling.flame",
    false,
  );
  const [metric, setMetric] = useAnalysisPreference<
    "pcSamples" | "pcIssued" | "pcStalled"
  >("sampling.metric", hostTrap ? "pcSamples" : "pcStalled");
  const [search, setSearch] = useAnalysisPreference<string>(
    "sampling.search",
    "",
  );
  const [selected, setSelected] = useAnalysisPreference<number | null>(
    "sampling.selected",
    null,
  );
  const [page, setPage] = useAnalysisPreference<number>("sampling.page", 0);
  const [order, setOrder] = useAnalysisPreference<string>(
    "sampling.order",
    hostTrap ? "samples" : "stalled",
  );
  const sampled = useMemo(
    () =>
      capture.instructions.filter(
        (i) =>
          sampleCount(i) !== null ||
          i.pcIssued != null ||
          i.pcStalled != null ||
          i.stallReasons != null,
      ),
    [capture],
  );
  const rows = useMemo(
    () =>
      sampled
        .filter((i) =>
          `${i.text} ${i.source}`.toLowerCase().includes(search.toLowerCase()),
        )
        .sort((a, b) =>
          order === "samples"
            ? (sampleCount(b) ?? -1) - (sampleCount(a) ?? -1)
            : order === "issued"
              ? (b.pcIssued ?? -1) - (a.pcIssued ?? -1)
              : (b.pcStalled ?? -1) - (a.pcStalled ?? -1),
        ),
    [sampled, search, order],
  );
  const all = useMemo(() => samplingTotals(capture.instructions), [capture]);
  const instruction = capture.instructions.find((i) => i.index === selected);
  const totals = useMemo(
    () => (instruction ? samplingTotals([instruction]) : all),
    [instruction, all],
  );
  const maximum = Math.max(1, ...totals.reasons.map((r) => r.count ?? 0));
  const current = Math.min(page, Math.max(0, Math.ceil(rows.length / 100) - 1));
  const rawSampling =
    hostTrap || capture.metadata.pc_sampling_method === "stochastic";
  if (capture.pcSamplingObservations)
    return <PcSamplingObservations capture={capture} />;
  if (!sampled.length)
    return (
      <Empty
        title={
          rawSampling
            ? "Samples have no resolved ISA locations"
            : "PC sampling was not collected"
        }
        detail={
          rawSampling
            ? `${capture.metadata.pc_sampling_records} ${capture.metadata.pc_sampling_method} samples were read; ${capture.metadata.pc_sampling_unresolved} have unresolved code objects. No ISA counts can be attributed from this input.`
            : "Open host-trap or stochastic rocprofv3 PC sampling JSON or decoded code.json data containing PC_Issued, PC_Stalled or Stall_Reasons columns. SQTT latency is not a PC sampling measurement."
        }
      />
    );
  return (
    <>
      <Panel
        title="PC sampling"
        detail="Whole-capture statistical samples · independent of SQTT range and hidden-latency controls"
      >
        {rawSampling && (
          <p className="notice">
            {capture.metadata.pc_sampling_records} raw samples ·{" "}
            {capture.metadata.pc_sampling_unresolved} unresolved samples
            excluded from ISA totals.
            {hostTrap
              ? " Host-trap does not report issued/stalled status or stall reasons."
              : ` ${capture.metadata.pc_sampling_lock_errors} samples report lock contention. Lock-contention samples remain included.`}{" "}
            Counts are statistical observations, not instruction executions.
          </p>
        )}
        {capture.metadata.pc_sampling_other_method_records && (
          <p className="notice">
            {capture.metadata.pc_sampling_other_method_records} observations
            from the other sampling method are excluded from this capture. Open
            the input again to inspect that method in a separate tab.
          </p>
        )}
        <div className="summary-activity">
          <div className="summary-legend">
            <div>
              <span>Attributed samples</span>
              <strong>{value(all.samples)}</strong>
              <small>
                {all.sampleRows} / {all.rows} rows have a sample total
              </small>
            </div>
            <div>
              <span>Issued samples</span>
              <strong>{value(all.issued)}</strong>
              <small>
                {all.issuedRows} / {all.rows} rows reported
              </small>
            </div>
            <div>
              <span>Stalled samples</span>
              <strong>{value(all.stalled)}</strong>
              <small>
                {all.stalledRows} / {all.rows} rows reported
              </small>
            </div>
          </div>
          <p>
            Counts are sampled observations, not instruction execution counts,
            cycles or a percentage of GPU time. Partial row coverage is shown
            explicitly. Reasons retain the producer’s category indices; their
            sum is not assumed to equal stalled samples.
          </p>
        </div>
      </Panel>
      <Panel
        title="Sampled stall reasons"
        detail={
          instruction
            ? `ISA ${instruction.index} · ${instruction.text}`
            : "All sampled instructions"
        }
        tools={
          instruction && (
            <button onClick={() => setSelected(null)}>
              All sampled instructions
            </button>
          )
        }
      >
        {totals.reasonRows ? (
          <div className="summary-bars">
            {totals.reasons.map((r) => (
              <div
                className="summary-bar-row"
                key={r.index}
                style={{ gridTemplateColumns: "165px 1fr 100px" }}
              >
                <span>
                  {r.index} · {r.name}
                </span>
                <div className="summary-bar-track">
                  <div
                    style={{ width: `${(100 * (r.count ?? 0)) / maximum}%` }}
                  />
                </div>
                <strong>{value(r.count)}</strong>
              </div>
            ))}
            <p className="summary-note">
              {totals.reasonRows} / {totals.rows} rows contain a reason array.
              Category names follow the upstream viewer’s decoded schema.
              Unknown categories are retained.
            </p>
          </div>
        ) : (
          <Empty
            title="Stall reasons unavailable"
            detail="Issued/stalled counts do not imply that a reason breakdown was collected."
          />
        )}
      </Panel>
      <div className="summary-pagination">
        <button onClick={() => setFlame(!flame)}>
          {flame ? "Hide sample flamegraph" : "Show sample flamegraph"}
        </button>
        {flame && (
          <select
            aria-label="Sample flamegraph weight"
            value={metric}
            onChange={(e) =>
              setMetric(
                e.target.value as "pcSamples" | "pcIssued" | "pcStalled",
              )
            }
          >
            <option value="pcSamples">Attributed samples</option>
            <option value="pcStalled">Stalled samples</option>
            <option value="pcIssued">Issued samples</option>
          </select>
        )}
      </div>
      {flame && (
        <FlameView
          key={metric}
          instructions={sampled}
          onSelect={onSelect}
          metric={metric}
          exportContext={`${capture.name} · whole capture`}
        />
      )}
      <Panel
        title="Sampled instructions"
        detail="Select a row for its reason breakdown, or open it in the linked ISA / source workspace"
      >
        <div className="summary-pagination">
          <input
            aria-label="Search sampled instructions"
            value={search}
            placeholder="Search ISA or source"
            onChange={(e) => {
              setSearch(e.target.value);
              setPage(0);
            }}
          />
          <select
            aria-label="Sort sampled instructions"
            value={order}
            onChange={(e) => {
              setOrder(e.target.value);
              setPage(0);
            }}
          >
            <option value="samples">Most attributed samples</option>
            <option value="stalled">Most stalled samples</option>
            <option value="issued">Most issued samples</option>
          </select>
        </div>
        <div className="summary-table-scroll">
          <table className="summary-table">
            <thead>
              <tr>
                <th>Line</th>
                <th>Instruction</th>
                <th>Samples</th>
                <th>Issued</th>
                <th>Stalled</th>
                <th>Source</th>
                <th>Details</th>
              </tr>
            </thead>
            <tbody>
              {rows.slice(current * 100, (current + 1) * 100).map((i) => (
                <tr key={i.index}>
                  <th scope="row">{i.index}</th>
                  <td>
                    <code>{i.text}</code>
                  </td>
                  <td>{value(sampleCount(i))}</td>
                  <td>{value(i.pcIssued)}</td>
                  <td>{value(i.pcStalled)}</td>
                  <td>{i.source || "No source mapping"}</td>
                  <td>
                    <button
                      aria-pressed={selected === i.index}
                      onClick={() => setSelected(i.index)}
                    >
                      Reasons for line {i.index}
                    </button>{" "}
                    <button onClick={() => onSelect(i.index)}>
                      Open ISA {i.index}
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        {!rows.length && (
          <p className="summary-note">
            No sampled instructions match this search.
          </p>
        )}
        <div className="summary-pagination">
          <button disabled={!current} onClick={() => setPage(current - 1)}>
            Previous instructions
          </button>
          <span>
            {rows.length ? current * 100 + 1 : 0}–
            {Math.min(rows.length, (current + 1) * 100)} of {rows.length}
          </span>
          <button
            disabled={(current + 1) * 100 >= rows.length}
            onClick={() => setPage(current + 1)}
          >
            Next instructions
          </button>
        </div>
      </Panel>
    </>
  );
}
