#Requires -Version 7.0

<#
.SYNOPSIS
    Packages the PowerShell baseline (ta.ps1) into bin\ta-ps.exe with ps2exe, the way the original
    C:\bin\Switch-Audio.exe was made, so the ps2exe hosting cost can be measured.

.DESCRIPTION
    Non-interactive and idempotent: every run deletes and rebuilds the two exes in bin\
    (gitignored).

    Outputs:
      bin\ta-ps.exe         primary: ps2exe -x64 -noConsole:$false (console subsystem, STA, no
                            config file), then the manifest ..\common\detached.manifest is
                            embedded with mt.exe (consoleAllocationPolicy=detached, bench
                            contract, DESIGN.md section 12)
      bin\ta-ps-anycpu.exe  the user's original packaging, unchanged: plain `Invoke-ps2exe in out`
                            (AnyCPU, console subsystem, STA, ps2exe's default asInvoker manifest).
                            Same flags as C:\bin\Switch-Audio.exe (PE32 AnyCPU "IL only", CUI).

    ps2exe 1.0.17 is a Windows PowerShell module: under PowerShell 7 it re-launches itself in
    powershell.exe, and the generated exe always targets .NET Framework 4.x and references
    System.Management.Automation 3.0.0.0, i.e. it hosts the Windows PowerShell 5.1 engine from the
    GAC, never PowerShell 7. The script records that (engine reference and the GAC file version)
    after building.

    Verification after building (no dumpbin needed; the PE header is read directly):
      * PE machine and subsystem (3 = Windows CUI expected),
      * embedded RT_MANIFEST #1 extracted with mt.exe (detached policy expected for ta-ps.exe),
      * referenced engine assembly.

    Prerequisites: Windows PowerShell 5.1 (powershell.exe), ps2exe 1.0.17
    (Install-Module ps2exe -Scope CurrentUser in Windows PowerShell), a Windows 10/11 SDK for
    mt.exe. AudioDeviceCmdlets is needed only to run the result, not to build it.
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$here = $PSScriptRoot
$script = Join-Path $here 'ta.ps1'
$manifest = (Resolve-Path (Join-Path $here '..\common\detached.manifest')).Path
$binDir = Join-Path $here 'bin'
$objDir = Join-Path $here 'obj'
$expectedPs2exe = [version]'1.0.17'

# ---------------------------------------------------------------------------------------------
# Tool discovery
# ---------------------------------------------------------------------------------------------

$winPs = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
if (-not (Test-Path -LiteralPath $winPs)) { throw "Windows PowerShell not found at $winPs" }

# mt.exe: newest Windows SDK first.
function Find-MtExe {
    $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    $candidates = @(Get-ChildItem -Path $kits -Directory -Filter '10.*' -ErrorAction SilentlyContinue |
        Sort-Object { [version]$_.Name } -Descending |
        ForEach-Object { Join-Path $_.FullName 'x64\mt.exe' } |
        Where-Object { Test-Path -LiteralPath $_ })
    if ($candidates.Count -eq 0) {
        $onPath = Get-Command mt.exe -ErrorAction SilentlyContinue
        if ($onPath) { return $onPath.Source }
        throw 'mt.exe not found (install the Windows 10/11 SDK)'
    }
    return $candidates[0]
}
$mt = Find-MtExe

# ---------------------------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------------------------

# Runs the ps2exe module's Invoke-ps2exe inside Windows PowerShell (ps2exe would re-launch
# powershell.exe itself when called from PowerShell 7; doing it explicitly keeps the quoting under
# our control). The command is passed as -EncodedCommand so paths with spaces or quotes survive.
function Invoke-Ps2exeInWindowsPowerShell([string]$InputFile, [string]$OutputFile, [string]$ExtraParameters) {
    $command = @"
`$ErrorActionPreference = 'Stop'
`$m = Get-Module -ListAvailable -Name ps2exe | Sort-Object Version -Descending | Select-Object -First 1
if (-not `$m) { Write-Error 'ps2exe module not found (Install-Module ps2exe -Scope CurrentUser)'; exit 3 }
if (`$m.Version -ne [version]'$expectedPs2exe') { Write-Warning "ps2exe `$(`$m.Version) found; this build was written for $expectedPs2exe" }
Import-Module `$m.Path
Write-Output "ps2exe `$(`$m.Version) from `$(`$m.ModuleBase)"
Invoke-ps2exe -inputFile '$($InputFile -replace "'", "''")' -outputFile '$($OutputFile -replace "'", "''")' $ExtraParameters
"@
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($command))
    $output = & $winPs -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand $encoded 2>&1
    $code = $LASTEXITCODE
    $output | ForEach-Object { Write-Host "    $_" }
    if ($code -ne 0 -or -not (Test-Path -LiteralPath $OutputFile)) {
        throw "ps2exe failed for $OutputFile (exit $code)"
    }
}

# Reads machine and subsystem from the PE headers (IMAGE_NT_HEADERS; the Subsystem field sits at
# offset 68 of the optional header for both PE32 and PE32+).
function Get-PeInfo([string]$Path) {
    $bytes = [IO.File]::ReadAllBytes($Path)
    $peOffset = [BitConverter]::ToInt32($bytes, 0x3C)
    if ([BitConverter]::ToUInt32($bytes, $peOffset) -ne 0x00004550) { throw "$Path is not a PE file" }
    $machine = [BitConverter]::ToUInt16($bytes, $peOffset + 4)
    $optional = $peOffset + 24
    $magic = [BitConverter]::ToUInt16($bytes, $optional)
    [pscustomobject]@{
        Format    = if ($magic -eq 0x20B) { 'PE32+' } else { 'PE32' }
        Machine   = switch ($machine) { 0x14C { 'x86/AnyCPU' } 0x8664 { 'x64' } default { '0x{0:X4}' -f $machine } }
        Subsystem = [int][BitConverter]::ToUInt16($bytes, $optional + 68)
    }
}

# Extracts RT_MANIFEST #1 to a file and returns its text.
function Get-EmbeddedManifest([string]$Exe, [string]$OutFile) {
    & $mt -nologo "-inputresource:$Exe;#1" "-out:$OutFile" | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "mt.exe could not extract the manifest from $Exe" }
    return Get-Content -LiteralPath $OutFile -Raw
}

# Engine the ps2exe host binds to: its System.Management.Automation reference, resolved like the
# .NET Framework loader would (GAC_MSIL), plus that file's version.
function Get-EngineReference([string]$Exe) {
    $stream = [IO.File]::OpenRead($Exe)
    try {
        $pe = [System.Reflection.PortableExecutable.PEReader]::new($stream)
        try {
            $md = [System.Reflection.Metadata.PEReaderExtensions]::GetMetadataReader($pe)
            $runtime = $md.MetadataVersion
            $sma = @(foreach ($h in $md.AssemblyReferences) {
                $r = $md.GetAssemblyReference($h)
                if ($md.GetString($r.Name) -eq 'System.Management.Automation') { $r.Version }
            })
        } finally {
            $pe.Dispose()
        }
    } finally {
        $stream.Dispose()
    }
    # ps2exe 1.0.17 always references SMA 3.0.0.0, the Windows PowerShell 5.1 engine in the GAC.
    # Anything else means a different ps2exe or engine: fail rather than print a wrong label.
    if ($sma.Count -ne 1) { throw "$Exe references System.Management.Automation $($sma.Count) times, expected once" }
    $sma = $sma[0]
    if ($sma -ne [version]'3.0.0.0') { throw "$Exe references unexpected System.Management.Automation $sma" }
    $engineName = 'Windows PowerShell 5.1'
    $gac = Join-Path $env:SystemRoot "Microsoft.NET\assembly\GAC_MSIL\System.Management.Automation\v4.0_$($sma)__31bf3856ad364e35\System.Management.Automation.dll"
    $fileVersion = if (Test-Path -LiteralPath $gac) { (Get-Item -LiteralPath $gac).VersionInfo.ProductVersion } else { 'not in GAC' }
    [pscustomobject]@{ ClrMetadata = $runtime; SmaReference = "$sma"; EngineName = $engineName; SmaFile = $gac; SmaVersion = $fileVersion }
}

# ---------------------------------------------------------------------------------------------
# Build
# ---------------------------------------------------------------------------------------------

New-Item -ItemType Directory -Force -Path $binDir, $objDir | Out-Null

$variants = [ordered]@{
    # The bench contract build: 64-bit only, console subsystem, detached console policy.
    'ta-ps'        = @{ Ps2exe = '-x64 -noConsole:$false'; Detached = $true }
    # The user's original packaging (no switches at all), for comparison.
    'ta-ps-anycpu' = @{ Ps2exe = ''; Detached = $false }
}

$results = @(foreach ($name in $variants.Keys) {
    $v = $variants[$name]
    $exe = Join-Path $binDir "$name.exe"
    foreach ($stale in @($exe, "$exe.config", "$exe.win32manifest")) {
        if (Test-Path -LiteralPath $stale) { Remove-Item -LiteralPath $stale -Force }
    }

    $flagText = if ($v.Ps2exe) { $v.Ps2exe } else { '(default flags)' }
    Write-Host "==> $name (ps2exe module Invoke-ps2exe $flagText, in Windows PowerShell)" -ForegroundColor Cyan
    Invoke-Ps2exeInWindowsPowerShell -InputFile $script -OutputFile $exe -ExtraParameters $v.Ps2exe

    if ($v.Detached) {
        # ps2exe has no option for a custom manifest, so replace the default one (RT_MANIFEST #1,
        # written by csc) with the bench manifest. It keeps asInvoker and adds supportedOS and the
        # detached console allocation policy.
        & $mt -nologo -manifest $manifest "-outputresource:$exe;#1" | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "mt.exe failed to embed the manifest into $exe" }
    }

    # --- Verify ---------------------------------------------------------------------------------
    $pe = Get-PeInfo $exe
    if ($pe.Subsystem -ne 3) { throw "$name has subsystem $($pe.Subsystem), expected 3 (console)" }
    $embedded = Get-EmbeddedManifest $exe (Join-Path $objDir "$name.manifest")
    $isDetached = $embedded -match '<consoleAllocationPolicy[^>]*>\s*detached\s*<'
    if ($v.Detached -and -not $isDetached) { throw "${name}: embedded manifest lacks consoleAllocationPolicy=detached" }
    $engine = Get-EngineReference $exe

    [pscustomobject]@{
        Exe       = "$name.exe"
        Bytes     = (Get-Item -LiteralPath $exe).Length
        PE        = "$($pe.Format) $($pe.Machine)"
        Subsystem = '3 (console)'
        Manifest  = if ($isDetached) { 'detached' } else { 'ps2exe default' }
        Engine    = "SMA $($engine.SmaReference) = $($engine.EngineName) (GAC file $($engine.SmaVersion), CLR $($engine.ClrMetadata))"
    }
})

Write-Host ''
$results | Format-Table -AutoSize | Out-String -Width 220 | Write-Host
Write-Host "mt.exe: $mt"
Write-Host ("bin\ta-ps.exe: {0:N0} bytes" -f (Get-Item -LiteralPath (Join-Path $binDir 'ta-ps.exe')).Length)
