export const definitionLimit = 256 * 1024;
export const libraryLimit = 2 * 1024 * 1024;
export const libraryKey = "rocprof:definition-library:v1";
export interface DefinitionFile {
  name: string;
  text: string;
}
export interface DefinitionDraft extends DefinitionFile {
  id: string;
}
export interface DefinitionLibrary {
  version: 1;
  selected: string;
  files: DefinitionDraft[];
}
export function validateDefinition(file: DefinitionFile) {
  if (
    typeof file.name !== "string" ||
    !file.name.trim() ||
    file.name.length > 120 ||
    /[\\/\x00-\x1f]/.test(file.name)
  )
    throw Error(
      "Use a filename of 1–120 characters without directory separators",
    );
  if (typeof file.text !== "string" || file.text.includes("\0"))
    throw Error("Definitions must be text without NUL bytes");
  if (new TextEncoder().encode(file.text).length > definitionLimit)
    throw Error("Definition file exceeds 256 KiB");
}
export function validateLibrary(value: unknown): DefinitionLibrary {
  if (!value || typeof value !== "object")
    throw Error("Invalid definition library");
  const library = value as DefinitionLibrary;
  if (
    library.version !== 1 ||
    !Array.isArray(library.files) ||
    !library.files.length ||
    library.files.length > 32
  )
    throw Error("Definition library must contain 1–32 files");
  let bytes = 0;
  const ids = new Set<string>(),
    names = new Set<string>();
  for (const file of library.files) {
    if (!file || typeof file !== "object")
      throw Error("Invalid definition file");
    validateDefinition(file);
    if (
      typeof file.id !== "string" ||
      !file.id ||
      ids.has(file.id) ||
      names.has(file.name)
    )
      throw Error("Duplicate or invalid definition identity");
    ids.add(file.id);
    names.add(file.name);
    bytes += new TextEncoder().encode(file.text).length;
  }
  if (bytes > libraryLimit) throw Error("Definition library exceeds 2 MiB");
  if (!ids.has(library.selected))
    throw Error("Selected definition file is missing");
  return library;
}
export function defaultLibrary(
  text = "Total := sum[SQ_INSTS_VALU, axis=[XCC,SE,CU]]",
): DefinitionLibrary {
  return {
    version: 1,
    selected: "default",
    files: [{ id: "default", name: "derived_counters.txt", text }],
  };
}
export function importDefinitions(
  library: DefinitionLibrary,
  imports: DefinitionFile[],
): DefinitionLibrary {
  const files = [...library.files];
  for (const file of imports) {
    validateDefinition(file);
    const dot = file.name.lastIndexOf("."),
      stem = (dot > 0 ? file.name.slice(0, dot) : file.name).slice(0, 90),
      extension = dot > 0 ? file.name.slice(dot).slice(0, 15) : "";
    let name = file.name,
      n = 2;
    while (files.some((f) => f.name === name))
      name = `${stem} (${n++})${extension}`;
    files.push({ ...file, name, id: crypto.randomUUID() });
  }
  return validateLibrary({ version: 1, files, selected: files.at(-1)!.id });
}
export function editDefinition(
  library: DefinitionLibrary,
  change: Partial<DefinitionFile>,
): DefinitionLibrary {
  return validateLibrary({
    ...library,
    files: library.files.map((f) =>
      f.id === library.selected ? { ...f, ...change } : f,
    ),
  });
}
