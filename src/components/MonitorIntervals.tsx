import { useAnalysisPreference } from "../analysisPreferences";
import type { Capture, Range } from "../types";
import { Panel } from "./AnalysisViews";
export default function MonitorIntervals({
  capture,
  range,
}: {
  capture: Capture;
  range: Range;
}) {
  const [query, setQuery] = useAnalysisPreference<string>("monitor.search", ""),
    [page, setPage] = useAnalysisPreference<number>("monitor.page", 0);
  if (!capture.monitorIntervals?.length) return null;
  const rows = capture.monitorIntervals.filter(
    (m) =>
      (m.start === m.end
        ? m.start >= range[0] && m.start <= range[1]
        : m.start < range[1] && m.end > range[0]) &&
      (m.deviceType + " " + m.deviceId + " " + m.name)
        .toLowerCase()
        .includes(query.toLowerCase()),
  );
  const current = Math.min(page, Math.max(0, Math.ceil(rows.length / 100) - 1));
  const visible = rows.slice(current * 100, (current + 1) * 100);
  return (
    <Panel
      title="Monitor intervals"
      detail="Recorded spans and raw values. Units are unspecified; gaps are not interpolated."
    >
      <input
        aria-label="Search monitor intervals"
        value={query}
        onChange={(e) => {
          setQuery(e.target.value);
          setPage(0);
        }}
        placeholder="Device or monitor name"
      />
      <p>
        {rows.length} matching intervals · page {current + 1} of{" "}
        {Math.max(1, Math.ceil(rows.length / 100))}
      </p>
      <div style={{ overflowX: "auto" }}>
        <table aria-label="Monitor interval records">
          <thead>
            <tr>
              <th>Device</th>
              <th>Monitor</th>
              <th>Start</th>
              <th>End</th>
              <th>Recorded span</th>
              <th>Value</th>
            </tr>
          </thead>
          <tbody>
            {visible.map((m) => (
              <tr key={m.id}>
                <td>
                  {m.deviceType} {m.deviceId}
                </td>
                <td>{m.name}</td>
                <td>
                  {m.start} {capture.unit}
                </td>
                <td>
                  {m.end} {capture.unit}
                </td>
                <td>
                  <svg
                    width="160"
                    height="18"
                    viewBox="0 0 160 18"
                    role="img"
                    aria-label={`${m.name}: ${m.start}–${m.end} ${capture.unit}`}
                  >
                    <rect
                      x={Math.min(
                        159,
                        Math.max(
                          0,
                          ((m.start - range[0]) / (range[1] - range[0])) * 160,
                        ),
                      )}
                      y="4"
                      width={Math.max(
                        1,
                        ((Math.min(m.end, range[1]) -
                          Math.max(m.start, range[0])) /
                          (range[1] - range[0])) *
                          160,
                      )}
                      height="10"
                      fill="currentColor"
                    >
                      <title>
                        {m.value ?? "Not recorded"} · {m.unit}
                      </title>
                    </rect>
                  </svg>
                </td>
                <td>
                  {m.value ?? "Not recorded"} · {m.unit}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {!rows.length && <p>No monitor intervals match this range and filter.</p>}
      <div>
        <button disabled={current === 0} onClick={() => setPage(current - 1)}>
          Previous monitor intervals
        </button>{" "}
        <button
          disabled={(current + 1) * 100 >= rows.length}
          onClick={() => setPage(current + 1)}
        >
          Next monitor intervals
        </button>
      </div>
    </Panel>
  );
}
