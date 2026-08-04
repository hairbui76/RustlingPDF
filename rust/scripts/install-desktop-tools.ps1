Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$QpdfVersion = '12.3.2'
$TesseractVersion = '5.5.3'
$TessdataVersion = '4.1.0'

$QpdfUrl = "https://github.com/qpdf/qpdf/releases/download/v$QpdfVersion/qpdf-$QpdfVersion-msvc64.zip"
$QpdfSha256 = '8941870a604e7c87ed24566b038d46c24ce76616254d2383c578f60c0677f202'
# Windows keeps the official upstream installer: unlike Linux and macOS,
# tesseract-ocr/tesseract does publish a Windows binary, and this asset was
# uploaded by maintainer stweil on the 5.5.3 release. It is first-party, so
# there is nothing to gain from building it ourselves.
$TesseractInstallerUrl = "https://github.com/tesseract-ocr/tesseract/releases/download/$TesseractVersion/tesseract-ocr-w64-setup-5.5.3.20260724.exe"
$TesseractInstallerSha256 = 'bee9e3434bd94fd65387d9be28cd467a41f61b1275383b55b0f59a1331270ae4'
$TesseractSourceUrl = "https://github.com/tesseract-ocr/tesseract/archive/${TesseractVersion}.tar.gz"
$TesseractSourceSha256 = '9218e62793116d42a9f6d14cd9348518b27f382096eea3d0f2d1a24616bb5884'
$EngUrl = "https://raw.githubusercontent.com/tesseract-ocr/tessdata_fast/$TessdataVersion/eng.traineddata"
$EngSha256 = '7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2'
$PdfTtfSha256 = 'c7845420925a23d88ed830a63957b8af85a66a8daf8d9fc90e843673b2ef1a59'
$PdfConfigSha256 = '54d56e81dfefe289b0d2e4c7bc9bbe662711f5b8255c128b73bcd66efb6bba1a'

# JBIG-free libtiff swap.
#
# The upstream Tesseract Windows installer's `libtiff-6.dll` is the MSYS2
# `mingw-w64-x86_64-libtiff` 4.7.2 build, configured `--enable-jbig`, so it
# NORMAL-imports `libjbig-0.dll` (JBIG-KIT, GPL-2.0-or-later). JBIG-KIT is the
# only GPL-2.0 component in any RustlingPDF bundle. Before the PE import-closure
# is computed below, the installer's `libtiff-6.dll` is replaced with a
# byte-for-byte-equivalent build that is identical except JBIG is disabled,
# produced by `.github/workflows/desktop-tools-libtiff-windows.yml` and pinned
# here by SHA-256. Because the closure is import-driven, a JBIG-free libtiff
# means `libjbig-0.dll` is never reached and drops out of the staged bundle on
# its own — nothing is deleted by hand. Bump procedure: RELEASING.md, "Bumping
# the bundled Tesseract".
$LibtiffNoJbigVersion = '4.7.2'
$LibtiffNoJbigRepository = 'hairbui76/RustlingPDF'
$LibtiffNoJbigTag = "desktop-tools/libtiff-$LibtiffNoJbigVersion-nojbig"
$LibtiffNoJbigAsset = "libtiff-$LibtiffNoJbigVersion-windows-x86_64-mingw-nojbig.dll"
$LibtiffNoJbigUrl = if ($env:RUSTLING_DESKTOP_LIBTIFF_NOJBIG_URL) {
    $env:RUSTLING_DESKTOP_LIBTIFF_NOJBIG_URL
}
else {
    "https://github.com/$LibtiffNoJbigRepository/releases/download/$LibtiffNoJbigTag/$LibtiffNoJbigAsset"
}
# PENDING_CI_BUILD until desktop-tools-libtiff-windows.yml has published the
# attested artefact and its SHA-256 is copied in here — the same discipline as
# `tesseract_linux_sha256` in install-desktop-tools.sh. An env override builds
# against an artefact published elsewhere.
$LibtiffNoJbigSha256 = if ($env:RUSTLING_DESKTOP_LIBTIFF_NOJBIG_SHA256) {
    $env:RUSTLING_DESKTOP_LIBTIFF_NOJBIG_SHA256
}
else {
    # Published by desktop-tools-libtiff-windows.yml (run with publish=true) to
    # the desktop-tools/libtiff-4.7.2-nojbig release. MSYS2/MinGW builds are not
    # bit-reproducible, so this is the exact digest of the published asset.
    'fb23a3ff5e226ec430753e6012f8185d1461329ab5f3c238c046fd03b4e3c7b9'
}

$Architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
if ($Architecture -ne 'x64') {
    throw "No pinned desktop-tool artifact set for Windows architecture=$Architecture. Supported release target: Windows/x64."
}

$Workspace = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$CacheRoot = Join-Path $Workspace '.desktop-tools'
$ArchiveDirectory = Join-Path $CacheRoot 'archives'
$InstallRoot = Join-Path $CacheRoot "$QpdfVersion-$TesseractVersion-windows-x86_64"
$Current = Join-Path $CacheRoot 'current'
$Work = Join-Path $CacheRoot 'work-windows-x86_64'
$LicenseSource = Join-Path $PSScriptRoot 'desktop-tools'

$QpdfArchive = Join-Path $ArchiveDirectory "qpdf-$QpdfVersion-msvc64.zip"
$TesseractInstaller = Join-Path $ArchiveDirectory "tesseract-$TesseractVersion-windows-x86_64.exe"
$TesseractSourceArchive = Join-Path $ArchiveDirectory "tesseract-$TesseractVersion.tar.gz"
$EngArchive = Join-Path $ArchiveDirectory "eng-$TessdataVersion.traineddata"
$LibtiffNoJbigDownload = Join-Path $ArchiveDirectory $LibtiffNoJbigAsset

function Test-Checksum {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$ExpectedSha256
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        return $false
    }
    $ActualSha256 = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
    return $ActualSha256 -eq $ExpectedSha256
}

function Assert-Checksum {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$ExpectedSha256
    )

    $ActualSha256 = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($ActualSha256 -ne $ExpectedSha256) {
        throw "Checksum mismatch for ${Path}: expected $ExpectedSha256, received $ActualSha256"
    }
}

function Get-PinnedFile {
    param(
        [Parameter(Mandatory = $true)][string]$Url,
        [Parameter(Mandatory = $true)][string]$ExpectedSha256,
        [Parameter(Mandatory = $true)][string]$Output
    )

    if (Test-Checksum -Path $Output -ExpectedSha256 $ExpectedSha256) {
        return
    }
    if (Test-Path -LiteralPath $Output) {
        Remove-Item -LiteralPath $Output -Force
    }

    Write-Output "Downloading pinned desktop tool input: $Url"
    Invoke-WebRequest -Headers @{ 'User-Agent' = 'RustlingPDF-Desktop-Build' } -Uri $Url -OutFile $Output
    if (-not (Test-Checksum -Path $Output -ExpectedSha256 $ExpectedSha256)) {
        $ActualSha256 = (Get-FileHash -LiteralPath $Output -Algorithm SHA256).Hash.ToLowerInvariant()
        Remove-Item -LiteralPath $Output -Force
        throw "Checksum mismatch for ${Url}: expected $ExpectedSha256, received $ActualSha256"
    }
}

# --- PE dependency closure -------------------------------------------------
#
# The bundle ships exactly the DLLs `qpdf.exe` and `tesseract.exe` can actually
# reach, computed by walking the PE import and delay-import tables. The rule
# this replaces — "copy every DLL sitting next to the executable" — dragged in
# Tesseract's *training-tool* stack: ICU (whose data DLL alone is 33 MiB),
# pango, cairo, glib, HarfBuzz, FreeType and fontconfig, none of which
# tesseract.exe or libtesseract-5.dll import. See SOURCES.md for the
# measurements.
#
# The parser is written out here rather than shelling out to dumpbin/objdump so
# the installer has no dependency on a Visual Studio or binutils installation
# being present and on PATH.

