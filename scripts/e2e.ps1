<#
.SYNOPSIS
    End-to-end check of toggle-audio.exe against the real Windows audio stack.

.DESCRIPTION
    1. Builds the release executables (cargo build --release), unless -SkipBuild or -Exe is given.
    2. Read-only checks: --version, --help, usage errors, list, get, set to the current default and
       --timing, compared with AudioDeviceCmdlets (Get-AudioDevice) as an independent oracle; then
       --version, list and get once more through toggle-audiow.exe.
    3. Unless -SkipToggle: writes a temporary configuration with -Device1 / -Device2, runs "toggle"
       2 x -Rounds times and "set" by name and by id, and verifies the default and default
       communications devices with Get-AudioDevice after every step.

    Every run of the program gets APPDATA pointed at a scratch directory, so the user's real
    configuration (%APPDATA%\toggle-audio\config.json) is never read or written. A finally block
    removes the scratch directory and restores the original default and default communications
    devices, also when a check fails or the script is interrupted with Ctrl+C.

    Requires the AudioDeviceCmdlets module (Install-Module AudioDeviceCmdlets -Scope CurrentUser).
    Works in Windows PowerShell 5.1 and PowerShell 7.

.PARAMETER Device1
    Endpoint id of the first device to toggle between. Default: PG42UQ on the reference machine.

.PARAMETER Device2
    Endpoint id of the second device. Default: PHL BDM4065 on the reference machine.

.PARAMETER Rounds
    Number of round trips (two toggles each). Default 2.

.PARAMETER SkipToggle
    Run only the read-only part; the default device is never changed.

.PARAMETER SkipBuild
    Do not run cargo; use the existing release build (honors CARGO_TARGET_DIR).

.PARAMETER Exe
    Path of the toggle-audio.exe to test; toggle-audiow.exe is expected next to it. Implies
    -SkipBuild.

.EXAMPLE
    pwsh scripts/e2e.ps1 -SkipToggle

.EXAMPLE
    pwsh scripts/e2e.ps1 -Device1 '{0.0.0.00000000}.{...}' -Device2 '{0.0.0.00000000}.{...}' -Rounds 3
#>
[CmdletBinding()]
param(
    [string]$Device1 = '{0.0.0.00000000}.{739b3554-bfed-4d61-b407-a818b317c991}',
    [string]$Device2 = '{0.0.0.00000000}.{5b124733-5d8f-428c-b83c-ee05ce6467fb}',
    [ValidateRange(1, 100)][int]$Rounds = 2,
    [switch]$SkipToggle,
    [switch]$SkipBuild,
    [string]$Exe
)

Set-StrictMode -Version 3.0
$ErrorActionPreference = 'Stop'

$RepoRoot = Split-Path -Parent $PSScriptRoot
$script:Failures = 0
$script:Checks = 0
$script:ExePath = $null
# APPDATA for every run of the program: toggle-audio reads the variable first, so this keeps the
# user's real configuration out of the test. Created only by the toggle part.
$script:AppData = Join-Path ([IO.Path]::GetTempPath()) ('toggle-audio-e2e-' + [guid]::NewGuid().ToString('N'))

function Write-Step([string]$Text) {
    Write-Host ''
    Write-Host "== $Text" -ForegroundColor Cyan
}

function Test-Check([bool]$Condition, [string]$Text) {
    $script:Checks++
    if ($Condition) {
        Write-Host "  ok    $Text" -ForegroundColor Green
    } else {
        $script:Failures++
        Write-Host "  FAIL  $Text" -ForegroundColor Red
    }
}

function Test-SameId([string]$A, [string]$B) {
    return [string]::Equals($A, $B, [StringComparison]::OrdinalIgnoreCase)
}

# Runs the executable (toggle-audio.exe unless -Path says otherwise) and returns its exit code,
# stdout (text and lines), stderr and wall time. Output is decoded as UTF-8 (what the program
# writes to pipes), whatever the console code page.
function Invoke-Exe([string[]]$Arguments, [string]$Path = $script:ExePath) {
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = $Path
    $info.EnvironmentVariables['APPDATA'] = $script:AppData
    $info.Arguments = ($Arguments | ForEach-Object { '"' + ($_ -replace '"', '\"') + '"' }) -join ' '
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.StandardOutputEncoding = New-Object System.Text.UTF8Encoding($false)
    $info.StandardErrorEncoding = New-Object System.Text.UTF8Encoding($false)
    $watch = [Diagnostics.Stopwatch]::StartNew()
    $process = [Diagnostics.Process]::Start($info)
    $stderrTask = $process.StandardError.ReadToEndAsync()
    $stdout = $process.StandardOutput.ReadToEnd()
    $process.WaitForExit()
    $watch.Stop()
    return [pscustomobject]@{
        ExitCode = $process.ExitCode
        Stdout   = $stdout
        Lines    = @($stdout -split "`n" | Where-Object { $_ -ne '' })
        Stderr   = $stderrTask.Result
        Ms       = $watch.Elapsed.TotalMilliseconds
    }
}

