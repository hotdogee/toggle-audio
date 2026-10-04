#Requires -Version 7.0
<#
.SYNOPSIS
    Builds the C (MSVC) toggle-audio benchmark reference and its comparison variants.

.DESCRIPTION
    Non-interactive and idempotent: every run rebuilds all variants from ta.c into bin\ (objects in
    obj\, both gitignored). Imports the MSVC x64 environment via ..\common\vsdevenv.ps1 (vcvars64.bat
    located with vswhere) unless an x64-targeting MSVC environment is already active.

    Outputs (all console-subsystem builds embed ..\common\detached.manifest):
      bin\ta-c.exe        primary: /MT static CRT, console subsystem + consoleAllocationPolicy=detached
      bin\ta-c-gui.exe    same, Windows (GUI) subsystem via /ENTRY:wmainCRTStartup
      bin\ta-c-md.exe     same as primary but /MD (dynamic CRT: needs vcruntime140.dll)
      bin\ta-c-nocrt.exe  same as primary but without any CRT (/DTA_NOCRT, /NODEFAULTLIB /ENTRY:entry)

    After building it verifies each exe (x64 machine type, PE subsystem, embedded manifest, no
    dynamic-CRT imports except in ta-c-md) and prints a size table.

.PARAMETER Only
    Build only the named variants (for example: -Only ta-c). Default: all.
