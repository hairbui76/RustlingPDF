import { describe, expect, it } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

/**
 * Regression guard for a defect that was green on every gate and dead on every
 * real bundle.
 *
 * Tauri v2 authorises each *plugin* command by a permission identifier listed
 * in a capability file. `writeFile` needs `fs:allow-write-file`, `rename`
 * needs `fs:allow-rename`, and so on. Calling one that is not granted is
 * refused at the IPC layer — at runtime, in a packaged app only.
 *
 * That is invisible to the rest of the suite because every test mocks
 * `@tauri-apps/*`. A change adding `stat` and `rename` to the save path
 * therefore passed 912 tests, a clean typecheck, lint and build, while making
 * Save completely non-functional on the shipped desktop app and silently
 * disabling the file-identity fix it was written to deliver.
 *
 * This test closes the loop statically: whatever the bridge imports must be
 * granted in `capabilities/default.json`.
 *
 * WHAT THIS CATCHES
 * - A plugin API used by the bridge with no matching grant — the exact defect
 *   above, for `@tauri-apps/plugin-*` packages.
 * - A grant that silently stops covering its API because a plugin upgrade
 *   renamed the underlying command: the command name is read out of the
 *   *installed* plugin at test time, not hardcoded here.
 * - A bridge API this test cannot resolve to a command at all, which fails
 *   loudly rather than passing by omission.
 *
 * WHAT THIS DOES NOT CATCH
 * - **Scope.** It checks that an identifier is granted, not that the path
 *   being touched falls inside that identifier's `allow` globs. `rename`
 *   resolves *both* its arguments against the scope, so a narrower scope than
 *   `**` could still refuse a save that this test calls fine.
 * - `@tauri-apps/api/*` core APIs (`path`, `event`, `webviewWindow`). They are
 *   covered by `core:default` rather than per-command grants, so they are
 *   listed in CORE_APIS_COVERED_BY_DEFAULT below and checked only for being
 *   known — a genuine residual gap, recorded rather than papered over.
 * - Anything outside `core/services/desktop/**`, which the ESLint carve-out
 *   and `desktopCommands.test.ts` already keep empty of Tauri imports.
 * - Whether a granted command actually works on a given OS.
 */

const HERE = dirname(fileURLToPath(import.meta.url));
const EDITOR_ROOT = join(HERE, "..", "..", "..", "..");
const CAPABILITY_FILE = join(
  EDITOR_ROOT,
  "src-tauri",
  "capabilities",
  "default.json",
);
const NODE_MODULES = join(EDITOR_ROOT, "..", "node_modules");

/**
 * Core APIs that `core:default` authorises. Verified against tauri 2.11.5's
 * `build.rs` PLUGINS table, where `core:path`'s `join` and `core:event`'s
 * `listen`/`unlisten` are all flagged enabled-by-default.
 */
const CORE_APIS_COVERED_BY_DEFAULT = new Set([
  "@tauri-apps/api/core:invoke",
  "@tauri-apps/api/path:join",
  "@tauri-apps/api/webviewWindow:getCurrentWebviewWindow",
]);

function bridgeSources(): { path: string; text: string }[] {
  const out: { path: string; text: string }[] = [];
  for (const entry of readdirSync(HERE)) {
    const full = join(HERE, entry);
    if (statSync(full).isDirectory()) continue;
    if (!/\.tsx?$/.test(entry) || /\.test\.tsx?$/.test(entry)) continue;
    out.push({ path: full, text: readFileSync(full, "utf8") });
  }
  return out;
}

/** `const { a, b } = await import("@tauri-apps/plugin-fs")` and static forms. */
function extractTauriApiUsage(text: string): Map<string, Set<string>> {
  const usage = new Map<string, Set<string>>();
  const add = (pkg: string, names: string) => {
    const set = usage.get(pkg) ?? new Set<string>();
    for (const raw of names.split(",")) {
      const name = raw
        .trim()
        .split(/\s+as\s+/)[0]
        .trim();
      if (name) set.add(name);
    }
    usage.set(pkg, set);
  };

  const dynamic =
    /const\s*\{([^}]*)\}\s*=\s*await\s+import\(\s*["'](@tauri-apps\/[^"']+)["']\s*\)/g;
  for (const m of text.matchAll(dynamic)) add(m[2], m[1]);

  const staticImport =
    /import\s*\{([^}]*)\}\s*from\s*["'](@tauri-apps\/[^"']+)["']/g;
  for (const m of text.matchAll(staticImport)) add(m[2], m[1]);

  return usage;
}

/**
 * Which Rust command a plugin's JS export invokes, read from the installed
 * package. Hardcoding this map would defeat half the point: a plugin upgrade
 * that renames a command has to show up here, not in a bug report.
 */
