import { useAnalysisPreference } from "../analysisPreferences";
import type { TimelinePreferences } from "../timelinePreferences";
import { useMemo } from "react";
import type { Capture, Range } from "../types";
import { Panel } from "./AnalysisViews";
import { Timeline } from "./Plots";
import { boundedRange } from "../analysis";
export default function HostCalls({
  capture,
  range,
  onRange,
  preferences,
  onPreferences,
}: {
  capture: Capture;
  range: Range;
  onRange: (range: Range) => void;
  preferences?: TimelinePreferences;
  onPreferences?: (preferences: TimelinePreferences) => void;
}) {
  const [search, setSearch] = useAnalysisPreference<string>("host.search", ""),
    [page, setPage] = useAnalysisPreference<number>("host.page", 0),
    [selected, setSelected] = useAnalysisPreference<string>(
      "host.selected",
      "",
    ),
    [linkPage, setLinkPage] = useAnalysisPreference<number>("host.linkPage", 0);
  const dispatches = useMemo(
    () => new Map(capture.dispatches.map((d) => [d.id, d])),
    [capture],
  );
  if (!capture.hostCalls?.length) return null;
  const calls = capture.hostCalls.filter(
    (c) =>
      (c.start === c.end
        ? c.start >= range[0] && c.start <= range[1]
        : c.start < range[1] && c.end > range[0]) &&
      (
        c.name +
        " " +
        c.pid +
        " " +
        c.tid +
        " " +
        (c.scope || "") +
        " " +
        (c.category || "")
      )
        .toLowerCase()
        .includes(search.toLowerCase()),
  );
  const current = Math.min(
    page,
    Math.max(0, Math.ceil(calls.length / 100) - 1),
  );
  const visible = calls.slice(current * 100, (current + 1) * 100);
  const call = capture.hostCalls.find((c) => c.id === selected);
  const choose = (id: string) => {
    setSelected(id);
    setLinkPage(0);
  };
  const links =
    call?.operations.slice(linkPage * 25, (linkPage + 1) * 25) || [];
  return (
    <Panel
      title="Host calls and CPU regions"
      detail="CPU call intervals, separate from GPU execution. Navigation follows recorded operation links only."
    >
      <input
        aria-label="Search host calls"
        value={search}
        placeholder="API name, PID or TID"
        onChange={(e) => {
          setSearch(e.target.value);
          setPage(0);
        }}
      />
      <p>
        {calls.length} matching calls · timeline shows page {current + 1} of{" "}
        {Math.max(1, Math.ceil(calls.length / 100))} · up to 100 calls per page
      </p>
      <Timeline
        preferences={preferences}
        onPreferences={onPreferences}
        preferenceId="host-calls"
        exportLabel={`${capture.name} · host calls · matching search`}
        unit={capture.unit}
        range={range}
        bounds={[capture.start, capture.end]}
        onRange={onRange}
        onSelect={(m) => choose(m.key)}
        loadExportMarks={async (window) =>
          calls
            .filter((c) => c.end >= window[0] && c.start <= window[1])
            .map((c) => ({
              key: c.id,
              start: c.start,
              end: c.end,
              lane: `${c.scope || ""} · PID ${c.pid} · TID ${c.tid}`,
              color: "#65a9d7",
              label: `${c.name} · CPU ${c.end - c.start} ${capture.unit}`,
              selected: c.id === selected,
            }))
        }
        marks={visible.map((c) => ({
          key: c.id,
          start: c.start,
          end: c.end,
          lane: `${c.scope || ""} · PID ${c.pid} · TID ${c.tid}`,
          color: "#65a9d7",
          label: `${c.name} · CPU ${c.end - c.start} ${capture.unit}`,
          selected: c.id === selected,
        }))}
      />
      <div className="table-scroll">
        <table aria-label="Host API records">
          <thead>
            <tr>
              <th>API</th>
              <th>PID / TID</th>
              <th>Start</th>
              <th>End</th>
              <th>CPU duration</th>
              <th>Operation links</th>
            </tr>
          </thead>
          <tbody>
            {visible.map((c) => (
              <tr key={c.id}>
                <td>
                  <button onClick={() => choose(c.id)}>{c.name}</button>
                </td>
                <td>
                  {c.pid} / {c.tid}
                </td>
                <td>{c.start}</td>
                <td>{c.end}</td>
                <td>
                  {c.end - c.start} {capture.unit}
                </td>
                <td>{c.operations.length}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {!calls.length && <p>No host calls match this range and filter.</p>}
      <div>
        <button disabled={!current} onClick={() => setPage(current - 1)}>
          Previous host calls
        </button>{" "}
        <button
          disabled={(current + 1) * 100 >= calls.length}
          onClick={() => setPage(current + 1)}
        >
          Next host calls
        </button>
      </div>
      {call && (
        <section aria-label="Selected host call">
          <h3>{call.name}</h3>
          <p>
            {call.scope} · {call.category || "Category not recorded"} ·{" "}
            {call.linkBasis || "Recorded operation links"}
          </p>
          <p>
            {call.id} · PID {call.pid} · TID {call.tid} · {call.start}–
            {call.end} {capture.unit}
          </p>
          <pre style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>
            {call.arguments ??
              "Arguments not recorded or string table unresolved"}
          </pre>
          <button
            onClick={() =>
              onRange(
                boundedRange(
                  [
                    call.start,
                    call.end > call.start ? call.end : call.start + 1,
                  ],
                  [capture.start, capture.end],
                ),
              )
            }
          >
            Focus CPU call
          </button>
          <p>{call.operations.length} recorded operation links</p>
          {links.map((id) => {
            const d = dispatches.get(id);
            return (
              <div key={id}>
                {d ? (
                  <button
                    onClick={() =>
                      onRange(
                        boundedRange(
                          [d.start, d.end > d.start ? d.end : d.start + 1],
                          [capture.start, capture.end],
                        ),
                      )
                    }
                  >
                    Focus GPU dispatch · {d.kernel} · {id}
                  </button>
                ) : (
                  <span>{id} · no imported kernel dispatch</span>
                )}
              </div>
            );
          })}
          <button
            disabled={!linkPage}
            onClick={() => setLinkPage(linkPage - 1)}
          >
            Previous operation links
          </button>{" "}
          <button
            disabled={(linkPage + 1) * 25 >= call.operations.length}
            onClick={() => setLinkPage(linkPage + 1)}
          >
            Next operation links
          </button>
        </section>
      )}
    </Panel>
  );
}
