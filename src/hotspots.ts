export function topHotspots<T extends { latency: number }>(rows: readonly T[]): T[] {
  const top: T[] = [];
  for (const row of rows) {
    if (!(row.latency > 0)) continue;
    if (top.length === 20 && row.latency <= top[19].latency) continue;
    let lo = 0;
    let hi = top.length;
    while (lo < hi) {
      const mid = (lo + hi) >>> 1;
      if (top[mid].latency >= row.latency) lo = mid + 1;
      else hi = mid;
    }
    top.splice(lo, 0, row);
    if (top.length > 20) top.pop();
  }
  return top;
}
