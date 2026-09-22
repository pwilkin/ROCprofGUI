import { useEffect, useRef, useState } from "react";
import type { AttRun } from "../bridge";
export default function RunChooser({
  runs,
  onSelect,
  onCancel,
}: {
  runs: AttRun[];
  onSelect: (run: AttRun) => void;
  onCancel: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [index, setIndex] = useState(0);
  useEffect(() => {
    const element = dialog.current!;
    element.showModal();
    return () => element.close();
  }, []);
  const run = runs[index];
  return (
    <dialog
      ref={dialog}
      className="modal run-dialog"
      aria-labelledby="run-title"
      onCancel={(e) => {
        e.preventDefault();
        onCancel();
      }}
    >
      <h2 id="run-title">Choose a profiling run</h2>
      <p>
        {runs.length} raw ATT runs found. Each choice includes every recorded
        shader engine for that trace prefix and run. Decoding starts only after
        selection.
      </p>
      <label>
        ATT profiling run{" "}
        <select
          aria-label="ATT profiling run"
          size={Math.min(8, runs.length)}
          value={index}
          onChange={(e) => setIndex(Number(e.target.value))}
        >
          {runs.map((r, n) => (
            <option value={n} key={r.path}>
              {r.prefix || "Unnamed prefix"} · run {r.run} ·{" "}
              {r.shaderEngines.length} SE files
            </option>
          ))}
        </select>
      </label>
      <p>
        Shader engines: {run.shaderEngines.join(", ")}
        <br />
        Trace data: {run.bytes.toLocaleString()} bytes
        <br />
        Code objects are resolved from the same directory; ambiguous object IDs
        cause an import error.
      </p>
      <div className="source-remap-controls">
        <button onClick={onCancel}>Cancel run selection</button>
        <button className="primary" onClick={() => onSelect(run)}>
          Decode selected run
        </button>
      </div>
    </dialog>
  );
}
