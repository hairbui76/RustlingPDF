import {
  Alert,
  Checkbox,
  Group,
  Paper,
  Select,
  Stack,
  Text,
  TextInput,
  Textarea,
} from "@mantine/core";
import { Button } from "@app/ui/Button";
import { SegmentedControl } from "@app/ui/SegmentedControl";
import type { DocumentUnderstandingParametersHook } from "@app/hooks/tools/documentUnderstanding/useDocumentUnderstandingParameters";
import type {
  DocumentUnderstandingMode,
  ExtractionField,
  ExtractionValueType,
} from "@app/tools/documentUnderstanding/types";

interface DocumentUnderstandingSettingsProps {
  parameters: DocumentUnderstandingParametersHook;
  aiEnabled: boolean;
}

const MODES: Array<{ value: DocumentUnderstandingMode; label: string }> = [
  { value: "summary", label: "Summary" },
  { value: "extraction", label: "Extract" },
  { value: "translation", label: "Translate" },
];

const VALUE_TYPES: Array<{ value: ExtractionValueType; label: string }> = [
  { value: "string", label: "Text" },
  { value: "number", label: "Number" },
  { value: "integer", label: "Integer" },
  { value: "boolean", label: "True / false" },
  { value: "date", label: "Date" },
  { value: "list", label: "List" },
];

export function DocumentUnderstandingSettings({
  parameters,
  aiEnabled,
}: DocumentUnderstandingSettingsProps) {
  const value = parameters.parameters;
  const updateFields = (fields: ExtractionField[]) =>
    parameters.updateParameter("extractionFields", fields);
  const updateField = (index: number, update: Partial<ExtractionField>) => {
    updateFields(
      value.extractionFields.map((field, fieldIndex) =>
        fieldIndex === index ? { ...field, ...update } : field,
      ),
    );
  };

  return (
    <Stack gap="sm">
      <Alert
        color={aiEnabled ? "orange" : "red"}
        title={aiEnabled ? "AI provider disclosure" : "AI engine is disabled"}
      >
        {aiEnabled
          ? "PDF bytes stay in RustlingPDF, but bounded extracted page text is sent to the AI provider configured by this server. No document or result is stored."
          : "An operator must enable the optional AI engine and configure Anthropic, OpenAI-compatible, or local Ollama before this tool can run."}
      </Alert>

      <SegmentedControl
        value={value.mode}
        onChange={(mode) => parameters.updateParameter("mode", mode)}
        options={MODES}
        fullWidth
        accent="ai"
        ariaLabel="Document understanding mode"
      />

      {value.mode === "summary" && (
        <Stack gap="sm">
          <Select
            label="Summary detail"
            data={[
              { value: "brief", label: "Brief" },
              { value: "standard", label: "Standard" },
              { value: "detailed", label: "Detailed" },
            ]}
            value={value.summaryDetail}
            onChange={(detail) => {
              if (
                detail === "brief" ||
                detail === "standard" ||
                detail === "detailed"
              ) {
                parameters.updateParameter("summaryDetail", detail);
              }
            }}
          />
          <Textarea
            label="Optional focus"
            description="For example: emphasize decisions, risks, or financial totals."
            minRows={3}
            maxLength={4_000}
            value={value.instructions}
            onChange={(event) =>
              parameters.updateParameter(
                "instructions",
                event.currentTarget.value,
              )
            }
          />
        </Stack>
      )}

      {value.mode === "extraction" && (
        <Stack gap="sm">
          <Text size="sm" c="dimmed">
            Define the fields to extract. Every returned value includes
            validated source pages, or an explicit null when no grounded value
            is found.
          </Text>
          {value.extractionFields.map((field, index) => (
            <Paper withBorder p="sm" key={`${index}-${field.key}`}>
              <Stack gap="xs">
                <Group grow align="start">
                  <TextInput
                    label={`Field ${index + 1} key`}
                    description="Letters, digits, _, -, or ."
                    value={field.key}
                    onChange={(event) =>
                      updateField(index, { key: event.currentTarget.value })
                    }
                  />
                  <Select
                    label="Value type"
                    data={VALUE_TYPES}
                    value={field.valueType}
                    onChange={(next) => {
                      if (next) {
                        updateField(index, {
                          valueType: next as ExtractionValueType,
                        });
                      }
                    }}
                  />
                </Group>
                <Textarea
                  label="What should be extracted?"
                  minRows={2}
                  maxLength={500}
                  value={field.description}
                  onChange={(event) =>
                    updateField(index, {
                      description: event.currentTarget.value,
                    })
                  }
                />
                <Group justify="space-between">
                  <Checkbox
                    label="Required"
                    checked={field.required}
                    onChange={(event) =>
                      updateField(index, {
                        required: event.currentTarget.checked,
                      })
                    }
                  />
                  <Button
                    variant="quiet"
                    accent="danger"
                    disabled={value.extractionFields.length === 1}
                    onClick={() =>
                      updateFields(
                        value.extractionFields.filter(
                          (_, fieldIndex) => fieldIndex !== index,
                        ),
                      )
                    }
                  >
                    Remove
                  </Button>
                </Group>
              </Stack>
            </Paper>
          ))}
          <Button
            variant="secondary"
            disabled={value.extractionFields.length >= 50}
            onClick={() =>
              updateFields([
                ...value.extractionFields,
                {
                  key: `field_${value.extractionFields.length + 1}`,
                  description: "",
                  valueType: "string",
                  required: false,
                },
              ])
            }
          >
            Add field
          </Button>
        </Stack>
      )}

      {value.mode === "translation" && (
        <Stack gap="sm">
          <Alert color="blue" title="Page and block order, not visual layout">
            The result preserves extracted page boundaries and block order. It
            does not rewrite the PDF or promise identical fonts, spacing, or
            line breaks.
          </Alert>
          <TextInput
            required
            label="Target language"
            placeholder="Vietnamese or vi"
            maxLength={100}
            value={value.targetLanguage}
            onChange={(event) =>
              parameters.updateParameter(
                "targetLanguage",
                event.currentTarget.value,
              )
            }
          />
          <TextInput
            label="Source language (optional)"
            placeholder="Auto-detect"
            maxLength={100}
            value={value.sourceLanguage}
            onChange={(event) =>
              parameters.updateParameter(
                "sourceLanguage",
                event.currentTarget.value,
              )
            }
          />
        </Stack>
      )}
    </Stack>
  );
}

export default DocumentUnderstandingSettings;
