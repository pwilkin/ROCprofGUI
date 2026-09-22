import { useState } from "react";

export const waveStateImportKey = "rocprof-studio.load-wave-states.v1";
let current: boolean | undefined;
export function loadWaveStatesPreference(): boolean {
  if (current === undefined) {
    try { current = localStorage.getItem(waveStateImportKey) !== "false"; }
    catch { current = true; }
  }
  return current;
}
export function useWaveStateImport() {
  const [enabled, setEnabled] = useState(loadWaveStatesPreference);
  const [error, setError] = useState("");
  function change(next: boolean) {
    current = next;
    setEnabled(next);
    try {
      localStorage.setItem(waveStateImportKey, String(next));
      setError("");
    } catch {
      setError("Import preference changed for this session; storage is unavailable.");
    }
  }
  return { enabled, change, error };
}
