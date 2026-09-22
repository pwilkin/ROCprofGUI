import type { Capture, Instruction } from "./types";
export interface CatalogHeader {
  transport: "instruction-pages-v1" | "instruction-pages-v2";
  observationCount?: number;
  transferId: string;
  instructionCount: number;
  maxPageRows: number;
  capture: Capture;
}
interface Page {
  transferId: string;
  captureId: string;
  offset: number;
  total: number;
  instructions: Instruction[];
}
interface ObservationPage {
  transferId: string;
  captureId: string;
  offset: number;
  total: number;
  observations: string[][];
}
export type CatalogInvoke = <T>(
  command: string,
  args: Record<string, unknown>,
) => Promise<T>;
function checkCancellation(signal: AbortSignal) {
  if (signal.aborted) throw Error("Import cancelled");
}
async function assembleCatalog(
  header: CatalogHeader,
  readPage: (offset: number, rows: number) => Promise<Page>,
  signal: AbortSignal,
  readObservations: (offset: number, rows: number) => Promise<ObservationPage>,
): Promise<Capture> {
  checkCancellation(signal);
  if (
    !["instruction-pages-v1", "instruction-pages-v2"].includes(
      header.transport,
    ) ||
    !header.transferId ||
    !Number.isSafeInteger(header.instructionCount) ||
    header.instructionCount < 0 ||
    header.instructionCount > 2_000_000 ||
    !Number.isSafeInteger(header.maxPageRows) ||
    header.maxPageRows < 1 ||
    header.maxPageRows > 4096 ||
    !Array.isArray(header.capture.instructions) ||
    header.capture.instructions.length !== 0
  )
    throw Error("Invalid catalog transfer header");
  const observationCount = header.observationCount;
  const observationHeader = header.capture.pcSamplingObservations;
  if (
    header.transport === "instruction-pages-v2" &&
    (!Number.isSafeInteger(observationCount) ||
      observationCount! < 0 ||
      observationCount! > 2_000_000 ||
      !observationHeader ||
      !Array.isArray(observationHeader.columns) ||
      !observationHeader.columns.every((c) => typeof c === "string") ||
      !Array.isArray(observationHeader.rows) ||
      observationHeader.rows.length !== 0)
  )
    throw Error("Invalid observation transfer header");
  if (
    header.transport === "instruction-pages-v1" &&
    observationCount !== undefined
  )
    throw Error("Unexpected observation count in legacy transfer header");
  const instructions: Instruction[] = [];
  while (instructions.length < header.instructionCount) {
    checkCancellation(signal);
    const page = await readPage(instructions.length, header.maxPageRows);
    checkCancellation(signal);
    if (
      page.transferId !== header.transferId ||
      page.captureId !== header.capture.id ||
      page.offset !== instructions.length ||
      page.total !== header.instructionCount ||
      !Array.isArray(page.instructions) ||
      !page.instructions.length ||
      page.instructions.length > header.maxPageRows ||
      page.instructions.length > header.instructionCount - instructions.length
    )
      throw Error("Invalid catalog transfer page");
    instructions.push(...page.instructions);
  }
  checkCancellation(signal);
  let observations = observationHeader;
  if (header.transport === "instruction-pages-v2") {
    const rows: string[][] = [];
    while (rows.length < observationCount!) {
      checkCancellation(signal);
      const page = await readObservations(rows.length, header.maxPageRows);
      checkCancellation(signal);
      if (
        page.transferId !== header.transferId ||
        page.captureId !== header.capture.id ||
        page.offset !== rows.length ||
        page.total !== observationCount ||
        !Array.isArray(page.observations) ||
        !page.observations.length ||
        page.observations.length > header.maxPageRows ||
        page.observations.length > observationCount! - rows.length ||
        page.observations.some(
          (row) =>
            !Array.isArray(row) ||
            row.length !== observationHeader!.columns.length ||
            row.some((v) => typeof v !== "string"),
        )
      )
        throw Error("Invalid observation transfer page");
      rows.push(...page.observations);
    }
    observations = { columns: observationHeader!.columns, rows };
  }
  checkCancellation(signal);
  return {
    ...header.capture,
    instructions,
    ...(observations ? { pcSamplingObservations: observations } : {}),
  };
}
export async function receiveCatalog(
  response: Capture | CatalogHeader,
  invoke: CatalogInvoke,
  signal: AbortSignal,
): Promise<Capture> {
  if (!("transport" in response)) {
    if (signal.aborted) {
      await invoke("close_capture", { captureId: response.id });
      throw Error("Import cancelled");
    }
    return response;
  }
  const header = response;
  let committed = false;
  let commitAttempted = false;
  try {
    const capture = await assembleCatalog(
      header,
      (offset, rows) =>
        invoke<Page>("read_catalog_page", {
          transferId: header.transferId,
          offset,
          rows,
        }),
      signal,
      (offset, rows) =>
        invoke<ObservationPage>("read_observation_page", {
          transferId: header.transferId,
          offset,
          rows,
        }),
    );
    commitAttempted = true;
    const id = await invoke<string>("finish_catalog_transfer", {
      transferId: header.transferId,
    });
    committed = true;
    if (id !== header.capture.id)
      throw Error("Catalog commit identity mismatch");
    checkCancellation(signal);
    return capture;
  } catch (error) {
    try {
      if (committed)
        await invoke("close_capture", { captureId: header.capture.id });
      else {
        await invoke("abort_catalog_transfer", {
          transferId: header.transferId,
        });
        if (commitAttempted)
          await invoke("close_capture", { captureId: header.capture.id });
      }
    } catch (cleanup) {
      throw Error(
        `${String(error)}; catalog cleanup failed: ${String(cleanup)}`,
      );
    }
    throw error;
  }
}

