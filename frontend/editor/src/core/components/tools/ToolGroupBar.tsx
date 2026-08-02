import { useCallback, useEffect, useMemo, useRef } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@app/ui/Button";
import { Tooltip } from "@app/components/shared/Tooltip";
import { useToolWorkflow } from "@app/contexts/ToolWorkflowContext";
import { useToolGroupSelection } from "@app/contexts/ToolGroupContext";
import {
  useToolGroups,
  type ToolGroupId,
} from "@app/hooks/tools/useToolGroups";
import { withViewTransition } from "@app/utils/viewTransition";
import { TOOL_GROUP_PANEL_ID } from "@app/constants/toolPanel";
import "@app/components/tools/ToolGroupBar.css";

/**
 * Top-level tool navigation: a floating bar at the bottom of the workbench
 * holding every tool *group*.
 *
 * Why groups and not tools: the previous right-hand list held all ~60 tools in
 * one column, so finding anything meant scrolling. Splitting it in two — the
 * dozen groups here, that group's handful of tools in the right panel — means
 * neither surface ever exceeds the viewport.
 *
 * It is a real flex row in the workbench column, not an overlay: a document
 * app puts the page in the middle of the screen, so the bar reserves its own
 * height instead of floating on top of the page. The rounded card and shadow
 * are the only thing that is "floating" about it.
 *
 * Semantics are the ARIA tabs pattern — `tablist` here, `tabpanel` on the tool
 * list in the right rail — with a roving tabindex, so the bar is one Tab stop
 * and the arrow keys move between groups.
 */
interface ToolGroupBarProps {
  /** Extra class on the region wrapper. The mobile layout uses it to stop the
      bar stretching, since `.mobile-slide-content > *` gives every child
      `flex: 1 1 auto`. */
  className?: string;
}

export default function ToolGroupBar({ className }: ToolGroupBarProps = {}) {
  const { t } = useTranslation();
  const {
    toolRegistry,
    selectedToolKey,
    leftPanelView,
    setLeftPanelView,
    sidebarsVisible,
    setSidebarsVisible,
    setSearchQuery,
    handleBackToTools,
  } = useToolWorkflow();
  const { selectedGroup, setSelectedGroup } = useToolGroupSelection();
  const groups = useToolGroups(toolRegistry);
  const buttonRefs = useRef(new Map<ToolGroupId, HTMLButtonElement>());

  // The group that owns the tool currently open, so opening a tool from search
  // (or from a deep link) still lights up the right group in the bar.
  const groupOfActiveTool = useMemo(() => {
    if (!selectedToolKey) return null;
    return (
      groups.find((g) => g.tools.some((entry) => entry.id === selectedToolKey))
        ?.id ?? null
    );
  }, [groups, selectedToolKey]);

  const activeGroup = groupOfActiveTool ?? selectedGroup;

  // Keep the bar and the panel in step when a tool is opened from elsewhere.
  useEffect(() => {
    if (groupOfActiveTool && groupOfActiveTool !== selectedGroup) {
      setSelectedGroup(groupOfActiveTool);
    }
  }, [groupOfActiveTool, selectedGroup, setSelectedGroup]);

  const handleSelect = useCallback(
    (id: ToolGroupId) => {
      withViewTransition(() => {
        setSelectedGroup(id);
        // Picking a group is a request to see that group's tools, so leave any
        // open tool and any active search, and make sure the panel is showing.
        setSearchQuery("");
        if (leftPanelView !== "toolPicker") handleBackToTools();
        setLeftPanelView("toolPicker");
        if (!sidebarsVisible) setSidebarsVisible(true);
      });
    },
    [
      handleBackToTools,
      leftPanelView,
      setLeftPanelView,
      setSearchQuery,
      setSelectedGroup,
      setSidebarsVisible,
      sidebarsVisible,
    ],
  );

  const handleKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLDivElement>) => {
      const keys = ["ArrowLeft", "ArrowRight", "Home", "End"];
      if (!keys.includes(event.key)) return;
      const index = groups.findIndex((g) => g.id === activeGroup);
      if (index === -1) return;
      let next = index;
      if (event.key === "ArrowLeft")
        next = (index - 1 + groups.length) % groups.length;
      else if (event.key === "ArrowRight") next = (index + 1) % groups.length;
      else if (event.key === "Home") next = 0;
      else if (event.key === "End") next = groups.length - 1;
      event.preventDefault();
      const target = groups[next];
      if (!target) return;
      handleSelect(target.id);
      buttonRefs.current.get(target.id)?.focus();
    },
    [activeGroup, groups, handleSelect],
  );

  if (groups.length === 0) return null;

  return (
    <div className={`tool-group-bar-region${className ? ` ${className}` : ""}`}>
      <div
        className="tool-group-bar"
        role="tablist"
        aria-label={t("toolGroupBar.label", "Tool groups")}
        aria-orientation="horizontal"
        onKeyDown={handleKeyDown}
      >
        {groups.map((group) => {
          const selected = group.id === activeGroup;
          const description = t(
            "toolGroupBar.itemTitle",
            "{{group}} ({{count}} tools)",
            {
              group: group.label,
              count: group.tools.length,
            },
          );
          return (
            // Icon only, name on hover. The shared Tooltip rather than a native
            // `title` because `title` never appears for a keyboard user: this
            // one opens on focus too, so tabbing through the bar still tells you
            // what each icon is. `aria-label` carries the same name for screen
            // readers, which is why removing the visible text costs them
            // nothing.
            <Tooltip
              key={group.id}
              content={description}
              position="top"
              arrow
              delay={250}
            >
              <Button
                type="button"
                variant="quiet"
                hover={false}
                role="tab"
                id={`tool-group-tab-${group.id}`}
                ref={(node: HTMLButtonElement | null) => {
                  if (node) buttonRefs.current.set(group.id, node);
                  else buttonRefs.current.delete(group.id);
                }}
                aria-selected={selected}
                aria-controls={TOOL_GROUP_PANEL_ID}
                aria-label={description}
                tabIndex={selected ? 0 : -1}
                className="tool-group-bar__item"
                data-selected={selected}
                onClick={() => handleSelect(group.id)}
              >
                <span className="tool-group-bar__icon" aria-hidden="true">
                  {group.icon}
                </span>
              </Button>
            </Tooltip>
          );
        })}
      </div>
    </div>
  );
}
