import React, { useEffect, useMemo, useState } from "react";
import {
  Alert,
  ScrollArea,
  Select,
  Switch,
  Text,
  Textarea,
  TextInput,
} from "@mantine/core";
import { Button } from "@app/ui/Button";
import { useFormFill } from "@app/tools/formFill/FormFillContext";
import {
  deleteFormFields,
  modifyFormFields,
} from "@app/tools/formFill/formApi";
import type {
  FormField,
  FormFieldModificationRequest,
  FormFieldType,
} from "@app/tools/formFill/types";
import styles from "@app/tools/formFill/FormFill.module.css";

const FIELD_TYPES: Array<{ value: FormFieldType; label: string }> = [
  { value: "text", label: "Text" },
  { value: "checkbox", label: "Checkbox" },
  { value: "radio", label: "Radio group" },
  { value: "combobox", label: "Dropdown" },
  { value: "listbox", label: "List box" },
  { value: "button", label: "Button" },
  { value: "signature", label: "Signature" },
];

function applyPdf(blob: Blob): void {
  window.dispatchEvent(new CustomEvent("formfill:apply", { detail: { blob } }));
}

function updateFromField(field: FormField): FormFieldModificationRequest {
  return {
    targetName: field.name,
    name: field.name,
    label: field.label,
    type: field.type,
    required: field.required,
    multiSelect: field.multiSelect,
    options: field.options ?? [],
    defaultValue: field.value ?? "",
    tooltip: field.tooltip ?? "",
  };
}

export function FormModifyPanel({ file }: { file: File | Blob | null }) {
  const { state, setActiveField } = useFormFill();
  const selectedField = useMemo(
    () =>
      state.fields.find((field) => field.name === state.activeFieldName) ??
      state.fields[0] ??
      null,
    [state.fields, state.activeFieldName],
  );
  const [draft, setDraft] = useState<FormFieldModificationRequest | null>(
    selectedField ? updateFromField(selectedField) : null,
  );
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setDraft(selectedField ? updateFromField(selectedField) : null);
  }, [selectedField]);

  const run = async (operation: "save" | "delete") => {
    if (!file || !selectedField || !draft) return;
    setSaving(true);
    setError(null);
    try {
      const blob =
        operation === "delete"
          ? await deleteFormFields(file, [selectedField.name])
          : await modifyFormFields(file, [draft]);
      applyPdf(blob);
    } catch (caught) {
      setError(
        caught instanceof Error
          ? caught.message
          : "Could not update the form field.",
      );
    } finally {
      setSaving(false);
    }
  };

  if (state.loading) {
    return (
      <div className={styles.simpleModeContent}>
        <Text size="xs">Analysing form fields…</Text>
      </div>
    );
  }

  if (!selectedField || !draft) {
    return (
      <div className={styles.simpleModeContent}>
        <div className={styles.designerEmpty}>
          No existing form fields were found. Use Create to add one.
        </div>
      </div>
    );
  }

  const showsOptions = ["checkbox", "radio", "combobox", "listbox"].includes(
    draft.type ?? "text",
  );

  return (
    <div className={styles.modeContent}>
      <ScrollArea className={styles.designerScroll}>
        <div className={styles.fieldListInner}>
          <Text size="xs" fw={700}>
            Select a field
          </Text>
          {state.fields.map((field) => (
            <Button
              variant="quiet"
              size="sm"
              key={field.name}
              className={`${styles.designerFieldRow} ${
                field.name === selectedField.name
                  ? styles.designerFieldRowSelected
                  : ""
              }`}
              onClick={() => setActiveField(field.name)}
            >
              <span>{field.label || field.name}</span>
              <small>{field.type}</small>
            </Button>
          ))}
          <div className={styles.propertyPanel}>
            <TextInput
              label="Name"
              size="xs"
              value={draft.name ?? ""}
              onChange={(event) =>
                setDraft({ ...draft, name: event.currentTarget.value })
              }
            />
            <TextInput
              label="Visible label"
              size="xs"
              value={draft.label ?? ""}
              onChange={(event) =>
                setDraft({ ...draft, label: event.currentTarget.value })
              }
            />
            <Textarea
              label="Accessible tooltip"
              size="xs"
              autosize
              value={draft.tooltip ?? ""}
              onChange={(event) =>
                setDraft({ ...draft, tooltip: event.currentTarget.value })
              }
            />
            <Select
              label="Type"
              size="xs"
              value={draft.type}
              data={FIELD_TYPES}
              onChange={(value) =>
                value && setDraft({ ...draft, type: value as FormFieldType })
              }
            />
            {showsOptions && (
              <Textarea
                label="Options (one per line)"
                size="xs"
                autosize
                minRows={2}
                value={(draft.options ?? []).join("\n")}
                onChange={(event) =>
                  setDraft({
                    ...draft,
                    options: event.currentTarget.value
                      .split(/\r?\n/)
                      .map((option) => option.trim())
                      .filter(Boolean),
                  })
                }
              />
            )}
            {!["button", "signature"].includes(draft.type ?? "text") && (
              <TextInput
                label="Default value"
                size="xs"
                value={draft.defaultValue ?? ""}
                onChange={(event) =>
                  setDraft({
                    ...draft,
                    defaultValue: event.currentTarget.value,
                  })
                }
              />
            )}
            <Switch
              size="xs"
              label="Required"
              checked={draft.required ?? false}
              onChange={(event) =>
                setDraft({
                  ...draft,
                  required: event.currentTarget.checked,
                })
              }
            />
            {(draft.type === "listbox" || draft.type === "combobox") && (
              <Switch
                size="xs"
                label="Multiple selection"
                checked={draft.multiSelect ?? false}
                disabled={draft.type !== "listbox"}
                onChange={(event) =>
                  setDraft({
                    ...draft,
                    multiSelect: event.currentTarget.checked,
                  })
                }
              />
            )}
          </div>
        </div>
      </ScrollArea>
      <div className={styles.modeFooter}>
        {error && (
          <Alert color="red" variant="light" p="xs">
            <Text size="xs">{error}</Text>
          </Alert>
        )}
        <div className={styles.designerActions}>
          <Button
            fullWidth
            loading={saving}
            disabled={!file || !draft.name?.trim()}
            onClick={() => run("save")}
          >
            Save field changes
          </Button>
          <Button
            variant="secondary"
            accent="danger"
            loading={saving}
            disabled={!file}
            onClick={() => run("delete")}
          >
            Delete
          </Button>
        </div>
      </div>
    </div>
  );
}

export default FormModifyPanel;
