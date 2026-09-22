import { useEffect, useId, useState } from "react";
import type { Range } from "../types";

export default function RangeBound({
  edge,
  label = "Range",
  range,
  bounds,
  onCommit,
}: {
  edge: 0 | 1;
  label?: string;
  range: Range;
  bounds: Range;
  onCommit: (range: Range) => void;
}) {
  const committed = range[edge];
  const [draft, setDraft] = useState(String(committed));
  const [error, setError] = useState("");
  const messageId = useId();
  useEffect(() => {
    setDraft(String(committed));
    setError("");
  }, [committed, bounds[0], bounds[1], edge]);
  function commit() {
    const value = Number(draft);
    const next: Range = [...range];
    next[edge] = Math.max(bounds[0], Math.min(bounds[1], value));
    if (!draft.trim() || !Number.isFinite(value) || next[0] >= next[1]) {
      setDraft(String(range[edge]));
      setError("Enter a finite bound with start less than end.");
      return;
    }
    setDraft(String(next[edge]));
    setError("");
    if (next[edge] !== range[edge]) onCommit(next);
  }
  return (
    <>
      <input
        aria-label={`${label} ${edge === 0 ? "start" : "end"}`}
        aria-describedby={messageId}
        type="number"
        step="any"
        value={draft}
        title="Apply with Enter or by leaving this field. Escape restores the current bound."
        onChange={(e) => {
          setDraft(e.target.value);
          setError("");
        }}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            commit();
          } else if (e.key === "Escape") {
            e.preventDefault();
            setDraft(String(range[edge]));
            setError("");
          }
        }}
      />
      <span id={messageId} aria-live="polite" className="range-error">
        {error}
      </span>
    </>
  );
}
