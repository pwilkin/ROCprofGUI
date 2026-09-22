import { useLayoutEffect, useState } from "react";
export const themeKey = "rocprof-studio.theme.v1";
export function useTheme() {
  const [theme, setTheme] = useState<"dark" | "light">(() => {
    try {
      return localStorage.getItem(themeKey) === "light" ? "light" : "dark";
    } catch {
      return "dark";
    }
  });
  const [error, setError] = useState("");
  useLayoutEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);
  function change(next: "dark" | "light") {
    setTheme(next);
    try {
      localStorage.setItem(themeKey, next);
      setError("");
    } catch {
      setError(
        "Theme changed for this session; preference storage is unavailable.",
      );
    }
  }
  return { theme, change, error };
}
export function plotColor(name: string) {
  return getComputedStyle(document.documentElement)
    .getPropertyValue(name)
    .trim();
}