# The oracle: what AudioDeviceCmdlets reports, independently of toggle-audio. Get-AudioDevice
# -Playback reads the multimedia role while toggle-audio's '*' and 'get' use the console role;
# both always move together here (Set-AudioDevice and toggle-audio set them as a pair).
function Get-OracleDefault { return (Get-AudioDevice -Playback).ID }
function Get-OracleCommunication { return (Get-AudioDevice -PlaybackCommunication).ID }
function Get-OracleEndpoints { return @(Get-AudioDevice -List | Where-Object { $_.Type -eq 'Playback' }) }

# Parses "list" output into objects with Id, Name, Flags and the number of tab-separated fields.
function ConvertFrom-List([string[]]$Lines) {
    foreach ($line in $Lines) {
        $fields = @($line -split "`t")
        [pscustomobject]@{
            Id         = $fields[0]
            Name       = if ($fields.Count -gt 1) { $fields[1] } else { '' }
            Flags      = if ($fields.Count -gt 2) { $fields[2] } else { '' }
            FieldCount = $fields.Count
        }
    }
}

# -------------------------------------------------------------------------------------------------
# Setup
# -------------------------------------------------------------------------------------------------

Import-Module AudioDeviceCmdlets -ErrorAction Stop

if ($Exe) {
    $script:ExePath = (Resolve-Path -LiteralPath $Exe).Path
} elseif ($SkipBuild) {
    $targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $RepoRoot 'target' }
    $script:ExePath = Join-Path $targetDir 'release\toggle-audio.exe'
} else {
    Write-Step 'cargo build --release'
    Push-Location $RepoRoot
    try {
        # The JSON messages name the produced executables, wherever CARGO_TARGET_DIR points.
        $messages = & cargo build --release --bins --message-format=json-render-diagnostics
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE" }
    } finally {
        Pop-Location
    }
    $artifact = $messages |
        Where-Object { $_.StartsWith('{') } |
        ForEach-Object { $_ | ConvertFrom-Json } |
        Where-Object { $_.reason -eq 'compiler-artifact' -and $_.target.name -eq 'toggle-audio' -and $_.executable } |
        Select-Object -Last 1
    if (-not $artifact) { throw 'cargo did not report the toggle-audio executable' }
    $script:ExePath = $artifact.executable
}
if (-not (Test-Path -LiteralPath $script:ExePath)) { throw "executable not found: $script:ExePath" }
$script:WindowedPath = Join-Path (Split-Path -Parent $script:ExePath) 'toggle-audiow.exe'
if (-not (Test-Path -LiteralPath $script:WindowedPath)) { throw "executable not found: $script:WindowedPath" }
Write-Host "Testing $script:ExePath and toggle-audiow.exe"
Write-Host "APPDATA for the program: $script:AppData"