#>
[CmdletBinding()]
param(
    [string[]]$Only
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$here = $PSScriptRoot
$src = Join-Path $here 'ta.c'
$manifest = (Resolve-Path (Join-Path $here '..\common\detached.manifest')).Path
$binDir = Join-Path $here 'bin'
$objRoot = Join-Path $here 'obj'

. (Join-Path $here '..\common\vsdevenv.ps1')
Import-VsDevEnv

# ---------------------------------------------------------------------------------------------
# Flags. See README.md for the reasoning behind each one.
# ---------------------------------------------------------------------------------------------
$commonCompile = @(
    '/nologo', '/W4', '/utf-8',
    '/O2',      # optimize for speed
    '/GL',      # whole-program optimization (paired with /LTCG)
    '/Gy',      # function-level COMDATs so /OPT:REF,ICF can drop/fold functions
    '/Gw',      # data-level COMDATs (same, for globals)
    '/GS-',     # no per-function stack-cookie checks in ta.c; required for the no-CRT build
    '/DUNICODE', '/D_UNICODE'
)
$commonLink = @(
    '/LTCG',               # link-time code generation for /GL objects
    '/OPT:REF', '/OPT:ICF',# strip unreferenced code/data, fold identical COMDATs
    '/MANIFEST:EMBED',     # embed the manifest as RT_MANIFEST resource #1
    "/MANIFESTINPUT:$manifest",
    '/MANIFESTUAC:NO'      # detached.manifest already carries trustInfo (asInvoker)
)

$variants = [ordered]@{
    'ta-c'       = @{ Compile = @('/MT'); Link = @('/SUBSYSTEM:CONSOLE', 'ole32.lib') }
    'ta-c-gui'   = @{ Compile = @('/MT'); Link = @('/SUBSYSTEM:WINDOWS', '/ENTRY:wmainCRTStartup', 'ole32.lib') }
    'ta-c-md'    = @{ Compile = @('/MD'); Link = @('/SUBSYSTEM:CONSOLE', 'ole32.lib') }
    # ole32.dll (and with it combase, rpcrt4, ...) is loaded only on the first COM call, so the
    # no-arguments scenario no longer pays for it. Isolates the import cost; not a product candidate.
    'ta-c-delayload' = @{ Compile = @('/MT'); Link = @('/SUBSYSTEM:CONSOLE', 'ole32.lib', 'delayimp.lib', '/DELAYLOAD:ole32.dll') }
    'ta-c-nocrt' = @{
        # /Zl: no default-library records in the .obj; /Oi: intrinsics (rep stosb/movsb helpers).
        Compile = @('/DTA_NOCRT', '/Oi', '/Zl')
        Link    = @('/SUBSYSTEM:CONSOLE', '/NODEFAULTLIB', '/ENTRY:entry', 'kernel32.lib', 'ole32.lib')
    }
}

$expectedSubsystem = @{ 'ta-c' = 3; 'ta-c-gui' = 2; 'ta-c-md' = 3; 'ta-c-delayload' = 3; 'ta-c-nocrt' = 3 }

New-Item -ItemType Directory -Force -Path $binDir | Out-Null

# Accept both -Only a,b (array) and -Only 'a,b' (a single string, as passed by pwsh -File).
$names = if ($Only) { @($Only | ForEach-Object { $_ -split ',' } | Where-Object { $_ }) } else { @($variants.Keys) }
$results = @(foreach ($name in $names) {
    if (-not $variants.Contains($name)) { throw "unknown variant '$name' (known: $($variants.Keys -join ', '))" }
    $v = $variants[$name]
    $objDir = Join-Path $objRoot $name
    New-Item -ItemType Directory -Force -Path $objDir | Out-Null
    $exe = Join-Path $binDir "$name.exe"
    if (Test-Path $exe) { Remove-Item -Force $exe }

    Write-Host "==> $name" -ForegroundColor Cyan
    $compileArgs = $commonCompile + $v.Compile + @("/Fo$objDir\", "/Fe$exe", $src)
    $linkArgs = $commonLink + $v.Link
    # cl/link print the source name and (localized) "Generating code" lines; show their output only
    # when something went wrong or a diagnostic was emitted.
    $clOutput = & cl.exe @compileArgs /link @linkArgs 2>&1
    $clExit = $LASTEXITCODE
    if ($clExit -ne 0 -or ($clOutput | Select-String -Pattern '\b(warning|error) [A-Z]+\d+' -Quiet)) {
        $clOutput | ForEach-Object { Write-Host "    $_" }
    }
    if ($clExit -ne 0) { throw "cl.exe failed for $name (exit $clExit)" }

    # --- Verify: PE subsystem ------------------------------------------------------------------
    $headers = & dumpbin.exe /nologo /headers $exe
    Assert-X64Image -Name $name -Headers $headers
    $subsysLine = ($headers | Select-String -Pattern '^\s*(\d+) subsystem' | Select-Object -First 1)
    $subsystem = [int]$subsysLine.Matches[0].Groups[1].Value
    if ($subsystem -ne $expectedSubsystem[$name]) {
        throw "$name has subsystem $subsystem, expected $($expectedSubsystem[$name])"
    }

    # --- Verify: embedded manifest contains the detached console policy ------------------------
    $extracted = Join-Path $objDir 'embedded.manifest'
    & mt.exe -nologo "-inputresource:$exe;#1" "-out:$extracted" | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "mt.exe could not extract the manifest from $name" }
    if (-not (Select-String -Path $extracted -Pattern '<consoleAllocationPolicy[^>]*>detached<' -Quiet)) {
        throw "${name}: embedded manifest lacks consoleAllocationPolicy=detached"
    }

    # --- Imports -------------------------------------------------------------------------------
    # dumpbin lists load-time imports first, then (if any) delay-loaded DLLs in a second block.
    $delaySection = $false
    $deps = foreach ($line in (& dumpbin.exe /nologo /dependents $exe)) {
        if ($line -match 'delay load dependencies') { $delaySection = $true; continue }
        if ($line -match '^\s+(\S+\.dll)\s*$') {
            if ($delaySection) { "$($Matches[1]) (delay)" } else { $Matches[1] }
        }
    }
    # Only ta-c-md may depend on the dynamic CRT (vcruntime140.dll is not part of Windows).
    if ($name -ne 'ta-c-md' -and ($deps -match '(?i)vcruntime|ucrtbase|api-ms-win-crt')) {
        throw "$name must not depend on the dynamic CRT: $($deps -join ', ')"
    }

    [pscustomobject]@{
        Exe       = "$name.exe"
        Bytes     = (Get-Item $exe).Length
        Subsystem = if ($subsystem -eq 3) { '3 (console)' } else { '2 (GUI)' }
        Manifest  = 'detached'
        Imports   = ($deps -join ', ')
    }
})

Write-Host ''
$results | Format-Table -AutoSize | Out-String -Width 200 | Write-Host
$primary = Join-Path $binDir 'ta-c.exe'
if (Test-Path $primary) {
    Write-Host ("bin\ta-c.exe: {0:N0} bytes" -f (Get-Item $primary).Length)
}