function Get-PeImportedDll {
    <#
        .SYNOPSIS
        Names of the DLLs a PE image imports, from both the import table and
        the delay-import table. Returns an empty array for a file that is not
        a PE image.
    #>
    param(
        [Parameter(Mandatory = $true)][string]$Path
    )

    $bytes = [System.IO.File]::ReadAllBytes($Path)
    if ($bytes.Length -lt 0x40) { return @() }
    # 'MZ'
    if ($bytes[0] -ne 0x4D -or $bytes[1] -ne 0x5A) { return @() }

    $peOffset = [System.BitConverter]::ToInt32($bytes, 0x3C)
    if ($peOffset -le 0 -or ($peOffset + 24) -ge $bytes.Length) { return @() }
    # 'PE\0\0'
    if ([System.BitConverter]::ToUInt32($bytes, $peOffset) -ne 0x00004550) { return @() }

    $coffOffset = $peOffset + 4
    $numberOfSections = [System.BitConverter]::ToUInt16($bytes, $coffOffset + 2)
    $sizeOfOptionalHeader = [System.BitConverter]::ToUInt16($bytes, $coffOffset + 16)
    $optionalOffset = $coffOffset + 20
    if ($sizeOfOptionalHeader -eq 0) { return @() }

    $magic = [System.BitConverter]::ToUInt16($bytes, $optionalOffset)
    if ($magic -eq 0x20B) {
        # PE32+
        $imageBase = [System.BitConverter]::ToUInt64($bytes, $optionalOffset + 24)
        $dataDirectoryOffset = $optionalOffset + 112
    }
    elseif ($magic -eq 0x10B) {
        # PE32
        $imageBase = [uint64][System.BitConverter]::ToUInt32($bytes, $optionalOffset + 28)
        $dataDirectoryOffset = $optionalOffset + 96
    }
    else {
        return @()
    }

    $sectionOffset = $optionalOffset + $sizeOfOptionalHeader
    $sections = @()
    for ($index = 0; $index -lt $numberOfSections; $index++) {
        $entry = $sectionOffset + ($index * 40)
        if (($entry + 40) -gt $bytes.Length) { break }
        $sections += [pscustomobject]@{
            VirtualAddress   = [System.BitConverter]::ToUInt32($bytes, $entry + 12)
            VirtualSize      = [System.BitConverter]::ToUInt32($bytes, $entry + 8)
            SizeOfRawData    = [System.BitConverter]::ToUInt32($bytes, $entry + 16)
            PointerToRawData = [System.BitConverter]::ToUInt32($bytes, $entry + 20)
        }
    }

    # An RVA that falls in no section, or past the end of the file, means a
    # malformed or truncated image: return $null so the caller stops walking
    # rather than reading arbitrary bytes.
    function Convert-RvaToOffset {
        param([uint32]$Rva)
        foreach ($section in $sections) {
            $span = [Math]::Max($section.VirtualSize, $section.SizeOfRawData)
            if ($Rva -ge $section.VirtualAddress -and $Rva -lt ($section.VirtualAddress + $span)) {
                $offset = $section.PointerToRawData + ($Rva - $section.VirtualAddress)
                if ($offset -lt $bytes.Length) { return [int]$offset }
                return $null
            }
        }
        return $null
    }

    function Read-AsciiAt {
        param([int]$Offset)
        $end = $Offset
        while ($end -lt $bytes.Length -and $bytes[$end] -ne 0) { $end++ }
        if ($end -eq $Offset) { return $null }
        return [System.Text.Encoding]::ASCII.GetString($bytes, $Offset, $end - $Offset)
    }

    $names = [System.Collections.Generic.List[string]]::new()

    # Data directory 1: import table. Descriptors are 20 bytes; the DLL name
    # RVA is at +12; an all-zero descriptor terminates the array.
    $importRva = [System.BitConverter]::ToUInt32($bytes, $dataDirectoryOffset + 8)
    if ($importRva -ne 0) {
        $cursor = Convert-RvaToOffset -Rva $importRva
        while ($null -ne $cursor -and ($cursor + 20) -le $bytes.Length) {
            $nameRva = [System.BitConverter]::ToUInt32($bytes, $cursor + 12)
            $firstThunk = [System.BitConverter]::ToUInt32($bytes, $cursor + 16)
            if ($nameRva -eq 0 -and $firstThunk -eq 0) { break }
            if ($nameRva -ne 0) {
                $nameOffset = Convert-RvaToOffset -Rva $nameRva
                if ($null -ne $nameOffset) {
                    $name = Read-AsciiAt -Offset $nameOffset
                    if ($name) { $names.Add($name) }
                }
            }
            $cursor += 20
        }
    }

    # Data directory 13: delay-import descriptors, 32 bytes each, DLL name at
    # +4. When bit 0 of Attributes (+0) is clear the descriptor stores virtual
    # addresses rather than RVAs, so the image base has to come back off.
    $delayRva = [System.BitConverter]::ToUInt32($bytes, $dataDirectoryOffset + (13 * 8))
    if ($delayRva -ne 0) {
        $cursor = Convert-RvaToOffset -Rva $delayRva
        while ($null -ne $cursor -and ($cursor + 32) -le $bytes.Length) {
            $attributes = [System.BitConverter]::ToUInt32($bytes, $cursor)
            $nameField = [uint64][System.BitConverter]::ToUInt32($bytes, $cursor + 4)
            if ($attributes -eq 0 -and $nameField -eq 0) { break }
            if ($nameField -ne 0) {
                if (($attributes -band 1) -eq 0 -and $nameField -ge $imageBase) {
                    $nameField = $nameField - $imageBase
                }
                $nameOffset = Convert-RvaToOffset -Rva ([uint32]$nameField)
                if ($null -ne $nameOffset) {
                    $name = Read-AsciiAt -Offset $nameOffset
                    if ($name) { $names.Add($name) }
                }
            }
            $cursor += 32
        }
    }

    return $names.ToArray()
}

