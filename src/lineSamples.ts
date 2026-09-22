export function lineSampleBudget(bias: number): number {
  const level = Number.isInteger(bias) ? Math.max(-20, Math.min(20, bias)) : 0;
  return Math.max(1, Math.min(8000, Math.floor(500 * 2 ** -level)));
}
export function lineSamples(input: [number, number][], bias = 0) {
  const budget = lineSampleBudget(bias);
  const runs: [number, number][] = [];
  let start = -1,
    finite = 0;
  for (let i = 0; i <= input.length; i++) {
    if (
      i < input.length &&
      Number.isFinite(input[i][0]) &&
      Number.isFinite(input[i][1])
    ) {
      if (start < 0) start = i;
      finite++;
    } else if (start >= 0) {
      runs.push([start, i]);
      start = -1;
      if (runs.length > 500) return { points: [], limited: true };
    }
  }
  const points: [number, number][] = [];
  for (const [first, end] of runs) {
    if (points.length) points.push([input[first][0], NaN]);
    const buckets = Math.max(1, Math.floor((budget * (end - first)) / finite));
    const stride = Math.ceil((end - first) / buckets);
    for (let i = first; i < end; i += stride) {
      const last = Math.min(end, i + stride) - 1;
      let low = i,
        high = i;
      for (let j = i + 1; j <= last; j++) {
        if (input[j][1] < input[low][1]) low = j;
        if (input[j][1] > input[high][1]) high = j;
      }
      for (const index of [...new Set([i, low, high, last])].sort(
        (a, b) => a - b,
      ))
        points.push(input[index]);
    }
  }
  return { points, limited: false };
}
