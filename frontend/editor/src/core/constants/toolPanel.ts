// Tool panel constants

export type ToolPanelMode = "sidebar" | "fullscreen";

export const DEFAULT_TOOL_PANEL_MODE: ToolPanelMode = "sidebar";

/**
 * DOM id of the tool list in the right rail. The bottom group bar is the
 * `tablist` and this is its `tabpanel`, so both sides need the same id.
 */
export const TOOL_GROUP_PANEL_ID = "tool-group-panel";
