export const OBSERVATION_SEARCH_BATCH = 4096;

export async function searchObservationRows(
  rows: readonly (readonly string[])[],
  query: string,
  signal: AbortSignal,
  yieldTask: () => Promise<void> = () =>
    new Promise((resolve) => setTimeout(resolve, 0)),
): Promise<number[] | null> {
  const check = () => {
    if (signal.aborted)
      throw new DOMException("Search cancelled", "AbortError");
  };
  check();
  if (!query) return null;
  const needle = query.toLowerCase();
  const matches: number[] = [];
  for (let start = 0; start < rows.length; start += OBSERVATION_SEARCH_BATCH) {
    await yieldTask();
    check();
    const end = Math.min(start + OBSERVATION_SEARCH_BATCH, rows.length);
    for (let index = start; index < end; index++) {
      if (rows[index].some((field) => field.toLowerCase().includes(needle)))
        matches.push(index);
    }
  }
  check();
  return matches;
}
