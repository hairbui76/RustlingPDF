import { Alert, Stack } from "@mantine/core";
import { useTranslation } from "react-i18next";
import { Button } from "@app/ui/Button";
import { createToolFlow } from "@app/components/tools/shared/createToolFlow";
import { useBaseTool } from "@app/hooks/tools/shared/useBaseTool";
import { useAiEngineEnabled } from "@app/hooks/useAiEngineEnabled";
import type { BaseToolProps, ToolComponent } from "@app/types/tool";
import {
  defaultParameters,
  useDocumentUnderstandingParameters,
} from "@app/hooks/tools/documentUnderstanding/useDocumentUnderstandingParameters";
import {
  type DocumentUnderstandingOperationHook,
  useDocumentUnderstandingOperation,
} from "@app/hooks/tools/documentUnderstanding/useDocumentUnderstandingOperation";
import DocumentUnderstandingSettings from "@app/tools/documentUnderstanding/DocumentUnderstandingSettings";
import DocumentUnderstandingResults from "@app/tools/documentUnderstanding/DocumentUnderstandingResults";

const DocumentUnderstanding = (props: BaseToolProps) => {
  const { t } = useTranslation();
  const aiEnabled = useAiEngineEnabled();
  const base = useBaseTool(
    "documentUnderstanding",
    useDocumentUnderstandingParameters,
    useDocumentUnderstandingOperation,
    props,
  );
  const operation = base.operation as DocumentUnderstandingOperationHook;

  return createToolFlow({
    files: {
      selectedFiles: base.selectedFiles,
      isCollapsed: operation.response !== null,
      minFiles: 1,
    },
    steps: [
      {
        title: t(
          "documentUnderstanding.settings",
          "Document understanding settings",
        ),
        isCollapsed: operation.response !== null,
        content: (
          <DocumentUnderstandingSettings
            parameters={base.params}
            aiEnabled={aiEnabled}
          />
        ),
      },
      {
        title: t("documentUnderstanding.results", "Result"),
        isVisible: operation.response !== null,
        isCollapsed: false,
        content: operation.response ? (
          <Stack gap="sm">
            <DocumentUnderstandingResults response={operation.response} />
            <Button variant="secondary" onClick={base.handleUndo}>
              {t("documentUnderstanding.startOver", "Start over")}
            </Button>
          </Stack>
        ) : null,
      },
    ],
    executeButton: {
      text: t("documentUnderstanding.submit", "Analyze document"),
      loadingText: t(
        "documentUnderstanding.processing",
        "Analyzing document...",
      ),
      onClick: base.handleExecute,
      endpointEnabled: aiEnabled ? base.endpointEnabled : false,
      paramsValid: base.params.validateParameters(),
      isVisible: operation.response === null,
    },
    belowExecuteButton:
      !aiEnabled && base.selectedFiles.length > 0 ? (
        <Alert color="red">
          This server has not enabled the optional AI engine.
        </Alert>
      ) : undefined,
    review: {
      isVisible: false,
      operation,
      title: t("documentUnderstanding.results", "Result"),
      onUndo: base.handleUndo,
    },
  });
};

const DocumentUnderstandingTool = DocumentUnderstanding as ToolComponent;
DocumentUnderstandingTool.tool = () => useDocumentUnderstandingOperation;
DocumentUnderstandingTool.getDefaultParameters = () => ({
  ...defaultParameters,
  extractionFields: defaultParameters.extractionFields.map((field) => ({
    ...field,
  })),
});

export default DocumentUnderstandingTool;
