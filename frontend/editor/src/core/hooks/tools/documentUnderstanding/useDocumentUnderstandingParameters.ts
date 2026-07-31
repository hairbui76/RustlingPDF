import type { BaseParameters } from "@app/types/parameters";
import {
  type BaseParametersHook,
  useBaseParameters,
} from "@app/hooks/tools/shared/useBaseParameters";
import type {
  DocumentUnderstandingMode,
  ExtractionField,
  SummaryDetail,
} from "@app/tools/documentUnderstanding/types";

export interface DocumentUnderstandingParameters extends BaseParameters {
  mode: DocumentUnderstandingMode;
  summaryDetail: SummaryDetail;
  instructions: string;
  extractionFields: ExtractionField[];
  sourceLanguage: string;
  targetLanguage: string;
}

export const defaultParameters: DocumentUnderstandingParameters = {
  mode: "summary",
  summaryDetail: "standard",
  instructions: "",
  extractionFields: [
    {
      key: "field_1",
      description: "",
      valueType: "string",
      required: false,
    },
  ],
  sourceLanguage: "",
  targetLanguage: "",
};

export type DocumentUnderstandingParametersHook =
  BaseParametersHook<DocumentUnderstandingParameters>;

function validExtractionFields(fields: ExtractionField[]): boolean {
  if (fields.length === 0 || fields.length > 50) return false;
  const keys = new Set<string>();
  for (const field of fields) {
    if (
      !/^[A-Za-z0-9_.-]{1,64}$/.test(field.key) ||
      !field.description.trim() ||
      field.description.length > 500 ||
      keys.has(field.key)
    ) {
      return false;
    }
    keys.add(field.key);
  }
  return true;
}

function validate(parameters: DocumentUnderstandingParameters): boolean {
  if (parameters.instructions.length > 4_000) return false;
  switch (parameters.mode) {
    case "summary":
      return true;
    case "extraction":
      return validExtractionFields(parameters.extractionFields);
    case "translation":
      return (
        parameters.targetLanguage.trim().length > 0 &&
        parameters.targetLanguage.length <= 100 &&
        parameters.sourceLanguage.length <= 100
      );
  }
}

export const useDocumentUnderstandingParameters =
  (): DocumentUnderstandingParametersHook =>
    useBaseParameters({
      defaultParameters,
      endpointName: "tools",
      validateFn: validate,
    });
