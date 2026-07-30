<#
.SYNOPSIS
    Install -> assert -> uninstall -> assert lifecycle check for the RustlingPDF MSI.

.DESCRIPTION
    Proves the promises in rust/contracts/desktop-windows-installer.md on a real
    Windows machine, which is the only place they can be proven: WiX is
    Windows-only and MSI behaviour cannot be emulated.

    The check:

      1. reads the MSI's ProductCode straight out of its Property table;
      1a. seeds the Explorer cascade tree v3.1.0 left behind (keys named with
         the unrendered "{{product_name}}" token), so the install-time cleanup
         of that leftover is proven without needing an actual v3.1.0 MSI;
      2. installs it silently *with* provisioning properties, so the
         CustomAction-written stirling-provisioning.json actually exists;
      3. drops sentinel files next to that provisioning file, standing in for
         the user state (settings.yml, custom_settings.yml, logs/) that an
         uninstall must NOT touch;
      4. asserts every registry node this product authors is present, and that
         the Add/Remove Programs entry offers Uninstall (ARPNOREMOVE unset)
         while Change and Repair are suppressed;
      5. asserts the installed ProductCode equals the MSI's own ProductCode --
         which is what makes re-running this exact .msi file enter maintenance
         mode instead of reinstalling;
      6. uninstalls silently;
      7. asserts every one of those registry nodes is gone from BOTH registry
         views, the provisioning file is gone, the install directory is gone,
         and the sentinel user-state files survived.

    Every leftover the uninstall deliberately keeps is reported as INFO rather
    than asserted, so the audit stays visible without failing the run.

    Requires an elevated shell (msiexec perMachine install).

.PARAMETER MsiPath
    Path to the built RustlingPDF .msi.

.PARAMETER LogDirectory
    Where to write the msiexec verbose logs. Defaults to a subdirectory of TEMP.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File verify-msi-lifecycle.ps1 -MsiPath .\RustlingPDF_3.1.0_x64_en-US.msi
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $MsiPath,

    [string] $LogDirectory = (Join-Path $env:TEMP 'rustlingpdf-msi-lifecycle')
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# ---------------------------------------------------------------------------
# Result accumulation
# ---------------------------------------------------------------------------

$script:Failures = New-Object System.Collections.Generic.List[string]
$script:Checks = New-Object System.Collections.Generic.List[object]

function Add-Result {
    param([string] $Phase, [string] $Name, [bool] $Ok, [string] $Detail)
    $script:Checks.Add([pscustomobject]@{ Phase = $Phase; Check = $Name; Result = $(if ($Ok) { 'PASS' } else { 'FAIL' }); Detail = $Detail })
    if ($Ok) {
        Write-Host ("  [PASS] {0}" -f $Name) -ForegroundColor Green
    }
    else {
        Write-Host ("  [FAIL] {0} -- {1}" -f $Name, $Detail) -ForegroundColor Red
        $script:Failures.Add("[$Phase] $Name -- $Detail")
    }
}

function Write-Info {
    param([string] $Message)
    Write-Host ("  [INFO] {0}" -f $Message) -ForegroundColor Cyan
}

function Write-Phase {
    param([string] $Message)
    Write-Host ''
    Write-Host ("=== {0} ===" -f $Message) -ForegroundColor Yellow
}

# ---------------------------------------------------------------------------
# Registry helpers -- both views are always checked.
#
# candle is invoked with `-arch x64` (tauri-bundler msi/mod.rs), which makes
# WiX v3 default every Component to Win64="yes", so these keys are expected in
# the 64-bit view. The 32-bit view is checked anyway: if a future change flips
# the component bitness, a key written to the WOW6432Node view would otherwise
# survive an uninstall that only looked at the 64-bit view, and this script
# would report a false PASS.
# ---------------------------------------------------------------------------

function Get-RegistryViewKey {
    param(
        [Microsoft.Win32.RegistryHive] $Hive,
        [Microsoft.Win32.RegistryView] $View,
        [string] $SubKey
    )
    try {
        # The base key wraps a predefined hive handle; the subkey it returns owns
        # an independent handle, so the base is deliberately not disposed here.
        $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey($Hive, $View)
        return $base.OpenSubKey($SubKey)
    }
    catch {
        return $null
    }
}

