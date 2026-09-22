export const laneHeights = [20, 28, 42, 56] as const;
export interface TimelinePreferences {
  zoomMode: "continuous" | "mip";
  detailPixels: number;
  stacked: boolean;
  laneHeight: number;
}
export const defaultTimelinePreferences: TimelinePreferences = {
  zoomMode: "continuous",
  detailPixels: 0,
  stacked: true,
  laneHeight: 28,
};
export function parseTimelinePreferences(
  value: string | null,
): TimelinePreferences {
  try {
    const data = JSON.parse(value || "null");
    return {
      zoomMode: data?.zoomMode === "mip" ? "mip" : "continuous",
      detailPixels: [-1, 0, 2, 4, 8].includes(data?.detailPixels)
        ? data.detailPixels
        : 0,
      stacked: typeof data?.stacked === "boolean" ? data.stacked : true,
      laneHeight: laneHeights.includes(data?.laneHeight) ? data.laneHeight : 28,
    };
  } catch {
    return { ...defaultTimelinePreferences };
  }
}
export const timelinePreferenceKey = (view: string) =>
  `rocprof.timeline.v1.${view}`;

export const timelineViews = [
  "wave",
  "cu",
  "util",
  "global",
  "dispatch",
  "markers",
  "host-calls",
] as const;
export type TimelinePreferenceSet = Record<string, TimelinePreferences>;
export function readTimelinePreferenceSet(): TimelinePreferenceSet {
  return Object.fromEntries(
    timelineViews.map((view) => {
      try {
        return [
          view,
          parseTimelinePreferences(
            localStorage.getItem(timelinePreferenceKey(view)),
          ),
        ];
      } catch {
        return [view, { ...defaultTimelinePreferences }];
      }
    }),
  );
}
export function restoreTimelinePreferenceSet(
  saved?: TimelinePreferenceSet,
): TimelinePreferenceSet {
  const defaults = readTimelinePreferenceSet();
  return Object.fromEntries(
    timelineViews.map((view) => [
      view,
      saved?.[view]
        ? parseTimelinePreferences(JSON.stringify(saved[view]))
        : defaults[view],
    ]),
  );
}
