import React, { useCallback, useEffect, useRef, useState } from "react";
import { LocalIcon, materialSymbol } from "@app/components/shared/LocalIcon";
import { useTranslation } from "react-i18next";
import { Text, Stack, Group } from "@mantine/core";
import { ActionIcon } from "@app/ui/ActionIcon";
import {
  draggable,
  dropTargetForElements,
} from "@atlaskit/pragmatic-drag-and-drop/element/adapter";
import { AutomationTool } from "@app/types/automation";
import { ToolRegistry } from "@app/data/toolsTaxonomy";
import { ToolId } from "@app/types/toolId";
import ToolSelector from "@app/components/tools/automate/ToolSelector";
import AutomationEntry from "@app/components/tools/automate/AutomationEntry";
import {
  type AutomationDropEdge,
  getAutomationDropIndex,
} from "@app/utils/automationReorder";

const AddCircleOutline = materialSymbol("add-circle-outline-rounded");

interface ToolListProps {
  tools: AutomationTool[];
  toolRegistry: Partial<ToolRegistry>;
  onToolUpdate: (index: number, updates: Partial<AutomationTool>) => void;
  onToolRemove: (index: number) => void;
  onToolReorder: (sourceIndex: number, destinationIndex: number) => void;
  onToolConfigure: (index: number) => void;
  onToolAdd: () => void;
  getToolName: (operation: string) => string;
  getToolDefaultParameters: (operation: string) => Record<string, unknown>;
}

interface SortableToolRowProps {
  tool: AutomationTool;
  index: number;
  totalTools: number;
  toolRegistry: Partial<ToolRegistry>;
  onToolSelect: (index: number, operation: string) => void;
  onToolRemove: (index: number) => void;
  onToolConfigure: (index: number) => void;
  onDropTool: (
    sourceToolId: string,
    targetToolId: string,
    edge: AutomationDropEdge,
  ) => void;
  onKeyboardReorder: (sourceIndex: number, destinationIndex: number) => void;
}

