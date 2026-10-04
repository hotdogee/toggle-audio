#Requires -Version 7

<#
.SYNOPSIS
    Builds bin\ta-go.exe: the Go implementation of the toggle-audio bench CLI.

.DESCRIPTION
    Non-interactive and idempotent. Steps:
      1. locate go.exe and mt.exe (parameters, environment, PATH, well-known locations);
      2. go vet + go test (skip with -SkipTests);
      3. go build -trimpath -ldflags "-s -w -buildid=" (console subsystem, CGO_ENABLED=0);
      4. embed ..\common\detached.manifest as RT_MANIFEST #1 with mt.exe;
      5. read the manifest back to verify it and print the binary size.

.PARAMETER GoExe
    Path to go.exe. Default: $env:GO_EXE, then go on PATH, then
    %LOCALAPPDATA%\Programs\go\bin\go.exe.

.PARAMETER MtExe
    Path to mt.exe. Default: $env:MT_EXE, then mt.exe on PATH, then the newest
    Windows 10/11 SDK under "Windows Kits\10\bin\<version>\x64".

.PARAMETER GoAmd64
    GOAMD64 microarchitecture level (v1..v4). Default v3 (AVX2, Haswell+/Zen+),
    see README.md. Use v1 for a binary that runs on any x64 CPU.

.PARAMETER SkipTests
    Skip go vet and go test.

.EXAMPLE
    pwsh -NoProfile -File bench\go\build.ps1
#>
[CmdletBinding()]
param(
    [string] $GoExe,
    [string] $MtExe,
    [ValidateSet('v1', 'v2', 'v3', 'v4')]
    [string] $GoAmd64 = 'v3',
    [switch] $SkipTests
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$srcDir = $PSScriptRoot
$binDir = Join-Path $srcDir 'bin'
$exe = Join-Path $binDir 'ta-go.exe'
$manifest = [IO.Path]::GetFullPath((Join-Path $srcDir '..\common\detached.manifest'))

function Resolve-Tool {
    param([string] $Explicit, [string] $EnvVar, [string] $CommandName, [string[]] $Candidates)
    if ($Explicit) {
        if (-not (Test-Path -LiteralPath $Explicit -PathType Leaf)) { throw "Not found: $Explicit" }
        return (Resolve-Path -LiteralPath $Explicit).Path
    }
    $fromEnv = [Environment]::GetEnvironmentVariable($EnvVar)
    if ($fromEnv -and (Test-Path -LiteralPath $fromEnv -PathType Leaf)) { return $fromEnv }
    $cmd = Get-Command $CommandName -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($cmd) { return $cmd.Source }
    foreach ($c in $Candidates) {
        if ($c -and (Test-Path -LiteralPath $c -PathType Leaf)) { return $c }
    }
    throw "Cannot find $CommandName. Pass its path as a parameter or set the $EnvVar environment variable."
}

# --- Tools -------------------------------------------------------------------
$GoExe = Resolve-Tool -Explicit $GoExe -EnvVar 'GO_EXE' -CommandName 'go.exe' -Candidates @(
    (Join-Path $env:LOCALAPPDATA 'Programs\go\bin\go.exe'),
    (Join-Path $env:ProgramFiles 'Go\bin\go.exe'))

$sdkBin = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
$sdkMt = @()
if (Test-Path -LiteralPath $sdkBin) {
    # @() keeps this an array even when exactly one SDK version is installed;
    # otherwise += below would concatenate two strings instead of appending.
    $sdkMt = @(Get-ChildItem -LiteralPath $sdkBin -Directory |
        Where-Object { $_.Name -match '^\d+\.\d+\.\d+\.\d+$' } |
        Sort-Object { [version]$_.Name } -Descending |
        ForEach-Object { Join-Path $_.FullName 'x64\mt.exe' })
    $sdkMt += (Join-Path $sdkBin 'x64\mt.exe')
}
$MtExe = Resolve-Tool -Explicit $MtExe -EnvVar 'MT_EXE' -CommandName 'mt.exe' -Candidates $sdkMt

if (-not (Test-Path -LiteralPath $manifest -PathType Leaf)) { throw "Manifest not found: $manifest" }

# --- Environment (restored afterwards) ---------------------------------------
$envVars = [ordered]@{
    GOROOT      = $null              # let go.exe derive GOROOT from its own location
    GOTOOLCHAIN = 'local'            # never download another toolchain
    GOFLAGS     = '-mod=readonly'    # never let the go command rewrite go.mod/go.sum
    GOOS        = 'windows'
    GOARCH      = 'amd64'
    GOAMD64     = $GoAmd64
    CGO_ENABLED = '0'                # pure Go: no C compiler, no libc/msvcrt import
    PATH        = "$(Split-Path -Parent $GoExe);$env:PATH"
}
$saved = @{}
foreach ($k in $envVars.Keys) {
    $saved[$k] = [Environment]::GetEnvironmentVariable($k)
    [Environment]::SetEnvironmentVariable($k, $envVars[$k])
}

function Invoke-Checked {
    param([string] $File, [string[]] $Arguments)
    & $File @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$([IO.Path]::GetFileName($File)) $($Arguments -join ' ') failed with exit code $LASTEXITCODE" }
}

Push-Location -LiteralPath $srcDir
try {
    Write-Host "go:       $GoExe ($(& $GoExe env GOVERSION))"
    Write-Host "mt:       $MtExe"
    Write-Host "GOAMD64:  $GoAmd64"

    if (-not $SkipTests) {
        Invoke-Checked $GoExe @('vet', '.')
        Invoke-Checked $GoExe @('test', '-count=1', '.')
    }

    New-Item -ItemType Directory -Force -Path $binDir | Out-Null
    # Remove the previous output so mt.exe always edits a freshly linked image.
    if (Test-Path -LiteralPath $exe) { Remove-Item -LiteralPath $exe -Force }

    # -trimpath      : no local file system paths in the binary (reproducible).
    # -buildvcs=false: no VCS stamping (works outside a git checkout, reproducible).
    # -s -w          : drop the symbol table and DWARF (smaller file, less to map).
    # -buildid=      : empty build id (reproducible bytes).
    # No -H windowsgui: console subsystem; the manifest suppresses the console window.
    Invoke-Checked $GoExe @('build', '-trimpath', '-buildvcs=false',
        '-ldflags=-s -w -buildid=', '-o', $exe, '.')

    # Embed the consoleAllocationPolicy=detached manifest as RT_MANIFEST resource #1.
    Invoke-Checked $MtExe @('-nologo', '-manifest', $manifest, "-outputresource:$exe;#1")

    # Verify: read the embedded manifest back.
    $check = Join-Path ([IO.Path]::GetTempPath()) "ta-go-manifest-$PID.xml"
    try {
        Invoke-Checked $MtExe @('-nologo', "-inputresource:$exe;#1", "-out:$check")
        if (-not (Select-String -LiteralPath $check -SimpleMatch 'consoleAllocationPolicy' -Quiet)) {
            throw 'Embedded manifest does not contain consoleAllocationPolicy.'
        }
    }
    finally {
        Remove-Item -LiteralPath $check -Force -ErrorAction SilentlyContinue
    }

    $size = (Get-Item -LiteralPath $exe).Length
    Write-Host ("Built {0} ({1:N0} bytes, {2:N2} MiB), manifest embedded" -f $exe, $size, ($size / 1MB))
}
finally {
    Pop-Location
    foreach ($k in $saved.Keys) { [Environment]::SetEnvironmentVariable($k, $saved[$k]) }
}
