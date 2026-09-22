import { useAnalysisPreference } from "../analysisPreferences";
import { useMemo } from "react";
import type { Capture, Dispatch } from "../types";
import { fmt } from "../types";
export default function EventCounters({
  capture,
  onDispatch,
}: {
  capture: Capture;
  onDispatch: (dispatch: Dispatch) => void;
}) {
  const [search, setSearch] = useAnalysisPreference<string>(
      "events.search",
      "",
    ),
    [page, setPage] = useAnalysisPreference<number>("events.page", 0);
  const dispatches = useMemo(
    () => new Map(capture.dispatches.map((d) => [d.id, d])),
    [capture],
  );
  const records = capture.eventCounters || [];
  const rows = useMemo(
    () =>
      records.filter((r) =>
        `${r.name} ${r.scope} ${r.unit} ${r.event || ""} ${r.counterId}`
          .toLowerCase()
          .includes(search.toLowerCase()),
      ),
    [records, search],
  );
  const last = Math.max(0, Math.ceil(rows.length / 100) - 1),
    current = Math.min(page, last);
  if (!records.length) return null;
  return (
    <section className="panel">
      <div className="panel-heading">
        <div>
          <h3>Event-associated counters</h3>
          <p>Per-event observations · no sample timestamps or interpolation</p>
        </div>
      </div>
      <p className="summary-note">
        These values are associated with recorded events. Dispatch links follow
        the recorded event identity; multiple matches remain ambiguous. Missing
        values are not zero. Workspace time ranges do not filter this table.
      </p>
      <div className="search-box">
        <input
          aria-label="Filter event counters"
          placeholder="Counter, scope or event identity…"
          value={search}
          onChange={(e) => {
            setSearch(e.target.value);
            setPage(0);
          }}
        />
        <span>
          {fmt(rows.length)} / {fmt(records.length)} values
        </span>
      </div>
      <div className="table-scroll">
        <table aria-label="Event-associated counter values">
          <thead>
            <tr>
              <th>Counter</th>
              <th>Value</th>
              <th>Units</th>
              <th>Scope</th>
              <th>Association</th>
            </tr>
          </thead>
          <tbody>
            {rows.slice(current * 100, (current + 1) * 100).map((r) => (
              <tr key={r.id}>
                <td title={`${r.counterId}\n${r.id}`}>{r.name}</td>
                <td
                  title={r.value === null ? "Missing value" : String(r.value)}
                >
                  {r.value === null ? "Not collected" : fmt(r.value)}
                </td>
                <td>{r.unit}</td>
                <td>{r.scope}</td>
                <td>
                  {r.dispatches.length === 0 ? (
                    r.event ? (
                      "Event without a kernel dispatch"
                    ) : (
                      "No event association"
                    )
                  ) : r.dispatches.length === 1 ? (
                    (() => {
                      const d = dispatches.get(r.dispatches[0]);
                      return d ? (
                        <button onClick={() => onDispatch(d)}>
                          {d.kernel}
                        </button>
                      ) : (
                        "Dispatch unavailable"
                      );
                    })()
                  ) : (
                    <details>
                      <summary>
                        Ambiguous: {r.dispatches.length} dispatches
                      </summary>
                      {r.dispatches.slice(0, 20).map((id) => {
                        const d = dispatches.get(id);
                        return (
                          <div key={id}>
                            {d ? (
                              <button onClick={() => onDispatch(d)}>
                                {d.kernel}
                              </button>
                            ) : (
                              id
                            )}
                          </div>
                        );
                      })}
                      {r.dispatches.length > 20 && (
                        <p>First 20 associations shown</p>
                      )}
                    </details>
                  )}
                  {r.event && (
                    <details>
                      <summary>Event identity</summary>
                      <code>{r.event}</code>
                    </details>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <div className="toolbar-inline">
        <button disabled={!current} onClick={() => setPage(current - 1)}>
          Previous event counters
        </button>
        <span>
          Page {current + 1} / {last + 1}
        </span>
        <button
          disabled={current === last}
          onClick={() => setPage(current + 1)}
        >
          Next event counters
        </button>
      </div>
    </section>
  );
}
