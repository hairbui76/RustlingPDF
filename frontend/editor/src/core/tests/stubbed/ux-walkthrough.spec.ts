/**
 * UI walkthrough capture spec (throwaway — not part of the CI suite's
 * intent, kept runnable for re-captures). Produces the screenshots under
 * screenshots/ux-walkthrough/ that walkthrough.html displays.
 *
 * Two capture groups:
 *  - Web surfaces: home, tool search, files view, settings (no Tauri).
 *  - Desktop update feature: a minimal fake `window.__TAURI_INTERNALS__`
 *    turns isDesktopRuntime() on and answers the exact commands the app
 *    invokes (get_backend_port, pop_opened_batches) plus the updater
 *    plugin's check/download_and_install, per scenario. The real 10s
 *    startup delay in DesktopUpdateBanner is waited out for real — no
 *    fake clocks around Mantine transitions.
 *
 * Themes: light, dark (preferences.theme in localStorage), rtl (ar-AR via
 * i18nextLng). Shots are named NN_view_<theme>.png so pairs line up.
 */
import { test } from "@app/tests/helpers/stub-test-base";
import { openSettings } from "@app/tests/helpers/ui-helpers";
import type { Page } from "@playwright/test";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const SHOT_DIR = join(
  dirname(fileURLToPath(import.meta.url)),
  "..",
  "..",
  "..",
  "..",
  "screenshots",
  "ux-walkthrough",
);
mkdirSync(SHOT_DIR, { recursive: true });

type Theme = "light" | "dark" | "rtl";
const THEMES: Theme[] = ["light", "dark", "rtl"];

function shotPath(name: string, theme: Theme): string {
  return join(SHOT_DIR, `${name}_${theme}.png`);
}

/** Seed theme/language before the app boots. */
async function applyTheme(page: Page, theme: Theme): Promise<void> {
  await page.addInitScript((t) => {
    const prefs = JSON.parse(
      localStorage.getItem("rustlingpdf_preferences") ?? "{}",
    );
    prefs.theme = t === "dark" ? "dark" : "light";
    localStorage.setItem("rustlingpdf_preferences", JSON.stringify(prefs));
    if (t === "rtl") {
      // The app's language policy demotes a bare localStorage value; a
      // "user" source (LanguageSource.User = 3) survives every override.
      localStorage.setItem("i18nextLng", "ar-AR");
      localStorage.setItem("i18nextLng-source", "3");
    }
  }, theme);
  if (theme === "dark") {
    await page.emulateMedia({ colorScheme: "dark" });
  }
}

/** Let Mantine portals/transitions finish before capturing. */
async function settle(page: Page): Promise<void> {
  await page.waitForTimeout(600);
}

/**
 * Minimal Tauri fake: enough for isDesktopRuntime() and the update flow.
 * `updateScenario` controls what downloadAndInstall does.
 */
async function fakeDesktop(
  page: Page,
  updateScenario: "idle" | "hang" | "fail",
): Promise<void> {
  await page.addInitScript((scenario) => {
    localStorage.setItem(
      "rustlingpdf.desktopBackendUrl",
      "http://127.0.0.1:5173",
    );
    (window as unknown as { isTauri: boolean }).isTauri = true;
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {
      metadata: {
        currentWebview: { label: "main" },
        currentWindow: { label: "main" },
      },
      transformCallback: () => 0,
      invoke: async (cmd: string) => {
        switch (cmd) {
          case "get_backend_port":
            return 5173;
          case "pop_opened_batches":
            return [];
          case "plugin:event|listen":
            return 1;
          case "plugin:event|unlisten":
            return null;
          case "plugin:updater|check":
            return {
              rid: 1,
              currentVersion: "0.0.7",
              version: "0.0.9",
              date: "2026-08-04",
              body: "",
            };
          case "plugin:updater|download_and_install":
            if (scenario === "hang") {
              return new Promise(() => {});
            }
            throw new Error("simulated install failure");
          case "plugin:process|restart":
            return null;
          default:
            console.warn("[fakeDesktop] unmocked invoke:", cmd);
            return null;
        }
      },
    };
  }, updateScenario);
}

/** The banner checks 10s after mount; wait it out plus network settle. */
async function waitForBanner(page: Page): Promise<void> {
  await page
    .getByTestId("desktop-update-banner")
    .waitFor({ state: "visible", timeout: 20_000 });
}

test.use({ autoGoto: false });

for (const theme of THEMES) {
  test.describe(`walkthrough (${theme})`, () => {
    // A realistic language list: with the stub's default single-language
    // list the Language picker renders empty and ar-AR (the RTL pass) gets
    // filtered out of supportedLngs, silently leaving the page LTR.
    test.use({
      stubOptions: {
        languages: ["en-US", "en-GB", "vi-VN", "ar-AR", "de-DE"],
        defaultLocale: "en-US",
      },
    });
    test(`web surfaces (${theme})`, async ({ page }) => {
      test.setTimeout(180_000);
      await applyTheme(page, theme);
      await page.goto("/", { waitUntil: "domcontentloaded" });
      await page
        .locator('[data-testid="files-button"]')
        .first()
        .waitFor({ timeout: 30_000 });
      await settle(page);
      await page.screenshot({
        path: shotPath("01_home_tools", theme),
        fullPage: false,
      });

      // Tool search open with results
      const search = page
        .locator('input[placeholder*="earch"], [data-testid="tool-search"]')
        .first();
      if (await search.isVisible().catch(() => false)) {
        await search.click();
        await search.fill("compress");
        await settle(page);
        await page.screenshot({ path: shotPath("02_tool_search", theme) });
        await page.keyboard.press("Escape");
      }

      // Settings dialog — General section (web build: no update toggle)
      await openSettings(page);
      await settle(page);
      await page.screenshot({ path: shotPath("03_settings_web", theme) });
    });

    test(`update banner available (${theme})`, async ({ page }) => {
      test.setTimeout(180_000);
      await applyTheme(page, theme);
      await fakeDesktop(page, "idle");
      await page.goto("/", { waitUntil: "domcontentloaded" });
      await waitForBanner(page);
      await settle(page);
      await page.screenshot({ path: shotPath("04_update_available", theme) });

      // Settings in desktop mode shows the new toggle + desktop description
      await openSettings(page);
      await settle(page);
      await page.screenshot({ path: shotPath("07_settings_desktop", theme) });
    });

    test(`update banner downloading (${theme})`, async ({ page }) => {
      test.setTimeout(180_000);
      await applyTheme(page, theme);
      await fakeDesktop(page, "hang");
      await page.goto("/", { waitUntil: "domcontentloaded" });
      await waitForBanner(page);
      await page.getByTestId("desktop-update-install").click();
      await settle(page);
      await page.screenshot({ path: shotPath("05_update_downloading", theme) });
    });

    test(`update banner failed (${theme})`, async ({ page }) => {
      test.setTimeout(180_000);
      await applyTheme(page, theme);
      await fakeDesktop(page, "fail");
      await page.goto("/", { waitUntil: "domcontentloaded" });
      await waitForBanner(page);
      await page.getByTestId("desktop-update-install").click();
      // The rejection lands quickly; wait for the failed copy to render.
      await settle(page);
      await page.screenshot({ path: shotPath("06_update_failed", theme) });
    });
  });
}
