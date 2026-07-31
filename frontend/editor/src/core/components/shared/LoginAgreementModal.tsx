import { useEffect, useState } from "react";
import {
  Box,
  Divider,
  Group,
  Modal,
  ScrollArea,
  Stack,
  Text,
} from "@mantine/core";
import { Button } from "@app/ui/Button";
import { useTranslation } from "react-i18next";
import Markdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import apiClient from "@app/services/apiClient";
import { Z_INDEX_AGREEMENT_MODAL } from "@app/styles/zIndex";

const ACCEPTED_STORAGE_KEY = "disclaimerAccepted";

interface DisclaimerResponse {
  enabled: boolean;
  showInAnonymousMode: boolean;
  content: string;
  format: string;
}

const markdownComponents: Components = {
  // Strip react-markdown's `node` prop so it isn't spread onto the DOM element.
  a({ node, ...props }) {
    return <a {...props} target="_blank" rel="noopener noreferrer" />;
  },
};

/**
 * Blocking disclaimer shown once per browser tab. Text is fetched live for
 * the current language.
 */
export default function LoginAgreementModal() {
  const { t, i18n } = useTranslation();

  const [opened, setOpened] = useState(false);
  const [content, setContent] = useState("");
  useEffect(() => {
    let cancelled = false;

    (async () => {
      try {
        const resp = await apiClient.get<DisclaimerResponse>(
          "/api/v1/config/login-disclaimer",
          {
            params: { lang: i18n.language },
            suppressErrorToast: true,
          },
        );
        const data = resp.data;
        if (cancelled || !data?.enabled) return;
        if (!data.showInAnonymousMode) return;
        if (!data.content || !data.content.trim()) return;

        const nonce = "session";
        let accepted: string | null = null;
        try {
          accepted = sessionStorage.getItem(ACCEPTED_STORAGE_KEY);
        } catch {
          accepted = null;
        }
        if (accepted === nonce) return;

        setContent(data.content);
        setOpened(true);
      } catch {
        // Fail open when the disclaimer cannot be loaded.
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [i18n.language]);

  const handleAccept = () => {
    try {
      sessionStorage.setItem(ACCEPTED_STORAGE_KEY, "session");
    } catch {
      /* ignore storage errors */
    }
    setOpened(false);
  };

  const handleDecline = () => {
    window.close();
    window.location.reload();
  };

  if (!opened) return null;

  return (
    <Modal
      opened={opened}
      onClose={() => {}}
      title={t("loginAgreementTitle", "Login Agreement")}
      centered
      size="lg"
      radius="md"
      closeOnClickOutside={false}
      closeOnEscape={false}
      withCloseButton={false}
      zIndex={Z_INDEX_AGREEMENT_MODAL}
    >
      <Stack>
        <ScrollArea.Autosize mah="50vh" type="auto">
          <Box px="xs">
            <Markdown
              remarkPlugins={[remarkGfm]}
              components={markdownComponents}
            >
              {content}
            </Markdown>
          </Box>
        </ScrollArea.Autosize>
        <Divider />
        <Group justify="space-between" gap="sm" align="center" wrap="wrap">
          <Text size="xs" c="dimmed" style={{ flex: 1, minWidth: 0 }}>
            {t(
              "loginAgreementProvider",
              "This notice is provided by your administrator, not RustlingPDF.",
            )}
          </Text>
          <Group gap="sm" wrap="nowrap">
            <Button variant="secondary" onClick={handleDecline}>
              {t("loginAgreementDecline", "Decline")}
            </Button>
            <Button variant="primary" onClick={handleAccept}>
              {t("loginAgreementAccept", "Accept")}
            </Button>
          </Group>
        </Group>
      </Stack>
    </Modal>
  );
}
