import { useEffect, useMemo, useCallback, useState } from "react";
import { type StepType } from "@reactour/tour";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";
import { dispatchTourState } from "@app/constants/events";
import { useOnboardingOrchestrator } from "@app/components/onboarding/orchestrator/useOnboardingOrchestrator";
import { useBypassOnboarding } from "@app/components/onboarding/useBypassOnboarding";
import OnboardingTour, {
  type AdvanceArgs,
  type CloseArgs,
} from "@app/components/onboarding/OnboardingTour";
import OnboardingModalSlide from "@app/components/onboarding/OnboardingModalSlide";
import { useTourRequest } from "@app/components/onboarding/useOnboardingEffects";
import { useOnboardingDownload } from "@app/components/onboarding/useOnboardingDownload";
import {
  SLIDE_DEFINITIONS,
  type SlideId,
  type ButtonAction,
} from "@app/components/onboarding/onboardingFlowConfig";
import ToolPanelModePrompt from "@app/components/tools/ToolPanelModePrompt";
import { useTourOrchestration } from "@app/contexts/TourOrchestrationContext";
import { getTourSteps } from "@app/components/onboarding/tourRegistry";
import { removeAllGlows } from "@app/components/onboarding/tourGlow";
import { useFilesModalContext } from "@app/contexts/FilesModalContext";
import "@app/components/onboarding/OnboardingTour.css";

export default function Onboarding() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const bypassOnboarding = useBypassOnboarding();
  const { state, actions } = useOnboardingOrchestrator();
  const { currentStep, isActive, isLoading, runtimeState, activeFlow } = state;

  const { osInfo, osOptions, setSelectedDownloadUrl, handleDownloadSelected } =
    useOnboardingDownload();
  const {
    tourRequested: externalTourRequested,
    requestedTourType,
    clearTourRequest,
  } = useTourRequest();
  const handleButtonAction = useCallback(
    async (action: ButtonAction) => {
      switch (action) {
        case "next":
        case "complete-close":
          actions.complete();
          break;
        case "prev":
          actions.prev();
          break;
        case "close":
          actions.skip();
          break;
        case "download-selected":
          handleDownloadSelected();
          actions.complete();
          break;
        case "launch-tools":
          actions.updateRuntimeState({ tourType: "whatsnew" });
          setIsTourOpen(true);
          break;
        case "skip-tour":
          actions.complete();
          break;
      }
    },
    [actions, handleDownloadSelected],
  );

  const isRTL =
    typeof document !== "undefined"
      ? document.documentElement.dir === "rtl"
      : false;
  const [isTourOpen, setIsTourOpen] = useState(false);

  useEffect(() => dispatchTourState(isTourOpen), [isTourOpen]);

  const { openFilesModal, closeFilesModal } = useFilesModalContext();
  const tourOrch = useTourOrchestration();

  const tourSteps = useMemo<StepType[]>(
    () =>
      getTourSteps(runtimeState.tourType, {
        t,
        workbench: tourOrch,
        openSettingsHelpSection: () => navigate("/settings/help"),
        openFilesModal,
        closeFilesModal,
      }),
    [
      runtimeState.tourType,
      t,
      tourOrch,
      navigate,
      openFilesModal,
      closeFilesModal,
    ],
  );

  useEffect(() => {
    if (externalTourRequested) {
      actions.updateRuntimeState({ tourType: requestedTourType });
      setIsTourOpen(true);
      clearTourRequest();
    }
  }, [externalTourRequested, requestedTourType, actions, clearTourRequest]);

  useEffect(() => {
    if (!isTourOpen) removeAllGlows();
    return () => removeAllGlows();
  }, [isTourOpen]);

  const finishTour = useCallback(() => {
    setIsTourOpen(false);
    tourOrch.restoreWorkbenchState();
    // Advance to next onboarding step after tour completes
    actions.complete();
  }, [actions, tourOrch]);

  const handleAdvanceTour = useCallback(
    (args: AdvanceArgs) => {
      const {
        setCurrentStep,
        currentStep: tourCurrentStep,
        steps,
        setIsOpen,
      } = args;
      if (steps && tourCurrentStep === steps.length - 1) {
        setIsOpen(false);
        finishTour();
      } else if (steps) {
        setCurrentStep((s) => (s === steps.length - 1 ? 0 : s + 1));
      }
    },
    [finishTour],
  );

  const handleCloseTour = useCallback(
    (args: CloseArgs) => {
      args.setIsOpen(false);
      finishTour();
    },
    [finishTour],
  );

  const currentSlideDefinition = useMemo(() => {
    if (
      !currentStep ||
      currentStep.type !== "modal-slide" ||
      !currentStep.slideId
    ) {
      return null;
    }
    return SLIDE_DEFINITIONS[currentStep.slideId as SlideId];
  }, [currentStep]);

  const currentSlideContent = useMemo(() => {
    if (!currentSlideDefinition) return null;
    return currentSlideDefinition.createSlide({
      osLabel: osInfo.label,
      osUrl: osInfo.url,
      osOptions,
      onDownloadUrlChange: setSelectedDownloadUrl,
    });
  }, [currentSlideDefinition, osInfo, osOptions, setSelectedDownloadUrl]);

  const modalSlideCount = useMemo(() => {
    return activeFlow.filter((step) => step.type === "modal-slide").length;
  }, [activeFlow]);

  const currentModalSlideIndex = useMemo(() => {
    if (!currentStep || currentStep.type !== "modal-slide") return 0;
    const modalSlides = activeFlow.filter(
      (step) => step.type === "modal-slide",
    );
    return modalSlides.findIndex((step) => step.id === currentStep.id);
  }, [activeFlow, currentStep]);

  if (bypassOnboarding) {
    return null;
  }

  // Always render the tour component (it controls its own visibility with isOpen)
  const tourComponent = (
    <OnboardingTour
      isOpen={isTourOpen}
      tourSteps={tourSteps}
      tourType={runtimeState.tourType}
      isRTL={isRTL}
      t={t}
      onAdvance={handleAdvanceTour}
      onClose={handleCloseTour}
    />
  );

  // If no active onboarding, just show the tour (which may or may not be open)
  if (isLoading || !isActive || !currentStep) {
    return tourComponent;
  }

  // If tour is open, hide the onboarding modal and just show the tour
  if (isTourOpen) {
    return tourComponent;
  }

  // Render the current onboarding step
  switch (currentStep.type) {
    case "tool-prompt":
      return (
        <ToolPanelModePrompt forceOpen={true} onComplete={actions.complete} />
      );

    case "modal-slide":
      if (!currentSlideDefinition || !currentSlideContent) return null;
      return (
        <OnboardingModalSlide
          slideDefinition={currentSlideDefinition}
          slideContent={currentSlideContent}
          runtimeState={runtimeState}
          modalSlideCount={modalSlideCount}
          currentModalSlideIndex={currentModalSlideIndex}
          onSkip={actions.skip}
          onAction={handleButtonAction}
          allowDismiss={currentStep.allowDismiss}
        />
      );

    default:
      return null;
  }
}
