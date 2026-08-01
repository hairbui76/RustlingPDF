import type { Meta, StoryObj } from "@storybook/react-vite";
// Load editor theme tokens so the modal surface renders correctly in Storybook.
import "@app/styles/theme.css";
import OnboardingModalSlide from "@app/components/onboarding/OnboardingModalSlide";
import {
  SLIDE_DEFINITIONS,
  type SlideId,
  type SlideFactoryParams,
} from "@app/components/onboarding/onboardingFlowConfig";
import {
  DEFAULT_RUNTIME_STATE,
  type OnboardingRuntimeState,
} from "@app/components/onboarding/orchestrator/onboardingConfig";

/**
 * Every onboarding modal slide, rendered through the real
 * {@link OnboardingModalSlide} + {@link SLIDE_DEFINITIONS} so design changes
 * here reflect the production flow. Each story is one slide (or a meaningful
 * variant of one), including its hero, stepper and action buttons.
 */

// Sensible defaults for every slide factory; individual stories override the
// few fields their slide actually reads.
const BASE_PARAMS: SlideFactoryParams = {
  osLabel: "macOS (Apple Silicon)",
  osUrl: "#",
  osOptions: [
    { label: "macOS (Apple Silicon)", url: "#", value: "mac-arm" },
    { label: "macOS (Intel)", url: "#", value: "mac-intel" },
    { label: "Windows", url: "#", value: "windows" },
    { label: "Linux", url: "#", value: "linux" },
  ],
  onDownloadUrlChange: () => {},
};

interface SlideStageProps {
  slideId: SlideId;
  params?: Partial<SlideFactoryParams>;
  runtime?: Partial<OnboardingRuntimeState>;
  allowDismiss?: boolean;
  /**
   * Total steps in the flow. Defaults to 1 → a single standalone card with no
   * progress bar or step pill. Set > 1
   * only to demonstrate the stepped-flow treatment.
   */
  stepCount?: number;
  /** 0-based active step, used only when stepCount > 1. */
  stepIndex?: number;
}

function SlideStage({
  slideId,
  params,
  runtime,
  allowDismiss = true,
  stepCount = 1,
  stepIndex = 0,
}: SlideStageProps) {
  const merged: SlideFactoryParams = { ...BASE_PARAMS, ...params };

  const definition = SLIDE_DEFINITIONS[slideId];
  const slideContent = definition.createSlide(merged);

  const runtimeState: OnboardingRuntimeState = {
    ...DEFAULT_RUNTIME_STATE,
    ...runtime,
  };

  return (
    <OnboardingModalSlide
      slideDefinition={definition}
      slideContent={slideContent}
      runtimeState={runtimeState}
      modalSlideCount={stepCount}
      currentModalSlideIndex={stepIndex}
      onSkip={() => {}}
      onAction={() => {}}
      allowDismiss={allowDismiss}
    />
  );
}

const meta: Meta<typeof SlideStage> = {
  title: "Onboarding/Modal Slides",
  component: SlideStage,
  parameters: { layout: "fullscreen" },
};
export default meta;

type Story = StoryObj<typeof SlideStage>;

/** "Welcome to RustlingPDF" — the V2 intro slide (rocket hero). */
export const Welcome: Story = { args: { slideId: "welcome" } };

/** The only stepped example: a multi-step flow shows the step pill + progress
 * bar. Every other story is a standalone single card (no steps). */
export const SteppedFlowExample: Story = {
  args: { slideId: "desktop-install", stepCount: 4, stepIndex: 2 },
};

/** Desktop app download prompt with an OS picker (dual-icon hero). */
export const DesktopInstall: Story = { args: { slideId: "desktop-install" } };

/** Quick tour offer before dropping the user into the tools. */
export const TourOverview: Story = { args: { slideId: "tour-overview" } };
