import React, { useMemo, useState } from "react";
import {
  Alert,
  NumberInput,
  ScrollArea,
  Select,
  Switch,
  Text,
  Textarea,
  TextInput,
} from "@mantine/core";
import { Button } from "@app/ui/Button";
import { useFormDesigner } from "@app/tools/formFill/FormDesignerContext";
import { createFormFields } from "@app/tools/formFill/formApi";
import type { FormFieldType } from "@app/tools/formFill/types";
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

export function FormDesignerPanel({ file }: { file: File | Blob | null }) {
  const designer = useFormDesigner();
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const selected = designer.selectedField;
  const selectedWidget = selected?.widgets[0] ?? null;
  const showsOptions =
    selected &&
    ["checkbox", "radio", "combobox", "listbox"].includes(selected.type);

  const fieldError = useMemo(() => {
    const names = new Set<string>();
    for (const field of designer.fields) {
      if (!field.name.trim()) return "Every field needs a name.";
      if (names.has(field.name.trim())) {
        return `Field name "${field.name.trim()}" is duplicated.`;
      }
      names.add(field.name.trim());
      if (
        ["combobox", "listbox"].includes(field.type) &&
        field.options.filter((option) => option.trim()).length === 0
      ) {
        return `${field.label || field.name} needs at least one option.`;
      }
      if (
        field.type === "radio" &&
        field.widgets.some((widget) => !widget.exportValue?.trim())
      ) {
        return `${field.label || field.name} needs one radio value per widget.`;
      }
    }
    return null;
  }, [designer.fields]);

  const handleCreate = async () => {
    if (!file || designer.fields.length === 0 || fieldError) {
      setError(
        fieldError ||
          (file
            ? "Draw at least one field on a page."
            : "Open a PDF before creating fields."),
      );
      return;
    }
    setSaving(true);
    setError(null);
    try {
      const blob = await createFormFields(file, designer.creationRequests);
      designer.reset();
      applyPdf(blob);
    } catch (caught) {
      setError(
        caught instanceof Error
          ? caught.message
          : "Could not create form fields.",
      );
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className={styles.modeContent}>
      <div className={styles.designerHeader}>
        <Text size="xs" fw={700}>
          Draw fields
        </Text>
        <Text size="xs" c="dimmed">
          Choose a type, then drag on any page. Drag existing drafts to move;
          use the corner handle to resize.
        </Text>
        <Select
          label="Field type"
          size="xs"
          value={designer.creationType}
          data={FIELD_TYPES}
          onChange={(value) =>
            value && designer.setCreationType(value as FormFieldType)
          }
        />
        <div className={styles.designerActions}>
          <Button
            variant="secondary"
            size="sm"
            disabled={!selected}
            onClick={() => designer.setAppendWidget(true)}
          >
            {designer.appendWidget ? "Draw next widget…" : "Add widget"}
          </Button>
          <Button
            variant="secondary"
            size="sm"
            disabled={designer.selectedIds.length === 0}
            onClick={designer.duplicateSelected}
          >
            Duplicate
          </Button>
          <Button
            variant="secondary"
            size="sm"
            disabled={designer.selectedIds.length < 2}
            onClick={() => designer.alignSelected("left")}
          >
            Align left
          </Button>
          <Button
            variant="secondary"
            size="sm"
            disabled={designer.selectedIds.length < 2}
            onClick={() => designer.alignSelected("top")}
          >
            Align top
          </Button>
        </div>
      </div>

      <ScrollArea className={styles.designerScroll}>
        <div className={styles.fieldListInner}>
          {designer.fields.length === 0 && (
            <div className={styles.designerEmpty}>
              No draft fields yet. Drag a rectangle on the PDF.
            </div>
          )}
          {designer.fields.map((field) => (
            <Button
              variant="quiet"
              size="sm"
              key={field.id}
              className={`${styles.designerFieldRow} ${
                designer.selectedIds.includes(field.id)
                  ? styles.designerFieldRowSelected
                  : ""
              }`}
              onClick={(event) =>
                designer.selectField(
                  field.id,
                  event.ctrlKey || event.metaKey || event.shiftKey,
                )
              }
            >
              <span>{field.label || field.name}</span>
              <small>
                {field.type} · {field.widgets.length} widget
                {field.widgets.length === 1 ? "" : "s"}
              </small>
            </Button>
          ))}

          {selected && (
            <div className={styles.propertyPanel}>
              <Text size="xs" fw={700}>
                Selected field
              </Text>
              <TextInput
                label="Name"
                size="xs"
                value={selected.name}
                onChange={(event) =>
                  designer.updateField(selected.id, {
                    name: event.currentTarget.value,
                  })
                }
              />
              <TextInput
                label="Visible label"
                size="xs"
                value={selected.label}
                onChange={(event) =>
                  designer.updateField(selected.id, {
                    label: event.currentTarget.value,
                  })
                }
              />
              <Textarea
                label="Accessible tooltip"
                size="xs"
                autosize
                minRows={2}
                value={selected.tooltip}
                onChange={(event) =>
                  designer.updateField(selected.id, {
                    tooltip: event.currentTarget.value,
                  })
                }
              />
              {showsOptions && (
                <Textarea
                  label="Options (one per line)"
                  size="xs"
                  autosize
                  minRows={2}
                  value={selected.options.join("\n")}
                  onChange={(event) =>
                    designer.updateField(selected.id, {
                      options: event.currentTarget.value
                        .split(/\r?\n/)
                        .map((option) => option.trim())
                        .filter(Boolean),
                    })
                  }
                />
              )}
              {!["button", "signature"].includes(selected.type) && (
                <TextInput
                  label="Default value"
                  size="xs"
                  value={selected.defaultValue}
                  onChange={(event) =>
                    designer.updateField(selected.id, {
                      defaultValue: event.currentTarget.value,
                    })
                  }
                />
              )}
              <div className={styles.propertyGrid}>
                <NumberInput
                  label="Font"
                  size="xs"
                  min={1}
                  value={selected.fontSize}
                  onChange={(value) =>
                    designer.updateField(selected.id, {
                      fontSize: Number(value) || 12,
                    })
                  }
                />
                <NumberInput
                  label="Tab order"
                  size="xs"
                  min={0}
                  value={selected.tabOrder}
                  onChange={(value) =>
                    designer.updateField(selected.id, {
                      tabOrder: value === "" ? undefined : Number(value),
                    })
                  }
                />
              </div>
              <div className={styles.switchGrid}>
                <Switch
                  size="xs"
                  label="Required"
                  checked={selected.required}
                  onChange={(event) =>
                    designer.updateField(selected.id, {
                      required: event.currentTarget.checked,
                    })
                  }
                />
                <Switch
                  size="xs"
                  label="Read only"
                  checked={selected.readOnly}
                  onChange={(event) =>
                    designer.updateField(selected.id, {
                      readOnly: event.currentTarget.checked,
                    })
                  }
                />
                {selected.type === "text" && (
                  <Switch
                    size="xs"
                    label="Multiline"
                    checked={selected.multiline}
                    onChange={(event) =>
                      designer.updateField(selected.id, {
                        multiline: event.currentTarget.checked,
                      })
                    }
                  />
                )}
                {selected.type === "listbox" && (
                  <Switch
                    size="xs"
                    label="Multiple selection"
                    checked={selected.multiSelect}
                    onChange={(event) =>
                      designer.updateField(selected.id, {
                        multiSelect: event.currentTarget.checked,
                      })
                    }
                  />
                )}
              </div>
              {selectedWidget && (
                <>
                  <Text size="xs" fw={700} mt="xs">
                    First widget (PDF points)
                  </Text>
                  <div className={styles.propertyGrid}>
                    {(["x", "y", "width", "height"] as const).map((key) => (
                      <NumberInput
                        key={key}
                        label={key}
                        size="xs"
                        min={key === "width" || key === "height" ? 1 : 0}
                        decimalScale={1}
                        value={selectedWidget[key]}
                        onChange={(value) =>
                          designer.updateWidget(
                            selected.id,
                            selectedWidget.id,
                            { [key]: Number(value) || 0 },
                          )
                        }
                      />
                    ))}
                  </div>
                </>
              )}
              <Button
                variant="secondary"
                accent="danger"
                size="sm"
                onClick={designer.deleteSelected}
              >
                Delete selected
              </Button>
            </div>
          )}
        </div>
      </ScrollArea>

      <div className={styles.modeFooter}>
        {(error || fieldError) && (
          <Alert color="red" variant="light" p="xs">
            <Text size="xs">{error || fieldError}</Text>
          </Alert>
        )}
        <Button
          fullWidth
          size="sm"
          loading={saving}
          disabled={!file || designer.fields.length === 0 || !!fieldError}
          onClick={handleCreate}
        >
          Create {designer.fields.length || ""} field
          {designer.fields.length === 1 ? "" : "s"}
        </Button>
      </div>
    </div>
  );
}

export default FormDesignerPanel;