function SortableToolRow({
  tool,
  index,
  totalTools,
  toolRegistry,
  onToolSelect,
  onToolRemove,
  onToolConfigure,
  onDropTool,
  onKeyboardReorder,
}: SortableToolRowProps) {
  const { t } = useTranslation();
  const rowRef = useRef<HTMLDivElement>(null);
  const dragHandleRef = useRef<HTMLButtonElement>(null);
  const [isDragging, setIsDragging] = useState(false);
  const [dropEdge, setDropEdge] = useState<AutomationDropEdge | null>(null);

  useEffect(() => {
    const element = rowRef.current;
    const dragHandle = dragHandleRef.current;
    if (!element || !dragHandle) return;

    const cleanupDrag = draggable({
      element,
      dragHandle,
      getInitialData: () => ({
        type: "automation-tool",
        toolId: tool.id,
      }),
      onDragStart: () => setIsDragging(true),
      onDrop: () => setIsDragging(false),
    });

    const cleanupDropTarget = dropTargetForElements({
      element,
      canDrop: ({ source }) =>
        source.data.type === "automation-tool" &&
        source.data.toolId !== tool.id,
      getData: ({ input, element: targetElement }) => {
        const bounds = targetElement.getBoundingClientRect();
        return {
          type: "automation-tool",
          toolId: tool.id,
          edge:
            input.clientY < bounds.top + bounds.height / 2 ? "before" : "after",
        };
      },
      getDropEffect: () => "move",
      onDragEnter: ({ self }) =>
        setDropEdge(self.data.edge as AutomationDropEdge),
      onDrag: ({ self }) => setDropEdge(self.data.edge as AutomationDropEdge),
      onDragLeave: () => setDropEdge(null),
      onDrop: ({ source, self }) => {
        setDropEdge(null);
        if (
          source.data.type !== "automation-tool" ||
          typeof source.data.toolId !== "string"
        ) {
          return;
        }

        const edge = self.data.edge;
        if (edge !== "before" && edge !== "after") return;
        onDropTool(source.data.toolId, tool.id, edge);
      },
    });

    return () => {
      cleanupDrag();
      cleanupDropTarget();
    };
  }, [onDropTool, tool.id]);

  const handleReorderKeyDown = (event: React.KeyboardEvent) => {
    let destinationIndex: number;
    switch (event.key) {
      case "ArrowUp":
        destinationIndex = Math.max(0, index - 1);
        break;
      case "ArrowDown":
        destinationIndex = Math.min(totalTools - 1, index + 1);
        break;
      case "Home":
        destinationIndex = 0;
        break;
      case "End":
        destinationIndex = totalTools - 1;
        break;
      default:
        return;
    }

    event.preventDefault();
    if (destinationIndex !== index) {
      onKeyboardReorder(index, destinationIndex);
    }
  };

  const reorderLabel = t(
    "automate.creation.tools.reorderLabel",
    "Reorder {{tool}}, step {{position}} of {{total}}",
    {
      tool: tool.name,
      position: index + 1,
      total: totalTools,
    },
  );

  return (
    <div
      ref={rowRef}
      role="listitem"
      aria-posinset={index + 1}
      aria-setsize={totalTools}
      data-automation-tool-id={tool.id}
      data-automation-operation={tool.operation}
      data-automation-step={index + 1}
      style={{
        borderRadius: "var(--mantine-radius-lg)",
        boxShadow:
          dropEdge === "before"
            ? "0 -3px 0 var(--c-primary)"
            : dropEdge === "after"
              ? "0 3px 0 var(--c-primary)"
              : "none",
        opacity: isDragging ? 0.55 : 1,
        transition: "box-shadow 120ms ease, opacity 120ms ease",
      }}
    >
      <div
        style={{
          border: "1px solid var(--mantine-color-gray-2)",
          borderRadius:
            tool.operation && !tool.configured
              ? "var(--mantine-radius-lg) var(--mantine-radius-lg) 0 0"
              : "var(--mantine-radius-lg)",
          backgroundColor: "var(--mantine-color-gray-2)",
          position: "relative",
          padding: "var(--mantine-spacing-xs)",
          paddingRight: index > 1 ? "2.25rem" : undefined,
          borderBottomWidth: tool.operation && !tool.configured ? "0" : "1px",
        }}
      >
        {index > 1 && (
          <ActionIcon
            variant="tertiary"
            size="sm"
            hover={false}
            onClick={() => onToolRemove(index)}
            aria-label={t("automate.creation.tools.remove", "Remove tool")}
            title={t("automate.creation.tools.remove", "Remove tool")}
            style={{
              position: "absolute",
              top: "50%",
              right: "8px",
              transform: "translateY(-50%)",
              zIndex: 1,
              color: "var(--mantine-color-gray-6)",
            }}
          >
            <LocalIcon icon="close-rounded" style={{ fontSize: 16 }} />
          </ActionIcon>
        )}

        <Group gap="xs" align="center" wrap="nowrap">
          <ActionIcon
            ref={dragHandleRef}
            variant="tertiary"
            size="sm"
            aria-label={reorderLabel}
            aria-keyshortcuts="ArrowUp ArrowDown Home End"
            title={t(
              "automate.creation.tools.reorderHelp",
              "Drag to reorder. Use arrow keys, Home, or End from the keyboard.",
            )}
            onKeyDown={handleReorderKeyDown}
            style={{
              color: "var(--mantine-color-gray-6)",
              cursor: isDragging ? "grabbing" : "grab",
              flexShrink: 0,
              touchAction: "none",
            }}
          >
            <LocalIcon icon="drag-indicator" style={{ fontSize: 18 }} />
          </ActionIcon>

          <div style={{ flex: 1, minWidth: 0 }}>
            <ToolSelector
              key={`tool-selector-${tool.id}`}
              onSelect={(newOperation) => onToolSelect(index, newOperation)}
              excludeTools={["automate"]}
              toolRegistry={toolRegistry}
              selectedValue={tool.operation}
              placeholder={tool.name}
            />
          </div>

          {tool.operation && (
            <ActionIcon
              variant="tertiary"
              size="sm"
              onClick={() => onToolConfigure(index)}
              aria-label={t(
                "automate.creation.tools.configure",
                "Configure tool",
              )}
              title={t("automate.creation.tools.configure", "Configure tool")}
              style={{ color: "var(--mantine-color-gray-6)" }}
            >
              <LocalIcon icon="settings-rounded" style={{ fontSize: 16 }} />
            </ActionIcon>
          )}
        </Group>
      </div>

      {tool.operation && !tool.configured && (
        <div
          style={{
            width: "100%",
            border: "1px solid var(--mantine-color-gray-2)",
            borderTop: "none",
            borderRadius:
              "0 0 var(--mantine-radius-lg) var(--mantine-radius-lg)",
            backgroundColor: "var(--c-active)",
            padding: "var(--mantine-spacing-xs)",
          }}
        >
          <Text pl="md" size="xs">
            {t("automate.creation.tools.notConfigured", "! Not Configured")}
          </Text>
        </div>
      )}
    </div>
  );
}