function Test-RegistryKeyInAnyView {
    param([Microsoft.Win32.RegistryHive] $Hive, [string] $SubKey)
    $views = @([Microsoft.Win32.RegistryView]::Registry64, [Microsoft.Win32.RegistryView]::Registry32)
    $found = @()
    foreach ($view in $views) {
        $key = Get-RegistryViewKey -Hive $Hive -View $view -SubKey $SubKey
        if ($null -ne $key) {
            $found += $view.ToString()
            $key.Dispose()
        }
    }
    return , $found
}

function Assert-RegistryKeyPresent {
    param([string] $Phase, [Microsoft.Win32.RegistryHive] $Hive, [string] $SubKey, [string] $Label)
    $found = Test-RegistryKeyInAnyView -Hive $Hive -SubKey $SubKey
    $ok = $found.Count -gt 0
    Add-Result -Phase $Phase -Name ("present: {0}" -f $Label) -Ok $ok -Detail $(
        if ($ok) { "views: $($found -join ', ')" } else { "$Hive\$SubKey not found in either registry view" }
    )
    return $ok
}

function Assert-RegistryKeyAbsent {
    param([string] $Phase, [Microsoft.Win32.RegistryHive] $Hive, [string] $SubKey, [string] $Label)
    $found = Test-RegistryKeyInAnyView -Hive $Hive -SubKey $SubKey
    $ok = $found.Count -eq 0
    Add-Result -Phase $Phase -Name ("removed: {0}" -f $Label) -Ok $ok -Detail $(
        if ($ok) { 'gone from both registry views' } else { "$Hive\$SubKey SURVIVED in: $($found -join ', ')" }
    )
}

# The whole script mutates the machine by design (it installs and uninstalls an
# MSI), so -WhatIf support on these internal helpers alone would be misleading
# rather than useful; the ShouldProcess rule is suppressed deliberately.
function New-RegistryTree {
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Internal helper of an inherently state-changing verification script.')]
    param(
        [Microsoft.Win32.RegistryHive] $Hive,
        [Microsoft.Win32.RegistryView] $View,
        [string] $SubKey,
        [hashtable] $Values
    )
    $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey($Hive, $View)
    $key = $base.CreateSubKey($SubKey)
    if ($null -ne $Values) {
        foreach ($name in $Values.Keys) { $key.SetValue($name, $Values[$name], [Microsoft.Win32.RegistryValueKind]::String) }
    }
    $key.Dispose()
}

function Remove-RegistryTree {
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Internal helper of an inherently state-changing verification script.')]
    param([Microsoft.Win32.RegistryHive] $Hive, [Microsoft.Win32.RegistryView] $View, [string] $SubKey)
    try {
        $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey($Hive, $View)
        $base.DeleteSubKeyTree($SubKey, $false)
    }
    catch {
        # Already gone (DeleteSubKeyTree throws when the key is absent) -- which
        # is the outcome this script usually wants anyway.
        Write-Verbose ("Remove-RegistryTree: {0}\{1} ({2}) not present: {3}" -f $Hive, $SubKey, $View, $_.Exception.Message)
    }
}

# ---------------------------------------------------------------------------
# MSI helpers
# ---------------------------------------------------------------------------

