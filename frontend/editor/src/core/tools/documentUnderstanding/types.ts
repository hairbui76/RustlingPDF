export type DocumentUnderstandingMode =
  "summary" | "extraction" | "translation";

export type SummaryDetail = "brief" | "standard" | "detailed";

export type ExtractionValueType =
  "string" | "number" | "integer" | "boolean" | "date" | "list";

export interface ExtractionField {
  key: string;
  description: string;
  valueType: ExtractionValueType;
  required: boolean;
}

export interface DocumentUnderstandingSource {
  fileName: string;
  pagesProcessed: number;
  charactersProcessed: number;
  maxPages: number;
  maxCharacters: number;
}

export interface ReferencedKeyPoint {
  text: string;
  pages: number[];
}

export interface SummaryResult {
  summary: string;
  keyPoints: ReferencedKeyPoint[];
}

export interface ExtractedValue {
  key: string;
  value: unknown;
  pages: number[];
  confidence: "high" | "medium" | "low";
  note?: string;
}

export interface ExtractionResult {
  values: ExtractedValue[];
}

export interface TranslationBlock {
  blockId: string;
  sourceText: string;
  translatedText: string;
}

export interface TranslatedPage {
  pageNumber: number;
  blocks: TranslationBlock[];
}

export interface TranslationResult {
  sourceLanguage?: string;
  targetLanguage: string;
  pages: TranslatedPage[];
}

export type DocumentUnderstandingResult =
  SummaryResult | ExtractionResult | TranslationResult;

export interface DocumentUnderstandingResponse {
  schemaVersion: 1;
  operation: DocumentUnderstandingMode;
  providerDisclosure: string;
  source: DocumentUnderstandingSource;
  result: DocumentUnderstandingResult;
}
