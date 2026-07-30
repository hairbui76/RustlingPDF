# Desktop: Windows installer (MSI) install, maintenance and uninstall

What the Windows installer does to a machine, and — the user-visible promise —
exactly what it takes back off again. Windows-only. The Explorer cascade menu
this installer registers is specified in `desktop-explorer-context-menu.md`; the
app's own startup behaviour in `desktop-native-startup.md`.

## Toolchain (what the behaviour below is derived from)

`bundle.targets` is `["deb","rpm","appimage","dmg","app","msi"]` — **MSI only** on
Windows, no NSIS target. The MSI is produced by tauri-bundler's own WiX template,
which this repository extends through one fragment:

| Layer | Version / file |
| --- | --- |
| CLI | `@tauri-apps/cli` 2.11.4 (pinned in `frontend/package-lock.json`, installed with `npm ci`) |
| Bundler | `tauri-bundler` 2.9.4 |
| Template | `crates/tauri-bundler/src/bundle/windows/msi/main.wxs` |
| Toolset | WiX 3.14.1 RTM (`WIX_URL` in the bundler's `msi/mod.rs` pins `wix3141rtm`, SHA256-verified) |
| Fragment | `frontend/editor/src-tauri/windows/wix/provisioning.wxs` (`wix.fragmentPaths` + `wix.componentGroupRefs`) |

Identifiers that must never be renamed (existing installs key off them): the
bundle identifier `stirling.pdf.dev`, the `Stirling-PDF` app-data directory, and
the pinned WiX `upgradeCode` `3305fba9-7e5e-5c09-bc71-eca0a65f4fee`.

**Fragments are NOT Handlebars-rendered.** The bundler renders `main.wxs` and
writes it out, but for fragment files it renders only into a throwaway string
used to sniff `xmlns` extension namespaces, then hands candle the **original
file** (`msi/mod.rs`: the loop that builds `candle_inputs` pushes `input_path`,
not the rendered text; `run_candle` compiles that path). The fragment's use of
`$(sys.SOURCEFILEDIR)` to locate `stirling-provision.exe` corroborates this — it
resolves to the fragment's own directory in the repo, which is where
`task desktop:provisioner` puts those binaries. Consequences are recorded under
[Known divergences](#known-divergences).

## Install

`msiexec /i RustlingPDF_<version>_x64_en-US.msi` (perMachine — `<Package
InstallScope="perMachine">`; WiX therefore also sets `ALLUSERS=1`).

Optional properties for unattended / MDM deploys, consumed by the deferred
`WriteProvisioningFile*` CustomActions: `STIRLING_SERVER_URL`,
`STIRLING_LOCK_CONNECTION`, `STIRLING_LOGIN_AGREEMENT`, `STIRLING_UPDATE_MODE`.
When any of them is set, `stirling-provision.exe` writes
`%PROGRAMDATA%\Stirling-PDF\stirling-provisioning.json`.

Installed surface:

- `[INSTALLDIR]` (default `%ProgramFiles%\RustlingPDF`): the app executable, the
  `rustling-processing` sidecar, `resources\pdfium\`, `stirling-provision.exe`,
  `stirling_thumbnail_handler.dll`;
- Start Menu folder + shortcut, Desktop shortcut, an "Uninstall RustlingPDF"
  shortcut;
- advertised `.pdf` ProgId / Extension / Verb (the "Open with" association);
- the thumbnail-handler COM registration and the Explorer cascade menu, both
  authored by `provisioning.wxs`;
- `HKCU\Software\RustlingPDF\RustlingPDF` (`InstallDir` plus shortcut markers),
  authored by the bundler template;
- the WebView2 Runtime, if absent, via the template's download-bootstrapper
  CustomAction.

## Re-running the installer: maintenance mode

**The claim holds: running the installer again on a machine that already has
RustlingPDF installed offers to uninstall it.** Evidence, from the exact
versions above:

1. `main.wxs` ends the UI with `<UIRef Id="WixUI_InstallDir" />`.
2. WiX 3.14.1's `WixUI_InstallDir` defines the maintenance sequence
   MaintenanceWelcomeDlg → MaintenanceTypeDlg → VerifyReadyDlg (`Publish`
   rows for `MaintenanceWelcomeDlg/Next`, `MaintenanceTypeDlg/RemoveButton` and
   `MaintenanceTypeDlg/Back` in `WixUI_InstallDir.wxs`). MSI enters this
   sequence whenever the package's ProductCode is already installed.
3. `MaintenanceTypeDlg` disables Change on `ARPNOMODIFY`, Repair on
   `ARPNOREPAIR` and Remove on `ARPNOREMOVE`. `main.wxs` sets
   `ARPNOREPAIR="yes"`, and `WixUI_InstallDir` itself authors
   `<Property Id="ARPNOMODIFY" Value="1" />`; **`ARPNOREMOVE` is set nowhere**.
   So Change and Repair are greyed out (and this dialog set publishes no
   navigation for the Change button at all) while **Remove is enabled** and
   leads to VerifyReadyDlg → uninstall.
4. The same properties reach Add/Remove Programs: the entry shows Uninstall
   with Change and Repair suppressed.

**Same file vs. rebuilt file — this distinction decides who actually sees
maintenance mode.** `main.wxs` declares `<Product Id="*">`, so the ProductCode is
regenerated on every build:

- a **user** re-running *the .msi file they downloaded* runs a package whose
  ProductCode equals the installed one → **maintenance mode**, Remove offered.
  This is the maintainer's expectation and it is satisfied.
- a **developer** who rebuilds the MSI locally at the same version gets a new
  ProductCode. With `<MajorUpgrade Schedule="afterInstallInitialize"
  AllowSameVersionUpgrades="yes"/>` that package is a **major upgrade**, not
  maintenance: it silently uninstalls the installed copy and installs itself.
  Seeing "it just reinstalls" from a rebuilt MSI is therefore *not* evidence
  that maintenance mode is broken.

## Uninstall — what is removed

Triggered by ARP → Uninstall, the Start Menu "Uninstall RustlingPDF" shortcut,
maintenance-mode Remove, or `msiexec /x {ProductCode}`. All four run the same
sequence with `REMOVE="ALL"`.

MSI-tracked, removed by the standard actions:

| Removed | Mechanism |
| --- | --- |
| Everything under `[INSTALLDIR]`, including `resources\pdfium\` | `RemoveFiles` / `RemoveFolders` |
| `[INSTALLDIR]` itself | `<RemoveFolder Id="INSTALLDIR" On="uninstall"/>` — only if it ends up empty |
| Start Menu folder + shortcut, Desktop shortcut, Uninstall shortcut | `RemoveShortcuts` + `RemoveFolder` rows |
| Advertised `.pdf` ProgId / Extension / Verb | `UnregisterProgIdInfo` / `UnregisterExtensionInfo` / `UnregisterClassInfo` |
| Add/Remove Programs entry, cached package in `C:\Windows\Installer` | `UnpublishProduct` |

Authored by `provisioning.wxs` (this is the part that previously leaked — every
key below used to survive uninstall as an empty skeleton, with the CLSID entry
still naming a DLL that no longer existed):

| Removed | Mechanism |
| --- | --- |
| `HKLM\SOFTWARE\Classes\CLSID\{2D2FBE3A-9A88-4308-A52E-7EF63CA7CF48}` **and its whole subtree**, i.e. `InprocServer32` with it | `ForceDeleteOnUninstall="yes"` on the CLSID key |
| `HKLM\SOFTWARE\Classes\.pdf\shellex\{E357FCCD-A995-4576-B01F-234630154E96}` | `ForceDeleteOnUninstall="yes"` on that node only |
| `HKLM\SOFTWARE\Classes\SystemFileAssociations\.pdf\shell\RustlingPDF` **and its whole subtree** (`shell\01_open…04_convert\command`) | `ForceDeleteOnUninstall="yes"` on the cascade root, and again on each `NN_*` verb key so no component depends on another's removal |
| `HKLM\SOFTWARE\Classes\SystemFileAssociations\.pdf\shell\{{product_name}}` — the v3.1.0 cascade tree — **removed at install time**, not uninstall | `<RemoveRegistryKey Action="removeOnInstall">`; see [Upgrading from v3.1.0](#upgrading-from-v310) |
| `HKCU\Software\RustlingPDF\RustlingPDF` (the bundler template's own key, which the template only ever empties) | `ForceDeleteOnUninstall="yes"` on the dedicated per-user `TemplateRegistryCleanupComponent` — see [Per-user data must live in its own component](#per-user-data-must-live-in-its-own-component-ice57) |
| `%PROGRAMDATA%\Stirling-PDF\stirling-provisioning.json` (and the `%APPDATA%` path if the install scope ever becomes per-user) | deferred `RemoveProvisioningFile*` CustomActions |

`ForceDeleteOnUninstall="yes"` compiles (WiX 3.14.1 `Compiler.cs`) to a Registry
row with `Name="-"` and a null value — MSI's "delete this key, with all its
values and subkeys, when the component is removed". It is a no-op at install
time.

### Why those deletion scopes and no wider

Force-deleting a key takes its entire subtree. The scope is therefore drawn at
the highest node **this product itself writes**, and no higher:

- `{2D2FBE3A-…}` is this product's own COM class id. Nothing else can live under
  it. The shared `SOFTWARE\Classes\CLSID` parent is never marked.
- `{E357FCCD-…}` under `.pdf\shellex` is the per-extension `IThumbnailProvider`
  slot. **This one is written, not owned** — see below. Its parent
  `.pdf\shellex` holds every other PDF shell extension on the machine (preview
  handlers, property handlers, other viewers' thumbnail providers) and `.pdf`
  itself is a system-wide file association — neither is touched.
- The cascade root under `SystemFileAssociations\.pdf\shell` is named for this
  product; its parent `…\.pdf\shell` is shared with every other application that
  adds a PDF verb.
- `HKCU\Software\RustlingPDF\RustlingPDF` is the template's product key. The
  manufacturer key one level up, `HKCU\Software\RustlingPDF`, is a shared
  namespace by convention and is left alone.

Part of what happens to those parents is settled: MSI reclaims a parent it
empties, *"the installer removes a registry key after removing the last value or
subkey under the key"* (Registry table remarks). The documented way to prevent
that is a dummy value with `+` in the Name column, which RustlingPDF declines —
it would create real litter to stop MSI from removing litter. That rule alone
explained the second dry-run, where every unseeded shared key vanished.

> ### ⚠ CONFIRMED DEFECT — uninstalling destroyed `HKLM\SOFTWARE\Classes\.pdf`
>
> **Status: measured and reproducible. Step A of the fix is authored below and
> awaiting its dry-run; the record of the mechanism is kept because it dictates
> what may and may not be changed here in future.**
>
> Uninstalling RustlingPDF removes the `HKLM\SOFTWARE\Classes\.pdf` key and
> everything under it, **including registry data belonging to other
> applications**. On a machine where another PDF application stores its
> registration there, uninstalling RustlingPDF breaks it.
>
> **The measurement** (dry-run 4, commit `a046be9`). The verification script
> checkpoints each seeded foreign value at three points; the transition is
> unambiguous:
>
> | Checkpoint | `HKLM\SOFTWARE\Classes\.pdf` |
> | --- | --- |
> | key state before seeding | already existed on the runner (`key pre-existed: True`) |
> | seed written | intact — 2 values, 3 subkeys |
> | after install | intact — 3 values, 4 subkeys (the install *added* to a populated foreign key) |
> | after uninstall | **the key itself is gone** |
>
> `.pdf\shellex` follows the same pattern and disappears as a subkey of `.pdf`.
> This kills both earlier theories: the key was not created by MSI as an
> intermediate, and it was not empty when it was removed, so MSI's empty-key
> reclamation cannot be the cause.
>
> **The mechanism.** `bundle.fileAssociations` in `tauri.conf.json` makes the
> bundler template author an **advertised** file association. The Extension table
> in the shipped MSI carries
> `'pdf' | 'Path' | 'RustlingPDF.pdf' | '' | 'ShortcutsFeature'`, and the docs are
> explicit that such a row *"generates a set of registry keys and values"* as part
> of product advertisement (Extension table), which `UnregisterExtensionInfo`
> then *"removes … from the registry"* (UnregisterExtensionInfo action). MSI owns
> `HKCR\.pdf` as a generated artifact and reclaims it **as a key**, not
> value-by-value.
>
> The uninstall log isolates it. Within `RemoveRegistryValues` every operation
> names a key this product authored — the deepest being
> `.pdf\shellex\{E357FCCD-…}` — and **nothing targets `.pdf` or `.pdf\shellex`**.
> The only later operations naming the extension are
> `RegExtensionInfoUnregister64(… Extension=pdf, ProgId=RustlingPDF.pdf …)` and
> `RegProgIdInfoUnregister64(…)`. The key is present before them and gone after.
>
> **`provisioning.wxs` is not the cause.** Nothing in this repository's fragment
> reaches above `.pdf\shellex\{E357FCCD-…}`. The fix is a product decision about
> the file association, not a fragment change. Corroboration:
> `SystemFileAssociations\.pdf\shell` — the one shared key with no Extension-table
> row — survives with its foreign content intact.
>
> **Options, with costs.** The owner picks; none is shipped yet.
>
> | Option | Effect | Cost | Verified? |
> | --- | --- | --- | --- |
> | **A. Drop `bundle.fileAssociations`** | No Extension/ProgId/Verb rows, so the destructive operation does not exist | RustlingPDF disappears from Windows' "Open with" list for PDFs and can no longer be chosen as the default PDF app. The Explorer cascade menu is unaffected — it is a separate `SystemFileAssociations` registration in `provisioning.wxs` | one-line config change; needs a dry-run |
> | **B. `+` dummy-value trick** | — | **Provably cannot work. Do not attempt.** `+` is a Registry-table mechanism processed by `RemoveRegistryValues`, which the docs require to run *before* `UnregisterExtensionInfo`; the destructive operation happens afterwards and is driven by a different table. The measurement settles it independently: the seeded foreign value is registry-identical to a `+` dummy value, and it did not prevent the deletion | ruled out |
> | **C. Non-advertised, value-scoped association** | Drop `bundle.fileAssociations`, then author the association in `provisioning.wxs`: our own `HKLM\SOFTWARE\Classes\RustlingPDF.pdf` ProgId key (ours outright, safe to force-delete) plus a **value** named `RustlingPDF.pdf` under the shared `.pdf\OpenWithProgIds`. Uninstall then removes one value from a shared key under ordinary Registry-table semantics and never the `.pdf` key | Does not claim the *default* handler — but on Windows 8+ an installer cannot do that anyway (`UserChoice` requires user consent), so the practical loss is close to zero. More authoring we own and must maintain | needs a dry-run |
> | **D. Fix upstream in tauri-bundler** | Correct for everyone | Lead time measured in releases; does not unblock v3.1.2. Worth filing regardless | n/a |
>
> **Decision: A now, C next, each measured separately.** Changing two things at
> once and not knowing which moved the needle is how this defect stayed hidden
> for a full day of dry-runs.
>
> #### Step A as authored — scoped to Windows, not global
>
> `bundle.fileAssociations` is **cross-platform** in Tauri, so deleting it from
> `tauri.conf.json` would have fixed Windows by silently regressing macOS. It is
> therefore overridden only for the Windows target, in
> `frontend/editor/src-tauri/tauri.windows.conf.json`:
>
> ```json
> { "bundle": { "fileAssociations": [] } }
> ```
>
> Platform config files (`tauri.<platform>.conf.json`) are merged into the base
> config with RFC 7386 merge-patch semantics, under which a non-object patch
> value replaces the target wholesale — so `[]` replaces the one-element array
> for the Windows bundle only. With no rows, the template emits no
> `Extension`/`ProgId`/`Verb` tables and `UnregisterExtensionInfo` has nothing to
> unregister.
>
> Why per-platform rather than global:
>
> - **macOS would have regressed.** The committed `Info.plist` contains only
>   `NSLocalNetworkUsageDescription` — no `CFBundleDocumentTypes` — so the macOS
>   PDF document-type association comes *solely* from `bundle.fileAssociations`.
> - **Linux is unaffected either way.** `rustlingpdf.desktop` declares
>   `MimeType=application/pdf;` itself, and that committed template is what the
>   deb/rpm bundles use.
> - The defect is Windows-only, so the fix is Windows-only.
>
> #### Two consequences that must not be discovered later
>
> 1. **"Set as default PDF app" stops working on Windows between A and C.**
>    `commands/default_app.rs` resolves the effective `.pdf` handler and matches
>    the ProgId against `rustling`/`stirling`. With no ProgId registered, that
>    check can never succeed and RustlingPDF will not appear in the Windows
>    Settings default-apps list. **Step C restores it** — registering our own
>    `HKLM\SOFTWARE\Classes\RustlingPDF.pdf` with a `shell\open\command` and
>    listing it under `.pdf\OpenWithProgIds` is exactly what makes an application
>    selectable as a default handler. C is therefore not optional polish; it is
>    what makes a shipped feature work again.
> 2. **Anyone already running an affected build takes the damage once, and this
>    fix cannot prevent it.** `MajorUpgrade` is scheduled `afterInstallInitialize`,
>    so upgrading from 3.1.0/3.1.1 runs the *old* cached package's uninstall
>    first — including its own `UnregisterExtensionInfo`, which destroys `.pdf`
>    before the new package installs. The same applies to uninstalling an
>    affected build. The damage is done by the old package, which the new one
>    cannot amend. Only fresh installs of a fixed build, and uninstalls of them,
>    are clean.
>
> #### What the dry-run for A must show
>
> `.pdf` and `.pdf\shellex` **survive with their foreign sentinels intact**, and
> everything that passed in dry-run 4 still passes. The four-key seed assertions
> in `verify-msi-lifecycle.ps1` are deliberately unchanged — they are now the
> regression test for this fix and must start failing again if it is reverted.

The manufacturer key `HKCU\Software\RustlingPDF` is left alone for the same
reason as the shared parents above, and is not affected by any of this.

### Per-user data must live in its own component (ICE57)

**Authoring constraint, load-bearing: no component in `provisioning.wxs` may mix
per-machine and per-user resources.** ICE57 "validates that individual components
do not mix per-machine and per-user data … checks registry entries, files,
directory key paths, and non-advertised shortcuts", and `light` runs the full ICE
suite on every build — tauri-bundler's `run_light` passes neither `-sval` nor
`-sice:`, and its command line is hardcoded (`-cultures:`, `-loc`, `*.wixobj`),
so there is **no way to suppress an ICE from `tauri.conf.json`**: `bundle.windows.wix`
exposes no `lightArgs`/`candleArgs` field in tauri-bundler 2.9.4.

The severity is asymmetric, which is why this bites harder than the bundler
template's own violation:

| Component shape | ICE57 result |
| --- | --- |
| per-machine KeyPath (a `<File>`) + per-user data | **`error LGHT0204`** — the build fails |
| HKCU registry KeyPath + per-machine data | `warning LGHT1076` — builds (this is main.wxs's own `CMP_UninstallShortcut`) |

The HKCU cleanup therefore lives in `TemplateRegistryCleanupComponent`, which
holds nothing but an HKCU KeyPath value and an HKCU deletion row. This is also
what the Registry-table docs recommend independently of ICE57: "registry entries
written to the HKCU hive [should] reference a component having the
RegistryKeyPath bit set … [to] ensure that the installer writes the necessary
registry entries when there are multiple users on the same computer".

Two details that make it residue-free rather than merely legal:

- the KeyPath value (`InstallerRegistryCleanup`) is written **inside** the key
  being deleted, so `ForceDeleteOnUninstall` takes the marker with it. Keying the
  component on a different HKCU path would trade the leftover key we are removing
  for a new leftover key of our own making;
- sharing the key with main.wxs's `RegistryEntries` component is fine — MSI
  refcounts registry *values*, not keys, and the value names differ. Both
  components sit in `Absent="disallow"` features, so one can never be removed
  while the other stays.

Expressing the removal without any component is impossible: `Registry.Component_`
is a non-nullable foreign key, so every removal row must belong to one.

Current state of the whole fragment — every component is homogeneous, and no row
uses `HKMU` (`Root=-1`), so ICE57's "can be either per-user or per-machine"
variants cannot fire either:

| Component | KeyPath | Data |
| --- | --- | --- |
| `ProvisionerBinaryComponent`, `ThumbnailHandlerDllComponent` | `<File>` | per-machine only |
| `ThumbnailHandlerClsidComponent`, `ThumbnailHandlerShellexComponent`, `PdfContextMenu{Root,Open,Merge,Compress,Convert}Component` | HKLM registry | per-machine only |
| `TemplateRegistryCleanupComponent` | HKCU registry | per-user only |

### The thumbnail-provider slot is held by succession, not ownership

`HKLM\SOFTWARE\Classes\.pdf\shellex\{E357FCCD-A995-4576-B01F-234630154E96}` is
the single rendezvous slot Windows consults for `.pdf` thumbnails. There is one
per file extension, and its default value names whichever handler currently
serves them, so the slot is held **last-writer-wins**: installing RustlingPDF
overwrites whatever was there, and another PDF application installing afterwards
overwrites us.

**The consequence, stated plainly:** if another application claims the slot after
RustlingPDF is installed, uninstalling RustlingPDF force-deletes the key and
takes *that application's* registration with it, leaving `.pdf` thumbnails dead
on that machine until the other application is repaired or reinstalled.

This is accepted, not overlooked. Plain MSI value-removal is no safer — it
deletes the key's default value whether or not someone else overwrote it — so the
alternative buys nothing but a leftover empty key, and last-writer-wins is how
every shell-extension handler on Windows behaves. MSI cannot express "delete only
if the value is still mine". The equivalent risk in the other direction is
already reality: another PDF viewer installing after RustlingPDF silently kills
RustlingPDF's thumbnails.

### Why the provisioning file is removed, but only the file

`stirling-provisioning.json` is **administrative configuration pushed at install
time, not user data**. Leaving it means a later reinstall silently inherits a
server URL, login-agreement flag and update policy that nobody in the new
installation chose — a stale admin policy is worse than no policy. So it goes.

Nothing else in that directory goes. `%APPDATA%\Stirling-PDF` is the desktop
app's `RUSTLING_BASE_PATH` (`utils/paths.rs`): it holds `settings.yml`,
`custom_settings.yml` and `logs/`. RustlingPDF keeps all user state client-side,
so this directory is precisely the thing a user would be upset to lose, and
standard Windows behaviour is to leave user data behind on uninstall.

Implementation choice: **a deferred CustomAction calling
`stirling-provision.exe` in removal mode, not an MSI `<RemoveFile>`**.
`<RemoveFile>` would be simpler and MSI-native, but a RemoveFile row cannot
carry a condition — it fires whenever its component is removed. That is fatal
here, because `<MajorUpgrade Schedule="afterInstallInitialize"/>` makes **every
update a full uninstall of the old product followed by an install of the new
one**, and the tauri updater runs the new MSI with `installMode: "passive"`
(`tauri.conf.json` → `plugins.updater.windows`) passing none of the `STIRLING_*`
properties. An unconditional removal would therefore delete the administrator's
provisioning file on every app update, with nothing to write it back. The
CustomActions carry `REMOVE="ALL" AND NOT UPGRADINGPRODUCTCODE`, the same idiom
`main.wxs` already uses for `RemoveShortcuts` and `DeleteUpdateTask`.

Supporting details:

- scheduled `After="RemoveShortcuts"` (sequence 3200), well before `RemoveFiles`
  (3500) deletes `stirling-provision.exe` — a `FileKey` CustomAction cannot run
  once its own executable is gone;
- `Impersonate="yes"` for the per-user path and `Impersonate="no"` for the
  all-users path, mirroring the write actions;
- `Return="ignore"`: leftover configuration must never block or roll back an
  uninstall. A failure lands in the MSI verbose log, and the lifecycle check
  asserts the file is actually gone;
- the binary refuses any `--output` whose file name is not exactly
  `stirling-provisioning.json` and never touches the containing directory
  (unit-tested in `provisioner/src/main.rs`). It runs elevated — as LocalSystem
  on the all-users branch — so a widened scope would be a privileged arbitrary
  delete. A symlink planted at the target path is deleted as a link, not
  followed;
- **known limitation**: the actions are `deferred` with no rollback
  counterpart, so an uninstall that fails *after* them and rolls back leaves the
  product installed with its provisioning file gone until MDM re-pushes it. A
  `commit` action would avoid this but cannot be used — by `InstallFinalize`,
  `RemoveFiles` has already deleted the executable it would have to run.

## Uninstall — what is deliberately left behind

| Left behind | Why |
| --- | --- |
| `%APPDATA%\Stirling-PDF\` (`settings.yml`, `custom_settings.yml`, `logs/`) | User data. Standard Windows behaviour; a reinstall picks it back up. |
| `%PROGRAMDATA%\Stirling-PDF\` (the directory, now without the provisioning file) | May hold other operator state; MSI cannot express "remove if empty". |
| The app's WebView2 profile/cache under the user's local app data | Browser profile — user data. |
| WebView2 Runtime | Shared, machine-wide, refcounted, has its own ARP entry. Removing it would break every other WebView2 app. |
| `%TEMP%\MicrosoftEdgeWebview2Setup.exe` | Downloaded by the bundler template's bootstrapper CustomAction into TEMP; not MSI-tracked. Windows/Storage Sense reclaims it. |
| `…\SystemFileAssociations\.pdf\shell`, `HKLM\SOFTWARE\Classes\CLSID` and `HKCU\Software\RustlingPDF` **when they still hold anything** | Shared nodes we write into but do not own; verified to survive with foreign content intact. If our node was the last thing in them, MSI reclaims the emptied key itself. |
| `HKLM\SOFTWARE\Classes\.pdf` and `.pdf\shellex` | **DESTROYED — confirmed defect, see the warning above.** Uninstalling removes these keys and any other application's data inside them. This contract cannot promise otherwise until the file-association mechanism is changed. |
| `HKCU\…\Explorer\FileExts\.pdf\OpenWithList` / `UserChoice` | Written by Windows when the *user* picks a default app. No installer owns or removes these. |

The bundler template also emits `<RemoveFolder Id="DesktopFolder"
On="uninstall"/>`, i.e. it asks MSI to delete the user's Desktop folder. It is
harmless — MSI only removes empty directories — but it is the template's
authoring, not ours, and it is worth knowing about when reading an uninstall
log.

## Upgrading from v3.1.0

v3.1.0 spelled the Explorer cascade key and its `MUIVerb` title
`{{product_name}}`, on the assumption that the bundler templated fragments. It
does not, and MSI's `Formatted` parser leaves a `{…}` run containing no
`[property]` substitution unchanged, so that release put the raw token in the
registry **and in the submenu label every user reads**. Both are now spelled
`RustlingPDF`.

That rename is not free, and the failure mode it could produce is worse than the
original bug — a machine showing a stale broken menu *next to* a correct one.
Why it does not happen:

- **Component GUIDs change, and that is safe here.** WiX derives a `Guid="*"`
  component GUID from the component's key path, so the five cascade components
  get new GUIDs. `<MajorUpgrade Schedule="afterInstallInitialize"/>` removes the
  old product **before** the new one installs, so the old GUIDs and the new ones
  never coexist in the installed state and no component rule is violated. (A
  later `RemoveExistingProducts` schedule would have been a real problem.) The
  other components are unaffected: `ProvisionerBinaryComponent` and
  `ThumbnailHandlerDllComponent` key off files, and the two thumbnail registry
  components key off unchanged key paths, so their GUIDs are stable. Moving the
  HKCU cleanup row out of `ProvisionerBinaryComponent` into its own component
  does not disturb that either — a `RemoveRegistryKey` row never participates in
  the key path, so the auto-GUID is computed from the same `<File>` as before.
- **The old registry nodes would otherwise survive.** v3.1.0's package has no
  `ForceDeleteOnUninstall` rows, so the upgrade's removal pass strips its
  registry *values* and leaves the `{{product_name}}` key skeletons. The new
  package therefore carries a `<RemoveRegistryKey Action="removeOnInstall">` for
  that exact key: a `RemoveRegistry` row with `Name="-"`, which MSI documents as
  "the key is to be deleted, **if present, with all of its values and
  subkeys**, when the component is installed". It runs in
  `RemoveRegistryValues` (sequence 2600) — after `RemoveExistingProducts` has
  uninstalled v3.1.0 and long before `WriteRegistryValues` (5000) writes the new
  keys. An upgraded machine ends up with exactly one cascade menu.
- **The token is reproduced verbatim in that row**, and the cleanup does not
  depend on knowing what the `Formatted` parser does with it: the
  `RemoveRegistry` `Key` column is the same `RegPath`/Formatted type as the
  `Registry` `Key` column v3.1.0 wrote through, fed the identical source string,
  so it resolves to whatever v3.1.0 actually created.
- **Not conditioned on `WIX_UPGRADE_DETECTED`**, because v3.1.0 could equally
  have been *uninstalled* first — which, before this change, left the same
  skeleton — and this version installed fresh. On a machine that never had
  v3.1.0 the row is a documented no-op.
- **Residual risk:** the row deletes one literal key path that only a
  bundler-templating bug can produce. A different Tauri application shipping this
  exact fragment bug would collide on the same key and lose its cascade menu.
  Judged negligible — the fragment is bespoke to this repository — but it is a
  judgement, not a proof.
- Fresh installs, upgrades and the removal-at-install behaviour are all exercised
  by `verify-msi-lifecycle.ps1`, which seeds a v3.1.0-shaped tree before
  installing and asserts it is gone afterwards, so this does not rest on desk
  analysis alone.

Other keys did not need this treatment: the thumbnail CLSID, the `.pdf\shellex`
node and the template's HKCU product key have unchanged paths, so the new
package's own `ForceDeleteOnUninstall` / `RemoveRegistryKey` rows cover the
skeletons v3.1.0 left, at the next uninstall.

## Known divergences

1. **The per-user provisioning branch is unreachable.** `<Package
   InstallScope="perMachine">` makes WiX author `ALLUSERS=1`, so the
   `(NOT ALLUSERS OR ALLUSERS=0)` conditions on both the write and the remove
   CustomActions are always false and only `%PROGRAMDATA%` is ever used. The
   per-user branch is kept, and kept symmetric between write and remove, so a
   future scope change cannot silently leave a file behind.
2. **`{{name}}` in a fragment is always dead.** Fragments are never templated,
   so any Handlebars token added to `provisioning.wxs` reaches the registry
   verbatim. The one deliberate occurrence is the legacy-cleanup row above; a
   sweep of the live markup confirms every other substitution is genuine
   installer syntax and must not be "corrected": `[!Path]` (`[!filekey]` short
   path, valid only in the Registry/IniFile `Value` column, which is where it is
   used), `[INSTALLDIR]` / `[AppDataFolder]` / `[CommonAppDataFolder]`
   (Directory properties — the latter two are declared in this fragment's
   `<DirectoryRef Id="TARGETDIR">` precisely so they resolve), `[STIRLING_*]`
   (the `Secure="yes"` public properties), `[WriteProvisioningFile*]` /
   `[RemoveProvisioningFile*]` (the deferred-CustomAction-data convention: a
   property named identically to its action), `$(sys.SOURCEFILEDIR)` (candle
   preprocessor, resolved at compile time against the fragment's own directory
   — which is where `task desktop:provisioner` stages the binaries), and the two
   braced GUIDs, which survive `Formatted` intact precisely because they contain
   no `[…]`.

## Interaction with `port/bundle-desktop-tools`

That branch adds `resources/tools/` (bundled qpdf and Tesseract) to the MSI
payload. Those are ordinary MSI-tracked files in components under `INSTALLDIR`,
so `RemoveFiles`/`RemoveFolders` uninstall them with no change to this contract.
Two things for whoever merges to check: that the new components live under
`INSTALLDIR` (so `<RemoveFolder Id="INSTALLDIR" On="uninstall"/>` can still find
the directory empty and remove it), and that none of those tools writes state
next to itself at runtime — `verify-msi-lifecycle.ps1` fails on a surviving
install directory and lists what is left in it.

## Verification

WiX is Windows-only and MSI semantics cannot be emulated, so off-Windows the
fragment is only ever desk-checked: XML well-formedness plus schema validation
against the WiX 3.14.1 `wix.xsd`, and review of the sequence/condition tables.

**Automated.** `frontend/editor/src-tauri/windows/scripts/verify-msi-lifecycle.ps1`
runs the full lifecycle and is wired into the windows leg of
`.github/workflows/desktop-build.yml` (after the artifact upload, so a failing
bundle is still downloadable). It reads the ProductCode from the MSI's own
Property table, seeds a v3.1.0-shaped cascade tree under the legacy
`{{product_name}}` key, seeds a foreign sentinel value into every shared registry
key the product writes into but does not own, installs silently with provisioning
properties, seeds sentinel files standing in for user state, asserts the registry
surface, the `MUIVerb` label, the disappearance of the legacy tree, and the ARP
flags (`NoRemove` unset, `NoModify`/`NoRepair` set), uninstalls, then asserts
every key is gone **from both the 64-bit and 32-bit registry views**, the
provisioning file is gone, the install directory is gone, and both the user-state
sentinels and the foreign registry sentinels survived byte-identical — so
over-deletion fails as loudly as under-deletion.

Every assertion is designed so it cannot pass vacuously, which took two rounds to
get right:

- checks that depend on a lookup succeeding (the ARP flag block, the `MUIVerb`
  read, the install directory) record an explicit **failure** when the lookup
  fails, instead of disappearing from the summary along with their `if` block;
- checks that iterate over seeded state are preceded by an assertion that the
  seeding actually happened, so an empty loop cannot read as success;
- the legacy-cleanup check is gated on the new cascade key actually existing.

Result detail is split: `-Detail` carries observed evidence and prints on pass or
fail, `-FailureDetail` carries "what went wrong" and prints only on failure. The
first real run printed failure text beside passing rows ("PASS … ARP entry
survived"), which is a reporting bug rather than a cosmetic one — it makes a
reader stop trusting the details and skim past a genuine failure.

Any registry or filesystem state the script seeds is torn down on the way out,
including when an assertion or msiexec call throws part-way through, and it only
ever removes a shared key it created itself — otherwise just the value it added.

The provisioner's removal logic is unit-tested on every PR in the desktop gate
(`.github/workflows/desktop.yml`).

**Manual runbook** (elevated `cmd`, ~5 minutes). Both registry views are queried
because the components' bitness — WiX defaults every component to 64-bit under
candle's `-arch x64` — decides which view the keys land in.

```bat
:: 1. install with a provisioning policy
msiexec /i RustlingPDF_3.1.0_x64_en-US.msi /qn /l*v %TEMP%\rpdf-install.log ^
    STIRLING_SERVER_URL=https://example.invalid STIRLING_UPDATE_MODE=disabled

:: 2. these must all EXIST now. The cascade root's MUIVerb must read
::    "RustlingPDF" -- a raw {{product_name}} token there is the v3.1.0 defect.
reg query "HKLM\SOFTWARE\Classes\CLSID\{2D2FBE3A-9A88-4308-A52E-7EF63CA7CF48}" /s /reg:64
reg query "HKLM\SOFTWARE\Classes\.pdf\shellex\{E357FCCD-A995-4576-B01F-234630154E96}" /reg:64
reg query "HKLM\SOFTWARE\Classes\SystemFileAssociations\.pdf\shell\RustlingPDF" /s /reg:64
reg query "HKCU\Software\RustlingPDF\RustlingPDF"
dir "%ProgramData%\Stirling-PDF\stirling-provisioning.json"

:: 2a. and the v3.1.0 leftover must be GONE, removed at install time.
::     To rehearse the upgrade on a machine that never had v3.1.0, create it
::     before step 1 and re-run this query after:
::       reg add "HKLM\SOFTWARE\Classes\SystemFileAssociations\.pdf\shell\{{product_name}}" /v MUIVerb /d "{{product_name}}" /reg:64
reg query "HKLM\SOFTWARE\Classes\SystemFileAssociations\.pdf\shell\{{product_name}}" /reg:64

:: 3. prove the ARP entry offers Uninstall (NoRemove absent; NoModify/NoRepair = 1)
reg query "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall" /s /f RustlingPDF

:: 4. drop a sentinel that uninstall must NOT delete
echo user state > "%ProgramData%\Stirling-PDF\sentinel.txt"

:: 5. uninstall (or: Settings > Apps > RustlingPDF > Uninstall,
::    or re-run the SAME .msi and choose Remove in the maintenance dialog)
msiexec /x {PRODUCT-CODE-FROM-STEP-3} /qn /l*v %TEMP%\rpdf-uninstall.log

:: 6. these must all be GONE (ERROR: The system was unable to find ...)
reg query "HKLM\SOFTWARE\Classes\CLSID\{2D2FBE3A-9A88-4308-A52E-7EF63CA7CF48}" /reg:64
reg query "HKLM\SOFTWARE\Classes\CLSID\{2D2FBE3A-9A88-4308-A52E-7EF63CA7CF48}" /reg:32
reg query "HKLM\SOFTWARE\Classes\.pdf\shellex\{E357FCCD-A995-4576-B01F-234630154E96}" /reg:64
reg query "HKLM\SOFTWARE\Classes\SystemFileAssociations\.pdf\shell\RustlingPDF" /reg:64
reg query "HKLM\SOFTWARE\Classes\SystemFileAssociations\.pdf\shell\RustlingPDF" /reg:32
reg query "HKCU\Software\RustlingPDF\RustlingPDF"
dir "%ProgramData%\Stirling-PDF\stirling-provisioning.json"
dir "%ProgramFiles%\RustlingPDF"

:: 7. these must still EXIST -- deleting any of these is over-deletion.
::     The /v query is the point: a bare "does the key exist" check proves
::     nothing, because MSI is entitled to reclaim these keys once they are
::     empty. What must survive is another application's DATA. Seed it before
::     step 1:
::       reg add "HKLM\SOFTWARE\Classes\.pdf" /v ForeignSentinel /d keepme /reg:64
::       reg add "HKLM\SOFTWARE\Classes\.pdf\shellex" /v ForeignSentinel /d keepme /reg:64
::       reg add "HKLM\SOFTWARE\Classes\SystemFileAssociations\.pdf\shell" /v ForeignSentinel /d keepme /reg:64
reg query "HKLM\SOFTWARE\Classes\.pdf" /v ForeignSentinel /reg:64
reg query "HKLM\SOFTWARE\Classes\.pdf\shellex" /v ForeignSentinel /reg:64
reg query "HKLM\SOFTWARE\Classes\SystemFileAssociations\.pdf\shell" /v ForeignSentinel /reg:64
reg query "HKLM\SOFTWARE\Classes\CLSID" /reg:64
dir "%ProgramData%\Stirling-PDF\sentinel.txt"
dir "%AppData%\Stirling-PDF"

:: 8. tidy up the seeds from step 7
reg delete "HKLM\SOFTWARE\Classes\.pdf" /v ForeignSentinel /f /reg:64
reg delete "HKLM\SOFTWARE\Classes\.pdf\shellex" /v ForeignSentinel /f /reg:64
reg delete "HKLM\SOFTWARE\Classes\SystemFileAssociations\.pdf\shell" /v ForeignSentinel /f /reg:64
```

On a machine with **no** other PDF application, expect `.pdf`, `.pdf\shellex` and
`…\.pdf\shell` to be gone entirely after step 5 unless you seeded them — that is
MSI reclaiming keys it emptied, not a defect.

If step 2 shows the cascade key under `{{product_name}}` rather than
`RustlingPDF`, the rename has regressed — fix `provisioning.wxs` and update this
contract and the context-menu contract together.
