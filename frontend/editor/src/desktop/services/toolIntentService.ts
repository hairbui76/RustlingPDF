import { getToolUrlPath } from "@app/data/toolsTaxonomy";
import { withBasePath } from "@app/constants/app";
import type { ToolId } from "@app/types/toolId";

/**
 * Explorer context-menu launch intents (desktop only).
 *
 * The Windows MSI registers cascade verbs that launch the app with
 * `--tool <action>`; the Rust side aggregates multi-select launches and hands
 * the frontend one batch tagged with the action name. This module maps that
 * action to a tool route.
 *
 * SINGLE SOURCE OF TRUTH triple — this list MUST stay identical to:
 * - `frontend/editor/src-tauri/src/launch_intent.rs` (`TOOL_INTENT_ACTIONS`)
 * - `frontend/editor/src-tauri/windows/wix/provisioning.wxs`
 *   (the `--tool <action>` verb commands)
 */
export const TOOL_INTENT_ACTIONS = [
  "open",
  "merge",
  "compress",
  "convert",
] as const;

export type ToolIntentAction = (typeof TOOL_INTENT_ACTIONS)[number];

/**
 * Tool the intent routes to. `open` deliberately maps to no tool: the files
 * just land in the workspace exactly like a plain file-association open.
 */
const INTENT_TOOL_IDS: Record<ToolIntentAction, ToolId | null> = {
  open: null,
  merge: "merge",
  compress: "compress",
  convert: "convert",
};

function isToolIntentAction(value: string): value is ToolIntentAction {
  return (TOOL_INTENT_ACTIONS as readonly string[]).includes(value);
}

/**
 * Resolve a launch intent to the tool it should open, or null for plain
 * opens: a missing intent, the `open` action, and any unknown action all
 * degrade to "just add the files".
 */
export function resolveToolIntent(
  intent: string | null | undefined,
): ToolId | null {
  if (!intent || !isToolIntentAction(intent)) {
    return null;
  }
  return INTENT_TOOL_IDS[intent];
}

/**
 * Navigate the current window to the tool a launch intent maps to. Returns
 * true when a navigation was dispatched, false for plain-open intents.
 *
 * Tool selection is owned by ToolWorkflowContext, which this desktop startup
 * seam sits outside of; pushing the tool's URL and dispatching a synthetic
 * popstate routes through the exact same URL-driven selection path as browser
 * back/forward (useNavigationUrlSync's popstate handler), including its
 * availability and premium checks.
 */
export function navigateToToolIntent(
  intent: string | null | undefined,
): boolean {
  const toolId = resolveToolIntent(intent);
  if (!toolId) {
    return false;
  }
  const targetPath = withBasePath(getToolUrlPath(toolId));
  if (window.location.pathname !== targetPath) {
    window.history.pushState(null, "", targetPath);
  }
  window.dispatchEvent(new PopStateEvent("popstate"));
  return true;
}
