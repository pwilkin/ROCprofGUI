export interface TraceAssociation {
  path: string;
  relocation?: string;
  mode: "unknown" | "single-dispatch" | "region";
  dispatch: string;
}
export function readTraceAssociations(
  value: unknown,
): TraceAssociation[] | null {
  if (
    typeof value !== "string" ||
    new TextEncoder().encode(value).length > 1024 * 1024
  )
    return null;
  try {
    const rows: unknown = JSON.parse(value);
    if (!Array.isArray(rows) || rows.length > 64) return null;
    const paths = new Set<string>();
    for (const row of rows) {
      if (
        !row ||
        typeof row !== "object" ||
        !["dispatch,mode,path", "dispatch,mode,path,relocation"].includes(Object.keys(row).sort().join())
      )
        return null;
      if (
        ![row.path, row.dispatch].every(
          (v) =>
            typeof v === "string" &&
            new TextEncoder().encode(v).length <= 8192 &&
            !v.includes("\0"),
        )
      )
        return null;
      if (
        !row.path.startsWith("/") ||
        row.path.startsWith("//") ||
        !row.path.toLowerCase().endsWith(".rocprof-att") ||
        paths.has(row.path)
      )
        return null;
      if (!["unknown", "single-dispatch", "region"].includes(row.mode))
        return null;
      if ((row.mode === "single-dispatch") !== row.dispatch.length > 0)
        return null;
      if (row.relocation !== undefined && (
        typeof row.relocation !== "string" ||
        (row.relocation !== "" && (
          new TextEncoder().encode(row.relocation).length > 8192 ||
          row.relocation.includes("\0") || !row.relocation.startsWith("/") ||
          row.relocation.startsWith("//") || !row.relocation.toLowerCase().endsWith(".rocprof-att")
        ))
      )) return null;
      paths.add(row.path);
    }
    return rows;
  } catch {
    return null;
  }
}
