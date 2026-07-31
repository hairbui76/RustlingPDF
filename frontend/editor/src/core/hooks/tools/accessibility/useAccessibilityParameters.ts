import { BaseParameters } from "@app/types/parameters";
import {
  BaseParametersHook,
  useBaseParameters,
} from "@app/hooks/tools/shared/useBaseParameters";

export interface AccessibilityParameters extends BaseParameters {}

export const defaultParameters: AccessibilityParameters = {};

export type AccessibilityParametersHook =
  BaseParametersHook<AccessibilityParameters>;

export const useAccessibilityParameters = (): AccessibilityParametersHook =>
  useBaseParameters({
    defaultParameters,
    endpointName: "check",
  });
