import { useCallback, useEffect, useState } from "react";
import { backendHealthMonitor } from "@app/services/backendHealthMonitor";
import type { BackendHealthState } from "@app/types/backendHealth";

export interface UseBackendHealthResult extends BackendHealthState {
  /** Force an immediate probe instead of waiting for the next poll. */
  checkHealth: () => Promise<boolean>;
}

/**
 * Health of the backend the app is talking to.
 *
 * Web: constant `healthy` (the page was served by that backend).
 * Desktop: live state of the bundled sidecar — see `backendHealthMonitor`.
 */
export function useBackendHealth(): UseBackendHealthResult {
  const [health, setHealth] = useState<BackendHealthState>(() =>
    backendHealthMonitor.getSnapshot(),
  );

  useEffect(() => backendHealthMonitor.subscribe(setHealth), []);

  const checkHealth = useCallback(() => backendHealthMonitor.checkNow(), []);

  return { ...health, checkHealth };
}
