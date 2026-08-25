import { useCallback, useState } from "react";
import {
  checkForDesktopUpdate,
  installDesktopUpdate,
  type DesktopUpdateInfo,
  type DesktopUpdatePhase,
} from "@app/services/desktop/desktopUpdater";

/**
 * Where a check has got to. `upToDate` exists only so a *manual* check can say
 * so: the startup check stays silent when there is nothing to offer, but a
 * user who presses a button has asked a question and deserves an answer.
 */
export type DesktopUpdateStatus =
  "idle" | "checking" | "available" | "upToDate";

export interface DesktopUpdateState {
  /** The offered update, once a check has found one. */
  update: DesktopUpdateInfo | null;
  status: DesktopUpdateStatus;
  /** Non-null while an install runs. */
  phase: DesktopUpdatePhase | null;
  /** The reason both install attempts failed, ready to show. */
  failure: string | null;
  /** A check or install is in flight; disable the controls that start one. */
  busy: boolean;
  check: () => Promise<void>;
  install: () => void;
}

/**
 * The desktop update state machine, shared by the startup banner and the
 * manual check in Settings → General so the two cannot drift apart.
 *
 * The service underneath keeps the pending-update handle, so an update found
 * by either caller is installable from either one.
 */
export function useDesktopUpdate(): DesktopUpdateState {
  const [update, setUpdate] = useState<DesktopUpdateInfo | null>(null);
  const [status, setStatus] = useState<DesktopUpdateStatus>("idle");
  const [phase, setPhase] = useState<DesktopUpdatePhase | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  const check = useCallback(async () => {
    setStatus("checking");
    setFailure(null);
    // checkForDesktopUpdate never rejects — it reports "nothing to offer" for
    // an offline machine and for installs that cannot update in place, which
    // is indistinguishable from being current as far as this hook can act.
    const info = await checkForDesktopUpdate();
    setUpdate(info);
    setStatus(info ? "available" : "upToDate");
  }, []);

  const install = useCallback(() => {
    setFailure(null);
    setPhase("downloading");
    installDesktopUpdate(setPhase).catch((error: unknown) => {
      console.error("[useDesktopUpdate] Install failed:", error);
      // The service already retried, so this is the final word. Naming the
      // reason beats a bare "could not be installed" for a packaged app whose
      // user has no devtools console to consult.
      setFailure(error instanceof Error ? error.message : String(error));
      setPhase(null);
    });
  }, []);

  return {
    update,
    status,
    phase,
    failure,
    busy: status === "checking" || (phase !== null && failure === null),
    check,
    install,
  };
}
