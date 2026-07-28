import { useCallback, useEffect, useState } from "react";
import { BASE_PATH } from "@app/constants/app";

type BackendStatus = "up" | "starting" | "down";

interface BackendProbeState {
  status: BackendStatus;
  loading: boolean;
}

/**
 * Lightweight backend probe that avoids global axios interceptors.
 * Used on the landing screen to decide whether the backend is reachable.
 */
export function useBackendProbe() {
  const [state, setState] = useState<BackendProbeState>({
    status: "starting",
    loading: true,
  });

  const probe = useCallback(async () => {
    const statusUrl = `${BASE_PATH || ""}/api/v1/info/status`;

    const next: BackendProbeState = {
      status: "starting",
      loading: false,
    };

    try {
      const res = await fetch(statusUrl, { method: "GET", cache: "no-store" });
      if (res.ok) {
        const data = await res.json().catch(() => null);
        next.status = data && data.status === "UP" ? "up" : "starting";
      } else if (res.status === 404 || res.status === 503) {
        next.status = "starting";
      } else {
        next.status = "down";
      }
    } catch {
      next.status = "down";
    }

    setState(next);
    return next;
  }, []);

  useEffect(() => {
    void probe();
  }, [probe]);

  return {
    ...state,
    probe,
  };
}
