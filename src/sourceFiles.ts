export interface SourceFile {
  name: string;
  text: string;
  reference?: { path: string; size: number; sha256: string };
}
export interface SourcePage {
  startLine: number;
  totalLines: number;
  lines: string[];
}
export type SourceMappings = Record<string, SourceFile>;
export const MAX_SOURCE_BYTES = 2 * 1024 * 1024;
export function mapSource(
  mappings: SourceMappings,
  path: string,
  file: SourceFile | null,
): SourceMappings {
  if (!path || path.length > 8192 || path.includes("\0"))
    throw Error("Invalid recorded source path");
  const next = { ...mappings };
  if (file === null) delete next[path];
  else {
    if (file.reference) {
      const ref = file.reference;
      if (
        file.text !== "" ||
        typeof ref.path !== "string" ||
        ref.path.length > 8192 ||
        ref.path.includes("\0") ||
        !/^(\/|[A-Za-z]:[\\/]|\\\\)/.test(ref.path) ||
        !Number.isSafeInteger(ref.size) ||
        ref.size < 0 ||
        ref.size > 512 * 1024 * 1024 ||
        !/^[a-f0-9]{64}$/.test(ref.sha256)
      )
        throw Error("Invalid source file reference");
    }
    if (
      typeof file.text !== "string" ||
      typeof file.name !== "string" ||
      file.text.includes("\0")
    )
      throw Error("Source file must be UTF-8 text without NUL bytes");
    if (new TextEncoder().encode(file.text).length > MAX_SOURCE_BYTES)
      throw Error("Source file exceeds 2 MiB");
    next[path] = file;
  }
  if (
    Object.values(next).reduce(
      (n, f) => n + new TextEncoder().encode(f.text).length,
      0,
    ) >
    16 * 1024 * 1024
  )
    throw Error("Source mappings exceed 16 MiB for this capture");
  return next;
}

export function mapSourceBatch(
  mappings: SourceMappings,
  files: SourceMappings,
  registered: string[] = [],
): SourceMappings {
  const previous = { ...mappings };
  for (const path of Object.keys(files)) {
    if (
      path.startsWith("/") &&
      registered.includes(path) &&
      !registered.includes(path.slice(1))
    )
      delete previous[path.slice(1)];
  }
  const next = { ...previous, ...files };
  if (Object.keys(next).length > 10000) throw Error("Too many source mappings");
  let bytes = 0;
  for (const [path, file] of Object.entries(next)) {
    mapSource({}, path, file);
    bytes += new TextEncoder().encode(file.text).length;
  }
  if (bytes > 16 * 1024 * 1024)
    throw Error("Source mappings exceed 16 MiB for this capture");
  return next;
}
