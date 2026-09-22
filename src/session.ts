import { readTimelinePreferenceSet } from "./timelinePreferences";
import type { Range } from "./types";
export interface Location {
  waveId: string;
  instruction: number;
  iteration: number;
  range: Range;
  timeline?: string;
}
export interface TabState {
  location: Location;
  view: string;
  mode: string;
  timeline: string;
  independentRanges?: boolean;
  timelineRanges?: Record<string, Range>;
  isaState?: import("./isaState").IsaState;
  analysisPreferences?: import("./analysisPreferences").AnalysisPreferences;
  dockLayout?: import("./components/DockWorkspace").DockLayout;
  timelinePreferences?: import("./timelinePreferences").TimelinePreferenceSet;
  plotAxes?: import("./plotAxes").PlotAxes;
  histogramBins?: number | null;
  cuEventGroups?: number | null;
  cuEvents?: boolean | null;
  globalLayers?: import("./globalTimeline").GlobalLayers | null;
  plotRanges?: import("./plotAxes").PlotAxes;
  scalarCounters?: import("./scalarCounters").ScalarDefinition[];
  nonhidden: boolean;
  includeIdle: boolean;
  history: Location[];
  historyIndex: number;
}
export function readPreference<T>(key: string, fallback: T): T {
  try {
    const value = localStorage.getItem(key);
    return value ? JSON.parse(value) : fallback;
  } catch {
    return fallback;
  }
}
export function savePreference(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* Preferences are optional when storage is unavailable. */
  }
}

export function initialTabState(capture: import("./types").Capture): TabState {
  const location: Location = {
    waveId: capture.waves[0]?.id || "",
    instruction: capture.instructions[1]?.index || 0,
    iteration: 0,
    range: [capture.start, capture.end],
    timeline: "wave",
  };
  return {
    location,
    view: "trace",
    mode: "capture",
    timeline: "wave",
    independentRanges: false,
    timelineRanges: {},
    timelinePreferences: readTimelinePreferenceSet(),
    nonhidden: false,
    includeIdle: false,
    history: [location],
    historyIndex: 0,
  };
}
