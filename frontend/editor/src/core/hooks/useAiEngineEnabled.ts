import { useAppConfig } from "@app/contexts/AppConfigContext";

/**
 * Whether the AI engine is enabled, per the backend's app-config.
 *
 * Read straight from the app-config, which already comes from whichever
 * backend the app talks to — the same value in every runtime, since desktop's
 * bundled sidecar reports its own AI configuration like any other backend.
 */
export function useAiEngineEnabled(): boolean {
  const { config } = useAppConfig();
  return Boolean(config?.aiEngineEnabled);
}