function Copy-PeImportClosure {
    <#
        .SYNOPSIS
        Copies the executable named by -RootName plus every DLL transitively
        reachable from it that exists in -SourceDirectory, into -Destination.
        Imports that are not present in -SourceDirectory are Windows system
        DLLs and are deliberately not shipped.
    #>
    param(
        [Parameter(Mandatory = $true)][string]$SourceDirectory,
        [Parameter(Mandatory = $true)][string]$RootName,
        [Parameter(Mandatory = $true)][string]$Destination
    )

    $available = @{}
    foreach ($file in Get-ChildItem -LiteralPath $SourceDirectory -File) {
        $available[$file.Name.ToLowerInvariant()] = $file
    }
    if (-not $available.ContainsKey($RootName.ToLowerInvariant())) {
        throw "Cannot compute an import closure: $RootName is not in $SourceDirectory"
    }

    $seen = [System.Collections.Generic.HashSet[string]]::new()
    $shipped = [System.Collections.Generic.List[System.IO.FileInfo]]::new()
    $system = [System.Collections.Generic.HashSet[string]]::new()
    $queue = [System.Collections.Generic.Queue[string]]::new()
    $queue.Enqueue($RootName)

    while ($queue.Count -gt 0) {
        $current = $queue.Dequeue()
        $key = $current.ToLowerInvariant()
        if (-not $seen.Add($key)) { continue }
        if (-not $available.ContainsKey($key)) {
            [void]$system.Add($current)
            continue
        }
        $file = $available[$key]
        [void]$shipped.Add($file)
        foreach ($dependency in (Get-PeImportedDll -Path $file.FullName)) {
            $queue.Enqueue($dependency)
        }
    }

    New-Item -ItemType Directory -Force $Destination | Out-Null
    $total = 0
    foreach ($file in $shipped) {
        Copy-Item -LiteralPath $file.FullName -Destination (Join-Path $Destination $file.Name) -Force
        $total += $file.Length
    }

    $skipped = $available.Values |
        Where-Object { $_.Extension -eq '.dll' -and -not $seen.Contains($_.Name.ToLowerInvariant()) }
    $skippedBytes = ($skipped | Measure-Object -Property Length -Sum).Sum
    if ($null -eq $skippedBytes) { $skippedBytes = 0 }

    Write-Output ("Import closure of {0}: {1} files, {2:N2} MiB" -f `
            $RootName, $shipped.Count, ($total / 1MB))
    Write-Output ("  resolved from the system directory: {0}" -f `
        (($system | Sort-Object) -join ', '))
    Write-Output ("  not shipped (unreachable): {0} DLLs, {1:N2} MiB" -f `
            @($skipped).Count, ($skippedBytes / 1MB))
    foreach ($file in ($skipped | Sort-Object Name)) {
        Write-Output ("    - {0} ({1:N2} MiB)" -f $file.Name, ($file.Length / 1MB))
    }
}

function Assert-NoJbigImport {
    <#
        .SYNOPSIS
        Fails if any staged PE file is, or imports, a JBIG library. This is the
        gate that proves the JBIG-free libtiff swap eliminated libjbig-0.dll:
        it walks BOTH the import and delay-import tables of every staged file
        (via Get-PeImportedDll) and rejects the bundle if any `jbig` reference
        survives, and separately rejects a shipped `*jbig*` DLL.
    #>
    param(
        [Parameter(Mandatory = $true)][string[]]$Directories
    )

    foreach ($directory in $Directories) {
        if (-not (Test-Path -LiteralPath $directory -PathType Container)) { continue }
        foreach ($file in Get-ChildItem -LiteralPath $directory -File) {
            if ($file.Name -match '(?i)jbig') {
                throw "A JBIG library was staged into the bundle: $($file.FullName). The JBIG-free libtiff swap did not take effect."
            }
            foreach ($import in (Get-PeImportedDll -Path $file.FullName)) {
                if ($import -match '(?i)jbig') {
                    throw "$($file.Name) still imports $import; libjbig has not been eliminated from the PE import closure."
                }
            }
        }
    }
    Write-Output "Verified: no JBIG library is shipped or imported anywhere in the staged bundle."
}

# --- MinGW debug-payload strip ----------------------------------------------
# The official Tesseract installer ships its MinGW runtime with full DWARF
# debug info — libstdc++-6.dll alone is 26.4 MB of which 21.2 MB is .debug_*
# sections — which is pure installer weight (measured: −5.8 MB compressed on
# the NSIS installer). Stripping is limited to UNSIGNED binaries:
# tesseract.exe and libtesseract-5.dll carry Authenticode signatures that a
# strip would invalidate, so they keep their (comparatively small) debug
# payload. The staged qpdf closure is MSVC-built and ships no debug sections,
# so only the Tesseract directory is processed.
#
# GNU objcopy is the required tool: llvm-objcopy (LLVM 19) rejects
# libstdc++-6.dll with "invalid SymbolTableIndex", verified 2026-08-04, so
# LLVM is deliberately NOT a fallback. The runner's MSYS2 provides objcopy
# via its binutils package, installed on demand below.
function Resolve-GnuObjcopy {
    $msysObjcopy = 'C:\msys64\usr\bin\objcopy.exe'
    if (Test-Path -LiteralPath $msysObjcopy) {
        return $msysObjcopy
    }
    $onPath = Get-Command objcopy -ErrorAction SilentlyContinue
    if ($null -ne $onPath) {
        return $onPath.Source
    }
    $pacman = 'C:\msys64\usr\bin\pacman.exe'
    if (Test-Path -LiteralPath $pacman) {
        Write-Output 'Installing binutils via MSYS2 pacman for objcopy'
        & $pacman -S --noconfirm binutils | Out-Null
        if (Test-Path -LiteralPath $msysObjcopy) {
            return $msysObjcopy
        }
    }
    throw 'No GNU objcopy available to strip the MinGW debug payload. Install binutils (MSYS2: pacman -S binutils) or put objcopy on PATH.'
}

function Remove-UnsignedPeDebugSections {
    param(
        [Parameter(Mandatory = $true)][string]$Directory
    )
    $objcopy = Resolve-GnuObjcopy
    $before = 0
    $after = 0
    foreach ($pe in Get-ChildItem -LiteralPath $Directory -File |
        Where-Object { $_.Extension -in @('.dll', '.exe') }) {
        $signature = Get-AuthenticodeSignature -LiteralPath $pe.FullName
        if ($signature.Status -ne 'NotSigned') {
            Write-Output "Keeping signed binary intact: $($pe.Name)"
            continue
        }
        $before += $pe.Length
        & $objcopy --strip-debug $pe.FullName
        if ($LASTEXITCODE -ne 0) {
            throw "objcopy --strip-debug failed on $($pe.FullName)"
        }
        $after += (Get-Item -LiteralPath $pe.FullName).Length
    }
    Write-Output ("Stripped MinGW debug sections: {0:N0} -> {1:N0} bytes" -f $before, $after)
    # Regression tripwire: libstdc++-6.dll is the whale (26.4 MB unstripped,
    # 4.6 MB stripped). If it is still fat the strip silently did nothing —
    # fail the build rather than ship the debug payload again.
    $libstdcxx = Join-Path $Directory 'libstdc++-6.dll'
    if ((Test-Path -LiteralPath $libstdcxx) -and (Get-Item -LiteralPath $libstdcxx).Length -gt 8MB) {
        throw "libstdc++-6.dll is still larger than 8 MB after the strip pass; the debug payload was not removed."
    }
}

function Copy-NamedNotices {
    param(
        [Parameter(Mandatory = $true)][string]$Source,
        [Parameter(Mandatory = $true)][string]$Destination
    )

    New-Item -ItemType Directory -Force $Destination | Out-Null
    Get-ChildItem -LiteralPath $Source -Recurse -File |
        Where-Object { $_.Name -match '^(LICENSE|LICENCE|NOTICE)' } |
        ForEach-Object {
            Copy-Item -LiteralPath $_.FullName -Destination (Join-Path $Destination $_.Name) -Force
        }
}

New-Item -ItemType Directory -Force $ArchiveDirectory | Out-Null
Write-Output "Installing pinned desktop tools for OS=Windows architecture=$Architecture"

Get-PinnedFile -Url $QpdfUrl -ExpectedSha256 $QpdfSha256 -Output $QpdfArchive
Get-PinnedFile -Url $TesseractInstallerUrl -ExpectedSha256 $TesseractInstallerSha256 -Output $TesseractInstaller
Get-PinnedFile -Url $TesseractSourceUrl -ExpectedSha256 $TesseractSourceSha256 -Output $TesseractSourceArchive
Get-PinnedFile -Url $EngUrl -ExpectedSha256 $EngSha256 -Output $EngArchive

if ($LibtiffNoJbigSha256 -eq 'PENDING_CI_BUILD') {
    throw @"
No published JBIG-free Windows libtiff artefact is pinned for $LibtiffNoJbigVersion yet.

The installer's libtiff-6.dll imports libjbig-0.dll (JBIG-KIT, GPL-2.0-or-later).
Rather than ship that, this script swaps in a JBIG-free libtiff-6.dll built by
this repository's own CI. Run the "Desktop tools - build Windows libtiff
(JBIG-free)" workflow (.github/workflows/desktop-tools-libtiff-windows.yml) with
publish=true, then copy the SHA-256 it reports into the LibtiffNoJbigSha256
variable in this script. The full procedure is under "Bumping the bundled
Tesseract" in RELEASING.md.

Expected release: https://github.com/$LibtiffNoJbigRepository/releases/tag/$LibtiffNoJbigTag
Expected asset:   $LibtiffNoJbigAsset

To build against an artefact published elsewhere, set both
RUSTLING_DESKTOP_LIBTIFF_NOJBIG_URL and RUSTLING_DESKTOP_LIBTIFF_NOJBIG_SHA256.
"@
}
Get-PinnedFile -Url $LibtiffNoJbigUrl -ExpectedSha256 $LibtiffNoJbigSha256 -Output $LibtiffNoJbigDownload

if (Test-Path -LiteralPath $Work) {
    Remove-Item -LiteralPath $Work -Recurse -Force
}
if (Test-Path -LiteralPath $InstallRoot) {
    Remove-Item -LiteralPath $InstallRoot -Recurse -Force
}

$QpdfExtracted = Join-Path $Work 'qpdf'
$TesseractExtracted = Join-Path $Work 'tesseract'
$TesseractSourceExtracted = Join-Path $Work 'tesseract-source'
$QpdfDestination = Join-Path $InstallRoot 'qpdf'
$TesseractDestination = Join-Path $InstallRoot 'tesseract'
$QpdfBin = Join-Path $QpdfDestination 'bin'
$QpdfLib = Join-Path $QpdfDestination 'lib'
$TesseractBin = Join-Path $TesseractDestination 'bin'
$TesseractLib = Join-Path $TesseractDestination 'lib'
$Tessdata = Join-Path $TesseractDestination 'tessdata'
$Licenses = Join-Path $InstallRoot 'licenses'
New-Item -ItemType Directory -Force `
    $QpdfExtracted, $TesseractExtracted, $TesseractSourceExtracted, `
    $QpdfBin, $QpdfLib, $TesseractBin, $TesseractLib, `
    (Join-Path $Tessdata 'configs'), $Licenses | Out-Null

Expand-Archive -LiteralPath $QpdfArchive -DestinationPath $QpdfExtracted -Force
$QpdfExecutable = Get-ChildItem -LiteralPath $QpdfExtracted -Recurse -File -Filter 'qpdf.exe' |
    Where-Object { $_.Directory.Name -eq 'bin' } |
    Select-Object -First 1
if ($null -eq $QpdfExecutable) {
    throw "The qpdf archive did not contain bin\qpdf.exe under $QpdfExtracted"
}
Copy-PeImportClosure `
    -SourceDirectory $QpdfExecutable.Directory.FullName `
    -RootName 'qpdf.exe' `
    -Destination $QpdfBin
$QpdfArchiveRoot = $QpdfExecutable.Directory.Parent.FullName

# The upstream 5.5.0 NSIS installer is checksum-pinned before execution. It
# installs only into the repository cache on the ephemeral build runner; the
# final bundle receives tesseract.exe and its adjacent runtime DLLs, not the
# installer, uninstaller, shortcuts, or training utilities.
$InstallerArguments = @('/S', "/D=$TesseractExtracted")
Write-Output "Extracting Tesseract installer to: $TesseractExtracted"
$InstallerProcess = Start-Process `
    -FilePath $TesseractInstaller `
    -ArgumentList $InstallerArguments `
    -Wait `
    -PassThru `
    -NoNewWindow
if ($InstallerProcess.ExitCode -ne 0) {
    throw "Tesseract installer failed with exit code $($InstallerProcess.ExitCode); extraction target was $TesseractExtracted"
}
$TesseractExecutable = Get-ChildItem -LiteralPath $TesseractExtracted -Recurse -File -Filter 'tesseract.exe' |
    Select-Object -First 1
if ($null -eq $TesseractExecutable) {
    throw "The Tesseract installer did not produce tesseract.exe under $TesseractExtracted"
}

# Swap the installer's JBIG-linked libtiff-6.dll for our JBIG-free build BEFORE
# the import closure is computed, so the closure walk never reaches
# libjbig-0.dll and it drops out of the staged bundle on its own. See the pin
# block at the top of this script.
$InstallerLibtiff = Join-Path $TesseractExecutable.Directory.FullName 'libtiff-6.dll'
if (-not (Test-Path -LiteralPath $InstallerLibtiff -PathType Leaf)) {
    throw "The Tesseract installer did not contain libtiff-6.dll next to tesseract.exe (searched $($TesseractExecutable.Directory.FullName)); the JBIG-free swap cannot proceed. If a newer installer renamed or dropped libtiff, revisit the swap in this script."
}
Copy-Item -LiteralPath $LibtiffNoJbigDownload -Destination $InstallerLibtiff -Force
Assert-Checksum -Path $InstallerLibtiff -ExpectedSha256 $LibtiffNoJbigSha256
Write-Output "Replaced the installer's libtiff-6.dll with the JBIG-free build ($LibtiffNoJbigAsset)"

Copy-PeImportClosure `
    -SourceDirectory $TesseractExecutable.Directory.FullName `
    -RootName 'tesseract.exe' `
    -Destination $TesseractBin

# Strip the MinGW DWARF payload from the staged (not cached) copies. Runs
# before the JBIG gate so that gate re-parses every stripped PE's import
# tables — a corrupted strip fails loudly there, and the staged
# `tesseract.exe --version` smoke test below exercises the stripped DLLs.
Remove-UnsignedPeDebugSections -Directory $TesseractBin

# Gate: the swap must have removed every JBIG reference from what was actually
# staged (both bins), across import and delay-import tables.
Assert-NoJbigImport -Directories @($QpdfBin, $TesseractBin)

# Windows' own bsdtar is selected explicitly so a Git Bash tar earlier on
# PATH cannot misparse drive letters as remote hosts.
$Tar = Join-Path $env:SystemRoot 'System32\tar.exe'
if (-not (Test-Path -LiteralPath $Tar)) {
    $Tar = 'tar'
}
& $Tar -xzf $TesseractSourceArchive -C $TesseractSourceExtracted
if ($LASTEXITCODE -ne 0) {
    throw "Could not extract $TesseractSourceArchive to $TesseractSourceExtracted"
}
$TesseractSourceRoot = Join-Path $TesseractSourceExtracted "tesseract-$TesseractVersion"
if (-not (Test-Path -LiteralPath $TesseractSourceRoot -PathType Container)) {
    throw "Tesseract source archive did not contain $TesseractSourceRoot"
}

Copy-Item -LiteralPath $EngArchive -Destination (Join-Path $Tessdata 'eng.traineddata') -Force
Copy-Item -LiteralPath (Join-Path $TesseractSourceRoot 'tessdata\pdf.ttf') -Destination (Join-Path $Tessdata 'pdf.ttf') -Force
Copy-Item -LiteralPath (Join-Path $TesseractSourceRoot 'tessdata\configs\pdf') -Destination (Join-Path $Tessdata 'configs\pdf') -Force
Assert-Checksum -Path (Join-Path $Tessdata 'eng.traineddata') -ExpectedSha256 $EngSha256
Assert-Checksum -Path (Join-Path $Tessdata 'pdf.ttf') -ExpectedSha256 $PdfTtfSha256
Assert-Checksum -Path (Join-Path $Tessdata 'configs\pdf') -ExpectedSha256 $PdfConfigSha256

Copy-Item -LiteralPath (Join-Path $LicenseSource 'THIRD-PARTY-NOTICES.txt') -Destination $Licenses -Force
Copy-Item -LiteralPath (Join-Path $LicenseSource 'SOURCES.md') -Destination $Licenses -Force
# Every vendored license text, including licenses/qpdf/{LICENSE.txt,NOTICE.md}
# (the upstream qpdf archives ship no license file at all) and the
# licenses/windows/ set covering the MinGW DLL closure. The copyleft texts the
# closure requires: the LGPL texts for libiconv, gettext, libidn2 and
# libunistring; and the GPL-3.0 + GCC Runtime Library Exception 3.1 texts for
# libgcc_s_seh-1.dll and libstdc++-6.dll — the latter two live at the
# license-texts top level (not under windows/) because they also cover the Unix
# builds, and the recursive copy here places them in licenses/ alongside
# windows/. (The JBIG-free libtiff swap above means no GPL-2.0 component ships,
# so no GPL-2.0 text is copied.)
Copy-Item -Path (Join-Path $LicenseSource 'license-texts\*') -Destination $Licenses -Recurse -Force
Copy-NamedNotices -Source $QpdfArchiveRoot -Destination (Join-Path $Licenses 'qpdf')
Copy-NamedNotices -Source $TesseractExtracted -Destination (Join-Path $Licenses 'tesseract')

$QpdfCommand = Join-Path $QpdfBin 'qpdf.exe'
$TesseractCommand = Join-Path $TesseractBin 'tesseract.exe'
& $QpdfCommand --version
if ($LASTEXITCODE -ne 0) {
    throw "Staged qpdf failed to execute: $QpdfCommand"
}
& $TesseractCommand --version
if ($LASTEXITCODE -ne 0) {
    throw "Staged Tesseract failed to execute: $TesseractCommand"
}

if (Test-Path -LiteralPath $Current) {
    Remove-Item -LiteralPath $Current -Recurse -Force
}
Copy-Item -LiteralPath $InstallRoot -Destination $Current -Recurse -Force
Remove-Item -LiteralPath $Work -Recurse -Force

Write-Output "Pinned desktop tools installed at: $Current"
Get-ChildItem -LiteralPath $Current -Recurse -File |
    Sort-Object FullName |
    ForEach-Object { Write-Output $_.FullName }
