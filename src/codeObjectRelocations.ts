export type CodeObjectRelocation = [id: string, uri: string, path: string];

export function readCodeObjectRelocations(
  value: unknown,
): CodeObjectRelocation[] | null {
  if (
    typeof value !== "string" ||
    new TextEncoder().encode(value).length > 1024 * 1024
  )
    return null;
  try {
    const rows: unknown = JSON.parse(value);
    if (!Array.isArray(rows) || rows.length > 256) return null;
    const keys = new Set<string>();
    for (const row of rows) {
      if (
        !Array.isArray(row) ||
        row.length !== 3 ||
        !row.every(
          (item) =>
            typeof item === "string" &&
            item.length > 0 &&
            new TextEncoder().encode(item).length <= 8192 &&
            !item.includes("\0"),
        )
      )
        return null;
      if (
        !row[1].startsWith("file://") ||
        !row[2].startsWith("/") ||
        row[2].startsWith("//")
      )
        return null;
      const key = JSON.stringify(row.slice(0, 2));
      if (keys.has(key)) return null;
      keys.add(key);
    }
    return rows as CodeObjectRelocation[];
  } catch {
    return null;
  }
}
