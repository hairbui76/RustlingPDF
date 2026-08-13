import React from "react";
import { useTranslation } from "react-i18next";
import HotkeysSection from "@app/components/shared/config/configSections/HotkeysSection";
import GeneralSection from "@app/components/shared/config/configSections/GeneralSection";
import HelpSection from "@app/components/shared/config/configSections/HelpSection";
import type {
  ConfigNavItem,
  ConfigNavSection,
} from "@app/components/shared/config/types";

// Re-exported for the many existing importers; the definitions live in
// config/types so type-only consumers don't pull the section tree in.
export type { ConfigNavItem, ConfigNavSection };

export interface ConfigColors {
  navBg: string;
  sectionTitle: string;
  navItem: string;
  navItemActive: string;
  navItemActiveBg: string;
  contentBg: string;
  headerBorder: string;
}

export const useConfigNavSections = (
  onRequestClose: () => void = () => {},
): ConfigNavSection[] => {
  const { t } = useTranslation();

  const sections: ConfigNavSection[] = [
    {
      title: t("settings.preferences.title", "Preferences"),
      items: [
        {
          key: "general",
          label: t("settings.general.title", "General"),
          icon: "settings-rounded",
          component: <GeneralSection />,
        },
        {
          key: "hotkeys",
          label: t("settings.hotkeys.title", "Keyboard Shortcuts"),
          icon: "keyboard-rounded",
          component: <HotkeysSection />,
        },
      ],
    },
    {
      title: t("settings.help.title", "Help"),
      items: [
        {
          key: "help",
          label: t("settings.help.label", "Tours"),
          icon: "help-rounded",
          component: <HelpSection onRequestClose={onRequestClose} />,
        },
      ],
    },
  ];

  return sections;
};
