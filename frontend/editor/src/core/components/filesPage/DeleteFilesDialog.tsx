import { useEffect, useState } from "react";
import { LocalIcon } from "@app/components/shared/LocalIcon";
import { useTranslation } from "react-i18next";
import { Alert, Group, Modal, Stack, Text } from "@mantine/core";

import { Button } from "@app/ui/Button";
import type { RustlingFileStub } from "@app/types/fileContext";

interface DeleteFilesDialogProps {
  opened: boolean;
  files: RustlingFileStub[];
  onClose: () => void;
  onConfirm: () => Promise<void>;
}

export function DeleteFilesDialog({
  opened,
  files,
  onClose,
  onConfirm,
}: DeleteFilesDialogProps) {
  const { t } = useTranslation();
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!opened) return;
    setSubmitting(false);
    setError(null);
  }, [opened]);

  const runConfirm = async () => {
    setSubmitting(true);
    setError(null);
    try {
      await onConfirm();
      onClose();
    } catch (err) {
      setError(
        err instanceof Error
          ? err.message
          : t("filesPage.deleteFilesError", "Could not delete. Try again."),
      );
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Modal
      opened={opened}
      onClose={onClose}
      title={t("filesPage.deleteFilesTitle", "Delete {{count}} file(s)?", {
        count: files.length,
      })}
      centered
      size="md"
    >
      <Stack gap="md">
        <Text size="sm">
          {t(
            "filesPage.deleteFilesLocalBody",
            "Delete {{count}} file(s) from this device? This cannot be undone.",
            { count: files.length },
          )}
        </Text>

        {error && (
          <Alert
            color="red"
            icon={
              <LocalIcon
                icon="error-outline-rounded"
                width="1.25rem"
                height="1.25rem"
              />
            }
            variant="light"
            role="alert"
          >
            {error}
          </Alert>
        )}

        <Group justify="flex-end">
          <Button variant="secondary" onClick={onClose} disabled={submitting}>
            {t("filesPage.cancel", "Cancel")}
          </Button>
          <Button accent="danger" loading={submitting} onClick={runConfirm}>
            {t("filesPage.delete", "Delete")}
          </Button>
        </Group>
      </Stack>
    </Modal>
  );
}