function Get-MsiProperty {
    param([string] $Path, [string] $Property)
    $installer = New-Object -ComObject WindowsInstaller.Installer
    $database = $null
    $view = $null
    $record = $null
    try {
        $database = $installer.GetType().InvokeMember('OpenDatabase', 'InvokeMethod', $null, $installer, @($Path, 0))
        $query = "SELECT Value FROM Property WHERE Property = '$Property'"
        $view = $database.GetType().InvokeMember('OpenView', 'InvokeMethod', $null, $database, @($query))
        $view.GetType().InvokeMember('Execute', 'InvokeMethod', $null, $view, $null) | Out-Null
        $record = $view.GetType().InvokeMember('Fetch', 'InvokeMethod', $null, $view, $null)
        if ($null -eq $record) { return $null }
        return $record.GetType().InvokeMember('StringData', 'GetProperty', $null, $record, @(1))
    }
    finally {
        # The view holds the .msi open; close and release it so the file is not
        # still locked when msiexec is handed the same path.
        if ($null -ne $view) {
            $view.GetType().InvokeMember('Close', 'InvokeMethod', $null, $view, $null) | Out-Null
        }
        foreach ($comObject in @($record, $view, $database, $installer)) {
            if ($null -ne $comObject) { [System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($comObject) | Out-Null }
        }
    }
}

function Invoke-MsiExec {
    param([string[]] $Arguments, [string] $Label)
    Write-Info ("msiexec {0}" -f ($Arguments -join ' '))
    $process = Start-Process -FilePath 'msiexec.exe' -ArgumentList $Arguments -Wait -PassThru -NoNewWindow
    # 0 = success, 3010 = success, reboot required.
    if ($process.ExitCode -ne 0 -and $process.ExitCode -ne 3010) {
        throw "$Label failed with msiexec exit code $($process.ExitCode). See the verbose log in $LogDirectory."
    }
    Write-Info ("{0} exit code {1}" -f $Label, $process.ExitCode)
}

function Get-ArpEntry {
    param([string] $ProductCode)
    $paths = @(
        "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$ProductCode",
        "SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\$ProductCode"
    )
    foreach ($path in $paths) {
        foreach ($view in @([Microsoft.Win32.RegistryView]::Registry64, [Microsoft.Win32.RegistryView]::Registry32)) {
            $key = Get-RegistryViewKey -Hive ([Microsoft.Win32.RegistryHive]::LocalMachine) -View $view -SubKey $path
            if ($null -ne $key) {
                $entry = [pscustomobject]@{
                    Path            = $path
                    View            = $view.ToString()
                    NoRemove        = $key.GetValue('NoRemove')
                    NoModify        = $key.GetValue('NoModify')
                    NoRepair        = $key.GetValue('NoRepair')
                    UninstallString = $key.GetValue('UninstallString')
                    InstallLocation = $key.GetValue('InstallLocation')
                    DisplayName     = $key.GetValue('DisplayName')
                }
                $key.Dispose()
                return $entry
            }
        }
    }
    return $null
}

# ---------------------------------------------------------------------------
# The surface under test
# ---------------------------------------------------------------------------

$ThumbnailClsid = '{2D2FBE3A-9A88-4308-A52E-7EF63CA7CF48}'
$ThumbnailProviderIid = '{E357FCCD-A995-4576-B01F-234630154E96}'

# The Explorer cascade verb key. tauri-bundler renders main.wxs through
# Handlebars but hands *fragments* to candle unrendered, so provisioning.wxs
# spells this literally; up to and including v3.1.0 it was spelled
# "{{product_name}}" and MSI's Formatted parser (which leaves a {...} run
# containing no [property] substitution unchanged) put that raw token in the
# registry AND in the user-visible MUIVerb label.
#
# Both spellings are still probed. The correct one must appear; the legacy one
# must NOT, either after a fresh install or after the upgrade-cleanup rehearsal
# below -- so a regression back to the unrendered token fails loudly instead of
# passing against whichever key happens to exist.
$VerbKey = 'RustlingPDF'
$LegacyVerbKey = '{{product_name}}'
$CascadeRoot = 'SOFTWARE\Classes\SystemFileAssociations\.pdf\shell'

# provisioning.wxs's components are 64-bit (candle runs with -arch x64, which
# makes WiX default Component/@Win64 to yes), so the legacy tree is rehearsed in
# the 64-bit view. If that assumption is ever wrong, the "present" check below
# reports which view the real keys landed in and the mismatch is visible rather
# than silent.
$SeedView = [Microsoft.Win32.RegistryView]::Registry64
$script:SeededLegacyKey = $false

function Remove-SeededLegacyKey {
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Internal helper of an inherently state-changing verification script.')]
    param()
    if ($script:SeededLegacyKey) {
        Remove-RegistryTree -Hive ([Microsoft.Win32.RegistryHive]::LocalMachine) -View $SeedView -SubKey "$CascadeRoot\$LegacyVerbKey"
        $script:SeededLegacyKey = $false
    }
}

# Registry state seeded by this script must not outlive it, including when an
# assertion or msiexec call throws part-way through.
trap {
    Remove-SeededLegacyKey
    break
}

$SystemProvisioningDir = Join-Path $env:ProgramData 'Stirling-PDF'
$UserProvisioningDir = Join-Path $env:APPDATA 'Stirling-PDF'
$ProvisioningFileName = 'stirling-provisioning.json'
$SentinelName = 'lifecycle-user-state-sentinel.txt'

# ---------------------------------------------------------------------------
# Run
# ---------------------------------------------------------------------------

$MsiPath = (Resolve-Path -LiteralPath $MsiPath).Path
New-Item -ItemType Directory -Force -Path $LogDirectory | Out-Null
# msiexec resolves relative paths against its own working directory, not
# PowerShell's location, so every path handed to it must be absolute.
$LogDirectory = (Resolve-Path -LiteralPath $LogDirectory).Path
$installLog = Join-Path $LogDirectory 'install.log'
$uninstallLog = Join-Path $LogDirectory 'uninstall.log'

Write-Phase 'MSI identity'
$productCode = Get-MsiProperty -Path $MsiPath -Property 'ProductCode'
$upgradeCode = Get-MsiProperty -Path $MsiPath -Property 'UpgradeCode'
$productName = Get-MsiProperty -Path $MsiPath -Property 'ProductName'
$productVersion = Get-MsiProperty -Path $MsiPath -Property 'ProductVersion'
Write-Info "MSI            : $MsiPath"
Write-Info "ProductName    : $productName"
Write-Info "ProductVersion : $productVersion"
Write-Info "ProductCode    : $productCode"
Write-Info "UpgradeCode    : $upgradeCode"

# The UpgradeCode is pinned in tauri.conf.json and existing installs depend on
# it; a rebuild that loses it turns every update into a side-by-side install.
# Compared brace- and case-insensitively: WiX normalises GUIDs to braced
# uppercase, tauri.conf.json spells it bare and lowercase.
$expectedUpgradeCode = '3305fba9-7e5e-5c09-bc71-eca0a65f4fee'
$normalisedUpgradeCode = ([string]$upgradeCode).Trim('{', '}')
Add-Result -Phase 'identity' -Name 'UpgradeCode matches the pinned tauri.conf.json value' `
    -Ok ($normalisedUpgradeCode -ieq $expectedUpgradeCode) -Detail "expected $expectedUpgradeCode, got $upgradeCode"

if ($null -ne (Get-ArpEntry -ProductCode $productCode)) {
    throw "ProductCode $productCode is already installed on this machine. Uninstall it first; this script refuses to clobber an existing install."
}

Write-Phase 'Rehearse a v3.1.0 leftover (upgrade-cleanup proof)'
# Recreate what v3.1.0 left on a machine: the cascade tree under the unrendered
# token. On an upgrade, the old package's uninstall strips its registry *values*
# but leaves these key skeletons, so the new package must delete the tree at
# install time (RemoveRegistry row, RemoveRegistryValues sequence 2600) or the
# machine ends up with a stale mislabelled menu beside the correct one. Seeding
# it here proves that row fires without needing an actual v3.1.0 MSI.
if ((Test-RegistryKeyInAnyView -Hive ([Microsoft.Win32.RegistryHive]::LocalMachine) -SubKey "$CascadeRoot\$LegacyVerbKey").Count -gt 0) {
    Write-Info "A legacy cascade key is already present on this machine; using it as-is rather than seeding."
}
else {
    New-RegistryTree -Hive ([Microsoft.Win32.RegistryHive]::LocalMachine) -View $SeedView -SubKey "$CascadeRoot\$LegacyVerbKey" `
        -Values @{ 'MUIVerb' = $LegacyVerbKey; 'SubCommands' = '' }
    New-RegistryTree -Hive ([Microsoft.Win32.RegistryHive]::LocalMachine) -View $SeedView -SubKey "$CascadeRoot\$LegacyVerbKey\shell\01_open\command" `
        -Values @{ '' = '"C:\legacy\RustlingPDF.exe" --tool open "%1"' }
    $script:SeededLegacyKey = $true
    Write-Info ("seeded legacy tree: HKLM\{0}\{1} ({2} view)" -f $CascadeRoot, $LegacyVerbKey, $SeedView)
}
Add-Result -Phase 'seed' -Name 'legacy cascade tree present before install' `
    -Ok ((Test-RegistryKeyInAnyView -Hive ([Microsoft.Win32.RegistryHive]::LocalMachine) -SubKey "$CascadeRoot\$LegacyVerbKey").Count -gt 0) `
    -Detail 'the upgrade-cleanup rehearsal cannot prove anything if the key was never created'

Write-Phase 'Install (silent, with provisioning properties)'
Invoke-MsiExec -Label 'install' -Arguments @(
    '/i', "`"$MsiPath`"",
    '/qn', '/norestart',
    'STIRLING_SERVER_URL=https://provisioning.example.invalid',
    'STIRLING_UPDATE_MODE=disabled',
    '/l*v', "`"$installLog`""
)

Write-Phase 'Post-install assertions'

$arp = Get-ArpEntry -ProductCode $productCode
Add-Result -Phase 'install' -Name 'Add/Remove Programs entry exists' -Ok ($null -ne $arp) -Detail 'no Uninstall registry entry for the ProductCode'
if ($null -ne $arp) {
    Write-Info ("ARP entry      : {0} ({1} view)" -f $arp.Path, $arp.View)
    Write-Info ("InstallLocation: {0}" -f $arp.InstallLocation)

    # This is the maintenance-mode promise, in registry form: Remove stays
    # available (ARPNOREMOVE is never set by the bundler template) while
    # ARPNOREPAIR/ARPNOMODIFY suppress Repair and Change.
    Add-Result -Phase 'install' -Name 'ARP allows Uninstall (NoRemove unset or 0)' `
        -Ok ($null -eq $arp.NoRemove -or $arp.NoRemove -eq 0) -Detail "NoRemove=$($arp.NoRemove)"
    Add-Result -Phase 'install' -Name 'ARP suppresses Change (NoModify=1)' `
        -Ok ($arp.NoModify -eq 1) -Detail "NoModify=$($arp.NoModify)"
    Add-Result -Phase 'install' -Name 'ARP suppresses Repair (NoRepair=1)' `
        -Ok ($arp.NoRepair -eq 1) -Detail "NoRepair=$($arp.NoRepair)"
    Add-Result -Phase 'install' -Name 'ARP UninstallString is present' `
        -Ok (-not [string]::IsNullOrWhiteSpace([string]$arp.UninstallString)) -Detail "UninstallString=$($arp.UninstallString)"

    # Re-running the *same file* matches the installed ProductCode, which is
    # what sends msiexec to MaintenanceWelcomeDlg/MaintenanceTypeDlg instead of
    # a fresh install. (A rebuilt MSI of the same version gets a new
    # ProductCode from <Product Id="*"> and becomes a major upgrade instead.)
    $stateInstaller = New-Object -ComObject WindowsInstaller.Installer
    $installedState = $stateInstaller.GetType().InvokeMember('ProductState', 'GetProperty', $null, $stateInstaller, @($productCode))
    Add-Result -Phase 'install' -Name 'MsiQueryProductState reports the ProductCode installed (maintenance mode on re-run)' `
        -Ok ($installedState -eq 5) -Detail "ProductState=$installedState (5 = INSTALLSTATE_DEFAULT)"
}

