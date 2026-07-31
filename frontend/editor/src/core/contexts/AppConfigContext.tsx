import React, {
  createContext,
  useContext,
  useMemo,
  useState,
  useEffect,
  ReactNode,
  useCallback,
} from "react";
import apiClient from "@app/services/apiClient";
import type { AppConfig, AppConfigBootstrapMode } from "@app/types/appConfig";

/**
 * Sleep utility for delays
 */
function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export interface AppConfigRetryOptions {
  maxRetries?: number;
  initialDelay?: number;
}

export type { AppConfig, AppConfigBootstrapMode };

interface AppConfigContextValue {
  config: AppConfig | null;
  loading: boolean;
  error: string | null;
  refetch: () => Promise<void>;
}

const AppConfigContext = createContext<AppConfigContextValue | undefined>({
  config: null,
  loading: true,
  error: null,
  refetch: async () => {},
});

/**
 * Provider component that fetches and provides app configuration
 * Should be placed at the top level of the app, before any components that need config
 */
export interface AppConfigProviderProps {
  children: ReactNode;
  retryOptions?: AppConfigRetryOptions;
  initialConfig?: AppConfig | null;
  bootstrapMode?: AppConfigBootstrapMode;
  autoFetch?: boolean;
  onConfigLoaded?: (config: AppConfig) => void;
}

export const AppConfigProvider: React.FC<AppConfigProviderProps> = ({
  children,
  retryOptions,
  initialConfig = null,
  bootstrapMode = "blocking",
  autoFetch = true,
  onConfigLoaded,
}) => {
  const isBlockingMode = bootstrapMode === "blocking";
  const [config, setConfig] = useState<AppConfig | null>(initialConfig);
  const [error, setError] = useState<string | null>(null);
  // Track how many times we've attempted to fetch. useRef avoids re-renders that can trigger loops.
  const fetchCountRef = React.useRef(0);
  const [hasResolvedConfig, setHasResolvedConfig] = useState(
    Boolean(initialConfig) && !isBlockingMode,
  );
  const [loading, setLoading] = useState(!hasResolvedConfig);

  const onConfigLoadedRef = React.useRef(onConfigLoaded);
  onConfigLoadedRef.current = onConfigLoaded;

  const maxRetries = retryOptions?.maxRetries ?? 0;
  const initialDelay = retryOptions?.initialDelay ?? 1000;

  const fetchConfig = useCallback(
    async (force = false) => {
      // Prevent duplicate fetches unless forced
      if (!force && fetchCountRef.current > 0) {
        console.debug("[AppConfig] Already fetched, skipping");
        return;
      }

      // Mark that we've attempted a fetch to prevent repeated auto-fetch loops
      fetchCountRef.current += 1;

      const shouldBlockUI = !hasResolvedConfig || isBlockingMode;
      if (shouldBlockUI) {
        setLoading(true);
      }
      setError(null);

      const startTime = performance.now();
      for (let attempt = 0; attempt <= maxRetries; attempt++) {
        try {
          if (attempt > 0) {
            const delay = initialDelay * Math.pow(2, attempt - 1);
            console.debug(
              `[AppConfig] Retry attempt ${attempt}/${maxRetries} after ${delay}ms delay...`,
            );
            await sleep(delay);
          }

          console.debug("[AppConfig] Fetching app config", {
            attempt,
            force,
            path: window.location.pathname,
          });
          const response = await apiClient.get<AppConfig>(
            "/api/v1/config/app-config",
            {
              suppressErrorToast: true,
            } as any,
          );
          const data = response.data;

          console.debug("[AppConfig] Config fetched successfully:", data);
          console.debug(
            "[AppConfig] Fetch duration ms:",
            (performance.now() - startTime).toFixed(2),
          );
          setConfig(data);
          setHasResolvedConfig(true);
          setLoading(false);
          onConfigLoadedRef.current?.(data);
          return; // Success - exit function
        } catch (err: any) {
          const status = err?.response?.status;

          // Check if we should retry (network errors or 5xx errors)
          const shouldRetry =
            (!status || status >= 500) && attempt < maxRetries;

          if (shouldRetry) {
            console.debug(
              `[AppConfig] Attempt ${attempt + 1} failed (status ${status || "network error"}):`,
              err.message,
              "- will retry...",
            );
            continue;
          }

          // Final attempt failed or non-retryable error (4xx)
          const errorMessage =
            err?.response?.data?.message ||
            err?.message ||
            "Unknown error occurred";
          setError(errorMessage);
          console.error(
            `[AppConfig] Failed to fetch app config after ${attempt + 1} attempts:`,
            err,
          );
          console.debug(
            "[AppConfig] Fetch duration ms:",
            (performance.now() - startTime).toFixed(2),
          );
          // Preserve an initial or previously fetched configuration.
          setConfig((current) => current ?? {});
          setHasResolvedConfig(true);
          break;
        }
      }

      setLoading(false);
    },
    [hasResolvedConfig, isBlockingMode, maxRetries, initialDelay],
  );

  useEffect(() => {
    if (autoFetch) {
      fetchConfig();
    }
  }, [autoFetch, fetchConfig]);

  const refetch = useCallback(() => fetchConfig(true), [fetchConfig]);

  const value = useMemo<AppConfigContextValue>(
    () => ({
      config,
      loading,
      error,
      refetch,
    }),
    [config, loading, error, refetch],
  );

  return (
    <AppConfigContext.Provider value={value}>
      {children}
    </AppConfigContext.Provider>
  );
};

/**
 * Hook to access application configuration
 * Must be used within AppConfigProvider
 */
export function useAppConfig(): AppConfigContextValue {
  const context = useContext(AppConfigContext);

  if (context === undefined) {
    throw new Error("useAppConfig must be used within AppConfigProvider");
  }

  return context;
}
