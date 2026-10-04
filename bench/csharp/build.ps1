#Requires -Version 7.0

<#
.SYNOPSIS
    Builds bench\csharp\bin\ta-cs.exe (C# .NET 9 NativeAOT) non-interactively.

.DESCRIPTION
    Runs `dotnet publish -c Release -r win-x64` on ta-cs.csproj, copies the native exe to
    bin\ta-cs.exe, checks that the consoleAllocationPolicy=detached manifest is embedded
    (embedding it with mt.exe if it is not), and prints the size.

    Prerequisites: .NET 9 SDK, and the "Desktop development with C++" workload of Visual Studio
    2022 or Build Tools (NativeAOT links with the MSVC linker and the Windows SDK).
    Safe to run repeatedly: the native image is reproducible (link.exe /Brepro), and bin\ta-cs.exe
    is only replaced when its bytes change.
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$here     = $PSScriptRoot
$project  = Join-Path $here 'ta-cs.csproj'
$manifest = Join-Path $here '..\common\detached.manifest' | Resolve-Path | Select-Object -ExpandProperty Path
$publish  = Join-Path $here 'obj\publish'
$binDir   = Join-Path $here 'bin'
$exe      = Join-Path $binDir 'ta-cs.exe'

# NativeAOT finds the MSVC linker through vcvarsall.bat, which runs vswhere.exe unqualified.
# Without the VS Installer directory on PATH its error text pollutes the linker path (MSB3073).
$vsInstaller = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer'
if (Test-Path $vsInstaller) {
    $env:PATH = "$vsInstaller;$env:PATH"
}

# Locate dotnet: PATH first, then the default per-machine install.
$dotnet = (Get-Command dotnet -ErrorAction SilentlyContinue)?.Source
if (-not $dotnet) {
    $dotnet = Join-Path $env:ProgramFiles 'dotnet\dotnet.exe'
    if (-not (Test-Path $dotnet)) { throw 'dotnet (.NET 9 SDK) not found.' }
}

$env:DOTNET_CLI_TELEMETRY_OPTOUT = '1'
$env:DOTNET_NOLOGO = '1'
$env:DOTNET_SKIP_FIRST_TIME_EXPERIENCE = '1'

# dotnet resolves the SDK (and global.json) from the current directory, not from the project
# path, so run from this folder for global.json's SDK pin to take effect.
Push-Location $here
try {
    $sdkVersion = (& $dotnet --version)
    $ilcVersion = if ((Get-Content -Raw $project) -match '<TaIlcVersion>([^<]+)</TaIlcVersion>') { $Matches[1] } else { 'SDK default' }
    Write-Host "dotnet publish (SDK $sdkVersion, ILCompiler $ilcVersion)"
    & $dotnet publish $project -c Release -r win-x64 -o $publish --nologo
    if ($LASTEXITCODE -ne 0) { throw "dotnet publish failed with exit code $LASTEXITCODE" }
} finally {
    Pop-Location
}

# Replace bin\ta-cs.exe only when the bytes changed. With /Brepro the native image is
# reproducible, so an unchanged rebuild keeps the existing file (and its hash: a new hash would
# trigger a fresh Defender scan on the next launch, see benchmark-method.md 1.6).
New-Item -ItemType Directory -Force -Path $binDir | Out-Null
$built = Join-Path $publish 'ta-cs.exe'
$builtHash = (Get-FileHash -Algorithm SHA256 $built).Hash
if ((Test-Path $exe) -and (Get-FileHash -Algorithm SHA256 $exe).Hash -eq $builtHash) {
    Write-Host 'bin\ta-cs.exe is unchanged (same SHA-256); not replaced'
} else {
    Copy-Item -Force $built $exe
    Write-Host 'bin\ta-cs.exe updated'
}

# Find mt.exe in the newest Windows SDK that has one.
function Find-Mt {
    $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    if (-not (Test-Path $kits)) { return $null }
    Get-ChildItem $kits -Directory |
        Where-Object { Test-Path (Join-Path $_.FullName 'x64\mt.exe') } |
        Sort-Object { [version]($_.Name -replace '[^0-9.]', '') } -Descending -ErrorAction SilentlyContinue |
        Select-Object -First 1 |
        ForEach-Object { Join-Path $_.FullName 'x64\mt.exe' }
}

# The SDK embeds <ApplicationManifest> as RT_MANIFEST #1 and ILC copies it into the native
# image. Verify that, and fall back to embedding it with mt.exe if it is ever missing.
$mt = Find-Mt
if ($mt) {
    $extracted = Join-Path $here 'obj\embedded.manifest'
    Remove-Item -Force -ErrorAction SilentlyContinue $extracted
    & $mt -nologo "-inputresource:$exe;#1" "-out:$extracted" 2>$null | Out-Null
    $embedded = (Test-Path $extracted) -and ((Get-Content -Raw $extracted) -match 'consoleAllocationPolicy')
    if (-not $embedded) {
        Write-Host 'Manifest not embedded by the SDK; embedding with mt.exe'
        & $mt -nologo -manifest $manifest "-outputresource:$exe;#1"
        if ($LASTEXITCODE -ne 0) { throw "mt.exe failed with exit code $LASTEXITCODE" }
    } else {
        Write-Host 'Manifest (consoleAllocationPolicy=detached) is embedded'
    }
} else {
    Write-Warning 'mt.exe not found; skipped the embedded-manifest check'
}

$size = (Get-Item $exe).Length
Write-Host ("{0}  {1:N0} bytes ({2:N2} MB)" -f $exe, $size, ($size / 1MB))
Write-Host ("SHA-256 {0}" -f (Get-FileHash -Algorithm SHA256 $exe).Hash)
