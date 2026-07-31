import WelcomeSlide from "@app/components/onboarding/slides/WelcomeSlide";
import DesktopInstallSlide from "@app/components/onboarding/slides/DesktopInstallSlide";
import TourOverviewSlide from "@app/components/onboarding/slides/TourOverviewSlide";
import AnalyticsChoiceSlide from "@app/components/onboarding/slides/AnalyticsChoiceSlide";
import type {
  OSOption,
  ButtonDefinition as ButtonDefinitionBase,
  HeroDefinition as HeroDefinitionBase,
  SlideDefinition as SlideDefinitionBase,
} from "@app/components/onboarding/onboardingSlideTypes";

export type { OSOption };

export type SlideId =
  "welcome" | "desktop-install" | "tour-overview" | "analytics-choice";

export type HeroType = "rocket" | "dual-icon" | "analytics";

export type ButtonAction =
  | "next"
  | "prev"
  | "close"
  | "complete-close"
  | "download-selected"
  | "launch-tools"
  | "skip-tour"
  | "enable-analytics"
  | "disable-analytics";

export type FlowState = object;

export interface SlideFactoryParams {
  osLabel: string;
  osUrl: string;
  osOptions?: OSOption[];
  onDownloadUrlChange?: (url: string) => void;
  analyticsError?: string | null;
  analyticsLoading?: boolean;
}

export type HeroDefinition = HeroDefinitionBase<HeroType>;
export type ButtonDefinition = ButtonDefinitionBase<ButtonAction, FlowState>;
export type SlideDefinition = SlideDefinitionBase<
  SlideId,
  ButtonAction,
  FlowState,
  HeroType,
  SlideFactoryParams
>;

export const SLIDE_DEFINITIONS: Record<SlideId, SlideDefinition> = {
  welcome: {
    id: "welcome",
    createSlide: () => WelcomeSlide(),
    hero: { type: "rocket" },
    buttons: [
      {
        key: "welcome-next",
        type: "button",
        label: "onboarding.buttons.next",
        variant: "primary",
        group: "right",
        action: "next",
      },
    ],
  },
  "desktop-install": {
    id: "desktop-install",
    createSlide: ({ osLabel, osUrl, osOptions, onDownloadUrlChange }) =>
      DesktopInstallSlide({ osLabel, osUrl, osOptions, onDownloadUrlChange }),
    hero: { type: "dual-icon" },
    buttons: [
      {
        key: "desktop-back",
        type: "icon",
        icon: "chevron-left",
        group: "left",
        action: "prev",
      },
      {
        key: "desktop-skip",
        type: "button",
        label: "onboarding.buttons.skipForNow",
        variant: "secondary",
        group: "left",
        action: "next",
      },
      {
        key: "desktop-download",
        type: "button",
        label: "onboarding.buttons.download",
        variant: "primary",
        group: "right",
        action: "download-selected",
      },
    ],
  },
  "tour-overview": {
    id: "tour-overview",
    createSlide: () => TourOverviewSlide(),
    hero: { type: "rocket" },
    buttons: [
      {
        key: "tour-overview-back",
        type: "icon",
        icon: "chevron-left",
        group: "left",
        action: "prev",
      },
      {
        key: "tour-overview-skip",
        type: "button",
        label: "onboarding.buttons.skipForNow",
        variant: "secondary",
        group: "left",
        action: "skip-tour",
      },
      {
        key: "tour-overview-show",
        type: "button",
        label: "onboarding.buttons.showMeAround",
        variant: "primary",
        group: "right",
        action: "launch-tools",
      },
    ],
  },
  "analytics-choice": {
    id: "analytics-choice",
    createSlide: ({ analyticsError }) =>
      AnalyticsChoiceSlide({ analyticsError }),
    hero: { type: "analytics" },
    buttons: [
      {
        key: "analytics-disable",
        type: "button",
        label: "no",
        variant: "secondary",
        group: "left",
        action: "disable-analytics",
      },
      {
        key: "analytics-enable",
        type: "button",
        label: "yes",
        variant: "primary",
        group: "right",
        action: "enable-analytics",
      },
    ],
  },
};
