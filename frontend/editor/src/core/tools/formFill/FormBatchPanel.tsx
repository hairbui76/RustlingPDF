import React, { useState } from "react";
import { Alert, Text } from "@mantine/core";
import { Button } from "@app/ui/Button";
import { batchFillFormFields } from "@app/tools/formFill/formApi";
import { downloadBlob } from "@app/utils/downloadUtils";
import styles from "@app/tools/formFill/FormFill.module.css";

export function FormBatchPanel({ file }: { file: File | Blob | null }) {
  const [dataFile, setDataFile] = useState<File | null>(null);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleBatchFill = async () => {
    if (!file || !dataFile) return;
    setRunning(true);
    setError(null);
    try {
      const result = await batchFillFormFields(file, dataFile);
      const sourceName = file instanceof File ? file.name : "document.pdf";
      const base = sourceName.replace(/\.[^.]+$/, "") || "document";
      downloadBlob(result, `${base}_batch_filled.zip`);
    } catch (caught) {
      setError(
        caught instanceof Error ? caught.message : "Batch filling failed.",
      );
    } finally {
      setRunning(false);
    }
  };

  return (
    <div className={styles.simpleModeContent}>
      <div className={styles.simpleModeCard}>
        <Text fw={700} size="sm">
          Fill from CSV or XLSX
        </Text>
        <Text size="xs" c="dimmed">
          The first row contains field names. Each following nonblank row
          becomes one PDF. Add an optional <code>_filename</code> column to name
          outputs.
        </Text>
        <label className={styles.filePicker}>
          <span>{dataFile ? dataFile.name : "Choose CSV or XLSX"}</span>
          <input
            type="file"
            accept=".csv,.xlsx,text/csv,application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
            onChange={(event) =>
              setDataFile(event.currentTarget.files?.[0] ?? null)
            }
          />
        </label>
        {error && (
          <Alert color="red" variant="light" p="xs">
            <Text size="xs">{error}</Text>
          </Alert>
        )}
        <Button
          fullWidth
          loading={running}
          disabled={!file || !dataFile}
          onClick={handleBatchFill}
        >
          Create batch ZIP
        </Button>
      </div>
    </div>
  );
}

export default FormBatchPanel;