$versionLine = Select-String -Path (Join-Path $RepoRoot 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
$cargoVersion = $versionLine.Matches[0].Groups[1].Value

# -------------------------------------------------------------------------------------------------
# Read-only checks
# -------------------------------------------------------------------------------------------------

Write-Step 'version, help and usage errors'
$r = Invoke-Exe @('--version')
Test-Check ($r.ExitCode -eq 0 -and $r.Stdout -eq "toggle-audio $cargoVersion`n") "--version prints 'toggle-audio $cargoVersion' (exit $($r.ExitCode))"
$r = Invoke-Exe @('--help')
Test-Check ($r.ExitCode -eq 0 -and $r.Stdout.Contains('Usage:')) "--help prints the usage (exit $($r.ExitCode))"
$r = Invoke-Exe @('no-such-command')
Test-Check ($r.ExitCode -eq 2 -and $r.Stderr.Contains('--help')) "an unknown command exits 2 with a --help hint (exit $($r.ExitCode))"
$r = Invoke-Exe @('set')
Test-Check ($r.ExitCode -eq 2) "'set' without a device exits 2 (exit $($r.ExitCode))"

Write-Step 'list'
$oracleDefault = Get-OracleDefault
$oracleCommunication = Get-OracleCommunication
$oracleEndpoints = Get-OracleEndpoints
$r = Invoke-Exe @('list')
$listed = @(ConvertFrom-List $r.Lines)
Test-Check ($r.ExitCode -eq 0) "list exits 0 (exit $($r.ExitCode))"
Test-Check (-not $r.Stdout.Contains("`r")) 'list output uses LF line endings'
Test-Check ($listed.Count -gt 0 -and @($listed | Where-Object { $_.FieldCount -ne 3 }).Count -eq 0) "every line is <id> TAB <name> TAB <flags> ($($listed.Count) devices)"
Test-Check (@($listed | Where-Object { @('*', 'c', '*c', '-') -notcontains $_.Flags }).Count -eq 0) 'flags are one of *, c, *c, -'
$starred = @($listed | Where-Object { $_.Flags.Contains('*') })
$communications = @($listed | Where-Object { $_.Flags.Contains('c') })
Test-Check ($starred.Count -eq 1 -and (Test-SameId $starred[0].Id $oracleDefault)) "'*' marks the default device $oracleDefault"
Test-Check ($communications.Count -eq 1 -and (Test-SameId $communications[0].Id $oracleCommunication)) "'c' marks the default communications device $oracleCommunication"
$listedIds = @($listed | ForEach-Object { $_.Id.ToLowerInvariant() } | Sort-Object)
$oracleIds = @($oracleEndpoints | ForEach-Object { $_.ID.ToLowerInvariant() } | Sort-Object)
Test-Check (($listedIds -join '|') -eq ($oracleIds -join '|')) 'list shows the same endpoints as Get-AudioDevice -List'
foreach ($endpoint in $oracleEndpoints) {
    $match = @($listed | Where-Object { Test-SameId $_.Id $endpoint.ID })
    Test-Check ($match.Count -eq 1 -and $match[0].Name -eq $endpoint.Name) "same name for $($endpoint.ID)"
}

Write-Step 'get'
$r = Invoke-Exe @('get')
$defaultName = ($oracleEndpoints | Where-Object { Test-SameId $_.ID $oracleDefault } | Select-Object -First 1).Name
Test-Check ($r.ExitCode -eq 0 -and $r.Lines.Count -eq 1) "get exits 0 with one line (exit $($r.ExitCode))"
Test-Check ($r.Lines.Count -eq 1 -and $r.Lines[0] -eq "$oracleDefault`t$defaultName") 'get prints <id> TAB <name> of the default device'

Write-Step 'set to the current default (changes nothing)'
# --comm or --no-comm is always explicit, so no configuration is consulted. --comm would also set
# the communications role, which is a change when it differs from the default device.
$commFlag = if (Test-SameId $oracleDefault $oracleCommunication) { '--comm' } else { '--no-comm' }
$r = Invoke-Exe @('set', $oracleDefault, $commFlag)
Test-Check ($r.ExitCode -eq 0) "set <current default id> $commFlag exits 0 (exit $($r.ExitCode))"
Test-Check ($r.Stdout -eq "$defaultName is already the default`n") "set reports that $defaultName is already the default"
Test-Check ((Test-SameId (Get-OracleDefault) $oracleDefault) -and (Test-SameId (Get-OracleCommunication) $oracleCommunication)) 'the defaults are unchanged'
$r = Invoke-Exe @('set', '{0.0.0.00000000}.{00000000-0000-0000-0000-000000000000}', '--no-comm')
Test-Check ($r.ExitCode -eq 4) "set <unknown id> exits 4 (exit $($r.ExitCode))"

Write-Step '--timing'
$r = Invoke-Exe @('get', '--timing')
$timingLines = @($r.Stderr -split "`n" | Where-Object { $_ -ne '' })
Test-Check ($r.ExitCode -eq 0 -and $r.Lines.Count -eq 1) 'get --timing leaves stdout unchanged'
# Negative values are possible: the process creation stamp is coarser than the QPC clock.
Test-Check ($timingLines.Count -ge 3 -and @($timingLines | Where-Object { $_ -notmatch "^timing`t[a-z_]+`t-?\d+(\.\d+)?$" }).Count -eq 0) "stderr has timing TAB <phase> TAB <microseconds> lines ($($timingLines.Count))"
Test-Check ($timingLines.Count -ge 2 -and $timingLines[0] -match "`tstart`t" -and $timingLines[-1] -match "`tend`t") 'the phases run from start to end'

Write-Step 'toggle-audiow.exe (Windows subsystem) with redirected output'
$r = Invoke-Exe @('--version') -Path $script:WindowedPath
Test-Check ($r.ExitCode -eq 0 -and $r.Stdout -eq "toggle-audio $cargoVersion`n") "toggle-audiow --version (exit $($r.ExitCode))"
$console = Invoke-Exe @('list')
$r = Invoke-Exe @('list') -Path $script:WindowedPath
Test-Check ($r.ExitCode -eq 0 -and $r.Stdout -eq $console.Stdout) "toggle-audiow list prints the same as toggle-audio list (exit $($r.ExitCode))"
$r = Invoke-Exe @('get') -Path $script:WindowedPath
Test-Check ($r.ExitCode -eq 0 -and $r.Lines.Count -eq 1 -and $r.Lines[0] -eq "$oracleDefault`t$defaultName") "toggle-audiow get (exit $($r.ExitCode))"
$r = Invoke-Exe @('bogus') -Path $script:WindowedPath
Test-Check ($r.ExitCode -eq 2 -and $r.Stderr.StartsWith('toggle-audio: ')) "toggle-audiow reports usage errors on redirected stderr (exit $($r.ExitCode))"

# -------------------------------------------------------------------------------------------------
# Toggle round trips (changes the default device; everything is restored in the finally block)
# -------------------------------------------------------------------------------------------------

if ($SkipToggle) {
    Write-Step 'toggle round trips skipped (-SkipToggle)'
} else {
    Write-Step "toggle between $Device1 and $Device2"
    $first = $oracleEndpoints | Where-Object { Test-SameId $_.ID $Device1 } | Select-Object -First 1
    $second = $oracleEndpoints | Where-Object { Test-SameId $_.ID $Device2 } | Select-Object -First 1
    if (-not $first -or -not $second -or (Test-SameId $Device1 $Device2)) {
        throw 'Device1 and Device2 must be two different active playback devices (see the list output above)'
    }

    $originalDefault = $oracleDefault
    $originalCommunication = $oracleCommunication
    # %APPDATA%\toggle-audio\config.json as the program sees it (APPDATA is the scratch directory).
    $configDir = Join-Path $script:AppData 'toggle-audio'
    $configPath = Join-Path $configDir 'config.json'

    try {
        $config = [ordered]@{
            version               = 1
            device1               = [ordered]@{ id = $first.ID; name = $first.Name }
            device2               = [ordered]@{ id = $second.ID; name = $second.Name }
            switch_communications = $true
        }
        New-Item -ItemType Directory -Force -Path $configDir | Out-Null
        [IO.File]::WriteAllText($configPath, ($config | ConvertTo-Json -Depth 3), (New-Object System.Text.UTF8Encoding($false)))
        Write-Host "  wrote a temporary configuration to $configPath"

        $timings = @()
        for ($i = 1; $i -le 2 * $Rounds; $i++) {
            $expected = if (Test-SameId (Get-OracleDefault) $first.ID) { $second } else { $first }
            $r = Invoke-Exe @('toggle')
            $timings += $r.Ms
            Test-Check ($r.ExitCode -eq 0) ('toggle {0}: exit {1} after {2:N1} ms' -f $i, $r.ExitCode, $r.Ms)
            Test-Check ($r.Stdout -eq "Switched to $($expected.Name)`n") "toggle ${i}: prints 'Switched to $($expected.Name)'"
            Test-Check (Test-SameId (Get-OracleDefault) $expected.ID) "toggle ${i}: Get-AudioDevice -Playback is $($expected.Name)"
            Test-Check (Test-SameId (Get-OracleCommunication) $expected.ID) "toggle ${i}: Get-AudioDevice -PlaybackCommunication is $($expected.Name)"
            $g = Invoke-Exe @('get')
            Test-Check ($g.Lines.Count -eq 1 -and $g.Lines[0].StartsWith($expected.ID, [StringComparison]::OrdinalIgnoreCase)) "toggle ${i}: get agrees"
        }
        $stats = $timings | Measure-Object -Minimum -Maximum -Average
        Write-Host ('  toggle wall time (process start to exit): min {0:N1} ms, mean {1:N1} ms, max {2:N1} ms' -f $stats.Minimum, $stats.Average, $stats.Maximum)

        Write-Step 'set by exact name and by id'
        $r = Invoke-Exe @('set', $second.Name)
        Test-Check ($r.ExitCode -eq 0 -and (Test-SameId (Get-OracleDefault) $second.ID)) "set '$($second.Name)' makes it the default (exit $($r.ExitCode))"
        $r = Invoke-Exe @('set', $first.ID)
        Test-Check ($r.ExitCode -eq 0 -and (Test-SameId (Get-OracleDefault) $first.ID)) "set <Device1 id> makes it the default (exit $($r.ExitCode))"
    } finally {
        Write-Step 'restore'
        Remove-Item -LiteralPath $script:AppData -Recurse -Force -ErrorAction SilentlyContinue
        Write-Host '  removed the temporary configuration'
        Set-AudioDevice -ID $originalDefault -DefaultOnly | Out-Null
        Set-AudioDevice -ID $originalCommunication -CommunicationOnly | Out-Null
        Test-Check ((Test-SameId (Get-OracleDefault) $originalDefault) -and (Test-SameId (Get-OracleCommunication) $originalCommunication)) "original defaults restored ($originalDefault, communications $originalCommunication)"
    }
}

Write-Host ''
if ($script:Failures -eq 0) {
    Write-Host "PASS: $script:Checks checks" -ForegroundColor Green
    exit 0
}
Write-Host "FAIL: $script:Failures of $script:Checks checks failed" -ForegroundColor Red
exit 1
