export interface SourceLocation {
  reference: string;
  path: string;
  line: number;
}
export function sourceLocations(source: string): SourceLocation[] {
  return source.split(" -> ").flatMap((reference) => {
    const match = reference.match(/^(.*):([0-9]+)$/);
    const line = Number(match?.[2]);
    return match?.[1] && Number.isSafeInteger(line) && line > 0
      ? [{ reference, path: match[1], line }]
      : [];
  });
}
