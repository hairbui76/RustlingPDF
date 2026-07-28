import { invoke } from "@tauri-apps/api/core";

/**
 * Desktop connection handling. The app always talks to the bundled local
 * backend (the sidecar) — there are no remote accounts or servers. What
 * remains of the old multi-mode service is the first-launch bookkeeping the
 * Tauri store still owns.
 */
export type ConnectionMode = "local";

export interface ConnectionConfig {
  mode: ConnectionMode;
}

export const LOCAL_MODE_STORAGE_KEY = "stirling-local-mode";

export class ConnectionModeService {
  private static instance: ConnectionModeService;

  static getInstance(): ConnectionModeService {
    if (!ConnectionModeService.instance) {
      ConnectionModeService.instance = new ConnectionModeService();
    }
    return ConnectionModeService.instance;
  }

  async getCurrentConfig(): Promise<ConnectionConfig> {
    return { mode: "local" };
  }

  async getCurrentMode(): Promise<ConnectionMode> {
    return "local";
  }

  /** The mode can never change; the unsubscribe is a no-op. */
  subscribeToModeChanges(
    _listener: (config: ConnectionConfig) => void,
  ): () => void {
    return () => {};
  }

  /** Mark first-launch setup as done so onboarding bootstrap runs only once. */
  async completeSetup(): Promise<void> {
    try {
      await invoke("complete_setup");
    } catch (error) {
      console.error("Failed to mark setup as completed:", error);
    }
  }

  async isFirstLaunch(): Promise<boolean> {
    try {
      return await invoke<boolean>("is_first_launch");
    } catch {
      return false;
    }
  }

  async resetSetupCompletion(): Promise<void> {
    try {
      await invoke("reset_setup_completion");
    } catch (error) {
      console.error("Failed to reset setup completion:", error);
    }
  }
}

export const connectionModeService = ConnectionModeService.getInstance();
