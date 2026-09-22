import { useEffect, useRef } from "react";
export default function SamplingChooser({
  onSelect,
  onCancel,
}: {
  onSelect: (method: "host-trap" | "stochastic") => void;
  onCancel: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const element = dialog.current!;
    element.showModal();
    return () => element.close();
  }, []);
  return (
    <dialog
      ref={dialog}
      className="modal run-dialog"
      aria-labelledby="sampling-choice-title"
      onCancel={(event) => {
        event.preventDefault();
        onCancel();
      }}
    >
      <h2 id="sampling-choice-title">Choose a PC sampling method</h2>
      <p>
        This file contains host-trap and stochastic observations. Open one
        method per capture to keep their sample distributions separate. Open the
        file again to compare the other method in another tab.
      </p>
      <p>
        SPM counters remain available with either choice. The choice is saved
        with the project.
      </p>
      <div className="source-remap-controls">
        <button onClick={onCancel}>Cancel method selection</button>
        <button onClick={() => onSelect("host-trap")}>
          Open host-trap samples
        </button>
        <button onClick={() => onSelect("stochastic")}>
          Open stochastic samples
        </button>
      </div>
    </dialog>
  );
}
