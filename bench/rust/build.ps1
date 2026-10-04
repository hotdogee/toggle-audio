#Requires -Version 7.0
<#
.SYNOPSIS
    Builds the Rust bench implementation and copies it to bench\rust\bin\ta-rs.exe.

.DESCRIPTION
    Non-interactive and idempotent.

    1. Refuses to run when RUSTFLAGS or CARGO_ENCODED_RUSTFLAGS is set: either one
       REPLACES the rustflags from .cargo\config.toml (static CRT, slim errors).
    2. Runs `cargo build --release --locked` from this directory (so this crate's
       .cargo\config.toml and the repository root's are both picked up), with the
       target directory pinned to bench\rust\target so CARGO_TARGET_DIR or a user
       build.target-dir cannot redirect it.
    3. Takes the exe path from Cargo's own JSON artifact message (never a guessed
       path), so CARGO_BUILD_TARGET or similar cannot make it copy a stale binary.
    4. Deletes bin\ta-rs.exe, then copies the fresh exe there.
    5. Verifies the result:
       - mt.exe (Windows SDK): RT_MANIFEST #1 contains consoleAllocationPolicy=detached.
       - dumpbin.exe (MSVC): subsystem 3 (Windows CUI); no dynamic CRT import
         (vcruntime140.dll, ucrtbase.dll, api-ms-win-crt-*) and no oleaut32.dll.
       A missing tool produces a warning; a failed check fails the build.
    6. Prints the binary size.

.EXAMPLE
    pwsh -NoProfile -File bench\rust\build.ps1
#>
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$here = $PSScriptRoot
$crateName = 'ta-rs'
$exeName = "$crateName.exe"

# --- Locate cargo (PATH first, then the default rustup location) -------------
$cargo = (Get-Command cargo -ErrorAction SilentlyContinue)?.Source
if (-not $cargo) {
    $candidate = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
    if (Test-Path $candidate) { $cargo = $candidate }
}
if (-not $cargo) {
    throw 'cargo not found. Install Rust from https://rustup.rs (stable, x86_64-pc-windows-msvc).'
}

# Both variables override the rustflags from .cargo\config.toml (static CRT,
# windows_slim_errors), which would silently produce a different binary.
foreach ($name in 'RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS') {
    $value = [Environment]::GetEnvironmentVariable($name)
    if ($value) { throw "$name is set ('$value'); unset it so .cargo\config.toml applies." }
}

# --- Build ---------------------------------------------------------------------
# --message-format=json-render-diagnostics: human-readable diagnostics still go
# to stderr; stdout carries one JSON message per line, including the
# compiler-artifact message with the absolute path of the linked exe.
$targetDir = Join-Path $here 'target'
Push-Location $here
try {
    $messages = & $cargo build --release --locked --target-dir $targetDir --message-format=json-render-diagnostics
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE" }
}
finally {
    Pop-Location
}

$built = $messages |
    Where-Object { $_ -like '{*' } |
    ForEach-Object { $_ | ConvertFrom-Json } |
    Where-Object { $_.reason -eq 'compiler-artifact' -and $_.target.name -eq $crateName -and $_.executable } |
    Select-Object -Last 1 -ExpandProperty executable
if (-not $built) { throw "cargo reported no executable for $crateName" }
if (-not (Test-Path -LiteralPath $built)) { throw "build output not found: $built" }

$binDir = Join-Path $here 'bin'
New-Item -ItemType Directory -Force -Path $binDir | Out-Null
$exe = Join-Path $binDir $exeName
# Delete first: if the copy fails, no stale binary may be left to be measured.
Remove-Item -LiteralPath $exe -Force -ErrorAction SilentlyContinue
if (Test-Path -LiteralPath $exe) { throw "could not delete the old $exe (is it running?)" }
Copy-Item -LiteralPath $built -Destination $exe

