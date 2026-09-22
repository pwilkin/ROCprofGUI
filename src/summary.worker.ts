import { hardwareSummary, type SummarySeries } from './summary';
import { PackedSummaryPoints } from './summaryPoints';
let architecture = '';
let series: (SummarySeries & {points: PackedSummaryPoints})[] = [];
self.onmessage = (event: MessageEvent) => {
  try {
    const message = event.data;
    if (message.kind === 'start') { architecture = message.architecture; series = []; }
    else if (message.kind === 'series') series.push({...message.series, points: new PackedSummaryPoints()});
    else if (message.kind === 'points') {
      const target = series[series.length - 1].points;
      target.append(message.buffer);
    } else if (message.kind === 'finish') {
      const result = hardwareSummary(architecture, series);
      series = [];
      self.postMessage({result});
    }
  } catch (error) {
    series = [];
    self.postMessage({error: String(error)});
  }
};