$installDir = $null
if ($null -ne $arp -and -not [string]::IsNullOrWhiteSpace([string]$arp.InstallLocation)) {
    $installDir = [string]$arp.InstallLocation
    Add-Result -Phase 'install' -Name 'install directory exists' -Ok (Test-Path -LiteralPath $installDir) -Detail $installDir
}

# Registry surface authored by provisioning.wxs.
Assert-RegistryKeyPresent -Phase 'install' -Hive ([Microsoft.Win32.RegistryHive]::LocalMachine) `
    -SubKey "SOFTWARE\Classes\CLSID\$ThumbnailClsid" -Label 'thumbnail handler CLSID' | Out-Null
Assert-RegistryKeyPresent -Phase 'install' -Hive ([Microsoft.Win32.RegistryHive]::LocalMachine) `
    -SubKey "SOFTWARE\Classes\CLSID\$ThumbnailClsid\InprocServer32" -Label 'thumbnail handler InprocServer32' | Out-Null
Assert-RegistryKeyPresent -Phase 'install' -Hive ([Microsoft.Win32.RegistryHive]::LocalMachine) `
    -SubKey "SOFTWARE\Classes\.pdf\shellex\$ThumbnailProviderIid" -Label '.pdf IThumbnailProvider registration' | Out-Null

$cascadeViews = Test-RegistryKeyInAnyView -Hive ([Microsoft.Win32.RegistryHive]::LocalMachine) -SubKey "$CascadeRoot\$VerbKey"
Assert-RegistryKeyPresent -Phase 'install' -Hive ([Microsoft.Win32.RegistryHive]::LocalMachine) `
    -SubKey "$CascadeRoot\$VerbKey" -Label "Explorer cascade root '$VerbKey'" | Out-Null
