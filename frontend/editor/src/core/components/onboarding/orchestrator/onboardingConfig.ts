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
  isDesktopApp: false,
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