# --- Verify the embedded manifest (needs the Windows SDK's mt.exe) -------------
$kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
$mt = $null
if (Test-Path $kits) {
    $mt = Get-ChildItem -Path $kits -Directory -Filter '10.*' |
        Sort-Object { [version]$_.Name } -Descending |
        ForEach-Object { Join-Path $_.FullName 'x64\mt.exe' } |
        Where-Object { Test-Path $_ } |
        Select-Object -First 1
}
if ($mt) {
    $extracted = Join-Path ([IO.Path]::GetTempPath()) "ta-rs-$PID.manifest"
    try {
        & $mt -nologo "-inputresource:$exe;#1" "-out:$extracted" | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "no RT_MANIFEST resource #1 in $exe (mt.exe exit $LASTEXITCODE)" }
        $manifest = Get-Content -LiteralPath $extracted -Raw
        if ($manifest -notmatch '<consoleAllocationPolicy[^>]*>\s*detached\s*</consoleAllocationPolicy>') {
            throw "embedded manifest of $exe lacks consoleAllocationPolicy=detached"
        }
        Write-Host 'manifest: consoleAllocationPolicy=detached embedded'
    }
    finally {
        Remove-Item -LiteralPath $extracted -Force -ErrorAction SilentlyContinue
    }
}
else {
    Write-Warning 'mt.exe (Windows SDK) not found; skipped the embedded-manifest check.'
}

# --- Verify subsystem and imports (needs MSVC's dumpbin.exe) --------------------
# The newest MSVC toolset of the newest VS install with the C++ x64 tools;
# without vswhere, the default VS 2022 folders are probed.
$vsRoots = @()
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (Test-Path $vswhere) {
    $vsRoots = @(& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath)
}
if (-not $vsRoots) {
    $vsRoots = foreach ($root in @($env:ProgramFiles, ${env:ProgramFiles(x86)})) {
        foreach ($edition in 'BuildTools', 'Community', 'Professional', 'Enterprise') {
            Join-Path $root "Microsoft Visual Studio\2022\$edition"
        }
    }
}
$dumpbin = $vsRoots |
    ForEach-Object { Join-Path $_ 'VC\Tools\MSVC' } |
    Where-Object { Test-Path $_ } |
    ForEach-Object { Get-ChildItem -Path $_ -Directory } |
    Sort-Object { [version]$_.Name } -Descending |
    ForEach-Object { Join-Path $_.FullName 'bin\Hostx64\x64\dumpbin.exe' } |
    Where-Object { Test-Path $_ } |
    Select-Object -First 1
if ($dumpbin) {
    $headers = & $dumpbin /nologo /headers $exe
    if ($LASTEXITCODE -ne 0) { throw "dumpbin /headers failed with exit code $LASTEXITCODE" }
    if (-not ($headers -match '^\s*3 subsystem \(Windows CUI\)')) {
        throw "$exe is not a console-subsystem (3, Windows CUI) image"
    }
    Write-Host 'subsystem: 3 (Windows CUI)'

    $dependents = & $dumpbin /nologo /dependents $exe
    if ($LASTEXITCODE -ne 0) { throw "dumpbin /dependents failed with exit code $LASTEXITCODE" }
    $unwanted = $dependents | Where-Object { $_ -match '^\s*(vcruntime140\.dll|ucrtbase\.dll|api-ms-win-crt-\S+\.dll|oleaut32\.dll)\s*$' }
    if ($unwanted) {
        throw "$exe imports $(($unwanted | ForEach-Object Trim) -join ', '); the rustflags from .cargo\config.toml were not applied"
    }
    Write-Host 'imports: no dynamic CRT (vcruntime140/ucrtbase/api-ms-win-crt-*), no oleaut32.dll'
}
else {
    Write-Warning 'dumpbin.exe (MSVC) not found; skipped the subsystem and import checks.'
}

$size = (Get-Item -LiteralPath $exe).Length
Write-Host ('{0}  {1:N0} bytes' -f $exe, $size)