foreach ($verb in @('01_open', '02_merge', '03_compress', '04_convert')) {
    Assert-RegistryKeyPresent -Phase 'install' -Hive ([Microsoft.Win32.RegistryHive]::LocalMachine) `
        -SubKey "$CascadeRoot\$VerbKey\shell\$verb\command" -Label "cascade verb $verb command" | Out-Null
}

# The submenu title the user actually reads. A raw Handlebars token here is the
# v3.1.0 defect; it must never come back.
#
# Read from whichever view(s) the cascade key actually landed in rather than
# assuming the 64-bit one, and record a failing result when it cannot be read at
# all -- otherwise an unreadable key would make this check disappear from the
# summary instead of failing it.
$muiVerbChecked = $false
foreach ($viewName in $cascadeViews) {
    $cascadeKey = Get-RegistryViewKey -Hive ([Microsoft.Win32.RegistryHive]::LocalMachine) `
        -View ([Microsoft.Win32.RegistryView]$viewName) -SubKey "$CascadeRoot\$VerbKey"
    if ($null -eq $cascadeKey) { continue }
    $muiVerb = [string]$cascadeKey.GetValue('MUIVerb')
    $cascadeKey.Dispose()
    Add-Result -Phase 'install' -Name "cascade MUIVerb is the product name, not a template token ($viewName)" `
        -Ok ($muiVerb -ceq $VerbKey) -Detail "MUIVerb='$muiVerb' (expected '$VerbKey')"
    $muiVerbChecked = $true
}
if (-not $muiVerbChecked) {
    Add-Result -Phase 'install' -Name 'cascade MUIVerb is the product name, not a template token' -Ok $false `
        -Detail "HKLM\$CascadeRoot\$VerbKey could not be opened in any registry view, so the label was never verified"
}

