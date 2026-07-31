import { test, expect, type Page, type Route } from "@playwright/test";
import {
  mockAppApis,
  seedCookieConsent,
  skipOnboarding,
} from "@app/tests/helpers/api-stubs";

/**
 * The LoginAgreementModal shows a blocking Accept/Decline disclaimer on
 * launch. Text comes from GET /api/v1/config/login-disclaimer for the current
 * language and acceptance is remembered for the browser tab.
 */

const MARKDOWN = "## Test Disclaimer\n\nThis is **mandatory** reading.";

interface DisclaimerStub {
  enabled?: boolean;
  showInAnonymousMode?: boolean;
  content?: string;
}

async function stubDisclaimer(page: Page, opts: DisclaimerStub = {}) {
  const {
    enabled = true,
    showInAnonymousMode = true,
    content = MARKDOWN,
  } = opts;
  await page.route("**/api/v1/config/login-disclaimer*", (route: Route) =>
    route.fulfill({
      json: { enabled, showInAnonymousMode, content, format: "markdown" },
    }),
  );
}

async function setUpDisclaimer(page: Page, disclaimer: DisclaimerStub = {}) {
  await seedCookieConsent(page);
  await skipOnboarding(page);
  await mockAppApis(page);
  await stubDisclaimer(page, disclaimer);
}

test.describe("Login agreement modal", () => {
  test("shows a blocking disclaimer with rendered markdown", async ({
    page,
  }) => {
    await setUpDisclaimer(page);
    await page.goto("/");

    await expect(
      page.getByText("Login Agreement", { exact: true }).first(),
    ).toBeVisible({ timeout: 15_000 });
    // Markdown is rendered (heading + bold), not shown as raw text.
    await expect(
      page.getByRole("heading", { name: "Test Disclaimer" }),
    ).toBeVisible();
    await expect(page.getByText("mandatory")).toBeVisible();
    await expect(page.getByRole("button", { name: "Accept" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Decline" })).toBeVisible();
  });

  test("Escape does not dismiss the modal (blocking)", async ({ page }) => {
    await setUpDisclaimer(page);
    await page.goto("/");
    await expect(
      page.getByRole("heading", { name: "Test Disclaimer" }),
    ).toBeVisible({ timeout: 15_000 });

    await page.keyboard.press("Escape");
    await page.waitForTimeout(400);
    await expect(
      page.getByRole("heading", { name: "Test Disclaimer" }),
    ).toBeVisible();
  });

  test("Accept dismisses and it does not reappear on reload", async ({
    page,
  }) => {
    await setUpDisclaimer(page);
    await page.goto("/");
    await expect(
      page.getByRole("heading", { name: "Test Disclaimer" }),
    ).toBeVisible({ timeout: 15_000 });

    await page.getByRole("button", { name: "Accept" }).click();
    await expect(
      page.getByRole("heading", { name: "Test Disclaimer" }),
    ).toBeHidden();

    await page.reload();
    await page.waitForTimeout(1000);
    await expect(
      page.getByRole("heading", { name: "Test Disclaimer" }),
    ).toBeHidden();
  });

  test("does not show when the feature is disabled", async ({ page }) => {
    await setUpDisclaimer(page, { enabled: false, content: "" });
    await page.goto("/");
    // App is usable; modal never appears.
    await page.waitForTimeout(1500);
    await expect(
      page.getByRole("heading", { name: "Test Disclaimer" }),
    ).toBeHidden();
  });

  test("shows in anonymous (no-login) mode when allowed", async ({ page }) => {
    await seedCookieConsent(page);
    await skipOnboarding(page);
    await mockAppApis(page);
    await stubDisclaimer(page, { showInAnonymousMode: true });
    await page.goto("/");

    await expect(
      page.getByRole("heading", { name: "Test Disclaimer" }),
    ).toBeVisible({ timeout: 15_000 });
  });

  test("does not show in anonymous mode when suppressed", async ({ page }) => {
    await seedCookieConsent(page);
    await skipOnboarding(page);
    await mockAppApis(page);
    await stubDisclaimer(page, { showInAnonymousMode: false });
    await page.goto("/");

    await page.waitForTimeout(1500);
    await expect(
      page.getByRole("heading", { name: "Test Disclaimer" }),
    ).toBeHidden();
  });
});
