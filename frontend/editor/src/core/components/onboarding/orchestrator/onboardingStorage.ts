const STORAGE_PREFIX = "onboarding";
const TOURS_TOOLTIP_KEY = `${STORAGE_PREFIX}::tours-tooltip-shown`;
const ONBOARDING_COMPLETED_KEY = `${STORAGE_PREFIX}::completed`;

export function isOnboardingCompleted(): boolean {
  if (typeof window === "undefined") return false;
  try {
    return localStorage.getItem(ONBOARDING_COMPLETED_KEY) === "true";
  } catch {
    return false;
  }
}

export function markOnboardingCompleted(): void {
  if (typeof window === "undefined") return;
  try {
    localStorage.setItem(ONBOARDING_COMPLETED_KEY, "true");
  } catch (error) {
    console.error(
      "[onboardingStorage] Error marking onboarding as completed:",
      error,
    );
  }
}

export function resetOnboardingProgress(): void {
  if (typeof window === "undefined") return;
  try {
    localStorage.removeItem(ONBOARDING_COMPLETED_KEY);
  } catch (error) {
    console.error(
      "[onboardingStorage] Error resetting onboarding progress:",
      error,
    );
  }
}

export function hasShownToursTooltip(): boolean {
  if (typeof window === "undefined") return false;
  try {
    return localStorage.getItem(TOURS_TOOLTIP_KEY) === "true";
  } catch {
    return false;
  }
}

export function markToursTooltipShown(): void {
  if (typeof window === "undefined") return;
  try {
    localStorage.setItem(TOURS_TOOLTIP_KEY, "true");
  } catch (error) {
    console.error(
      "[onboardingStorage] Error marking tours tooltip as shown:",
      error,
    );
  }
}
