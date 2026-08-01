import React from "react";
import { Anchor, Group, Paper, Stack, Text } from "@mantine/core";
import { useTranslation } from "react-i18next";
import LocalIcon from "@app/components/shared/LocalIcon";
import { useAppConfig } from "@app/contexts/AppConfigContext";
import { useFooterInfo } from "@app/hooks/useFooterInfo";

interface LegalLink {
  key: string;
  label: string;
  href: string;
}

const LegalSection: React.FC = () => {
  const { t } = useTranslation();
  const { config } = useAppConfig();
  const { footerInfo } = useFooterInfo();

  const privacyPolicy = config?.privacyPolicy ?? footerInfo?.privacyPolicy;
  const termsAndConditions =
    config?.termsAndConditions ?? footerInfo?.termsAndConditions;
  const accessibilityStatement =
    config?.accessibilityStatement ?? footerInfo?.accessibilityStatement;
  const cookiePolicy = config?.cookiePolicy ?? footerInfo?.cookiePolicy;
  const impressum = config?.impressum ?? footerInfo?.impressum;

  const isValidLink = (link?: string) => link && link.trim().length > 0;

  const legalLinks: LegalLink[] = [
    ...(isValidLink(privacyPolicy)
      ? [
          {
            key: "privacy",
            label: t("legal.privacy", "Privacy Policy"),
            href: privacyPolicy!,
          },
        ]
      : []),
    ...(isValidLink(termsAndConditions)
      ? [
          {
            key: "terms",
            label: t("legal.terms", "Terms and Conditions"),
            href: termsAndConditions!,
          },
        ]
      : []),
    ...(isValidLink(accessibilityStatement)
      ? [
          {
            key: "accessibility",
            label: t("legal.accessibility", "Accessibility"),
            href: accessibilityStatement!,
          },
        ]
      : []),
    ...(isValidLink(cookiePolicy)
      ? [
          {
            key: "cookie",
            label: t("legal.cookie", "Cookie Policy"),
            href: cookiePolicy!,
          },
        ]
      : []),
    ...(isValidLink(impressum)
      ? [
          {
            key: "impressum",
            label: t("legal.impressum", "Impressum"),
            href: impressum!,
          },
        ]
      : []),
  ];

  const renderLink = (link: LegalLink) => (
    <Anchor
      key={link.key}
      href={link.href}
      target="_blank"
      rel="noopener noreferrer"
      size="sm"
    >
      <Group gap={6} wrap="nowrap">
        {link.label}
        <LocalIcon icon="open-in-new-rounded" width="0.9rem" height="0.9rem" />
      </Group>
    </Anchor>
  );

  return (
    <Stack gap="lg">
      <Paper withBorder p="md" radius="md">
        <Stack gap="md">
          <div>
            <Text fw={600} size="sm">
              {t("settings.legal.documents.title", "Legal Documents")}
            </Text>
            <Text size="xs" c="dimmed" mt={4}>
              {t(
                "settings.legal.documents.description",
                "Policies and legal information for this service.",
              )}
            </Text>
          </div>
          <Stack gap="sm">{legalLinks.map(renderLink)}</Stack>
        </Stack>
      </Paper>
    </Stack>
  );
};

export default LegalSection;
