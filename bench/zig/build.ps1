#Requires -Version 7.0
<#
.SYNOPSIS
    Builds the Zig toggle-audio benchmark implementations.

.DESCRIPTION
    Non-interactive and idempotent: every run rebuilds both executables into bin\ (Zig's local
    cache for both variants goes to obj\zig-cache via --cache-dir and ZIG_LOCAL_CACHE_DIR; the
    shared libc/compiler_rt builds stay in Zig's global cache; both directories are gitignored).

    Outputs (both console subsystem with ..\common\detached.manifest embedded by Zig itself):
      bin\ta-zig.exe    primary: native Zig (ta.zig), hand-declared Win32/COM, no libc
      bin\ta-zigcc.exe  secondary: the C reference ..\c\ta.c compiled unchanged by `zig cc`
                        (Clang + MinGW-w64 CRT) through the ta-mingw.c wrapper, to compare
                        Clang/MinGW against MSVC on identical source

    After building it verifies each exe (PE subsystem = 3, embedded manifest with
    consoleAllocationPolicy=detached, imported DLLs when dumpbin is available) and prints sizes.

    Zig is located in this order: -Zig parameter, $env:ZIG, `zig` on PATH, the WinGet package
    directory (where `winget install zig.zig` puts it).

.PARAMETER Zig
    Full path to zig.exe (optional).

.PARAMETER Only
    Build only the named variants: ta-zig, ta-zigcc. Default: both.
#>
[CmdletBinding()]
param(
    [string]$Zig,
    [string[]]$Only
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$here = $PSScriptRoot
$manifest = (Resolve-Path (Join-Path $here '..\common\detached.manifest')).Path
$binDir = Join-Path $here 'bin'
$objDir = Join-Path $here 'obj'
$cacheDir = Join-Path $objDir 'zig-cache'

# ---------------------------------------------------------------------------------------------
# Tool discovery
# ---------------------------------------------------------------------------------------------
function Find-Zig {
    if ($Zig) { return (Resolve-Path $Zig).Path }
    if ($env:ZIG -and (Test-Path $env:ZIG)) { return (Resolve-Path $env:ZIG).Path }
    $onPath = Get-Command zig.exe -ErrorAction SilentlyContinue
    if ($onPath) { return $onPath.Source }
    $wingetRoot = Join-Path $env:LOCALAPPDATA 'Microsoft\WinGet\Packages'
    if (Test-Path $wingetRoot) {
        $found = Get-ChildItem -Path $wingetRoot -Directory -Filter 'zig.zig_*' -ErrorAction SilentlyContinue |
            ForEach-Object { Get-ChildItem -Path $_.FullName -Recurse -Filter 'zig.exe' -ErrorAction SilentlyContinue } |
            Sort-Object {
                # Package dirs look like zig-x86_64-windows-0.17.0; sort by version, not by string.
                $v = $_.Directory.Name -replace '^.*?-(\d+\.\d+\.\d+).*$', '$1'
                try { [version]$v } catch { [version]'0.0.0' }
            } -Descending | Select-Object -First 1
        if ($found) { return $found.FullName }
    }
    throw 'zig.exe not found. Install Zig 0.17 (winget install zig.zig) or pass -Zig <path>.'
}

# Optional verification tools. Missing tools only skip the corresponding check.
function Find-MtExe {
    $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    if (-not (Test-Path $kits)) { return $null }
    Get-ChildItem -Path $kits -Directory -Filter '10.*' | Sort-Object { [version]$_.Name } -Descending |
        ForEach-Object { Join-Path $_.FullName 'x64\mt.exe' } | Where-Object { Test-Path $_ } |
        Select-Object -First 1
}

function Find-Dumpbin {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path $vswhere)) { return $null }
    & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
        -find 'VC\Tools\MSVC\**\bin\Hostx64\x64\dumpbin.exe' | Select-Object -First 1
}

# Reads the Subsystem field of the PE optional header (3 = console, 2 = Windows GUI) directly,
# so this check needs no external tool.
function Get-PeSubsystem([string]$Path) {
    $bytes = [System.IO.File]::ReadAllBytes($Path)
    $peOffset = [BitConverter]::ToInt32($bytes, 0x3C)
    if ([BitConverter]::ToUInt32($bytes, $peOffset) -ne 0x00004550) { throw "$Path is not a PE file" }
    # PE signature (4) + COFF file header (20) + offset of Subsystem in the optional header (68).
    return [BitConverter]::ToUInt16($bytes, $peOffset + 4 + 20 + 68)
}

$zigExe = Find-Zig
$zigVersion = (& $zigExe version).Trim()
Write-Host "zig: $zigExe ($zigVersion)"
if (-not $zigVersion.StartsWith('0.17.')) {
    throw "ta.zig needs Zig 0.17.x, found $zigVersion at $zigExe (pass -Zig <path to a 0.17 zig.exe>)"
}
$mt = Find-MtExe
$dumpbin = Find-Dumpbin

