import { isDesktopRuntime } from "@app/services/desktop/desktopRuntime";

export type OnboardingStepId = "welcome" | "desktop-install" | "tour-overview";

export type OnboardingStepType = "modal-slide" | "tool-prompt";

export interface OnboardingRuntimeState {
  tourRequested: boolean;
  tourType: string;
  isDesktopApp: boolean;
  desktopSlideEnabled: boolean;
}

export type OnboardingConditionContext = OnboardingRuntimeState;

export interface OnboardingStep {
  id: OnboardingStepId;
  type: OnboardingStepType;
  condition: (ctx: OnboardingConditionContext) => boolean;
  slideId?: "welcome" | "desktop-install" | "tour-overview";
  allowDismiss?: boolean;
}

export const DEFAULT_RUNTIME_STATE: OnboardingRuntimeState = {
  tourRequested: false,
  tourType: "whatsnew",
  // Every step below is gated on `!isDesktopApp`, so hardcoding false here
  // showed the desktop app the web onboarding — including a `desktop-install`
  // slide inviting the user to download the app they are already running.
  // Safe to evaluate at module load: the Tauri globals are injected into the
  // webview before any application script runs.
  isDesktopApp: isDesktopRuntime(),
  desktopSlideEnabled: true,
};

export const ONBOARDING_STEPS: OnboardingStep[] = [
  {
    id: "welcome",
    type: "modal-slide",
    slideId: "welcome",
    condition: (ctx) => !ctx.isDesktopApp,
  },
  {
    id: "desktop-install",
    type: "modal-slide",
    slideId: "desktop-install",
    condition: (ctx) => !ctx.isDesktopApp && ctx.desktopSlideEnabled,
  },
  {
    id: "tour-overview",
    type: "modal-slide",
    slideId: "tour-overview",
    condition: (ctx) => !ctx.isDesktopApp,
  },
];

export function getStepById(id: OnboardingStepId): OnboardingStep | undefined {
  return ONBOARDING_STEPS.find((step) => step.id === id);
}

export function getStepIndex(id: OnboardingStepId): number {
  return ONBOARDING_STEPS.findIndex((step) => step.id === id);
}
