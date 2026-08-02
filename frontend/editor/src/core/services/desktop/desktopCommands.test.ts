import { describe, expect, it } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { DESKTOP_COMMANDS } from "@app/services/desktop/desktopRuntime";

/**
 * Regression guard for the class of failure that shipped as v0.0.1.
 *
 * A cleanup commit deleted `frontend/editor/src/desktop/` — a build-mode
 * override tree whose files silently shadowed their `core` namesakes. Every
 * override fell back to its core version, most of which were empty stubs, and
 * *nothing failed*: types checked, lint passed, tests passed, the bundle
 * built. The result was a desktop app in which all 19 Tauri commands
 * registered in `src-tauri/src/lib.rs` had no JavaScript caller at all.
 *
 * These tests make the JS↔Rust command contract explicit and checkable.
 *
 * WHAT THIS CATCHES
 * - A command registered in Rust that no JS code calls, and is not knowingly
 *   listed below (the exact failure that shipped — the whole frontend half
 *   disappearing takes every entry out of DESKTOP_COMMANDS at once).
 * - A DESKTOP_COMMANDS entry that no call site uses, i.e. the plumbing exists
 *   but nothing is wired to it.
 * - A DESKTOP_COMMANDS name Rust does not register (a typo, or a Rust-side
 *   rename that did not reach the frontend).
 * - A stale ledger entry: a command listed as uncalled that has since gained a
 *   caller. The ledger has to shrink as behaviour is restored, so it cannot
 *   quietly become a list of everything.
 *
 * WHAT THIS DOES NOT CATCH
 * - Whether a call site is *reachable*. A `desktopInvoke` inside a component
 *   that is never rendered, or behind a condition that is never true, counts
 *   as a caller here.
 * - Whether the arguments, the return shape, or the semantics are right. The
 *   Tauri IPC is untyped across the boundary; only the command *name* is
 *   compared.
 * - Whether the command works at runtime. Nothing here starts a webview.
 * - Rust commands registered somewhere other than the single
 *   `generate_handler!` in `lib.rs` (a plugin's own commands, for instance).
 * - Any desktop behaviour that does not go through a command at all — event
 *   listeners, `@tauri-apps/plugin-fs`, `-dialog`, `-http`. Those are covered
 *   only by the runtime-branch unit tests next to this file.
 */

const HERE = dirname(fileURLToPath(import.meta.url));
const CORE_SRC = join(HERE, "..", "..");
const LIB_RS = join(CORE_SRC, "..", "..", "src-tauri", "src", "lib.rs");

/**
 * Commands Rust registers that the frontend knowingly does not call, with the
 * reason. This is a ledger of known-dead surface, not a permanent excuse list:
 * every entry is a feature that was deleted along with `src/desktop` and has
 * not been restored. Restoring one means deleting its line here.
 */
const UNCALLED_BY_DESIGN: Record<string, string> = {
  start_backend:
    "Invoked by the Rust setup hook itself (lib.rs); a second start from JS would race it.",
  get_opened_files:
    "Back-compat flattened view that drops tool intents; the frontend uses pop_opened_batches.",
  pop_opened_files:
    "Back-compat flattened pop that drops tool intents; the frontend uses pop_opened_batches.",
  clear_opened_files:
    "pop_opened_batches is already an atomic take, so nothing needs a separate clear.",
  open_in_new_window:
    "Multi-window UI not restored — see extensions/openInNewWindow.ts.",
  open_files_in_new_window:
    "Multi-window UI not restored — see extensions/openInNewWindow.ts.",
  pop_window_file_ids:
    "Multi-window UI not restored — see extensions/openInNewWindow.ts.",
  get_tauri_logs: "Desktop diagnostics view not restored.",
  is_default_pdf_handler: "Default-PDF-handler settings UI not restored.",
  set_as_default_pdf_handler: "Default-PDF-handler settings UI not restored.",
  is_first_launch:
    "Desktop-specific first-launch onboarding modal not restored.",
  complete_setup:
    "Desktop-specific first-launch onboarding modal not restored.",
  reset_setup_completion:
    "Desktop-specific first-launch onboarding modal not restored.",
  proxy_local_pdf_request:
    "Native PDF.js range-request proxy not restored; the viewer reads bytes through FileContext.",
  get_desktop_os: "No frontend behaviour branches on the desktop OS today.",
  // KNOWN USER-VISIBLE GAP, not a clean omission. WorkbenchBar renders a Print
  // button in the viewer that calls the embedpdf print plugin, i.e. the
  // webview's own print path. The deleted layer bridged that to this command
  // precisely because webview printing is unreliable in a Tauri window, so on
  // desktop the button most likely does nothing and says nothing. Restoring
  // the bridge needs a real bundle to verify — printing cannot be exercised
  // on a headless box — so it is recorded here rather than guessed at.
  print_pdf_file_native: "Native print bridge not restored.",
  get_app_version: "Desktop version-info UI not restored.",
};