# The upgrade-cleanup row: the seeded v3.1.0 tree must be gone from every view
# the new cascade key landed in.
#
# Scoped to those views rather than to both unconditionally, because the seed is
# planted in the 64-bit view on the assumption that the components are 64-bit;
# scoping keeps a wrong assumption from producing a confusing failure here
# (the "present" check above already reports which view MSI really used). The
# check is also gated on the new cascade key actually existing, so it can never
# pass vacuously against an install that registered nothing at all.
$legacyViews = Test-RegistryKeyInAnyView -Hive ([Microsoft.Win32.RegistryHive]::LocalMachine) -SubKey "$CascadeRoot\$LegacyVerbKey"
$legacyLeftInCascadeViews = @($legacyViews | Where-Object { $cascadeViews -contains $_ })
Add-Result -Phase 'install' -Name 'legacy cascade tree removed at install (no stale menu beside the correct one)' `
    -Ok ($cascadeViews.Count -gt 0 -and $legacyLeftInCascadeViews.Count -eq 0) `
    -Detail "new cascade key in: [$($cascadeViews -join ', ')]; legacy key survived in: [$($legacyLeftInCascadeViews -join ', ')]"
$legacyElsewhere = @($legacyViews | Where-Object { $cascadeViews -notcontains $_ })
if ($legacyElsewhere.Count -gt 0) {
    Write-Info ("legacy cascade key still present in a view the new keys did NOT use ({0}) -- check the component bitness assumption" -f ($legacyElsewhere -join ', '))
}
if ($legacyViews.Count -eq 0) { $script:SeededLegacyKey = $false }

