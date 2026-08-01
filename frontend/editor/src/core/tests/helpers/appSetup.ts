import { Page } from "@playwright/test";

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
 * Open the app at the home page with onboarding pre-dismissed. The backend has
 * no accounts, so there is nothing to sign in to — every live spec starts
 * straight on the workbench.
 */
export async function openApp(page: Page): Promise<void> {
  // Skip onboarding before navigating so the modal never appears
  await skipOnboarding(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });
}

/**
 * Dismiss all startup dialogs. Uses Escape to close overlays without
 * triggering side effects.
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
 * Open the app and dismiss any welcome dialogs.
 */
export async function openAppAndSetup(page: Page): Promise<void> {
  await openApp(page);
  await dismissWelcomeDialog(page);
}