export async function requestCatalogCommand(
  command: string,
  args: Record<string, unknown>,
  transferId: string,
  invoke: CatalogInvoke,
  signal: AbortSignal,
): Promise<Capture | null> {
  checkCancellation(signal);
  let response: Capture | CatalogHeader | null;
  try {
    response = await invoke<Capture | CatalogHeader | null>(command, {
      ...args,
      instructionPages: true,
      transferId,
    });
    if (
      response &&
      "transport" in response &&
      response.transferId !== transferId
    )
      throw Error("Catalog request identity mismatch");
  } catch (error) {
    try {
      await invoke("abort_catalog_transfer", { transferId });
    } catch (cleanup) {
      throw Error(
        `${String(error)}; catalog cleanup failed: ${String(cleanup)}`,
      );
    }
    throw error;
  }
  return response ? receiveCatalog(response, invoke, signal) : null;
}

export async function requestCatalog(
  path: string,
  transferId: string,
  invoke: CatalogInvoke,
  signal: AbortSignal,
  loadWaveStates = true,
  samplingMethod?: string,
): Promise<Capture> {
  const capture = await requestCatalogCommand(
    "open_capture",
    { path, loadWaveStates, ...(samplingMethod ? { samplingMethod } : {}) },
    transferId,
    invoke,
    signal,
  );
  if (!capture) throw Error("Capture import returned no capture");
  return capture;
}

export interface CatalogBatchHeader {
  transport: "catalog-batch-v1";
  transferId: string;
  captures: CatalogHeader[];
}
export async function assembleCatalogBatch(
  header: CatalogBatchHeader,
  invoke: CatalogInvoke,
  signal: AbortSignal,
): Promise<Capture[]> {
  checkCancellation(signal);
  if (
    header.transport !== "catalog-batch-v1" ||
    !header.transferId ||
    !Array.isArray(header.captures) ||
    !header.captures.length ||
    header.captures.length > 7 ||
    header.captures.filter((c) => c.capture.id !== "demo").length > 6 ||
    new Set(header.captures.map((c) => c.capture.id)).size !==
      header.captures.length ||
    header.captures.some(
      (c, index) =>
        !c.capture.id || c.transferId !== `${header.transferId}:${index}`,
    )
  )
    throw Error("Invalid catalog batch header");
  const captures: Capture[] = [];
  for (const [captureIndex, capture] of header.captures.entries()) {
    captures.push(
      await assembleCatalog(
        capture,
        (offset, rows) =>
          invoke<Page>("read_project_catalog_page", {
            transferId: header.transferId,
            captureIndex,
            offset,
            rows,
          }),
        signal,
        (offset, rows) =>
          invoke<ObservationPage>("read_project_observation_page", {
            transferId: header.transferId,
            captureIndex,
            offset,
            rows,
          }),
      ),
    );
  }
  checkCancellation(signal);
  return captures;
}
