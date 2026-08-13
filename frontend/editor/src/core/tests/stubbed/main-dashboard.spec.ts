import { test, expect } from "@app/tests/helpers/stub-test-base";
import { openSettings } from "@app/tests/helpers/ui-helpers";

test.describe("2. Main Dashboard / Home Page", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
  });

  test.describe("2.1 Dashboard Layout and Tool Categories", () => {
    test("should display all navigation elements and tool categories", async ({
      page,
    }) => {
      await expect(
        page.locator('[data-testid="files-button"]').first(),
      ).toBeVisible({ timeout: 10000 });
      await expect(
        page.locator('[data-testid="config-button"]').first(),
      ).toBeVisible();

      await expect(page.getByPlaceholder(/search/i).first()).toBeVisible();

      await expect(
        page.getByRole("button", { name: /fullscreen|sidebar/i }).first(),
      ).toBeVisible();

      const categories = [
        /Recommended/,
        /Signing/,
        /Document Security/,
        /Verification/,
        /Document Review/,
        /Page Formatting/,
        /Extraction/,
        /Removal/,
        /Automation/,
        /General/,
        /Advanced Formatting/,
        /Developer Tools/,
      ];

      for (const category of categories) {
        await expect(page.getByText(category).first()).toBeVisible({
          timeout: 10000,
        });
      }
    });
  });

  test.describe("2.2 Dashboard - Recommended Tools", () => {
    test("should display recommended tools and navigate to merge", async ({
      page,
    }) => {
      const recommendedTools = [
        /PDF Text Editor/i,
        /Merge/i,
        /Compare/i,
        /Compress/i,
        /Convert/i,
        /Redact/i,
      ];

      for (const tool of recommendedTools) {
        await expect(page.getByText(tool).first()).toBeVisible({
          timeout: 10000,
        });
      }

      await page
        .getByText(/^Merge$/i)
        .first()
        .click();

      await expect(page).toHaveURL(/\/merge/, { timeout: 10000 });

      await page.goto("/");

      await expect(page.getByPlaceholder(/search/i).first()).toBeVisible();
    });
  });

  test.describe("2.3 Dashboard - File Upload Area", () => {
    test("should display file upload area with buttons", async ({ page }) => {
      const uploadButton = page
        .getByRole("button", { name: /upload|add files/i })
        .first();
      await expect(uploadButton).toBeVisible({ timeout: 10000 });
    });
  });

  test.describe("2.4 Dashboard - no legal or licence surfaces", () => {
    test("neither the dashboard nor Settings offers legal/licence pages", async ({
      page,
    }) => {
      await expect(
        page.locator('[data-testid="config-button"]').first(),
      ).toBeVisible({ timeout: 10000 });

      // The dashboard footer (survey + legal links) was removed
      await expect(page.locator(".footer-link")).toHaveCount(0);
      await expect(page.getByText("Survey")).toHaveCount(0);

      // Settings keeps Preferences and Help only; the Legal group (legal
      // documents + backend/frontend third-party licences) is gone.
      await openSettings(page);
      await expect(
        page.locator('[data-tour="admin-general-nav"]').first(),
      ).toBeVisible({ timeout: 10000 });
      for (const navKey of [
        "legal",
        "backendThirdPartyLicenses",
        "frontendThirdPartyLicenses",
      ]) {
        await expect(
          page.locator(`[data-tour="admin-${navKey}-nav"]`),
        ).toHaveCount(0);
      }
    });
  });
});
