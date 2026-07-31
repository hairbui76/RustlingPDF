import React from "react";
import { Group, Paper, Stack, Text } from "@mantine/core";
import { Button } from "@app/ui/Button";
import { useTranslation } from "react-i18next";
import LocalIcon from "@app/components/shared/LocalIcon";
import { requestStartTour } from "@app/constants/events";

interface HelpSectionProps {
  onRequestClose: () => void;
}

const HelpSection: React.FC<HelpSectionProps> = ({ onRequestClose }) => {
  const { t } = useTranslation();

  const startTour = () => {
    onRequestClose();
    setTimeout(() => requestStartTour("tools"), 300);
  };

  return (
    <Stack gap="lg">
      <Paper withBorder p="md" radius="md">
        <Stack gap="md">
          <Group justify="space-between" align="center">
            <div>
              <Text fw={600} size="sm">
                {t("settings.help.toolsTour.title", "Tools Tour")}
              </Text>
              <Text size="xs" c="dimmed" mt={4}>
                {t(
                  "settings.help.toolsTour.description",
                  "Walk through uploading files, picking a tool, and reviewing results.",
                )}
              </Text>
            </div>
            <Button
              variant="secondary"
              size="sm"
              leftSection={
                <LocalIcon
                  icon="build-outline-rounded"
                  width="1rem"
                  height="1rem"
                />
              }
              onClick={startTour}
            >
              {t("settings.help.toolsTour.start", "Start")}
            </Button>
          </Group>
        </Stack>
      </Paper>
    </Stack>
  );
};

export default HelpSection;
