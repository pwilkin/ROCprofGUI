import { categories, fmt, type Token } from "./types";

export function tokenLabeler(location: string) {
  const labels = new Map<string, string>();
  return (token: Token): string => {
    const stall = Object.is(token.stall, -0) ? "-0" : token.stall;
    const key = `${token.category}:${token.instruction}:${stall}`;
    const cached = labels.get(key);
    if (cached !== undefined) return cached;
    const label = `${location} · ${categories[token.category] || "OTHER"} · ISA ${token.instruction} · stall ${fmt(token.stall)}`;
    if (labels.size === 4096) labels.delete(labels.keys().next().value!);
    labels.set(key, label);
    return label;
  };
}