export default function ToolList({
  tools,
  toolRegistry,
  onToolUpdate,
  onToolRemove,
  onToolReorder,
  onToolConfigure,
  onToolAdd,
  getToolName,
  getToolDefaultParameters,
}: ToolListProps) {
  const { t } = useTranslation();

  const handleToolSelect = (index: number, newOperation: string) => {
    const defaultParams = getToolDefaultParameters(newOperation);
    const toolEntry = toolRegistry[newOperation as ToolId];
    // If tool has no settingsComponent, it's automatically configured
    const isConfigured = !toolEntry?.automationSettings;

    onToolUpdate(index, {
      operation: newOperation,
      name: getToolName(newOperation),
      configured: isConfigured,
      parameters: defaultParams,
    });
  };

  const handleReorder = useCallback(
    (sourceIndex: number, destinationIndex: number) => {
      if (
        sourceIndex === destinationIndex ||
        sourceIndex < 0 ||
        destinationIndex < 0 ||
        sourceIndex >= tools.length ||
        destinationIndex >= tools.length
      ) {
        return;
      }

      const movedTool = tools[sourceIndex];
      onToolReorder(sourceIndex, destinationIndex);
      setAnnouncement(
        t(
          "automate.creation.tools.reordered",
          "{{tool}} moved to step {{position}}.",
          {
            tool: movedTool.name,
            position: destinationIndex + 1,
          },
        ),
      );
    },
    [onToolReorder, t, tools],
  );

  const handleDropTool = useCallback(
    (sourceToolId: string, targetToolId: string, edge: AutomationDropEdge) => {
      const sourceIndex = tools.findIndex((tool) => tool.id === sourceToolId);
      const targetIndex = tools.findIndex((tool) => tool.id === targetToolId);
      if (sourceIndex < 0 || targetIndex < 0) return;

      handleReorder(
        sourceIndex,
        getAutomationDropIndex(sourceIndex, targetIndex, edge, tools.length),
      );
    },
    [handleReorder, tools],
  );

  const [announcement, setAnnouncement] = useState("");

  return (
    <div>
      <Text
        size="sm"
        fw={500}
        mb="xs"
        style={{ color: "var(--mantine-color-text)" }}
      >
        {t("automate.creation.tools.selected", "Selected Tools")} (
        {tools.length})
      </Text>
      <Text size="xs" c="dimmed" mb="xs">
        {t(
          "automate.creation.tools.reorderInstructions",
          "Drag steps to reorder them, or focus a drag handle and use the arrow keys.",
        )}
      </Text>
      <div
        aria-live="polite"
        aria-atomic="true"
        style={{
          position: "absolute",
          width: 1,
          height: 1,
          padding: 0,
          margin: -1,
          overflow: "hidden",
          clip: "rect(0, 0, 0, 0)",
          whiteSpace: "nowrap",
          border: 0,
        }}
      >
        {announcement}
      </div>
      <Stack gap="0" role="list">
        {tools.map((tool, index) => (
          <React.Fragment key={tool.id}>
            <SortableToolRow
              tool={tool}
              index={index}
              totalTools={tools.length}
              toolRegistry={toolRegistry}
              onToolSelect={handleToolSelect}
              onToolRemove={onToolRemove}
              onToolConfigure={onToolConfigure}
              onDropTool={handleDropTool}
              onKeyboardReorder={handleReorder}
            />
            {index < tools.length - 1 && (
              <div style={{ textAlign: "center", padding: "8px 0" }}>
                <Text size="xs" c="dimmed">
                  ↓
                </Text>
              </div>
            )}
          </React.Fragment>
        ))}

        {/* Arrow before Add Tool Button */}
        {tools.length > 0 && (
          <div style={{ textAlign: "center", padding: "8px 0" }}>
            <Text size="xs" c="dimmed">
              ↓
            </Text>
          </div>
        )}

        {/* Add Tool Button */}
        <div
          style={{
            border: "1px solid var(--mantine-color-gray-2)",
            borderRadius: "var(--mantine-radius-sm)",
            overflow: "hidden",
          }}
        >
          <AutomationEntry
            title={t("automate.creation.tools.addTool", "Add Tool")}
            badgeIcon={AddCircleOutline}
            operations={[]}
            onClick={onToolAdd}
            keepIconColor={true}
          />
        </div>
      </Stack>
    </div>
  );
}
