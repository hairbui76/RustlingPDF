# Desktop: Windows Explorer context menu (cascade quick actions)

Right-clicking one or more `.pdf` files in Windows Explorer shows a
"RustlingPDF" cascade submenu with quick actions that launch/focus the app
with the selected files preloaded into the chosen tool. Windows-only, MSI
registered; Linux/macOS keep their existing file-association "Open With"
behavior unchanged.

## Registry surface (MSI-provisioned)

`frontend/editor/src-tauri/windows/wix/provisioning.wxs` appends five
registry components to `ProvisioningComponentGroup` (all `Guid="*"`, one
`KeyPath="yes"` value each, `HKLM` — the MSI is perMachine):

```
HKLM\SOFTWARE\Classes\SystemFileAssociations\.pdf\shell\RustlingPDF
  MUIVerb     = "RustlingPDF"          (cascade title)
  SubCommands = ""                     (empty ⇒ enumerate the shell subkey — Win7+ registry-only cascade)
  Icon        = "<short path to RustlingPDF.exe>",0
  shell\01_open\      MUIVerb="Open"      MultiSelectModel="Player"
  shell\01_open\command      (Default) = "<exe>" --tool open "%1"
  shell\02_merge\     MUIVerb="Merge"     MultiSelectModel="Player"
  shell\02_merge\command     (Default) = "<exe>" --tool merge "%1"
  shell\03_compress\  MUIVerb="Compress"  MultiSelectModel="Player"
  shell\03_compress\command  (Default) = "<exe>" --tool compress "%1"
  shell\04_convert\   MUIVerb="Convert"   MultiSelectModel="Player"
  shell\04_convert\command   (Default) = "<exe>" --tool convert "%1"
```

- `<exe>` is `[!Path]` in the WXS — the runtime-formatted path of the main
  executable's `File` entry in the bundler-generated `main.wxs`, the same
  reference the bundler's own file-association and deep-link commands use.
- **The verb key and `MUIVerb` are spelled literally in the WXS, not
  `{{product_name}}`.** tauri-bundler renders `main.wxs` through Handlebars but
  hands *fragment* files to candle unrendered, and MSI's `Formatted` parser
  leaves a `{…}` run containing no `[property]` substitution unchanged — so
  v3.1.0, which used the token, shipped a cascade whose key **and whose
  user-visible submenu title** were the raw string `{{product_name}}`. Both are
  now the literal `RustlingPDF`, and the package deletes the v3.1.0 tree at
  install time so an upgraded machine does not end up with a stale mislabelled
  menu beside the correct one. The rename, the component-GUID consequences and
  the upgrade proof are in `desktop-windows-installer.md`. If the product name
  ever changes, this fragment must change with it — nothing substitutes it.
- The `NN_` prefixes order the submenu (enumeration is alphabetical).
- `MultiSelectModel=Player` lifts Explorer's default 15-item multi-select cap
  (`MultipleInvokePromptMinimum`); large selections still work.
- Uninstall removes the whole tree. This is **not** automatic: MSI on its own
  only removes the registry *values* a component wrote, so until
  `ForceDeleteOnUninstall="yes"` was added to the cascade root and to each
  `NN_*` verb key, every one of these keys survived uninstall as an empty
  skeleton. See `desktop-windows-installer.md` for the deletion scopes and why
  the shared `…\.pdf\shell` parent is deliberately left alone.
- MSI upgrades are safe because the pinned UpgradeCode's
  `afterInstallInitialize` major-upgrade removes the old product first and the
  new components are additive with auto-derived GUIDs.

## Action set — single source of truth

The action names form a triple that MUST stay identical, enforced by tests
and review:

1. `frontend/editor/src-tauri/windows/wix/provisioning.wxs` — the
   `--tool <action>` verb commands;
2. `frontend/editor/src-tauri/src/launch_intent.rs` — `TOOL_INTENT_ACTIONS`
   (`["open", "merge", "compress", "convert"]`, unit-tested);
3. `frontend/editor/src/core/services/toolIntentService.ts` —
   `TOOL_INTENT_ACTIONS` (vitest-pinned) plus the intent→tool-route map
   (`open` → no navigation; `merge`/`compress`/`convert` → the same-named SPA
   tool routes).

## Launch intent grammar (Rust, `launch_intent.rs`)

`parse_launch_args` extends the old existing-paths-only CLI parsing:

- `--tool <name>` may appear anywhere between paths; the value is consumed
  (never treated as a path); the last occurrence wins.
- `<name>` outside the allowlist, or a trailing `--tool` with no value,
  degrades the launch to a plain open (`tool: None`) — never an error.
- Every other argument is kept iff it names an existing path, so bare-path
  launches (double-click file association, CLI) parse byte-compatibly with
  the previous behavior.
