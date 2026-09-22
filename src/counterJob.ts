import {
  counterLimits,
  validateEvaluation,
  type EvaluationRequest,
  type EvaluationResult,
} from "./counterEvaluation";
export interface CounterJob {
  result: Promise<EvaluationResult>;
  cancel: () => void;
}
export function startCounterJob(request: EvaluationRequest): CounterJob {
  validateEvaluation(request);
  const worker = new Worker(new URL("./counter.worker.ts", import.meta.url), {
    type: "module",
  });
  let finish: (error?: Error, result?: EvaluationResult) => void;
  let finished = false;
  const result = new Promise<EvaluationResult>((resolve, reject) => {
    const timer = setTimeout(
      () => finish(Error("Counter evaluation exceeded 30 seconds")),
      counterLimits.timeoutMs,
    );
    finish = (error, value) => {
      if (finished) return;
      finished = true;
      clearTimeout(timer);
      worker.terminate();
      if (error) reject(error);
      else resolve(value!);
    };
    worker.onmessage = (
      event: MessageEvent<{ error?: string; result?: EvaluationResult }>,
    ) => {
      if (event.data.error) finish(Error(event.data.error));
      else if (event.data.result) finish(undefined, event.data.result);
      else finish(Error("Invalid counter worker response"));
    };
    worker.onerror = (event) => {
      event.preventDefault();
      finish(Error(event.message || "Counter worker failed to start"));
    };
    worker.onmessageerror = () =>
      finish(Error("Counter worker response could not be decoded"));
    try {
      worker.postMessage(request);
    } catch (error) {
      finish(Error(String(error)));
    }
  });
  return {
    result,
    cancel: () => finish(Error("Counter evaluation cancelled")),
  };
}
