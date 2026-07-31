/// <reference types="vite/client" />
import React, { useEffect } from "react";
import type { Decorator, Preview } from "@storybook/react-vite";
import { initialize, mswLoader } from "msw-storybook-addon";
import { MemoryRouter } from "react-router-dom";
import { withThemeByDataAttribute } from "@storybook/addon-themes";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import { parse as parseToml } from "smol-toml";
import { rtlLanguages, supportedLanguages } from "@core/i18n/languages";
import { PreferencesProvider } from "@core/contexts/PreferencesContext";
import { ThemeProvider } from "@core/components/shared/ThemeProvider";

import "@mantine/core/styles.css";
import "@core/tokens/base.css";
import "@core/styles/index.css";

void React;

const localeModules = import.meta.glob<string>(
  "../editor/public/locales/*/translation.toml",
  { query: "?raw", import: "default", eager: true },
);

const resources: Record<string, { translation: Record<string, unknown> }> = {};
for (const [path, raw] of Object.entries(localeModules)) {
  const language = path.match(/\/locales\/([^/]+)\/translation\.toml$/)?.[1];
  if (!language) continue;
  try {
    resources[language] = {
      translation: parseToml(raw) as Record<string, unknown>,
    };
  } catch {
    resources[language] = { translation: {} };
  }
}

if (!i18next.isInitialized) {
  void i18next.use(initReactI18next).init({
    lng: "en-US",
    fallbackLng: "en-US",
    supportedLngs: Object.keys(resources),
    resources,
    interpolation: { escapeValue: false },
    react: { useSuspense: false },
    initImmediate: false,
  });
} else {
  for (const [language, bundle] of Object.entries(resources)) {
    i18next.addResourceBundle(
      language,
      "translation",
      bundle.translation,
      true,
      true,
    );
  }
}

initialize({ onUnhandledRequest: "bypass" });

const withLocale: Decorator = (Story, context) => {
  const locale = (context.globals.locale as string) ?? "en-US";
  useEffect(() => {
    void i18next.changeLanguage(locale);
    document.documentElement.dir = rtlLanguages.includes(locale)
      ? "rtl"
      : "ltr";
    document.documentElement.lang = locale;
  }, [locale]);
  return <Story />;
};

const withProviders: Decorator = (Story) => (
  <MemoryRouter initialEntries={["/"]}>
    <PreferencesProvider>
      <ThemeProvider>
        <Story />
      </ThemeProvider>
    </PreferencesProvider>
  </MemoryRouter>
);

const preview: Preview = {
  loaders: [mswLoader],
  parameters: {
    layout: "padded",
    controls: {
      matchers: { color: /(background|color)$/i, date: /Date$/i },
    },
    backgrounds: {
      default: "app",
      values: [
        { name: "app", value: "var(--c-bg)" },
        { name: "surface", value: "var(--c-surface)" },
      ],
    },
    a11y: {
      context: "#storybook-root",
      config: {},
      options: {},
      test: "todo",
    },
  },
  globalTypes: {
    locale: {
      name: "Locale",
      description: "Active interface language",
      defaultValue: "en-US",
      toolbar: {
        icon: "globe",
        items: Object.entries(supportedLanguages).map(([value, title]) => ({
          value,
          title: `${value} - ${title}`,
        })),
        dynamicTitle: true,
      },
    },
  },
  decorators: [
    withLocale,
    withProviders,
    withThemeByDataAttribute({
      themes: { light: "light", dark: "dark" },
      defaultTheme: "light",
      attributeName: "data-theme",
    }),
  ],
};

export default preview;
