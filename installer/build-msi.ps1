#Requires -Version 7.0

<#
.SYNOPSIS
    Builds the Toggle Audio MSI (WiX Toolset 7) and its SHA256 file.

.DESCRIPTION
    1. Reads the [package] version from Cargo.toml and checks that it is a valid
       MSI ProductVersion (three numeric parts, major and minor <= 255,
       patch <= 65535; SemVer pre-release suffixes are rejected).
    2. Checks that installer/License.rtf matches LICENSE (make-license-rtf.ps1 -Check).
    3. Runs "cargo build --release --locked" from the repository root unless
       -SkipBuild (cargo reads .cargo/config.toml, which links the C runtime
       statically, from the current directory, not from --manifest-path). An
       existing CARGO_TARGET_DIR is respected; otherwise target/ is used. Then
       fails if either executable imports the dynamic C runtime
       (VCRUNTIME140.dll or api-ms-win-crt-*.dll), so a package that would need
       the Visual C++ redistributable is never produced.
    4. Runs "wix build" on installer/toggle-audio.wxs. README.md is packaged
       when it exists at the repository root.
    5. Runs "wix msi validate" (ICE validation; no administrator rights needed).
    6. Reads ProductVersion, UpgradeCode and ProductCode back from the MSI and
       checks the first two.
    7. Prints the path, size and SHA256 and writes <msi>.sha256 next to it
       (sha256sum format: lowercase hash, two spaces, file name, LF).

    Non-interactive: never prompts, exits non-zero on any failure.

.PARAMETER Configuration
    Cargo profile whose executables are packaged: Release (default) or Debug.

.PARAMETER SkipBuild
    Package existing executables instead of running cargo build.

.PARAMETER OutDir
    Output folder. Default: installer/out. Relative paths are resolved against
    the repository root.

.PARAMETER AcceptEula
    Pass "-acceptEula wix7" to wix for this invocation only. Not needed when
    "wix eula accept wix7" has been run on the machine (as CI does).

.EXAMPLE
    ./installer/build-msi.ps1
.EXAMPLE
    ./installer/build-msi.ps1 -SkipBuild
#>
[CmdletBinding()]
param(
    [ValidateSet('Release', 'Debug')]
    [string] $Configuration = 'Release',
    [switch] $SkipBuild,
    [string] $OutDir = 'installer/out',
    [switch] $AcceptEula
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$UpgradeCode = '{C82A4013-F2FF-448E-A4AE-63CD73760A63}'
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$Wxs = Join-Path $PSScriptRoot 'toggle-audio.wxs'

function Invoke-Native {
    param([string] $Exe, [string[]] $Arguments, [string] $What)
    Write-Host "> $([IO.Path]::GetFileName($Exe)) $($Arguments -join ' ')"
    & $Exe @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$What failed with exit code $LASTEXITCODE." }
}

# ---- 1. Version from Cargo.toml ([package] table) -------------------------------------
$version = $null
$inPackage = $false
foreach ($line in Get-Content -LiteralPath (Join-Path $RepoRoot 'Cargo.toml')) {
    if ($line -match '^\s*\[([^\]]+)\]\s*(#.*)?$') { $inPackage = ($Matches[1].Trim() -eq 'package'); continue }
    if ($inPackage -and $line -match '^\s*version\s*=\s*"([^"]+)"') { $version = $Matches[1]; break }
}
if (-not $version) { throw 'Could not find version = "..." in the [package] table of Cargo.toml.' }
if ($version -notmatch '^(\d+)\.(\d+)\.(\d+)$') {
    throw "Cargo.toml version '$version' is not a plain MAJOR.MINOR.PATCH version. MSI ProductVersion has no pre-release field, and two MSIs with the same three-part version break upgrades."
}
if ([int]$Matches[1] -gt 255 -or [int]$Matches[2] -gt 255 -or [int]$Matches[3] -gt 65535) {
    throw "Version '$version' exceeds the MSI ProductVersion limits (major and minor <= 255, patch <= 65535)."
}
Write-Host "Toggle Audio $version ($Configuration)"

# ---- 2. License.rtf must match LICENSE -------------------------------------------------
& (Join-Path $PSScriptRoot 'make-license-rtf.ps1') -Check
if ($LASTEXITCODE -ne 0) { throw 'installer/License.rtf is out of date (see above).' }

# ---- 3. Executables ------------------------------------------------------------------------
$targetDir = if ($env:CARGO_TARGET_DIR) {
    if ([IO.Path]::IsPathRooted($env:CARGO_TARGET_DIR)) { $env:CARGO_TARGET_DIR } else { Join-Path $RepoRoot $env:CARGO_TARGET_DIR }
} else {
    Join-Path $RepoRoot 'target'
}
$profileDir = if ($Configuration -eq 'Release') { 'release' } else { 'debug' }
$binDir = [IO.Path]::GetFullPath((Join-Path $targetDir $profileDir))

if (-not $SkipBuild) {
    $cargo = (Get-Command cargo -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1).Source
    if (-not $cargo) { throw 'cargo was not found on PATH. Install Rust (rustup) or pass -SkipBuild.' }
    $cargoArgs = @('build', '--locked', '--manifest-path', (Join-Path $RepoRoot 'Cargo.toml'))
    if ($Configuration -eq 'Release') { $cargoArgs += '--release' }
    # Run from the repository root: cargo looks for .cargo/config.toml (the
    # +crt-static rustflags) starting at the current directory, not at the manifest.
    Push-Location -LiteralPath $RepoRoot
    try {
        Invoke-Native $cargo $cargoArgs 'cargo build'
    } finally {
        Pop-Location
    }
}
foreach ($exe in 'toggle-audio.exe', 'toggle-audiow.exe') {
    $exePath = Join-Path $binDir $exe
    if (-not (Test-Path -LiteralPath $exePath)) { throw "Missing $binDir\$exe. Build first or drop -SkipBuild." }
    # The executables must not need the Visual C++ redistributable. A dynamically
    # linked CRT shows up as these import names in the PE file.
    $bytes = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($exePath))
    foreach ($dll in 'VCRUNTIME140', 'api-ms-win-crt-') {
        if ($bytes.IndexOf($dll, [StringComparison]::OrdinalIgnoreCase) -ge 0) {
            throw "$exe imports $dll*.dll (dynamic C runtime). Build from the repository root so .cargo/config.toml applies (+crt-static), and do not override RUSTFLAGS."
        }
    }
}
Write-Host 'Static C runtime: OK (no VCRUNTIME140 or api-ms-win-crt imports).'
Write-Host "Executables: $binDir"

