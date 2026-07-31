import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useAppConfig } from "@app/contexts/AppConfigContext";
import {
  ONBOARDING_STEPS,
  type OnboardingConditionContext,
  type OnboardingRuntimeState,
  type OnboardingStep,
  type OnboardingStepId,
  DEFAULT_RUNTIME_STATE,
} from "@app/components/onboarding/orchestrator/onboardingConfig";
import {
  isOnboardingCompleted,
  markOnboardingCompleted,
} from "@app/components/onboarding/orchestrator/onboardingStorage";
import { useBypassOnboarding } from "@app/components/onboarding/useBypassOnboarding";

const SESSION_TOUR_REQUESTED = "onboarding::session::tour-requested";
const SESSION_TOUR_TYPE = "onboarding::session::tour-type";

function getInitialRuntimeState(
  baseState: OnboardingRuntimeState,
): OnboardingRuntimeState {
  if (typeof window === "undefined") return baseState;
  try {
    return {
      ...baseState,
      tourRequested: sessionStorage.getItem(SESSION_TOUR_REQUESTED) === "true",
      tourType: sessionStorage.getItem(SESSION_TOUR_TYPE) ?? baseState.tourType,
    };
  } catch {
    return baseState;
  }
}

function persistRuntimeState(state: Partial<OnboardingRuntimeState>): void {
  if (typeof window === "undefined") return;
  try {
    if (state.tourRequested !== undefined) {
      sessionStorage.setItem(
        SESSION_TOUR_REQUESTED,
        state.tourRequested ? "true" : "false",
      );
    }
    if (state.tourType !== undefined) {
      sessionStorage.setItem(SESSION_TOUR_TYPE, state.tourType);
    }
  } catch (error) {
    console.error(
      "[useOnboardingOrchestrator] Error persisting runtime state:",
      error,
    );
  }
}

function clearRuntimeStateSession(): void {
  if (typeof window === "undefined") return;
  try {
    sessionStorage.removeItem(SESSION_TOUR_REQUESTED);
    sessionStorage.removeItem(SESSION_TOUR_TYPE);
  } catch {
    // Storage is optional.
  }
}

export interface OnboardingOrchestratorState {
  isActive: boolean;
  currentStep: OnboardingStep | null;
  currentStepIndex: number;
  totalSteps: number;
  runtimeState: OnboardingRuntimeState;
  activeFlow: OnboardingStep[];
  isComplete: boolean;
  isLoading: boolean;
}

export interface OnboardingOrchestratorActions {
  next: () => void;
  prev: () => void;
  skip: () => void;
  complete: () => void;
  updateRuntimeState: (updates: Partial<OnboardingRuntimeState>) => void;
  refreshFlow: () => void;
  startStep: (stepId: OnboardingStepId) => void;
  pause: () => void;
  resume: () => void;
}

export interface UseOnboardingOrchestratorResult {
  state: OnboardingOrchestratorState;
  actions: OnboardingOrchestratorActions;
}

export interface UseOnboardingOrchestratorOptions {
  defaultRuntimeState?: OnboardingRuntimeState;
}

export function useOnboardingOrchestrator(
  options?: UseOnboardingOrchestratorOptions,
): UseOnboardingOrchestratorResult {
  const defaultState = options?.defaultRuntimeState ?? DEFAULT_RUNTIME_STATE;
  const { config, loading: configLoading } = useAppConfig();
  const bypassOnboarding = useBypassOnboarding();
  const [runtimeState, setRuntimeState] = useState<OnboardingRuntimeState>(() =>
    getInitialRuntimeState(defaultState),
  );
  const [isPaused, setIsPaused] = useState(false);
  const [isInitialized, setIsInitialized] = useState(false);
  const [manuallyStarted, setManuallyStarted] = useState(false);
  const [currentStepIndex, setCurrentStepIndex] = useState(-1);
  const initialIndexSet = useRef(false);

  useEffect(() => {
    setRuntimeState((previous) => ({
      ...previous,
      desktopSlideEnabled: config?.enableDesktopInstallSlide ?? true,
    }));
  }, [config?.enableDesktopInstallSlide]);

  const conditionContext = useMemo<OnboardingConditionContext>(
    () => runtimeState,
    [runtimeState],
  );
  const activeFlow = useMemo(
    () => ONBOARDING_STEPS.filter((step) => step.condition(conditionContext)),
    [conditionContext],
  );

  useEffect(() => {
    if (configLoading || initialIndexSet.current) return;
    setCurrentStepIndex(activeFlow.length);
    initialIndexSet.current = true;
  }, [activeFlow.length, configLoading]);

  useEffect(() => {
    if (!configLoading && !isInitialized) setIsInitialized(true);
  }, [configLoading, isInitialized]);

  const totalSteps = activeFlow.length;
  const isComplete =
    isInitialized &&
    (totalSteps === 0 ||
      currentStepIndex >= totalSteps ||
      isOnboardingCompleted());
  const currentStep =
    currentStepIndex >= 0 && currentStepIndex < totalSteps
      ? activeFlow[currentStepIndex]
      : null;
  const isActive =
    !bypassOnboarding &&
    !isPaused &&
    !isComplete &&
    isInitialized &&
    manuallyStarted &&
    currentStep !== null;
  const isLoading =
    configLoading ||
    !isInitialized ||
    !initialIndexSet.current ||
    (currentStepIndex === -1 && activeFlow.length > 0);

  useEffect(() => {
    if (isComplete) clearRuntimeStateSession();
  }, [isComplete]);

  const advance = useCallback(() => {
    setCurrentStepIndex((current) => {
      const nextIndex = current + 1;
      if (nextIndex >= totalSteps) markOnboardingCompleted();
      return nextIndex;
    });
  }, [totalSteps]);

  const prev = useCallback(() => {
    setCurrentStepIndex((current) => Math.max(current - 1, 0));
  }, []);

  const skip = useCallback(() => {
    markOnboardingCompleted();
    setCurrentStepIndex(totalSteps);
  }, [totalSteps]);

  const updateRuntimeState = useCallback(
    (updates: Partial<OnboardingRuntimeState>) => {
      persistRuntimeState(updates);
      setRuntimeState((previous) => ({ ...previous, ...updates }));
    },
    [],
  );

  const refreshFlow = useCallback(() => {
    initialIndexSet.current = false;
    setCurrentStepIndex(-1);
  }, []);

  const startStep = useCallback(
    (stepId: OnboardingStepId) => {
      const index = activeFlow.findIndex((step) => step.id === stepId);
      if (index === -1) return;
      setCurrentStepIndex(index);
      setIsPaused(false);
      setManuallyStarted(true);
    },
    [activeFlow],
  );

  return {
    state: {
      isActive,
      currentStep,
      currentStepIndex,
      totalSteps,
      runtimeState,
      activeFlow,
      isComplete,
      isLoading,
    },
    actions: {
      next: advance,
      prev,
      skip,
      complete: advance,
      updateRuntimeState,
      refreshFlow,
      startStep,
      pause: () => setIsPaused(true),
      resume: () => setIsPaused(false),
    },
  };
}
