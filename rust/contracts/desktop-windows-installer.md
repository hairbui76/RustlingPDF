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
| `HKCU\Software\RustlingPDF\RustlingPDF` (the bundler template's own key, which the template only ever empties) | `<RemoveRegistryKey Action="removeOnUninstall">` on `ProvisionerBinaryComponent` |
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

The price of that restraint is one possible leftover: if RustlingPDF was the
first application ever to register a `.pdf` shell extension on the machine, the
now-empty `HKLM\SOFTWARE\Classes\.pdf\shellex` key survives. MSI has no
"delete only if empty" primitive, and an empty key is strictly less harmful than
deleting another vendor's registrations. Same reasoning for the manufacturer
key.

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
| `HKLM\SOFTWARE\Classes\.pdf\shellex` when it becomes empty, and `HKCU\Software\RustlingPDF` | Shared nodes — see the scope reasoning above. |
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
  components key off unchanged key paths, so their GUIDs are stable.
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
`{{product_name}}` key, installs silently with provisioning properties, seeds
sentinel files standing in for user state, asserts the registry surface, the
`MUIVerb` label, the disappearance of the legacy tree, and the ARP flags
(`NoRemove` unset, `NoModify`/`NoRepair` set), uninstalls, then asserts every key
is gone **from both the 64-bit and 32-bit registry views**, the provisioning file
is gone, the install directory is gone, and the sentinels survived. Shared
parents are asserted still present, so over-deletion fails as loudly as
under-deletion; the legacy-cleanup check is gated on the new cascade key
actually existing so it cannot pass vacuously, and any registry state the script
seeds is torn down even when an assertion throws part-way through.

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

:: 7. these must still EXIST -- deleting any of these is over-deletion
reg query "HKLM\SOFTWARE\Classes\.pdf" /reg:64
reg query "HKLM\SOFTWARE\Classes\CLSID" /reg:64
reg query "HKLM\SOFTWARE\Classes\SystemFileAssociations\.pdf\shell" /reg:64
dir "%ProgramData%\Stirling-PDF\sentinel.txt"
dir "%AppData%\Stirling-PDF"
```

If step 2 shows the cascade key under `{{product_name}}` rather than
`RustlingPDF`, the rename has regressed — fix `provisioning.wxs` and update this
contract and the context-menu contract together.
