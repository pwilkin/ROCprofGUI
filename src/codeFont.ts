import { useLayoutEffect, useState } from "react";

export const codeFontKey = "rocprof-studio.code-font.v1";
export function parseCodeFont(value: unknown): number {
  return typeof value === "number" &&
    Number.isInteger(value) &&
    value >= 5 &&
    value <= 19
    ? value
    : 9;
}
export function useCodeFont() {
  const [size, setSize] = useState(() => {
    try {
      return parseCodeFont(
        JSON.parse(localStorage.getItem(codeFontKey) || "null"),
      );
    } catch {
      return 9;
    }
  });
  const [error, setError] = useState("");
  useLayoutEffect(() => {
    document.documentElement.style.setProperty("--code-font-size", `${size}pt`);
  }, [size]);
  function change(value: number) {
    const next = parseCodeFont(value);
    setSize(next);
    try {
      localStorage.setItem(codeFontKey, JSON.stringify(next));
      setError("");
    } catch {
      setError(
        "Code font changed for this session; preference storage is unavailable.",
      );
    }
  }
  return { size, change, error };
}
