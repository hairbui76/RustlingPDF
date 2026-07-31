import type { AutomationTool } from "@app/types/automation";

export type AutomationDropEdge = "before" | "after";

/**
 * Resolve a before/after drop into the final index after the source item has
 * been removed from the list.
 */
export function getAutomationDropIndex(
  sourceIndex: number,
  targetIndex: number,
  edge: AutomationDropEdge,
  itemCount: number,
): number {
  if (itemCount <= 0) return 0;

  let insertionIndex = targetIndex + (edge === "after" ? 1 : 0);
  if (sourceIndex < insertionIndex) {
    insertionIndex -= 1;
  }

  return Math.max(0, Math.min(insertionIndex, itemCount - 1));
}

/** Move one automation step without changing its id, parameters, or status. */
export function reorderAutomationTools(
  tools: AutomationTool[],
  sourceIndex: number,
  destinationIndex: number,
): AutomationTool[] {
  if (
    sourceIndex === destinationIndex ||
    sourceIndex < 0 ||
    destinationIndex < 0 ||
    sourceIndex >= tools.length ||
    destinationIndex >= tools.length
  ) {
    return tools;
  }

  const reordered = [...tools];
  const [movedTool] = reordered.splice(sourceIndex, 1);
  reordered.splice(destinationIndex, 0, movedTool);
  return reordered;
}
