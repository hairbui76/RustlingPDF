import { describe, expect, test } from "vitest";
import type { AutomationTool } from "@app/types/automation";
import {
  getAutomationDropIndex,
  reorderAutomationTools,
} from "@app/utils/automationReorder";

const tools: AutomationTool[] = [
  {
    id: "merge",
    operation: "merge",
    name: "Merge",
    configured: true,
    parameters: { generateToc: true },
  },
  {
    id: "compress",
    operation: "compress",
    name: "Compress",
    configured: true,
    parameters: { compressionLevel: 3 },
  },
  {
    id: "rotate",
    operation: "rotate",
    name: "Rotate",
    configured: true,
    parameters: { angle: 90 },
  },
];

describe("automation reorder", () => {
  test("moves a step while preserving its complete configuration", () => {
    const reordered = reorderAutomationTools(tools, 0, 2);

    expect(reordered.map((tool) => tool.id)).toEqual([
      "compress",
      "rotate",
      "merge",
    ]);
    expect(reordered[2]).toBe(tools[0]);
    expect(reordered[2].parameters).toEqual({ generateToc: true });
    expect(tools.map((tool) => tool.id)).toEqual([
      "merge",
      "compress",
      "rotate",
    ]);
  });

  test("resolves drops before and after targets in both directions", () => {
    expect(getAutomationDropIndex(0, 2, "after", 3)).toBe(2);
    expect(getAutomationDropIndex(0, 2, "before", 3)).toBe(1);
    expect(getAutomationDropIndex(2, 0, "before", 3)).toBe(0);
    expect(getAutomationDropIndex(2, 0, "after", 3)).toBe(1);
  });

  test("ignores invalid and no-op moves", () => {
    expect(reorderAutomationTools(tools, 1, 1)).toBe(tools);
    expect(reorderAutomationTools(tools, -1, 1)).toBe(tools);
    expect(reorderAutomationTools(tools, 0, 3)).toBe(tools);
  });
});
