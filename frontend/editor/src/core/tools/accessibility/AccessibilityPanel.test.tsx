import React from "react";
import { MantineProvider } from "@mantine/core";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import AccessibilityPanel from "@app/tools/accessibility/AccessibilityPanel";
import type { AccessibilityReport } from "@app/tools/accessibility/types";

const report: AccessibilityReport = {
  schemaVersion: 1,
  summary: {
    passed: 1,
    failed: 5,
    manualReview: 1,
    total: 7,
    remediable: 5,
  },
  document: {
    pageCount: 1,
    hasStructureTree: true,
    marked: false,
    figureCount: 1,
    formFieldCount: 1,
    structurePreviewTruncated: false,
    structureOrder: [
      {
        objectNumber: 12,
        generation: 0,
        role: "Figure",
        pageIndex: 0,
      },
    ],
  },
  findings: [
    {
      ruleId: "reading-order.logical",
      status: "manual",
      severity: "warning",
      scope: "document",
      title: "Logical reading order",
      message: "Review the ordered structure preview.",
      remediation: "manual",
    },
    {
      ruleId: "structure.marked",
      status: "fail",
      severity: "error",
      scope: "document",
      title: "Document marked as tagged",
      message: "The structure tree is not marked.",
      remediation: "automatic",
    },
    {
      ruleId: "reading-order.annotation-tabs",
      status: "fail",
      severity: "error",
      scope: "page",
      title: "Annotation tab order",
      message: "Use structure order.",
      remediation: "automatic",
      pageIndex: 0,
    },
    {
      ruleId: "document.language",
      status: "fail",
      severity: "error",
      scope: "document",
      title: "Default document language",
      message: "No language is set.",
      remediation: "automatic",
    },
    {
      ruleId: "figure.alternative-text",
      status: "fail",
      severity: "error",
      scope: "structure",
      title: "Figure alternative text",
      message: "A description is required.",
      remediation: "userInput",
      objectNumber: 12,
      generation: 0,
      pageIndex: 0,
    },
    {
      ruleId: "form-field.accessible-name",
      status: "fail",
      severity: "error",
      scope: "formField",
      title: "Accessible form-field name",
      message: "A label is required.",
      remediation: "userInput",
      objectNumber: 20,
      generation: 0,
      fieldName: "customer.name",
    },
    {
      ruleId: "structure.tree",
      status: "pass",
      severity: "info",
      scope: "document",
      title: "Tagged structure tree",
      message: "A structure tree exists.",
      remediation: "manual",
    },
  ],
};

describe("AccessibilityPanel", () => {
  it("builds explicit repair targets and keeps the conformance limit visible", async () => {
    const user = userEvent.setup();
    const onApply = vi.fn().mockResolvedValue(undefined);
    render(
      <MantineProvider>
        <AccessibilityPanel
          report={report}
          isLoading={false}
          endpointEnabled
          status=""
          errorMessage={null}
          onApply={onApply}
        />
      </MantineProvider>,
    );

    expect(screen.getByText(/does not certify PDF\/UA/i)).toBeInTheDocument();
    await user.type(
      screen.getByLabelText("Default document language"),
      "en-US",
    );
    await user.type(screen.getByLabelText("Figure on page 1"), "Revenue chart");
    await user.type(screen.getByLabelText("customer.name"), "Customer name");
    await user.click(
      screen.getByRole("button", { name: "Apply repairs and re-check" }),
    );

    expect(onApply).toHaveBeenCalledWith({
      documentLanguage: "en-US",
      markAsTagged: true,
      structureTabOrderPages: [0],
      alternativeTexts: [
        {
          objectNumber: 12,
          generation: 0,
          text: "Revenue chart",
        },
      ],
      formFieldTooltips: [
        {
          fieldName: "customer.name",
          text: "Customer name",
        },
      ],
    });
  });
});
