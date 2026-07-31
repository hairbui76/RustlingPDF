import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Alert,
  Badge,
  Box,
  Card,
  Group,
  Stack,
  Text,
  TextInput,
  Tooltip,
} from "@mantine/core";

import { ActionIcon } from "@app/ui/ActionIcon";
import { LocalIcon } from "@app/components/shared/LocalIcon";
import type {
  SavedSignature,
  SavedSignatureType,
} from "@app/hooks/tools/sign/useSavedSignatures";

interface SavedSignaturesSectionProps {
  signatures: SavedSignature[];
  disabled?: boolean;
  isAtCapacity: boolean;
  maxLimit: number;
  onUseSignature: (signature: SavedSignature) => void;
  onDeleteSignature: (signature: SavedSignature) => void;
  onRenameSignature: (id: string, label: string) => void;
  translationScope?: string;
}

const typeBadgeColor: Record<SavedSignatureType, string> = {
  canvas: "indigo",
  image: "teal",
  text: "grape",
};

export const SavedSignaturesSection = ({
  signatures,
  disabled = false,
  isAtCapacity,
  maxLimit,
  onUseSignature,
  onDeleteSignature,
  onRenameSignature,
  translationScope = "sign",
}: SavedSignaturesSectionProps) => {
  const { t } = useTranslation();
  const translate = useCallback(
    (key: string, defaultValue: string, options?: Record<string, unknown>) =>
      t(`${translationScope}.${key}`, { defaultValue, ...options }),
    [t, translationScope],
  );
  const [activeIndex, setActiveIndex] = useState(0);
  const [labelDrafts, setLabelDrafts] = useState<Record<string, string>>({});
  const activeSignature = signatures[activeIndex];

  useEffect(() => {
    setActiveIndex((previous) =>
      Math.min(previous, Math.max(signatures.length - 1, 0)),
    );
    setLabelDrafts((previous) => {
      const next: Record<string, string> = {};
      signatures.forEach((signature) => {
        next[signature.id] = previous[signature.id] ?? signature.label ?? "";
      });
      return next;
    });
  }, [signatures]);

  const typeLabel = (type: SavedSignatureType) => {
    switch (type) {
      case "canvas":
        return translate("saved.type.canvas", "Drawing");
      case "image":
        return translate("saved.type.image", "Upload");
      case "text":
        return translate("saved.type.text", "Text");
    }
  };

  const renderPreview = (signature: SavedSignature) => {
    const commonStyle = {
      height: "120px",
      borderRadius: "0.5rem",
      backgroundColor: "#ffffff",
      padding: "0.5rem",
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
      overflow: "hidden",
    } as const;

    if (signature.type === "text") {
      return (
        <Box style={commonStyle}>
          <Text
            size="lg"
            style={{
              fontFamily: signature.fontFamily,
              fontSize: `${signature.fontSize}px`,
              color: signature.textColor,
              whiteSpace: "nowrap",
            }}
          >
            {signature.signerName}
          </Text>
        </Box>
      );
    }

    return (
      <Box style={commonStyle}>
        <Box
          component="img"
          src={signature.dataUrl}
          alt={signature.label}
          style={{ maxWidth: "100%", maxHeight: "100%", objectFit: "contain" }}
        />
      </Box>
    );
  };

  const commitLabel = (signature: SavedSignature) => {
    const label = labelDrafts[signature.id]?.trim() ?? "";
    if (!label || label === signature.label) {
      setLabelDrafts((previous) => ({
        ...previous,
        [signature.id]: signature.label,
      }));
      return;
    }
    onRenameSignature(signature.id, label);
  };

  return (
    <Stack gap="sm">
      <Stack gap={0}>
        <Text fw={600} size="md">
          {translate("saved.heading", "Saved signatures")}
        </Text>
        <Text size="sm" c="dimmed">
          {translate(
            "saved.description",
            "Saved signatures stay in this browser.",
          )}
        </Text>
      </Stack>

      {isAtCapacity && (
        <Alert
          color="yellow"
          title={translate("saved.limitTitle", "Limit reached")}
        >
          <Text size="sm">
            {translate(
              "saved.limitDescription",
              "Remove a saved signature before adding new ones (max {{max}}).",
              { max: maxLimit },
            )}
          </Text>
        </Alert>
      )}

      {signatures.length === 0 || !activeSignature ? (
        <Card withBorder>
          <Stack gap="xs">
            <Text fw={500}>
              {translate("saved.emptyTitle", "No saved signatures yet")}
            </Text>
            <Text size="sm" c="dimmed">
              {translate(
                "saved.emptyDescription",
                'Draw, upload, or type a signature, then use "Save to library" to keep up to {{max}} favourites.',
                { max: maxLimit },
              )}
            </Text>
          </Stack>
        </Card>
      ) : (
        <>
          <Alert
            color="blue"
            title={translate("saved.browserStorageTitle", "Browser storage")}
          >
            <Text size="xs">
              {translate(
                "saved.browserStorageDescription",
                "These signatures remain on this device and are removed if browser data is cleared.",
              )}
            </Text>
          </Alert>

          <Group justify="space-between" align="center">
            <Text size="sm" c="dimmed">
              {translate("saved.carouselPosition", "{{current}} of {{total}}", {
                current: activeIndex + 1,
                total: signatures.length,
              })}
            </Text>
            <Group gap={4}>
              <ActionIcon
                variant="secondary"
                aria-label={translate("saved.prev", "Previous")}
                onClick={() =>
                  setActiveIndex((previous) => Math.max(0, previous - 1))
                }
                disabled={disabled || activeIndex === 0}
              >
                <LocalIcon icon="chevron-left-rounded" width={18} height={18} />
              </ActionIcon>
              <ActionIcon
                variant="secondary"
                aria-label={translate("saved.next", "Next")}
                onClick={() =>
                  setActiveIndex((previous) =>
                    Math.min(signatures.length - 1, previous + 1),
                  )
                }
                disabled={disabled || activeIndex >= signatures.length - 1}
              >
                <LocalIcon
                  icon="chevron-right-rounded"
                  width={18}
                  height={18}
                />
              </ActionIcon>
            </Group>
          </Group>

          <Card withBorder padding="sm">
            <Stack gap="sm">
              <Group justify="space-between" align="center">
                <Badge
                  color={typeBadgeColor[activeSignature.type]}
                  variant="light"
                >
                  {typeLabel(activeSignature.type)}
                </Badge>
                <Group gap="xs">
                  <Tooltip label={translate("saved.use", "Use signature")}>
                    <ActionIcon
                      variant="secondary"
                      aria-label={translate("saved.use", "Use signature")}
                      onClick={() => onUseSignature(activeSignature)}
                      disabled={disabled}
                    >
                      <LocalIcon
                        icon="check-circle-outline-rounded"
                        width={18}
                        height={18}
                      />
                    </ActionIcon>
                  </Tooltip>
                  <Tooltip label={translate("saved.delete", "Delete")}>
                    <ActionIcon
                      variant="tertiary"
                      accent="danger"
                      aria-label={translate("saved.delete", "Delete")}
                      onClick={() => onDeleteSignature(activeSignature)}
                      disabled={disabled}
                    >
                      <LocalIcon
                        icon="delete-outline-rounded"
                        width={18}
                        height={18}
                      />
                    </ActionIcon>
                  </Tooltip>
                </Group>
              </Group>

              {renderPreview(activeSignature)}
              <TextInput
                label={translate("saved.label", "Label")}
                value={labelDrafts[activeSignature.id] ?? activeSignature.label}
                onChange={(event) =>
                  setLabelDrafts((previous) => ({
                    ...previous,
                    [activeSignature.id]: event.currentTarget.value,
                  }))
                }
                onBlur={() => commitLabel(activeSignature)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") event.currentTarget.blur();
                  if (event.key === "Escape") {
                    setLabelDrafts((previous) => ({
                      ...previous,
                      [activeSignature.id]: activeSignature.label,
                    }));
                    event.currentTarget.blur();
                  }
                }}
                disabled={disabled}
              />
            </Stack>
          </Card>
        </>
      )}
    </Stack>
  );
};