Assert-RegistryKeyPresent -Phase 'install' -Hive ([Microsoft.Win32.RegistryHive]::CurrentUser) `
    -SubKey 'Software\RustlingPDF\RustlingPDF' -Label 'bundler template HKCU product key' | Out-Null

# Provisioning file. The MSI is perMachine (Package/@InstallScope), so WiX sets
# ALLUSERS=1 and only the CommonAppDataFolder branch of the write CustomAction
# can fire; the per-user path is probed anyway so a future scope change cannot
# quietly leave an unremoved file behind.
$systemProvisioning = Join-Path $SystemProvisioningDir $ProvisioningFileName
$userProvisioning = Join-Path $UserProvisioningDir $ProvisioningFileName
$provisioned = @()
if (Test-Path -LiteralPath $systemProvisioning) { $provisioned += $systemProvisioning }
if (Test-Path -LiteralPath $userProvisioning) { $provisioned += $userProvisioning }
Add-Result -Phase 'install' -Name 'provisioning file was written' -Ok ($provisioned.Count -gt 0) `
    -Detail "probed $systemProvisioning and $userProvisioning"
foreach ($file in $provisioned) { Write-Info ("provisioned: {0}" -f $file) }

# Sentinels standing in for the user state an uninstall must preserve.
$sentinels = @()
foreach ($dir in @($SystemProvisioningDir, $UserProvisioningDir)) {
    if (Test-Path -LiteralPath $dir) {
        $sentinel = Join-Path $dir $SentinelName
        Set-Content -LiteralPath $sentinel -Value 'user state that uninstall must not delete' -Encoding UTF8
        $sentinels += $sentinel
        Write-Info ("sentinel seeded: {0}" -f $sentinel)
    }
}

Write-Phase 'Uninstall (silent)'
Invoke-MsiExec -Label 'uninstall' -Arguments @(
    '/x', "`"$productCode`"",
    '/qn', '/norestart',
    '/l*v', "`"$uninstallLog`""
)

Write-Phase 'Post-uninstall assertions'

Add-Result -Phase 'uninstall' -Name 'Add/Remove Programs entry removed' -Ok ($null -eq (Get-ArpEntry -ProductCode $productCode)) -Detail 'ARP entry survived'

Assert-RegistryKeyAbsent -Phase 'uninstall' -Hive ([Microsoft.Win32.RegistryHive]::LocalMachine) `
    -SubKey "SOFTWARE\Classes\CLSID\$ThumbnailClsid" -Label 'thumbnail handler CLSID (and InprocServer32 subtree)'
Assert-RegistryKeyAbsent -Phase 'uninstall' -Hive ([Microsoft.Win32.RegistryHive]::LocalMachine) `
    -SubKey "SOFTWARE\Classes\.pdf\shellex\$ThumbnailProviderIid" -Label '.pdf IThumbnailProvider registration'
Assert-RegistryKeyAbsent -Phase 'uninstall' -Hive ([Microsoft.Win32.RegistryHive]::LocalMachine) `
    -SubKey "$CascadeRoot\$VerbKey" -Label "Explorer cascade tree '$VerbKey'"
Assert-RegistryKeyAbsent -Phase 'uninstall' -Hive ([Microsoft.Win32.RegistryHive]::LocalMachine) `
    -SubKey "$CascadeRoot\$LegacyVerbKey" -Label "legacy cascade tree '$LegacyVerbKey'"