# ---------------------------------------------------------------------------------------------
# Variants. See README.md for the reasoning behind each flag.
# ---------------------------------------------------------------------------------------------
$variants = [ordered]@{
    'ta-zig' = @{
        Source = Join-Path $here 'ta.zig'
        Args   = {
            param($out)
            @(
                'build-exe', (Join-Path $here 'ta.zig'),
                $manifest,                      # Zig compiles a .manifest input into RT_MANIFEST #1
                '-target', 'x86_64-windows-gnu',# explicit target => baseline x86-64 CPU, no -mcpu=native
                '-O', 'ReleaseFast',            # optimize for speed, no safety checks
                '-fstrip',                      # no debug info / PDB
                '-fsingle-threaded',            # no threads of our own: skips Zig's TLS setup
                '--subsystem', 'console',       # CUI; detached manifest prevents a console window
                '--name', 'ta-zig',
                "-femit-bin=$out",
                '--cache-dir', $cacheDir
            )
        }
    }
    'ta-zigcc' = @{
        Source = Join-Path $here '..\c\ta.c'
        Args   = {
            param($out)
            @(
                'cc',
                '-target', 'x86_64-windows-gnu',# Clang + MinGW-w64 headers/CRT (UCRT) + import libs
                '-O2',                          # same optimization level as MSVC /O2
                '-s',                           # strip symbols
                '-municode',                    # ta.c's CRT entry point is wmain
                '-Wall',
                (Join-Path $here 'ta-mingw.c'), # wrapper that #includes ..\c\ta.c unchanged
                $manifest,                      # embedded as RT_MANIFEST #1, like build-exe
                '-lole32', '-luuid',
                '-o', $out
            )
        }
    }
}

New-Item -ItemType Directory -Force -Path $binDir, $objDir | Out-Null
# `zig cc` has no --cache-dir option; this sends its per-compilation cache to obj\ as well.
$env:ZIG_LOCAL_CACHE_DIR = $cacheDir

# Accept both -Only a,b (array) and -Only 'a,b' (a single string, as passed by pwsh -File).
$names = if ($Only) { @($Only | ForEach-Object { $_ -split ',' } | Where-Object { $_ }) } else { @($variants.Keys) }
$results = @(foreach ($name in $names) {
    if (-not $variants.Contains($name)) { throw "unknown variant '$name' (known: $($variants.Keys -join ', '))" }
    $v = $variants[$name]
    if (-not (Test-Path $v.Source)) {
        Write-Warning "${name}: source $($v.Source) not found, skipped"
        continue
    }
    $exe = Join-Path $binDir "$name.exe"
    if (Test-Path $exe) { Remove-Item -Force $exe }

    Write-Host "==> $name" -ForegroundColor Cyan
    $zigArgs = & $v.Args $exe
    $output = & $zigExe @zigArgs 2>&1
    $exit = $LASTEXITCODE
    $output | ForEach-Object { Write-Host "    $_" }
    if ($exit -ne 0) { throw "zig failed for $name (exit $exit)" }
    # zig cc can leave an import library / PDB next to the exe; keep bin\ to executables only.
    Get-ChildItem -Path $binDir -File | Where-Object { $_.BaseName -eq $name -and $_.Extension -in '.lib', '.pdb' } |
        Remove-Item -Force

    # --- Verify: PE subsystem ------------------------------------------------------------------
    $subsystem = Get-PeSubsystem $exe
    if ($subsystem -ne 3) { throw "$name has subsystem $subsystem, expected 3 (console)" }

    # --- Verify: embedded manifest contains the detached console policy ------------------------
    if ($mt) {
        $extracted = Join-Path $objDir "$name.manifest"
        & $mt -nologo "-inputresource:$exe;#1" "-out:$extracted" | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "mt.exe could not extract the manifest from $name" }
        $manifestText = Get-Content -Raw $extracted
    } else {
        # Fallback without the SDK: RT_MANIFEST resources are stored as plain UTF-8 text.
        $manifestText = [System.Text.Encoding]::UTF8.GetString([System.IO.File]::ReadAllBytes($exe))
    }
    if ($manifestText -notmatch '<consoleAllocationPolicy[^>]*>detached<') {
        throw "${name}: embedded manifest lacks consoleAllocationPolicy=detached"
    }

    # --- Imports (informational) ---------------------------------------------------------------
    $deps = if ($dumpbin) {
        (& $dumpbin /nologo /dependents $exe | Select-String -Pattern '^\s+(\S+\.dll)\s*$' |
            ForEach-Object { $_.Matches[0].Groups[1].Value }) -join ', '
    } else { '(dumpbin not found)' }

    [pscustomobject]@{
        Exe       = "$name.exe"
        Bytes     = (Get-Item $exe).Length
        Subsystem = '3 (console)'
        Manifest  = if ($mt) { 'detached (mt.exe)' } else { 'detached (byte scan)' }
        Imports   = $deps
    }
})

Write-Host ''
$results | Format-Table -AutoSize | Out-String -Width 200 | Write-Host
$primary = Join-Path $binDir 'ta-zig.exe'
if (Test-Path $primary) {
    Write-Host ("bin\ta-zig.exe: {0:N0} bytes" -f (Get-Item $primary).Length)
}
