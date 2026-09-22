import {
  assembleCatalogBatch,
  type CatalogBatchHeader,
  type CatalogInvoke,
} from "./catalogTransfer";
import type { OpenedProject } from "./bridge";
type ProjectHeader = CatalogBatchHeader & { project: OpenedProject["project"] };
export async function requestProject(
  transferId: string,
  invoke: CatalogInvoke,
  signal: AbortSignal,
): Promise<OpenedProject | null> {
  if (signal.aborted) throw Error("Project import cancelled");
  try {
    const response = await invoke<OpenedProject | ProjectHeader | null>(
      "open_project",
      { instructionPages: true, transferId },
    );
    if (!response) return null;
    if (!("transport" in response)) return response;
    if (
      response.transferId !== transferId ||
      response.project.entries.length !== response.captures.length ||
      !Number.isSafeInteger(response.project.active) ||
      response.project.active < 0 ||
      response.project.active >= response.captures.length
    )
      throw Error("Invalid project transfer header");
    const captures = await assembleCatalogBatch(response, invoke, signal);
    if (signal.aborted) throw Error("Project import cancelled");
    const ids = await invoke<string[]>("finish_project_transfer", {
      transferId,
    });
    if (
      ids.length !== captures.length ||
      ids.some((id, index) => id !== captures[index].id)
    )
      throw Error("Project commit identity mismatch");
    if (signal.aborted) throw Error("Project import cancelled");
    return { captures, project: response.project, pendingTransfer: transferId };
  } catch (error) {
    try {
      await invoke("abort_catalog_transfer", { transferId });
    } catch (cleanup) {
      throw Error(
        `${String(error)}; project rollback failed: ${String(cleanup)}`,
      );
    }
    throw error;
  }
}
export async function acknowledgeProject(
  transferId: string | undefined,
  invoke: CatalogInvoke,
): Promise<void> {
  if (!transferId) return;
  try {
    await invoke("acknowledge_project_transfer", { transferId });
  } catch {
    await invoke("acknowledge_project_transfer", { transferId });
  }
}
