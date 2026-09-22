import { hardwareSummary, hardwareSummaryUnavailable, type HardwareSummary } from './summary';
import type { Series } from './types';
export function requestHardwareSummary(architecture: string, series: Series[], signal: AbortSignal): Promise<HardwareSummary> {
  return new Promise((resolve, reject) => {
    if (signal.aborted) { reject(new DOMException('Summary cancelled', 'AbortError')); return; }
    if (hardwareSummaryUnavailable(architecture, series)) { resolve(hardwareSummary(architecture, series)); return; }
    const worker = new Worker(new URL('./summary.worker.ts', import.meta.url), {type: 'module'});
    let settled = false;
    const finish = (result?: HardwareSummary, error?: unknown) => {
      if (settled) return;
      settled = true;
      signal.removeEventListener('abort', abort);
      worker.terminate();
      if (error !== undefined) reject(error); else resolve(result!);
    };
    const abort = () => finish(undefined, new DOMException('Summary cancelled', 'AbortError'));
    signal.addEventListener('abort', abort, {once: true});
    worker.onmessage = event => {
      if (event.data.error !== undefined) finish(undefined, Error(event.data.error));
      else if (event.data.result) finish(event.data.result);
      else finish(undefined, Error('Invalid Activity summary response'));
    };
    worker.onerror = event => finish(undefined, Error(event.message || 'Activity summary worker failed'));
    worker.onmessageerror = () => finish(undefined, Error('Activity summary response could not be read'));
    void (async () => {
      worker.postMessage({kind: 'start', architecture});
      for (const s of series) {
        if (settled) return;
        if (!/^SE \d+ · CU \d+ · bank \d+$/.test(s.scope)) continue;
        worker.postMessage({kind:'series', series: {name:s.name, scope:s.scope, unit:s.unit}});
        for (let offset = 0; offset < s.points.length; offset += 4096) {
          if (settled) return;
          const end = Math.min(s.points.length, offset + 4096);
          const values = new Float64Array((end - offset) * 2);
          for (let i = offset; i < end; i++) {
            values[(i - offset) * 2] = s.points[i][0];
            values[(i - offset) * 2 + 1] = s.points[i][1];
          }
          worker.postMessage({kind:'points', buffer:values.buffer}, [values.buffer]);
          await new Promise(r => setTimeout(r, 0));
        }
      }
      if (!settled) worker.postMessage({kind:'finish'});
    })().catch(error => finish(undefined, error));
  });
}
