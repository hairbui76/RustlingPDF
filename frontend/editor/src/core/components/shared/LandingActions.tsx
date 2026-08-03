import React from "react";
import { Group, Tooltip } from "@mantine/core";
import { Button } from "@app/ui/Button";
import { ActionIcon } from "@app/ui/ActionIcon";
import LocalIcon from "@app/components/shared/LocalIcon";
import { useFilesModalContext } from "@app/contexts/FilesModalContext";
import { useFileActionTerminology } from "@app/hooks/useFileActionTerminology";
import { useFileActionIcons } from "@app/hooks/useFileActionIcons";
import { useMobileUploadAvailability } from "@app/hooks/useMobileUploadAvailability";

type LandingActionsProps = {
  fileInputRef: React.RefObject<HTMLInputElement | null>;
  onUploadClick: () => void;
  onMobileUploadClick: () => void;
  onFileSelect: (event: React.ChangeEvent<HTMLInputElement>) => void;
};

export function LandingActions({
  fileInputRef,
  onUploadClick,
  onMobileUploadClick,
  onFileSelect,
}: LandingActionsProps) {
  const terminology = useFileActionTerminology();
  const { openFilesModal } = useFilesModalContext();
  const icons = useFileActionIcons();
  const showMobileUpload = useMobileUploadAvailability();

  return (
    <>
      {/* One visual group, uniform 36px control height, a single accented
          action. "Open from computer" is the primary — it is the shortest path
          from an empty workbench to a document. The in-app file browser and the
          phone-scan shortcut are alternates, so they stay quiet and neutral
          rather than competing for the same attention. */}
      <Group gap={8} justify="center" wrap="wrap" mb="xs">
        <Button
          size="md"
          className="landing-btn-primary"
          leftSection={
            <LocalIcon icon={icons.uploadIconName} width="1rem" height="1rem" />
          }
          onClick={(e) => {
            e.stopPropagation();
            onUploadClick();
          }}
        >
          {terminology.uploadFromComputer}
        </Button>

        <Button
          size="md"
          variant="secondary"
          className="landing-btn-secondary"
          leftSection={<LocalIcon icon="add" width="1rem" height="1rem" />}
          onClick={(e) => {
            e.stopPropagation();
            openFilesModal();
          }}
        >
          {terminology.addFiles}
        </Button>

        {showMobileUpload && (
          <Tooltip label={terminology.mobileUpload} position="bottom">
            <ActionIcon
              size="md"
              variant="secondary"
              aria-label={terminology.mobileUpload}
              className="landing-btn-secondary landing-btn-icon"
              onClick={(e) => {
                e.stopPropagation();
                onMobileUploadClick();
              }}
            >
              <LocalIcon icon="qr-code-rounded" width="1rem" height="1rem" />
            </ActionIcon>
          </Tooltip>
        )}
      </Group>
      <input
        ref={fileInputRef}
        type="file"
        multiple
        onChange={onFileSelect}
        style={{ display: "none" }}
      />
    </>
  );
}
