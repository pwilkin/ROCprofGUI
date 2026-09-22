import { useEffect, useMemo, useState } from "react";
import { useAnalysisPreference } from "../analysisPreferences";
import type { Capture } from "../types";
import { searchObservationRows } from "../observationSearch";
import { Panel } from "./AnalysisViews";

export default function PcSamplingObservations({
  capture,
}: {
  capture: Capture;
}) {
  const observations = capture.pcSamplingObservations!;
  const [search, setSearch] = useAnalysisPreference<string>(
    "sampling.search",
    "",
  );
  const [page, setPage] = useAnalysisPreference<number>("sampling.page", 0);
  const [result, setResult] = useState<{
    observations: typeof observations;
    query: string;
    matches: number[] | null;
    error?: string;
  } | null>(null);
  useEffect(() => {
    const controller = new AbortController();
    if (search) {
      searchObservationRows(observations.rows, search, controller.signal).then(
        (matches) => {
          if (!controller.signal.aborted)
            setResult({ observations, query: search, matches });
        },
        (error) => {
          if (!controller.signal.aborted)
            setResult({
              observations,
              query: search,
              matches: [],
              error: String(error),
            });
        },
      );
    } else setResult(null);
    return () => controller.abort();
  }, [observations, search]);
  const active =
    result?.observations === observations && result.query === search
      ? result
      : null;
  const pending = !!search && !active;
  const count = search
    ? (active?.matches?.length ?? 0)
    : observations.rows.length;
  const pages = Math.max(1, Math.ceil(count / 100));
  const current = Math.min(page, pages - 1);
  const visible = search
    ? (active?.matches ?? []).slice(current * 100, (current + 1) * 100)
    : Array.from(
        { length: Math.min(100, count - current * 100) },
        (_, i) => current * 100 + i,
      );
  const status = useMemo(() => {
    const column = observations.columns.indexOf("Wave_Issued_Instruction");
    if (column < 0) return null;
    const issued = observations.rows.reduce(
      (count, row) => count + Number(row[column].trim() === "1"),
      0,
    );
    return { issued, stalled: observations.rows.length - issued };
  }, [observations]);
  return (
    <Panel
      title="PC sampling observations"
      detail="Individual CSV records · input order and duplicates preserved"
    >
      <p className="notice">
        {observations.rows.length.toLocaleString()}{" "}
        {capture.metadata.pc_sampling_method} samples.
        {status
          ? ` ${status.issued.toLocaleString()} issued · ${status.stalled.toLocaleString()} not issued.`
          : " Issued/stalled status was not collected."}{" "}
        CSV omits resolved PC and code-object identities. Equal instruction text
        can describe different ISA locations. Open matching SDK JSON for ISA
        attribution. Dispatch and correlation IDs are recorded values; no
        dispatch association is inferred. Stall reasons on issued samples are
        not meaningful; the table retains the producer’s original fields.
      </p>
      <div className="toolbar-inline">
        <input
          aria-label="Search PC sampling observations"
          placeholder="Search any recorded field…"
          value={search}
          onChange={(event) => {
            setSearch(event.target.value);
            setPage(0);
          }}
        />
        <span role="status">
          {pending
            ? "Searching observations…"
            : `${count.toLocaleString()} matching samples`}
        </span>
        <button
          disabled={pending || current === 0}
          onClick={() => setPage(current - 1)}
        >
          Previous samples
        </button>
        <span>
          Page {current + 1} / {pages}
        </span>
        <button
          disabled={pending || current + 1 >= pages}
          onClick={() => setPage(current + 1)}
        >
          Next samples
        </button>
      </div>
      <div className="table-scroll" aria-busy={pending}>
        {active?.error && (
          <p role="alert">Observation search failed: {active.error}</p>
        )}
        <table aria-label="PC sampling CSV observations">
          <thead>
            <tr>
              <th>Input row</th>
              {observations.columns.map((column) => (
                <th key={column}>{column}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {visible.map((index) => (
              <tr key={index}>
                <td>{index + 1}</td>
                {observations.rows[index].map((field, column) => (
                  <td key={column} style={{ whiteSpace: "pre-wrap" }}>
                    {field}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
        {!pending && !active?.error && !count && (
          <p>No samples match the search.</p>
        )}
      </div>
    </Panel>
  );
}