Assert-RegistryKeyAbsent -Phase 'uninstall' -Hive ([Microsoft.Win32.RegistryHive]::CurrentUser) `
    -SubKey 'Software\RustlingPDF\RustlingPDF' -Label 'bundler template HKCU product key'

# Shared parents must survive: deleting them would take other applications'
# registrations with them.
foreach ($shared in @('SOFTWARE\Classes\CLSID', 'SOFTWARE\Classes\.pdf', 'SOFTWARE\Classes\SystemFileAssociations\.pdf\shell')) {
    $found = Test-RegistryKeyInAnyView -Hive ([Microsoft.Win32.RegistryHive]::LocalMachine) -SubKey $shared
    Add-Result -Phase 'uninstall' -Name ("shared key preserved: HKLM\{0}" -f $shared) -Ok ($found.Count -gt 0) `
        -Detail 'OVER-DELETION: a registry key shared with the rest of the system was removed'
}

Add-Result -Phase 'uninstall' -Name 'provisioning file removed (all users)' -Ok (-not (Test-Path -LiteralPath $systemProvisioning)) -Detail $systemProvisioning
Add-Result -Phase 'uninstall' -Name 'provisioning file removed (per user)' -Ok (-not (Test-Path -LiteralPath $userProvisioning)) -Detail $userProvisioning

foreach ($sentinel in $sentinels) {
    Add-Result -Phase 'uninstall' -Name ("user state preserved: {0}" -f $sentinel) -Ok (Test-Path -LiteralPath $sentinel) `
        -Detail 'uninstall deleted a file it does not own'
}

if ($null -ne $installDir) {
    Add-Result -Phase 'uninstall' -Name 'install directory removed' -Ok (-not (Test-Path -LiteralPath $installDir)) -Detail $installDir
    if (Test-Path -LiteralPath $installDir) {
        Get-ChildItem -LiteralPath $installDir -Recurse -Force -ErrorAction SilentlyContinue |
            ForEach-Object { Write-Info ("leftover in install dir: {0}" -f $_.FullName) }
    }
}

Write-Phase 'Deliberate leftovers (reported, not asserted)'
foreach ($path in @(
        $SystemProvisioningDir,
        $UserProvisioningDir,
        (Join-Path $env:LOCALAPPDATA 'stirling.pdf.dev'),
        (Join-Path $env:APPDATA 'stirling.pdf.dev'),
        (Join-Path $env:TEMP 'MicrosoftEdgeWebview2Setup.exe')
    )) {
    if (Test-Path -LiteralPath $path) { Write-Info ("kept by design: {0}" -f $path) }
}
$webview2 = Get-RegistryViewKey -Hive ([Microsoft.Win32.RegistryHive]::LocalMachine) -View ([Microsoft.Win32.RegistryView]::Registry32) `
    -SubKey 'SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
if ($null -ne $webview2) {
    Write-Info ("kept by design: WebView2 Runtime pv={0} (shared, refcounted, has its own ARP entry)" -f $webview2.GetValue('pv'))
    $webview2.Dispose()
}

# Cleanup -- only state this script created.
foreach ($sentinel in $sentinels) { Remove-Item -LiteralPath $sentinel -Force -ErrorAction SilentlyContinue }
Remove-SeededLegacyKey

Write-Phase 'Summary'
$script:Checks | Format-Table -AutoSize | Out-String -Width 200 | Write-Host
Write-Info "install log  : $installLog"
Write-Info "uninstall log: $uninstallLog"

if ($script:Failures.Count -gt 0) {
    Write-Host ''
    Write-Host ("{0} check(s) failed:" -f $script:Failures.Count) -ForegroundColor Red
    foreach ($failure in $script:Failures) { Write-Host ("  - {0}" -f $failure) -ForegroundColor Red }
    exit 1
}

Write-Host ''
Write-Host 'MSI install/uninstall lifecycle verified.' -ForegroundColor Green
exit 0
