import { createContext, useEffect, useState } from "react";

export type StrokeMode = "system" | "device";
export const displayScaleKey = "rocprof-studio.display-scale.v1";
export const DisplayScaleContext = createContext({
  ratio: 1,
  strokeScale: 1,
  mode: "system" as StrokeMode,
});
export function safePixelRatio(value: number) {
  return Number.isFinite(value) && value > 0 ? value : 1;
}
export function useDisplayScale() {
  const [mode, setMode] = useState<StrokeMode>(() => {
    try {
      return localStorage.getItem(displayScaleKey) === "device"
        ? "device"
        : "system";
    } catch {
      return "system";
    }
  });
  const [ratio, setRatio] = useState(() =>
    safePixelRatio(window.devicePixelRatio),
  );
  const [error, setError] = useState("");
  useEffect(() => {
    let media: MediaQueryList;
    const update = () => {
      media?.removeEventListener("change", update);
      const next = safePixelRatio(window.devicePixelRatio);
      setRatio(next);
      media = window.matchMedia(`(resolution: ${next}dppx)`);
      media.addEventListener("change", update);
    };
    update();
    window.addEventListener("resize", update);
    return () => {
      media.removeEventListener("change", update);
      window.removeEventListener("resize", update);
    };
  }, []);
  function change(next: StrokeMode) {
    setMode(next);
    try {
      localStorage.setItem(displayScaleKey, next);
      setError("");
    } catch {
      setError(
        "Display scaling changed for this session; preference storage is unavailable.",
      );
    }
  }
  return {
    ratio,
    mode,
    strokeScale: mode === "device" ? 1 / ratio : 1,
    change,
    error,
  };
}