function rustRegisteredCommands(): string[] {
  const source = readFileSync(LIB_RS, "utf8");
  const match = source.match(/generate_handler!\s*\[([\s\S]*?)\]/);
  if (!match) {
    throw new Error(
      `No generate_handler![...] found in ${LIB_RS}. If the registration moved, ` +
        "point this guard at its new home rather than deleting the guard.",
    );
  }
  return match[1]
    .split(",")
    .map((entry) => entry.replace(/\/\/.*$/gm, "").trim())
    .filter((entry) => /^[a-z_][a-z0-9_]*$/.test(entry));
}

function coreSourceFiles(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      out.push(...coreSourceFiles(full));
    } else if (/\.tsx?$/.test(entry) && !/\.test\.tsx?$/.test(entry)) {
      out.push(full);
    }
  }
  return out;
}

const REGISTRY_FILE = join(HERE, "desktopRuntime.ts");

/** Non-test core sources, excluding the registry itself. */
const callSiteSources = coreSourceFiles(CORE_SRC)
  .filter((file) => file !== REGISTRY_FILE)
  .map((file) => ({ path: file, text: readFileSync(file, "utf8") }));

describe("desktop command contract", () => {
  const rustCommands = rustRegisteredCommands();

  it("reads a plausible command list out of lib.rs", () => {
    // A parse that silently yields nothing would make every assertion below
    // vacuously pass — the exact shape of failure this file exists to prevent.
    expect(rustCommands.length).toBeGreaterThan(10);
    expect(rustCommands).toContain("get_backend_port");
  });

  it("every registered Rust command is either called or knowingly listed", () => {
    const called = new Set<string>(Object.values(DESKTOP_COMMANDS));
    const orphaned = rustCommands.filter(
      (command) => !called.has(command) && !(command in UNCALLED_BY_DESIGN),
    );

    expect(
      orphaned,
      "These Tauri commands are registered in src-tauri/src/lib.rs but nothing " +
        "in the frontend invokes them. Either wire them up through " +
        "DESKTOP_COMMANDS, remove them from the invoke_handler, or add them to " +
        "UNCALLED_BY_DESIGN in this file with the reason.",
    ).toEqual([]);
  });

  it("every DESKTOP_COMMANDS entry is registered in Rust", () => {
    const registered = new Set(rustCommands);
    const unknown = Object.values(DESKTOP_COMMANDS).filter(
      (command) => !registered.has(command),
    );

    expect(
      unknown,
      "These command names are invoked from the frontend but are not in the " +
        "invoke_handler in src-tauri/src/lib.rs, so calling them fails at " +
        "runtime with an unhandled-command error.",
    ).toEqual([]);
  });

  it("every DESKTOP_COMMANDS entry has at least one call site", () => {
    const withoutCallers = Object.keys(DESKTOP_COMMANDS).filter((key) => {
      const reference = `DESKTOP_COMMANDS.${key}`;
      return !callSiteSources.some((file) => file.text.includes(reference));
    });

    expect(
      withoutCallers,
      "These DESKTOP_COMMANDS entries are referenced nowhere outside the " +
        "registry, so the Rust side has a command with no frontend behaviour " +
        "behind it. This is what shipped in v0.0.1 for all 19 commands.",
    ).toEqual([]);
  });

  it("nothing in the uncalled ledger has quietly gained a caller", () => {
    const registered = new Set(rustCommands);
    const stale = Object.keys(UNCALLED_BY_DESIGN).filter(
      (command) => !registered.has(command),
    );
    expect(
      stale,
      "These are listed as knowingly-uncalled commands but Rust no longer " +
        "registers them; drop the ledger entries.",
    ).toEqual([]);

    const nowCalled = Object.keys(UNCALLED_BY_DESIGN).filter((command) =>
      (Object.values(DESKTOP_COMMANDS) as string[]).includes(command),
    );
    expect(
      nowCalled,
      "These commands are now wired up; remove their UNCALLED_BY_DESIGN entries " +
        "so the ledger keeps shrinking as desktop behaviour is restored.",
    ).toEqual([]);
  });

  it("only the desktop bridge directory names a Tauri module", () => {
    const bridgeDir = relative(CORE_SRC, HERE);
    const offenders = callSiteSources
      .filter((file) => /["']@tauri-apps\//.test(file.text))
      .map((file) => relative(CORE_SRC, file.path))
      .filter((file) => !file.startsWith(bridgeDir));

    expect(
      offenders,
      "Tauri modules must only be named inside core/services/desktop/**, and " +
        "only in dynamic imports. A static import anywhere else pulls Tauri " +
        "into the web bundle's entry chunk.",
    ).toEqual([]);
  });

  it("the bridge imports Tauri modules dynamically only", () => {
    const bridgeFiles = callSiteSources.filter((file) =>
      file.path.startsWith(HERE),
    );
    expect(bridgeFiles.length).toBeGreaterThan(0);

    const staticImports = bridgeFiles
      .filter((file) =>
        /^\s*import\s[^\n]*from\s+["']@tauri-apps\//m.test(file.text),
      )
      .map((file) => relative(CORE_SRC, file.path));

    expect(
      staticImports,
      'A static `import ... from "@tauri-apps/..."` is evaluated as soon as ' +
        "the module graph loads, so the web build would download and run Tauri " +
        "code it can never use. Use `await import(...)` behind isDesktopRuntime().",
    ).toEqual([]);
  });
});