- macOS `RunEvent::Opened` and in-window drag-drop bypass CLI parsing
  entirely and stay on the intent-less path unchanged.

## Multi-select aggregation

Explorer invokes classic registry verbs once per selected file: N selected
files = N process launches = 1 cold launch and/or N-1 single-instance
callbacks. Intent-carrying launches are buffered per action name in a
process-wide aggregator (`IntentAggregator`) and flushed as ONE batch:

- sliding debounce: a batch flushes once no same-intent launch has arrived
  for 500 ms (`INTENT_DEBOUNCE_WINDOW`);
- hard cap: a batch never waits more than 3 s from its first launch
  (`INTENT_MAX_AGGREGATION`; worst case it flushes within one debounce window
  past the cap while launches keep trickling in);
- distinct intents aggregate independently; generation tokens are monotonic
  across batches so a stale sleeper can never flush a newer batch early;
- worst-case degradation is the same files arriving in several batches —
  never file loss (every launch has exactly one paired flush attempt).

Timing is unit-tested with tokio's paused clock (deterministic).

## Queue and event semantics

The opened-files queue (`commands/files.rs`) now stores
`OpenedFileBatch { paths, tool }` entries. The intent rides in the queue, not
in an event payload, so a cold launch whose webview mounts seconds after the
flush still sees it. Consecutive intent-less adds coalesce into one trailing
batch; intent batches are separate units and are never appended to.

- `pop_opened_batches` (new command) — atomic pop of the batches with
  intents; what the desktop frontend consumes.
- `pop_opened_files` / `get_opened_files` / `clear_opened_files` — kept
  backwards-compatible (flattened paths view, intents dropped).
- The existing `files-changed` event is emitted unchanged (unit payload,
  window-targeted) as a nudge to re-pop the queue; plain opens are
  emit-per-launch exactly as before.

## Frontend routing

There is no separate desktop build layer. `frontend/editor/src/core` is the
only product layer, and this behaviour lives in it behind an explicit runtime
check (`isDesktopRuntime()` in `core/services/desktop/desktopRuntime.ts`, the
single Tauri-detection helper). On web every step below is a no-op.

`core/hooks/useOpenedFile` drains the queue: once on mount, for a cold launch
whose batch was enqueued before the webview existed, and again on each
`files-changed` event emitted at this window, for a warm launch. Batches
accumulate rather than replace, so an event arriving mid-load cannot drop an
earlier launch's files.

`core/hooks/useAppInitialization` — mounted once by `AppProviders` inside
`FileContextProvider` — reads each batch's paths, publishes the
quickKey→path mapping through `pendingFilePathMappings` (which is what
attaches `localFilePath` as `addFiles` builds each stub, the same mechanism
the native open dialog uses), adds the files with `selectFiles: true`, then
calls `navigateToToolIntent(batch.tool)`. That routes a mapped intent to its
tool by pushing the tool's URL and dispatching a synthetic `popstate` — the
exact URL-driven selection path used by browser back/forward
(`useNavigationUrlSync`), including its availability checks. Batches process
sequentially in launch order, so the last intent's navigation wins. `open`,
unknown intents, and `tool: null` add files with no navigation. An unreadable
path is skipped with a log; the rest of its batch still opens.

The frontend consumes only `pop_opened_batches`. `get_opened_files`,
`pop_opened_files` and `clear_opened_files` remain registered and unit-tested
on the Rust side but have no JavaScript caller; that is recorded, with the
reason, in the ledger in
`frontend/editor/src/core/services/desktop/desktopCommands.test.ts`, which
fails if a registered command loses (or silently never had) a caller.

## Caveats

- **Windows 11**: registry verbs under `SystemFileAssociations` appear in the
  legacy context menu — the user must click "Show more options" (or
  Shift+F10). Top-level Win11 placement requires a packaged (MSIX/sparse)
  `IExplorerCommand` extension: documented follow-up, out of v1 scope.
- **NSIS dev builds have no menu**: `task desktop:build:dev:windows` builds
  NSIS, which does not consume `wix.fragmentPaths`. Only the MSI (the release
  bundle target) registers the menu. Not a bug.
- **WiX compile proof**: WiX cannot compile on Linux; the fragment is
  XML-validated against the WiX 3.14.1 `wix.xsd` locally, the MSI build on the
  `windows-latest` CI leg is the compile gate, and
  `windows/scripts/verify-msi-lifecycle.ps1` on that same leg installs and
  uninstalls the result and asserts these keys appear and then disappear.
- **The fragment is NOT Handlebars-rendered.** An earlier revision of this
  contract claimed it was; the bundler renders fragments only into a throwaway
  string used to detect `xmlns` extensions and compiles the original file
  (tauri-bundler 2.9.4, `bundle/windows/msi/mod.rs`). Anything spelled
  `{{name}}` in this fragment reaches the registry verbatim.
