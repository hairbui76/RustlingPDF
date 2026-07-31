import OnboardingModalSlide from "@app/components/onboarding/OnboardingModalSlide";
import {
  SLIDE_DEFINITIONS,
  type ButtonAction,
  type ButtonDefinition,
  type SlideFactoryParams,
  type SlideId,
} from "@app/components/onboarding/onboardingFlowConfig";
import type { OnboardingRuntimeState } from "@app/components/onboarding/orchestrator/onboardingConfig";

interface StaticOnboardingSlideProps {
  slideId: SlideId;
  runtimeState: OnboardingRuntimeState;
  onSkip: () => void;
  onAction: (action: ButtonAction) => void;
  allowDismiss?: boolean;
  /** Slide-specific createSlide params layered over the empty defaults. */
  params?: Partial<SlideFactoryParams>;
  /** Optional transform of the definition's buttons (e.g. drop a back button). */
  transformButtons?: (buttons: ButtonDefinition[]) => ButtonDefinition[];
}

/**
 * Renders a single onboarding slide outside the normal step flow, such as the
 * analytics-consent prompt.
 *
 * Rendered as its own component (keyed by slideId at the call site) so the
 * slide's internal hooks live in an isolated, stable scope rather than the
 * parent's, which avoids hook-order churn when the active interrupt changes.
 */
export default function StaticOnboardingSlide({
  slideId,
  runtimeState,
  onSkip,
  onAction,
  allowDismiss,
  params,
  transformButtons,
}: StaticOnboardingSlideProps) {
  const base = SLIDE_DEFINITIONS[slideId];
  const definition = transformButtons
    ? { ...base, buttons: transformButtons(base.buttons) }
    : base;

  const slideContent = definition.createSlide({
    osLabel: "",
    osUrl: "",
    ...params,
  });

  return (
    <OnboardingModalSlide
      slideDefinition={definition}
      slideContent={slideContent}
      runtimeState={runtimeState}
      modalSlideCount={1}
      currentModalSlideIndex={0}
      onSkip={onSkip}
      onAction={onAction}
      allowDismiss={allowDismiss}
    />
  );
}