# ---- 4. wix build -------------------------------------------------------------------------
$wix = (Get-Command wix -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1).Source
if (-not $wix) {
    $candidate = Join-Path $env:USERPROFILE '.dotnet\tools\wix.exe'
    if (Test-Path -LiteralPath $candidate) { $wix = $candidate }
}
if (-not $wix) { throw 'wix.exe not found on PATH or in %USERPROFILE%\.dotnet\tools. Install it with: dotnet tool install --global wix --version 7.0.0' }
$wixVersion = (& $wix --version | Select-Object -First 1).Trim()
Write-Host "WiX: $wix ($wixVersion)"
if ($wixVersion -notmatch '^7\.') { Write-Warning "Expected WiX 7.x; found $wixVersion. Extension versions must match the wix.exe major version." }

$outDirFull = if ([IO.Path]::IsPathRooted($OutDir)) { $OutDir } else { Join-Path $RepoRoot $OutDir }
$outDirFull = [IO.Path]::GetFullPath($outDirFull)
New-Item -ItemType Directory -Force -Path $outDirFull | Out-Null
$msiName = "toggle-audio-$version-x64.msi"
$msi = Join-Path $outDirFull $msiName
$shaFile = "$msi.sha256"
foreach ($stale in $msi, $shaFile, [IO.Path]::ChangeExtension($msi, '.wixpdb')) {
    if (Test-Path -LiteralPath $stale) { Remove-Item -LiteralPath $stale -Force }
}

$eulaArgs = if ($AcceptEula) { @('-acceptEula', 'wix7') } else { @() }
$wixArgs = @($eulaArgs) + @(
    'build', '-arch', 'x64',
    '-ext', 'WixToolset.UI.wixext',
    '-ext', 'WixToolset.Util.wixext',
    '-d', "Version=$version",
    '-d', "BinDir=$binDir"
)
$readme = Join-Path $RepoRoot 'README.md'
if (Test-Path -LiteralPath $readme) {
    $wixArgs += @('-d', "ReadmeFile=$readme")
} else {
    Write-Warning 'README.md not found at the repository root; the MSI is built without it.'
}
$wixArgs += @('-o', $msi, $Wxs)
Invoke-Native $wix $wixArgs 'wix build (if it reports the EULA, run "wix eula accept wix7" once or pass -AcceptEula)'

# ---- 5. ICE validation ----------------------------------------------------------------------
Invoke-Native $wix (@($eulaArgs) + @('msi', 'validate', $msi)) 'wix msi validate'
Write-Host 'ICE validation passed.'

# ---- 6. Read identity back from the package -----------------------------------------------
$props = @{}
$installer = New-Object -ComObject WindowsInstaller.Installer
try {
    $db = $installer.OpenDatabase($msi, 0)   # 0 = read-only
    $view = $db.OpenView("SELECT ``Property``, ``Value`` FROM ``Property`` WHERE ``Property`` = 'ProductVersion' OR ``Property`` = 'UpgradeCode' OR ``Property`` = 'ProductCode'")
    $view.Execute()
    while ($record = $view.Fetch()) { $props[$record.StringData(1)] = $record.StringData(2) }
    $view.Close()
} finally {
    foreach ($o in @(Get-Variable view, db, installer -ValueOnly -ErrorAction SilentlyContinue)) {
        if ($o) { [void][Runtime.InteropServices.Marshal]::ReleaseComObject($o) }
    }
}
if ($props['ProductVersion'] -ne $version) { throw "MSI ProductVersion '$($props['ProductVersion'])' does not match Cargo.toml '$version'." }
if ($props['UpgradeCode'] -ne $UpgradeCode) { throw "MSI UpgradeCode '$($props['UpgradeCode'])' is not $UpgradeCode. The UpgradeCode must never change." }

# ---- 7. Hash -----------------------------------------------------------------------------------
$hash = (Get-FileHash -LiteralPath $msi -Algorithm SHA256).Hash.ToLowerInvariant()
[IO.File]::WriteAllText($shaFile, "$hash  $msiName`n", [Text.Encoding]::ASCII)
$size = (Get-Item -LiteralPath $msi).Length

Write-Host ''
Write-Host "MSI:            $msi"
Write-Host "Size:           $size bytes"
Write-Host "SHA256:         $hash"
Write-Host "ProductVersion: $($props['ProductVersion'])"
Write-Host "ProductCode:    $($props['ProductCode'])  (new for every build)"
Write-Host "UpgradeCode:    $($props['UpgradeCode'])  (constant)"
Write-Host "Hash file:      $shaFile"
