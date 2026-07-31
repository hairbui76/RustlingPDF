export type AccessibilityFindingStatus = "pass" | "fail" | "manual";
export type AccessibilityRemediation = "automatic" | "userInput" | "manual";

export interface AccessibilityFinding {
  ruleId: string;
  status: AccessibilityFindingStatus;
  severity: "error" | "warning" | "info";
  scope: string;
  title: string;
  message: string;
  remediation: AccessibilityRemediation;
  pageIndex?: number;
  objectNumber?: number;
  generation?: number;
  fieldName?: string;
}

export interface AccessibilityStructurePreview {
  objectNumber?: number;
  generation?: number;
  role: string;
  pageIndex?: number;
  alternativeText?: string;
}

export interface AccessibilityReport {
  schemaVersion: 1;
  summary: {
    passed: number;
    failed: number;
    manualReview: number;
    total: number;
    remediable: number;
  };
  document: {
    pageCount: number;
    language?: string;
    hasStructureTree: boolean;
    marked: boolean;
    figureCount: number;
    formFieldCount: number;
    structurePreviewTruncated: boolean;
    structureOrder: AccessibilityStructurePreview[];
  };
  findings: AccessibilityFinding[];
}

export interface AccessibilityRepairs {
  documentLanguage?: string;
  markAsTagged?: true;
  structureTabOrderPages?: number[];
  alternativeTexts?: Array<{
    objectNumber: number;
    generation: number;
    text: string;
  }>;
  formFieldTooltips?: Array<{
    fieldName: string;
    text: string;
  }>;
}
