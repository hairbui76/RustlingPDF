import React, { useMemo, useRef } from "react";
import { Box, Stack } from "@mantine/core";
import { useTranslation } from "react-i18next";
import "@app/components/tools/toolPicker/ToolPicker.css";
import { useFavoriteToolItems } from "@app/hooks/tools/useFavoriteToolItems";
import NoToolsFound from "@app/components/tools/shared/NoToolsFound";
import ToolButton from "@app/components/tools/toolPicker/ToolButton";
import { useToolWorkflowData } from "@app/contexts/ToolWorkflowContext";
import { useToolGroupSelection } from "@app/contexts/ToolGroupContext";
import {
  RECOMMENDED_GROUP_ID,
  useToolGroups,
} from "@app/hooks/tools/useToolGroups";
import { TOOL_GROUP_PANEL_ID } from "@app/constants/toolPanel";
import { ToolPickerFooterExtensions } from "@app/components/tools/toolPicker/ToolPickerFooterExtensions";

interface ToolPickerProps {
  selectedToolKey: string | null;
  onSelect: (id: string) => void;
}

const HEADER_TEXT_STYLE: React.CSSProperties = {
  fontSize: "0.68rem",
  fontWeight: 600,
  padding: "0.25rem 0 0.35rem 0.5rem",
  textTransform: "uppercase",
  letterSpacing: "0.06em",
  color: "var(--c-text-subtle)",
};
const SCROLLABLE_STYLE: React.CSSProperties = {
  flex: 1,
  overflowY: "auto",
  overflowX: "hidden",
  minHeight: 0,
  height: "100%",
  marginTop: -2,
};
const CONTAINER_STYLE: React.CSSProperties = {
  display: "flex",
  flexDirection: "column",
  background: "var(--c-bg-raised)",
};
const toTitleCase = (s: string) =>
  s.replace(
    /\w\S*/g,
    (txt) => txt.charAt(0).toUpperCase() + txt.slice(1).toLowerCase(),
  );

/**
 * The right rail's tool list: the tools in the group currently selected in the
 * bottom group bar, and nothing else. Search has its own view (SearchResults),
 * so this never has to render the whole catalogue.
 */
const ToolPicker = ({ selectedToolKey, onSelect }: ToolPickerProps) => {
  const { t } = useTranslation();

  const scrollableRef = useRef<HTMLDivElement>(null);

  const { favoriteTools, toolRegistry } = useToolWorkflowData();

  const favoriteToolItems = useFavoriteToolItems(favoriteTools, toolRegistry);

  // Group detail view: the bottom bar picks the group, this lists its tools.
  const { selectedGroup } = useToolGroupSelection();
  const groups = useToolGroups(toolRegistry);
  const activeGroup = useMemo(
    () => groups.find((g) => g.id === selectedGroup) ?? groups[0],
    [groups, selectedGroup],
  );
  const showFavourites =
    favoriteToolItems.length > 0 && activeGroup?.id === RECOMMENDED_GROUP_ID;

  return (
    <Box h="100%" style={CONTAINER_STYLE}>
      <Box
        ref={scrollableRef}
        style={SCROLLABLE_STYLE}
        className="tool-picker-scrollable"
      >
        {/* Group detail: only the group picked in the bottom bar. The list is
            the `tabpanel` for that bar's `tab`, hence the id/role/labelledby.
            Favourites ride along at the top of Recommended so pinned tools stay
            one click away without needing a group of their own. */}
        <div
          id={TOOL_GROUP_PANEL_ID}
          role="tabpanel"
          aria-labelledby={`tool-group-tab-${selectedGroup}`}
          tabIndex={-1}
        >
          <Stack px="sm" pb="sm" pt={4} gap="xs">
            {showFavourites && (
              <Box w="100%">
                <div style={HEADER_TEXT_STYLE}>
                  {t("toolPanel.fullscreen.favorites", "Favourites")}
                </div>
                <div>
                  {favoriteToolItems.map(({ id, tool }) => (
                    <ToolButton
                      key={`fav-${id}`}
                      id={id}
                      tool={tool}
                      isSelected={selectedToolKey === id}
                      onSelect={onSelect}
                      hasStars
                    />
                  ))}
                </div>
              </Box>
            )}
            {activeGroup && activeGroup.tools.length > 0 ? (
              <Box w="100%">
                <div style={HEADER_TEXT_STYLE}>
                  {toTitleCase(activeGroup.label)}
                </div>
                <div>
                  {activeGroup.tools.map(({ id, tool }) => (
                    <ToolButton
                      key={`grp-${id}`}
                      id={id}
                      tool={tool}
                      isSelected={selectedToolKey === id}
                      onSelect={onSelect}
                      hasStars
                    />
                  ))}
                </div>
              </Box>
            ) : (
              !showFavourites && <NoToolsFound />
            )}
          </Stack>
        </div>
      </Box>
      <ToolPickerFooterExtensions />
    </Box>
  );
};

export default ToolPicker;
