import { useEffect } from "react";
import { useAppConfig } from "@app/contexts/AppConfigContext";
import HomePage from "@app/pages/HomePage";
import { useBackendProbe } from "@app/hooks/useBackendProbe";
import { useTranslation } from "react-i18next";
import { Button } from "@app/ui/Button";

/**
 * Landing component. The app has no login: every visitor lands on the
 * HomePage. The only special case is a backend that is not reachable yet,
 * which shows a branded status screen that auto-retries.
 */
export default function Landing() {
  const { config, loading: configLoading, refetch } = useAppConfig();
  const backendProbe = useBackendProbe();
  const { t } = useTranslation();

  const loading = configLoading || backendProbe.loading;

  // Periodically probe while the backend isn't up so the screen can
  // auto-advance when it comes online.
  useEffect(() => {
    if (backendProbe.status === "up") {
      return;
    }
    const tick = async () => {
      const result = await backendProbe.probe();
      if (result.status === "up") {
        await refetch();
      }
    };
    const intervalId = window.setInterval(() => {
      void tick();
    }, 5000);
    return () => window.clearInterval(intervalId);
  }, [backendProbe.status, backendProbe.probe, backendProbe, refetch]);

  useEffect(() => {
    if (backendProbe.status === "up") {
      void refetch();
    }
  }, [backendProbe.status, refetch]);

  // Show loading while resolving the app config
  if (loading) {
    return (
      <div
        style={{
          minHeight: "100vh",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <div className="text-center">
          <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-blue-600 mx-auto mb-3"></div>
          <div className="text-gray-600">
            {t("common.loading", "Loading...")}
          </div>
        </div>
      </div>
    );
  }

  // Backend not reachable and no config either: show a branded status screen
  if (!config && backendProbe.status !== "up") {
    const handleRetry = async () => {
      const result = await backendProbe.probe();
      if (result.status === "up") {
        await refetch();
      }
    };
    return (
      <div
        style={{
          minHeight: "100vh",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <div
          style={{
            padding: "1.5rem",
            marginTop: "1rem",
            borderRadius: "0.75rem",
            maxWidth: "28rem",
            backgroundColor:
              "color-mix(in srgb, var(--c-primary) 8%, transparent)",
            border:
              "1px solid color-mix(in srgb, var(--c-primary) 20%, transparent)",
          }}
        >
          <h2
            style={{ margin: "0 0 0.75rem 0", color: "var(--c-text)" }}
            className="text-lg font-semibold"
          >
            {t("backendStartup.notFoundTitle", "Backend not found")}
          </h2>
          <p style={{ margin: "0 0 0.75rem 0", color: "var(--c-text)" }}>
            {t(
              "backendStartup.unreachable",
              "The application cannot currently connect to the backend. Verify the backend status and network connectivity, then try again.",
            )}
          </p>
          <Button
            type="button"
            onClick={handleRetry}
            className="px-4 py-[0.75rem] rounded-[0.625rem] text-base font-semibold mt-5 border-0 cursor-pointer"
            style={{ width: "fit-content" }}
          >
            {t("backendStartup.retry", "Retry")}
          </Button>
        </div>
      </div>
    );
  }

  return <HomePage />;
}
