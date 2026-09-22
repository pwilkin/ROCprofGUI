export type MatrixInstruction = {
  name: string;
  family: string;
  scaled: boolean;
  accumulator: string;
  shape: string;
  blocks: string;
  inputs: string;
};
export type IsaArchitecture = {
  id: string;
  name: string;
  release: string;
  schema: string;
  copyright: string;
  license: string;
  file: string;
  sha256: string;
  count: number;
  mnemonics: string[];
  matrix: MatrixInstruction[];
};
export type IsaManifest = {
  version: number;
  source: string;
  download: string;
  manuals: string;
  archiveSha256: string;
  architectures: IsaArchitecture[];
};
export type IsaOperand = {
  field: string;
  format: string;
  type: string;
  bits: string;
  input: boolean;
  output: boolean;
  implicit: boolean;
};
export type IsaInstruction = {
  name: string;
  description: string;
  group: string;
  subgroups: string[];
  flags: string[];
  matrix: Omit<MatrixInstruction, "name"> | null;
  encodings: {
    name: string;
    condition: string;
    opcode: string;
    radix: string;
    operands: IsaOperand[];
  }[];
};
export type IsaDocument = {
  id: string;
  instructions: IsaInstruction[];
  formats: Record<
    string,
    { description: string; type: string; bits: string; components: string }
  >;
  encodings: Record<
    string,
    {
      description: string;
      bits: string;
      fields: {
        name: string;
        description: string;
        ranges: { offset: string; bits: string }[];
      }[];
    }
  >;
};
export function instructionMnemonic(text: string): string {
  return (
    text
      .trim()
      .split(/\s+/)[0]
      ?.toUpperCase()
      .replace(/_E(?:32|64)$/, "") || ""
  );
}
export function mnemonicKey(text: string): string {
  const mnemonic = instructionMnemonic(text);
  const aliases: Record<string, string> = {
    DS_READ_B32: "DS_LOAD_B32",
    GLOBAL_LOAD_DWORD: "GLOBAL_LOAD_B32",
    S_LOAD_DWORDX4: "S_LOAD_B128",
  };
  return (aliases[mnemonic] || mnemonic).replaceAll("_", "");
}
export function findIsaInstruction(document: IsaDocument, text: string) {
  const name = instructionMnemonic(text);
  return (
    document.instructions.find((i) => i.name === name) ||
    document.instructions.find((i) => mnemonicKey(i.name) === mnemonicKey(name))
  );
}
export function matrixTypes(matrix: MatrixInstruction[]) {
  return [
    ...new Set(
      matrix.map((m) => `${m.inputs.replace(/_1K$/, "")} → ${m.accumulator}`),
    ),
  ].sort();
}
