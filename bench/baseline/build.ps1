#Requires -Version 7.0
<#
.SYNOPSIS
    Builds the process-start baselines and the spawnbench launcher.

.DESCRIPTION
    Non-interactive and idempotent. Produces, in bin\ (gitignored):
      nop.exe               no-CRT ExitProcess(0), Windows (GUI) subsystem, no manifest
      nop-con.exe           same, console subsystem, no manifest
      nop-con-detached.exe  same, console subsystem + ..\common\detached.manifest
      spawnbench.exe        CreateProcessW launcher that times N runs of a command (/MT, console)
    Uses the MSVC environment helper from ..\common\vsdevenv.ps1 (vcvars64.bat located via vswhere).
    Verifies each machine type (x64), subsystem and manifest, then prints sizes and imported DLLs.
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$here = $PSScriptRoot
$manifest = (Resolve-Path (Join-Path $here '..\common\detached.manifest')).Path
$binDir = Join-Path $here 'bin'
$objRoot = Join-Path $here 'obj'

. (Join-Path $here '..\common\vsdevenv.ps1')
Import-VsDevEnv

# nop: smallest possible image. /O1 size, /GS- and /Zl because there is no CRT, /NODEFAULTLIB +
# /ENTRY:entry so the entry point is our function and only kernel32 is imported.
$nopCompile = @('/nologo', '/W4', '/O1', '/GS-', '/Zl')
$nopLink = @('/NODEFAULTLIB', '/ENTRY:entry', '/OPT:REF', '/OPT:ICF', 'kernel32.lib')
$withManifest = @('/MANIFEST:EMBED', "/MANIFESTINPUT:$manifest", '/MANIFESTUAC:NO')

$targets = @(
    @{ Name = 'nop';              Src = 'nop.c'; Compile = $nopCompile; Link = $nopLink + @('/SUBSYSTEM:WINDOWS', '/MANIFEST:NO'); Subsystem = 2; Manifest = $false }
    @{ Name = 'nop-con';          Src = 'nop.c'; Compile = $nopCompile; Link = $nopLink + @('/SUBSYSTEM:CONSOLE', '/MANIFEST:NO'); Subsystem = 3; Manifest = $false }
    @{ Name = 'nop-con-detached'; Src = 'nop.c'; Compile = $nopCompile; Link = $nopLink + @('/SUBSYSTEM:CONSOLE') + $withManifest; Subsystem = 3; Manifest = $true }
    @{
        Name = 'spawnbench'; Src = 'spawnbench.c'
        Compile = @('/nologo', '/W4', '/O2', '/MT', '/DUNICODE', '/D_UNICODE')
        Link = @('/SUBSYSTEM:CONSOLE', '/OPT:REF', '/OPT:ICF', '/MANIFEST:NO')
        Subsystem = 3; Manifest = $false
    }
)

New-Item -ItemType Directory -Force -Path $binDir | Out-Null

$results = @(foreach ($t in $targets) {
    $objDir = Join-Path $objRoot $t.Name
    New-Item -ItemType Directory -Force -Path $objDir | Out-Null
    $exe = Join-Path $binDir "$($t.Name).exe"
    if (Test-Path $exe) { Remove-Item -Force $exe }

    Write-Host "==> $($t.Name)" -ForegroundColor Cyan
    $clOutput = & cl.exe @($t.Compile) "/Fo$objDir\" "/Fe$exe" (Join-Path $here $t.Src) /link @($t.Link) 2>&1
    $clExit = $LASTEXITCODE
    if ($clExit -ne 0 -or ($clOutput | Select-String -Pattern '\b(warning|error) [A-Z]+\d+' -Quiet)) {
        $clOutput | ForEach-Object { Write-Host "    $_" }
    }
    if ($clExit -ne 0) { throw "cl.exe failed for $($t.Name) (exit $clExit)" }

    $headers = & dumpbin.exe /nologo /headers $exe
    Assert-X64Image -Name $t.Name -Headers $headers
    $subsystem = [int](($headers | Select-String -Pattern '^\s*(\d+) subsystem' | Select-Object -First 1).Matches[0].Groups[1].Value)
    if ($subsystem -ne $t.Subsystem) { throw "$($t.Name) has subsystem $subsystem, expected $($t.Subsystem)" }

    $extracted = Join-Path $objDir 'embedded.manifest'
    if (Test-Path $extracted) { Remove-Item -Force $extracted }
    & mt.exe -nologo "-inputresource:$exe;#1" "-out:$extracted" 2>&1 | Out-Null
    $hasDetached = (Test-Path $extracted) -and
        (Select-String -Path $extracted -Pattern '<consoleAllocationPolicy[^>]*>detached<' -Quiet)
    if ($hasDetached -ne $t.Manifest) {
        throw "$($t.Name): detached manifest present=$hasDetached, expected $($t.Manifest)"
    }

    $deps = & dumpbin.exe /nologo /dependents $exe |
        Where-Object { $_ -match '^\s+\S+\.dll\s*$' } | ForEach-Object { $_.Trim() }
    if ($deps -match '(?i)vcruntime|ucrtbase|api-ms-win-crt') {
        throw "$($t.Name) must not depend on the dynamic CRT: $($deps -join ', ')"
    }
    [pscustomobject]@{
        Exe       = "$($t.Name).exe"
        Bytes     = (Get-Item $exe).Length
        Subsystem = if ($subsystem -eq 3) { '3 (console)' } else { '2 (GUI)' }
        Manifest  = if ($hasDetached) { 'detached' } else { 'none' }
        Imports   = ($deps -join ', ')
    }
})

Write-Host ''
$results | Format-Table -AutoSize | Out-String -Width 200 | Write-Host
