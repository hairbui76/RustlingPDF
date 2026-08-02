/**
 * The desktop (Tauri) bridge.
 *
 * `frontend/editor/src/core` is the only product layer: there is no second
 * build mode and no path-alias override tree. Desktop behaviour therefore
 * lives in `core` behind an explicit runtime check, and every `@tauri-apps/*`
 * module is reached through a *dynamic* import from this directory only —
 * enforced by the ESLint `no-restricted-imports` carve-out that names
 * `core/services/desktop/**` and by `desktopCommands.test.ts`.
 *
 * Consequences of that shape, all deliberate:
 *
 * - The web bundle never evaluates a Tauri module. Rollup emits the dynamic
 *   imports as separate chunks that are only fetched if a call actually
 *   happens, and no call happens because every entry point here returns early
 *   when {@link isDesktopRuntime} is false.
 * - There is exactly one definition of "am I in Tauri?". Ad-hoc re-tests
 *   (`"__TAURI__" in window`, a build-time flag, an env var) are what let the
 *   previous desktop layer disagree with itself.
 */

/**
 * Globals the Tauri v2 webview injects before any application script runs.
 *
 * `window.isTauri` is what `@tauri-apps/api`'s own `isTauri()` reads;
 * `window.__TAURI_INTERNALS__` is the IPC surface and is present in every
 * Tauri v2 webview regardless of the `withGlobalTauri` setting. Both are
 * checked so a change to either injection keeps detection working — a false
 * negative here silently downgrades the desktop app to a web app in a window,
 * which is precisely the regression this module exists to prevent.
 */
interface TauriInjectedGlobals {
  isTauri?: boolean;
  __TAURI_INTERNALS__?: unknown;
}

/** True when running inside the Tauri desktop shell. The only such check. */
export function isDesktopRuntime(): boolean {
  if (typeof window === "undefined") {
    return false;
  }
  const injected = window as unknown as TauriInjectedGlobals;
  return (
    injected.isTauri === true ||
    (injected.__TAURI_INTERNALS__ !== undefined &&
      injected.__TAURI_INTERNALS__ !== null)
  );
}

/**
 * Every Tauri command this frontend invokes, keyed by a stable identifier.
 *
 * This is the JavaScript half of the command contract with
 * `frontend/editor/src-tauri/src/lib.rs`'s `invoke_handler!`. Adding a command
 * on the Rust side without adding it here — or leaving an entry here with no
 * call site — fails `desktopCommands.test.ts`.
 */
export const DESKTOP_COMMANDS = {
  /** Current loopback port of the bundled processing sidecar, if it is up. */
  getBackendPort: "get_backend_port",
  /** Atomic pop of the queued opened-file batches (paths + tool intent). */
  popOpenedBatches: "pop_opened_batches",
} as const;

export type DesktopCommandName =
  (typeof DESKTOP_COMMANDS)[keyof typeof DESKTOP_COMMANDS];

/** Thrown when a desktop-only command is invoked outside the desktop shell. */
export class DesktopRuntimeUnavailableError extends Error {
  constructor(command: string) {
    super(`Desktop command "${command}" is not available in this runtime`);
    this.name = "DesktopRuntimeUnavailableError";
  }
}

/**
 * Invoke a Tauri command. Rejects with {@link DesktopRuntimeUnavailableError}
 * on web *before* touching `@tauri-apps/api`, so the web bundle never loads it.
 */
export async function desktopInvoke<T>(
  command: DesktopCommandName,
  args?: Record<string, unknown>,
): Promise<T> {
  if (!isDesktopRuntime()) {
    throw new DesktopRuntimeUnavailableError(command);
  }
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(command, args);
}

/**
 * Listen for an event emitted at *this* webview window.
 *
 * The Rust side emits with `window.emit` / `app.emit_to(label, ...)`, so each
 * window sees only its own events; listening on the global event bus instead
 * would make every window react to every other window's queue.
 *
 * Resolves to an unlisten function — a no-op on web, so callers can always
 * call it from a cleanup path without branching.
 */
export async function listenOnDesktopWindow(
  event: string,
  handler: () => void,
): Promise<() => void> {
  if (!isDesktopRuntime()) {
    return () => {};
  }
  const { getCurrentWebviewWindow } =
    await import("@tauri-apps/api/webviewWindow");
  return getCurrentWebviewWindow().listen(event, () => handler());
}
