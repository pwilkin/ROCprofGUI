export class TimelineWindowBudgetError extends Error {
  readonly code = "MCP_TIMELINE_WINDOW_BUDGET";
}
export function isTimelineWindowBudgetError(
  error: unknown,
): error is TimelineWindowBudgetError {
  return error instanceof TimelineWindowBudgetError;
}
export function drawnRecordBudgetError(max: number): Error {
  return Error(
    `Timeline export exceeds ${max.toLocaleString("en-US")} drawn records; export a smaller range`,
  );
}
