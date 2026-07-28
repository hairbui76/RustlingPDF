import { Page } from "@playwright/test";

/**
 * Ensure the cookie consent banner doesn't appear by setting the consent cookie.
 * Call this before navigating or after clearing cookies.
 */
export async function ensureCookieConsent(page: Page): Promise<void> {
  await page.context().addCookies([
    {
      name: "cc_cookie",
      value: JSON.stringify({
        categories: ["necessary"],
        revision: 0,
        data: null,
        rfc_cookie: false,
      }),
      domain: "localhost",
      path: "/",
    },
  ]);
}

/**
 * Mark onboarding as completed in localStorage to prevent the onboarding
 * modal from appearing. This is more reliable than trying to click through
 * the onboarding slides, which can cause unintended tool selections.
 *
 * Uses addInitScript so the localStorage is set before the React app reads it.
 */
export async function skipOnboarding(page: Page): Promise<void> {
  await page.addInitScript(() => {
    localStorage.setItem("onboarding::completed", "true");
    localStorage.setItem("onboarding::tours-tooltip-shown", "true");
  });
}

/**
 * Open the app at the home page with the cookie consent and onboarding
 * pre-dismissed. The backend has no accounts, so there is nothing to sign in
 * to — every live spec starts straight on the workbench.
 */
export async function openApp(page: Page): Promise<void> {
  await ensureCookieConsent(page);
  // Skip onboarding before navigating so the modal never appears
  await skipOnboarding(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });
}

/**
 * Dismiss all startup dialogs (welcome + cookie consent + any others).
 * Uses Escape key to close overlays without triggering side effects.
 */
export async function dismissWelcomeDialog(page: Page): Promise<void> {
  // Give dialogs time to render
  await page.waitForTimeout(1000);

  // Try up to 5 times to dismiss all overlays via Escape
  for (let i = 0; i < 5; i++) {
    const hasOverlay = await page
      .locator(".mantine-Modal-overlay, .mantine-Overlay-root")
      .first()
      .isVisible()
      .catch(() => false);
    if (!hasOverlay) break;

    await page.keyboard.press("Escape");
    await page.waitForTimeout(500);
  }
}

/**
 * Dismiss the cookie consent banner if it appears.
 * The banner is rendered inside #cc-main by the CookieConsent library.
 */
export async function dismissCookieConsent(page: Page): Promise<void> {
  try {
    // Target buttons specifically inside the cookie consent container
    const ccMain = page.locator("#cc-main");
    const dismissBtn = ccMain
      .locator(
        'button:has-text("Tidak, terima kasih"), button:has-text("No Thanks"), button:has-text("Oke"), button:has-text("OK")',
      )
      .first();
    if (await dismissBtn.isVisible({ timeout: 2000 })) {
      await dismissBtn.click({ force: true });
      await page.waitForTimeout(500);
    }
  } catch {
    // No cookie consent banner present
  }
}

/**
 * Open the app and dismiss any welcome dialogs.
 */
export async function openAppAndSetup(page: Page): Promise<void> {
  await openApp(page);
  // Cookie consent may appear on top, dismiss it first
  await dismissCookieConsent(page);
  await dismissWelcomeDialog(page);
  // In case cookie appeared after welcome was dismissed
  await dismissCookieConsent(page);
}
