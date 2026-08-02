import { useMemo } from "react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import AutoAwesomeRoundedIcon from "@mui/icons-material/AutoAwesomeRounded";
import React from "react";

import {
  SUBCATEGORY_ORDER,
  SubcategoryId,
  ToolCategoryId,
  getSubcategoryIcon,
  getSubcategoryLabel,
  type ToolRegistry,
  type ToolRegistryEntry,
} from "@app/data/toolsTaxonomy";
import { ToolId } from "@app/types/toolId";

/** Top-level tool group: the pseudo-group "recommended", then each subcategory. */
export const RECOMMENDED_GROUP_ID = "recommended" as const;
export type ToolGroupId = typeof RECOMMENDED_GROUP_ID | SubcategoryId;

export interface ToolGroupEntry {
  id: ToolId;
  tool: ToolRegistryEntry;
}

export interface ToolGroup {
  id: ToolGroupId;
  label: string;
  icon: ReactNode;
  tools: ToolGroupEntry[];
}

/**
 * The bottom bar's top-level choice.
 *
 * Derived from the whole registry rather than the search-filtered list, so the
 * set of groups — and therefore the bar's width — never changes while the user
 * types. Filtering by query is the search view's job, not the bar's.
 *
 * A tool appears in exactly one group: recommended tools go to "recommended",
 * everything else to its subcategory. That keeps every group short enough to
 * fit a window without scrolling, which is the whole point of the split.
 */
export function useToolGroups(
  toolRegistry: Partial<ToolRegistry>,
): ToolGroup[] {
  const { t } = useTranslation();

  return useMemo(() => {
    const recommended: ToolGroupEntry[] = [];
    const bySubcategory = new Map<SubcategoryId, ToolGroupEntry[]>();

    Object.entries(toolRegistry).forEach(([rawId, tool]) => {
      if (!tool) return;
      const entry: ToolGroupEntry = { id: rawId as ToolId, tool };
      if (tool.categoryId === ToolCategoryId.RECOMMENDED_TOOLS) {
        // Mirrors useToolSections: only tools that can actually be opened.
        const ready =
          tool.component !== null ||
          !!tool.link ||
          rawId === "read" ||
          rawId === "multiTool";
        if (ready) recommended.push(entry);
        return;
      }
      const list = bySubcategory.get(tool.subcategoryId);
      if (list) list.push(entry);
      else bySubcategory.set(tool.subcategoryId, [entry]);
    });

    const groups: ToolGroup[] = [];
    if (recommended.length > 0) {
      groups.push({
        id: RECOMMENDED_GROUP_ID,
        label: t("toolPanel.fullscreen.recommended", "Recommend"),
        icon: React.createElement(AutoAwesomeRoundedIcon),
        tools: recommended,
      });
    }
    SUBCATEGORY_ORDER.forEach((subcategoryId) => {
      const tools = bySubcategory.get(subcategoryId);
      if (!tools || tools.length === 0) return;
      groups.push({
        id: subcategoryId,
        label: getSubcategoryLabel(t, subcategoryId),
        icon: getSubcategoryIcon(subcategoryId),
        tools,
      });
    });
    return groups;
  }, [toolRegistry, t]);
}
