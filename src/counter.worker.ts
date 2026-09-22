import { evaluateCounters, type EvaluationRequest } from "./counterEvaluation";
self.onmessage = (event: MessageEvent<EvaluationRequest>) => {
  try {
    self.postMessage({ result: evaluateCounters(event.data) });
  } catch (error) {
    self.postMessage({
      error: error instanceof Error ? error.message : String(error),
    });
  }
};
