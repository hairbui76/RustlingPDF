import { useEffect, useMemo, useState } from "react";
import {
  Alert,
  Badge,
  Checkbox,
  Divider,
  Group,
  Paper,
  Stack,
  Text,
  TextInput,
} from "@mantine/core";
import { Button } from "@app/ui/Button";
import type {
  AccessibilityFinding,
  AccessibilityRepairs,
  AccessibilityReport,
} from "@app/tools/accessibility/types";
import styles from "@app/tools/accessibility/Accessibility.module.css";

interface AccessibilityPanelProps {
  report: AccessibilityReport;
  isLoading: boolean;
  endpointEnabled: boolean | null;
  status: string;
  errorMessage: string | null;
  onApply: (repairs: AccessibilityRepairs) => Promise<void>;
}

function findingTarget(finding: AccessibilityFinding): string | null {
  if (finding.fieldName) return finding.fieldName;
  if (finding.objectNumber !== undefined && finding.generation !== undefined) {
    return `object ${finding.objectNumber} ${finding.generation}`;
  }
  if (finding.pageIndex !== undefined) {
    return `page ${finding.pageIndex + 1}`;
  }
  return null;
}

export function AccessibilityPanel({
  report,
  isLoading,
  endpointEnabled,
  status,
  errorMessage,
  onApply,
}: AccessibilityPanelProps) {
  const [language, setLanguage] = useState("");
  const [markAsTagged, setMarkAsTagged] = useState(false);
  const [tabOrderPages, setTabOrderPages] = useState<Set<number>>(new Set());
  const [alternativeTexts, setAlternativeTexts] = useState<
    Record<string, string>
  >({});
  const [formTooltips, setFormTooltips] = useState<Record<string, string>>({});

  const failedTabOrder = useMemo(
    () =>
      report.findings.filter(
        (finding) =>
          finding.ruleId === "reading-order.annotation-tabs" &&
          finding.status === "fail" &&
          finding.pageIndex !== undefined,
      ),
    [report.findings],
  );
  const missingAlternatives = useMemo(
    () =>
      report.findings.filter(
        (finding) =>
          finding.ruleId === "figure.alternative-text" &&
          finding.status === "fail" &&
          finding.remediation === "userInput" &&
          finding.objectNumber !== undefined &&
          finding.generation !== undefined,
      ),
    [report.findings],
  );
  const missingFormLabels = useMemo(
    () =>
      report.findings.filter(
        (finding) =>
          finding.ruleId === "form-field.accessible-name" &&
          finding.status === "fail" &&
          finding.remediation === "userInput" &&
          finding.fieldName,
      ),
    [report.findings],
  );

  useEffect(() => {
    setLanguage(report.document.language ?? "");
    setMarkAsTagged(
      report.document.hasStructureTree && !report.document.marked,
    );
    setTabOrderPages(
      new Set(
        report.findings
          .filter(
            (finding) =>
              finding.ruleId === "reading-order.annotation-tabs" &&
              finding.status === "fail" &&
              finding.pageIndex !== undefined,
          )
          .map((finding) => finding.pageIndex as number),
      ),
    );
    setAlternativeTexts({});
    setFormTooltips({});
  }, [report]);

  const repairs = useMemo<AccessibilityRepairs>(() => {
    const next: AccessibilityRepairs = {};
    const normalizedLanguage = language.trim();
    if (
      normalizedLanguage &&
      normalizedLanguage !== (report.document.language ?? "")
    ) {
      next.documentLanguage = normalizedLanguage;
    }
    if (markAsTagged && !report.document.marked) {
      next.markAsTagged = true;
    }
    if (tabOrderPages.size > 0) {
      next.structureTabOrderPages = [...tabOrderPages].sort(
        (left, right) => left - right,
      );
    }
    const alternatives = missingAlternatives.flatMap((finding) => {
      const key = `${finding.objectNumber}:${finding.generation}`;
      const text = alternativeTexts[key]?.trim();
      return text
        ? [
            {
              objectNumber: finding.objectNumber as number,
              generation: finding.generation as number,
              text,
            },
          ]
        : [];
    });
    if (alternatives.length > 0) next.alternativeTexts = alternatives;
    const tooltips = missingFormLabels.flatMap((finding) => {
      const fieldName = finding.fieldName as string;
      const text = formTooltips[fieldName]?.trim();
      return text ? [{ fieldName, text }] : [];
    });
    if (tooltips.length > 0) next.formFieldTooltips = tooltips;
    return next;
  }, [
    alternativeTexts,
    formTooltips,
    language,
    markAsTagged,
    missingAlternatives,
    missingFormLabels,
    report.document.language,
    report.document.marked,
    tabOrderPages,
  ]);
  const hasRepairs = Object.keys(repairs).length > 0;

  return (
    <Stack gap="sm">
      <Alert color="blue" title="Bounded accessibility report">
        This checks named PDF structure rules. It does not certify PDF/UA.
        Semantic reading order and description quality still need human review.
      </Alert>

      <Group gap="xs">
        <Badge color={report.summary.failed === 0 ? "green" : "red"}>
          {report.summary.failed} failed
        </Badge>
        <Badge color="green" variant="light">
          {report.summary.passed} passed
        </Badge>
        <Badge color="yellow" variant="light">
          {report.summary.manualReview} manual
        </Badge>
      </Group>

      <Paper withBorder p="sm">
        <Stack gap="xs">
          <Text fw={700}>Document repairs</Text>
          {!report.document.language ||
          report.findings.some(
            (finding) =>
              finding.ruleId === "document.language" &&
              finding.status === "fail",
          ) ? (
            <TextInput
              label="Default document language"
              description="Use a language tag such as en, en-US, vi, or fr-CA."
              placeholder="en-US"
              value={language}
              onChange={(event) => setLanguage(event.currentTarget.value)}
            />
          ) : (
            <Text size="sm">
              Language: <strong>{report.document.language}</strong>
            </Text>
          )}
          {report.document.hasStructureTree && !report.document.marked && (
            <Checkbox
              checked={markAsTagged}
              onChange={(event) => setMarkAsTagged(event.currentTarget.checked)}
              label="Mark the existing structure tree as tagged"
            />
          )}
          {failedTabOrder.map((finding) => {
            const pageIndex = finding.pageIndex as number;
            return (
              <Checkbox
                key={pageIndex}
                checked={tabOrderPages.has(pageIndex)}
                onChange={(event) => {
                  const checked = event.currentTarget.checked;
                  setTabOrderPages((current) => {
                    const next = new Set(current);
                    if (checked) next.add(pageIndex);
                    else next.delete(pageIndex);
                    return next;
                  });
                }}
                label={`Use structure tab order on page ${pageIndex + 1}`}
              />
            );
          })}
        </Stack>
      </Paper>

      {missingAlternatives.length > 0 && (
        <Paper withBorder p="sm">
          <Stack gap="sm">
            <div>
              <Text fw={700}>Figure descriptions</Text>
              <Text size="xs" c="dimmed">
                Describe the meaning or purpose; do not use filenames or
                placeholder text.
              </Text>
            </div>
            {missingAlternatives.map((finding) => {
              const key = `${finding.objectNumber}:${finding.generation}`;
              return (
                <TextInput
                  key={key}
                  label={`Figure on ${
                    finding.pageIndex === undefined
                      ? "unknown page"
                      : `page ${finding.pageIndex + 1}`
                  }`}
                  description={`PDF object ${finding.objectNumber} ${finding.generation}`}
                  value={alternativeTexts[key] ?? ""}
                  onChange={(event) => {
                    const value = event.currentTarget.value;
                    setAlternativeTexts((current) => ({
                      ...current,
                      [key]: value,
                    }));
                  }}
                />
              );
            })}
          </Stack>
        </Paper>
      )}

      {missingFormLabels.length > 0 && (
        <Paper withBorder p="sm">
          <Stack gap="sm">
            <div>
              <Text fw={700}>Accessible form labels</Text>
              <Text size="xs" c="dimmed">
                Enter the name a screen reader should announce.
              </Text>
            </div>
            {missingFormLabels.map((finding) => {
              const fieldName = finding.fieldName as string;
              return (
                <TextInput
                  key={fieldName}
                  label={fieldName}
                  value={formTooltips[fieldName] ?? ""}
                  onChange={(event) => {
                    const value = event.currentTarget.value;
                    setFormTooltips((current) => ({
                      ...current,
                      [fieldName]: value,
                    }));
                  }}
                />
              );
            })}
          </Stack>
        </Paper>
      )}

      <Paper withBorder p="sm">
        <Stack gap="xs">
          <Text fw={700}>Findings</Text>
          {report.findings.map((finding, index) => {
            const target = findingTarget(finding);
            return (
              <div
                className={styles.finding}
                key={`${finding.ruleId}-${target ?? "document"}-${index}`}
              >
                <Group justify="space-between" align="start" wrap="nowrap">
                  <div>
                    <Text size="sm" fw={600}>
                      {finding.title}
                    </Text>
                    {target && (
                      <Text size="xs" c="dimmed">
                        {target}
                      </Text>
                    )}
                  </div>
                  <Badge
                    size="xs"
                    color={
                      finding.status === "pass"
                        ? "green"
                        : finding.status === "fail"
                          ? "red"
                          : "yellow"
                    }
                  >
                    {finding.status}
                  </Badge>
                </Group>
                <Text size="xs">{finding.message}</Text>
              </div>
            );
          })}
        </Stack>
      </Paper>

      {report.document.structureOrder.length > 0 && (
        <details>
          <summary className={styles.summary}>
            Ordered structure preview ({report.document.structureOrder.length}
            {report.document.structurePreviewTruncated ? "+" : ""})
          </summary>
          <Stack gap={4} mt="xs">
            {report.document.structureOrder.map((entry, index) => (
              <Text
                size="xs"
                key={`${entry.objectNumber ?? "direct"}-${entry.generation ?? 0}-${index}`}
              >
                {index + 1}. {entry.role}
                {entry.pageIndex === undefined
                  ? ""
                  : ` · page ${entry.pageIndex + 1}`}
                {entry.alternativeText ? ` · ${entry.alternativeText}` : ""}
              </Text>
            ))}
          </Stack>
        </details>
      )}

      <Divider />
      {status && (
        <Text size="xs" c="dimmed">
          {status}
        </Text>
      )}
      {errorMessage && <Alert color="red">{errorMessage}</Alert>}
      {report.summary.failed === 0 && (
        <Alert color="green">
          All native machine-checkable rules pass. Complete the remaining manual
          reading-order review before treating the document as accessible.
        </Alert>
      )}
      <Button
        fullWidth
        disabled={!hasRepairs || isLoading || endpointEnabled === false}
        onClick={() => onApply(repairs)}
      >
        {isLoading
          ? "Applying and re-checking..."
          : "Apply repairs and re-check"}
      </Button>
    </Stack>
  );
}

export default AccessibilityPanel;
