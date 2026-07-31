import { Alert, Badge, Code, Group, Paper, Stack, Text } from "@mantine/core";
import type {
  DocumentUnderstandingResponse,
  ExtractionResult,
  SummaryResult,
  TranslationResult,
} from "@app/tools/documentUnderstanding/types";
import styles from "@app/tools/documentUnderstanding/DocumentUnderstanding.module.css";

interface DocumentUnderstandingResultsProps {
  response: DocumentUnderstandingResponse;
}

function PageReferences({ pages }: { pages: number[] }) {
  return (
    <Group gap={4}>
      {pages.map((page) => (
        <Badge key={page} size="xs" variant="light">
          Page {page}
        </Badge>
      ))}
    </Group>
  );
}

function renderValue(value: unknown): string {
  if (value === null || value === undefined) return "Not found";
  if (typeof value === "string") return value;
  return JSON.stringify(value);
}

export function DocumentUnderstandingResults({
  response,
}: DocumentUnderstandingResultsProps) {
  return (
    <Stack gap="sm">
      <Alert color="orange" title="Provider disclosure">
        {response.providerDisclosure}
      </Alert>
      <Group gap="xs">
        <Badge variant="light">{response.source.pagesProcessed} pages</Badge>
        <Badge variant="light">
          {response.source.charactersProcessed.toLocaleString()} characters
        </Badge>
        <Text size="xs" c="dimmed">
          Limits: {response.source.maxPages} pages /{" "}
          {response.source.maxCharacters.toLocaleString()} characters
        </Text>
      </Group>

      {response.operation === "summary" && (
        <SummaryView result={response.result as SummaryResult} />
      )}
      {response.operation === "extraction" && (
        <ExtractionView result={response.result as ExtractionResult} />
      )}
      {response.operation === "translation" && (
        <TranslationView result={response.result as TranslationResult} />
      )}
    </Stack>
  );
}

function SummaryView({ result }: { result: SummaryResult }) {
  return (
    <Stack gap="sm">
      <Paper withBorder p="sm">
        <Text className={styles.preserveWhitespace}>{result.summary}</Text>
      </Paper>
      {result.keyPoints.length > 0 && (
        <Stack gap="xs">
          <Text fw={700}>Key points</Text>
          {result.keyPoints.map((point, index) => (
            <Paper withBorder p="sm" key={`${index}-${point.text}`}>
              <Stack gap={6}>
                <Text size="sm">{point.text}</Text>
                <PageReferences pages={point.pages} />
              </Stack>
            </Paper>
          ))}
        </Stack>
      )}
    </Stack>
  );
}

function ExtractionView({ result }: { result: ExtractionResult }) {
  return (
    <Stack gap="xs">
      {result.values.map((item) => (
        <Paper withBorder p="sm" key={item.key}>
          <Stack gap={6}>
            <Group justify="space-between">
              <Code>{item.key}</Code>
              <Badge
                size="xs"
                color={
                  item.confidence === "high"
                    ? "green"
                    : item.confidence === "medium"
                      ? "yellow"
                      : "gray"
                }
              >
                {item.confidence} confidence
              </Badge>
            </Group>
            <Text
              size="sm"
              c={item.value === null ? "dimmed" : undefined}
              className={styles.preserveWhitespace}
            >
              {renderValue(item.value)}
            </Text>
            <PageReferences pages={item.pages} />
            {item.note && (
              <Text size="xs" c="dimmed">
                {item.note}
              </Text>
            )}
          </Stack>
        </Paper>
      ))}
    </Stack>
  );
}

function TranslationView({ result }: { result: TranslationResult }) {
  return (
    <Stack gap="sm">
      <Text size="sm">
        {result.sourceLanguage
          ? `${result.sourceLanguage} → ${result.targetLanguage}`
          : `Auto-detect → ${result.targetLanguage}`}
      </Text>
      {result.pages.map((page) => (
        <Paper withBorder p="sm" key={page.pageNumber}>
          <Stack gap="sm">
            <Text fw={700}>Page {page.pageNumber}</Text>
            {page.blocks.map((block) => (
              <div className={styles.translationBlock} key={block.blockId}>
                <Text size="xs" c="dimmed">
                  Source
                </Text>
                <Text size="sm" className={styles.preserveWhitespace}>
                  {block.sourceText}
                </Text>
                <Text size="xs" c="dimmed" mt={4}>
                  Translation
                </Text>
                {block.translatedText ? (
                  <Text size="sm" className={styles.preserveWhitespace}>
                    {block.translatedText}
                  </Text>
                ) : (
                  <Badge size="xs" color="yellow" variant="light">
                    Not translated
                  </Badge>
                )}
              </div>
            ))}
          </Stack>
        </Paper>
      ))}
    </Stack>
  );
}

export default DocumentUnderstandingResults;
