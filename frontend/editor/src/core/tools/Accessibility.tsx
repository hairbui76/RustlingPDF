import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { createToolFlow } from "@app/components/tools/shared/createToolFlow";
import { useBaseTool } from "@app/hooks/tools/shared/useBaseTool";
import { useEndpointEnabled } from "@app/hooks/useEndpointConfig";
import type { BaseToolProps, ToolComponent } from "@app/types/tool";
import {
  defaultParameters,
  useAccessibilityParameters,
} from "@app/hooks/tools/accessibility/useAccessibilityParameters";
import {
  AccessibilityOperationHook,
  useAccessibilityOperation,
} from "@app/hooks/tools/accessibility/useAccessibilityOperation";
import AccessibilityPanel from "@app/tools/accessibility/AccessibilityPanel";
import type { AccessibilityRepairs } from "@app/tools/accessibility/types";

const Accessibility = (props: BaseToolProps) => {
  const { t } = useTranslation();
  const base = useBaseTool(
    "accessibility",
    useAccessibilityParameters,
    useAccessibilityOperation,
    props,
  );
  const operation = base.operation as AccessibilityOperationHook;
  const remediationEndpoint = useEndpointEnabled("remediate");

  const applyRepairs = useCallback(
    async (repairs: AccessibilityRepairs) => {
      const source = operation.checkedFile ?? base.selectedFiles[0];
      if (!source) return;
      try {
        const output = await operation.applyRepairs(source, repairs);
        props.onComplete?.([output]);
      } catch (error) {
        props.onError?.(
          error instanceof Error
            ? error.message
            : t("accessibility.error", "Accessibility remediation failed"),
        );
      }
    },
    [base.selectedFiles, operation, props, t],
  );

  return createToolFlow({
    files: {
      selectedFiles: base.selectedFiles,
      isCollapsed: operation.report !== null,
      minFiles: 1,
    },
    steps: operation.report
      ? [
          {
            title: t("accessibility.results", "Accessibility report"),
            isCollapsed: false,
            content: (
              <AccessibilityPanel
                report={operation.report}
                isLoading={operation.isLoading}
                endpointEnabled={remediationEndpoint.enabled}
                status={operation.status}
                errorMessage={operation.errorMessage}
                onApply={applyRepairs}
              />
            ),
          },
        ]
      : [],
    executeButton: {
      text: t("accessibility.submit", "Check accessibility"),
      loadingText: t("accessibility.processing", "Checking accessibility..."),
      onClick: base.handleExecute,
      endpointEnabled: base.endpointEnabled,
      paramsValid: base.params.validateParameters(),
      isVisible: operation.report === null,
    },
    review: {
      isVisible: false,
      operation,
      title: t("accessibility.results", "Accessibility report"),
      onUndo: base.handleUndo,
    },
  });
};

const AccessibilityTool = Accessibility as ToolComponent;
AccessibilityTool.tool = () => useAccessibilityOperation;
AccessibilityTool.getDefaultParameters = () => ({ ...defaultParameters });

export default AccessibilityTool;