function commandForPluginApi(pkg: string, api: string): string | null {
  const source = readFileSync(
    join(NODE_MODULES, pkg, "dist-js", "index.js"),
    "utf8",
  );
  // Body of `async function <api>(...)` up to the next top-level declaration.
  const start = source.search(new RegExp(`async function ${api}\\s*\\(`, "m"));
  if (start === -1) return null;
  const rest = source.slice(start);
  const end = rest.slice(1).search(/^(async function|function|class|const) /m);
  const body = end === -1 ? rest : rest.slice(0, end + 1);
  const invoked = body.match(/invoke\(\s*['"]plugin:([a-z-]+)\|([a-z_]+)['"]/);
  return invoked ? invoked[2] : null;
}

/** Autogenerated per-command permissions are always `allow-<kebab command>`. */
function identifierFor(pkg: string, command: string): string {
  const plugin = pkg.replace("@tauri-apps/plugin-", "");
  return `${plugin}:allow-${command.replace(/_/g, "-")}`;
}

function grantedIdentifiers(): Set<string> {
  const capability = JSON.parse(readFileSync(CAPABILITY_FILE, "utf8")) as {
    permissions: Array<string | { identifier: string }>;
  };
  return new Set(
    capability.permissions.map((permission) =>
      typeof permission === "string" ? permission : permission.identifier,
    ),
  );
}

describe("desktop plugin permissions", () => {
  const sources = bridgeSources();
  const granted = grantedIdentifiers();

  it("finds the bridge sources and the capability file", () => {
    // Without this, every assertion below passes vacuously.
    expect(sources.length).toBeGreaterThan(2);
    expect(granted.size).toBeGreaterThan(5);
    expect(granted).toContain("fs:allow-read-file");
  });

  it("every plugin API the bridge uses is explicitly granted", () => {
    const missing: string[] = [];
    const unresolved: string[] = [];

    for (const source of sources) {
      for (const [pkg, apis] of extractTauriApiUsage(source.text)) {
        if (!pkg.startsWith("@tauri-apps/plugin-")) continue;
        for (const api of apis) {
          const command = commandForPluginApi(pkg, api);
          if (!command) {
            unresolved.push(
              `${relative(EDITOR_ROOT, source.path)}: ${pkg}.${api}`,
            );
            continue;
          }
          const identifier = identifierFor(pkg, command);
          if (!granted.has(identifier)) {
            missing.push(
              `${identifier} (needed by ${pkg}.${api}, used in ${relative(EDITOR_ROOT, source.path)})`,
            );
          }
        }
      }
    }

    expect(
      unresolved,
      "Could not resolve these bridge APIs to a Tauri command, so their " +
        "permission cannot be checked. Fix the resolver rather than removing " +
        "the check — an unresolvable API is how this class of bug hides.",
    ).toEqual([]);

    expect(
      missing,
      "These permission identifiers are missing from " +
        "src-tauri/capabilities/default.json. The calls are refused at the IPC " +
        "layer in a packaged app, and nothing else in this suite can see that " +
        "because @tauri-apps is mocked everywhere. Grant them explicitly — " +
        "relying on a `<plugin>:default` set is not accepted here, because a " +
        "set's contents are invisible from this side of the build.",
    ).toEqual([]);
  });

  it("every core API the bridge uses is a known core:default command", () => {
    // Core APIs have no per-command grant to check, so the guarantee is
    // weaker: this only asserts that no *unreviewed* core API creeps in.
    expect(granted).toContain("core:default");

    const unreviewed: string[] = [];
    for (const source of sources) {
      for (const [pkg, apis] of extractTauriApiUsage(source.text)) {
        if (pkg.startsWith("@tauri-apps/plugin-")) continue;
        for (const api of apis) {
          if (!CORE_APIS_COVERED_BY_DEFAULT.has(`${pkg}:${api}`)) {
            unreviewed.push(`${pkg}:${api}`);
          }
        }
      }
    }

    expect(
      unreviewed,
      "These @tauri-apps/api usages have not been checked against what " +
        "core:default authorises. Confirm the command is enabled-by-default in " +
        "tauri's build.rs PLUGINS table, then add it to " +
        "CORE_APIS_COVERED_BY_DEFAULT.",
    ).toEqual([]);
  });

  it("filesystem grants cover the whole filesystem", () => {
    // `rename` resolves BOTH of its paths against the scope, and an in-place
    // save renames a temp file over the user's document. A narrower scope
    // would refuse saves for paths outside it, which the identifier check
    // above cannot see.
    const capability = JSON.parse(readFileSync(CAPABILITY_FILE, "utf8")) as {
      permissions: Array<
        string | { identifier: string; allow?: Array<{ path: string }> }
      >;
    };

    const fsGrants = capability.permissions.filter(
      (
        permission,
      ): permission is { identifier: string; allow?: { path: string }[] } =>
        typeof permission !== "string" &&
        permission.identifier.startsWith("fs:"),
    );
    expect(fsGrants.length).toBeGreaterThan(0);

    const unscoped = fsGrants
      .filter((grant) => !grant.allow?.some((entry) => entry.path === "**"))
      .map((grant) => grant.identifier);

    expect(
      unscoped,
      "Every fs grant needs the `**` scope. The app opens and saves whatever " +
        "the user picks in a native dialog, so a narrower scope refuses real " +
        "documents; and `rename` checks its source AND destination, so a " +
        "partial scope breaks in-place save specifically.",
    ).toEqual([]);
  });
});
