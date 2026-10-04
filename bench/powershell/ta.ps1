<#
.SYNOPSIS
    PowerShell baseline for the toggle-audio bench CLI contract.

.DESCRIPTION
    Implements the common bench CLI contract (docs/research/benchmark-method.md section 1, with the
    phase format of bench/c/ta.c) so the PowerShell approach can be measured with exactly the same
    harness as the native implementations:

      ta.ps1 list                 one line per ACTIVE render endpoint: "<id>`t<name>`t<flags>"
                                  flags: "*" default (eConsole), "c" default communications,
                                  "*c" both, "-" neither
      ta.ps1 get                  "<id>`t<name>" of the default (eConsole) render endpoint
      ta.ps1 set <id>             validate (exists + ACTIVE), then SetDefaultEndpoint for
                                  eConsole, eMultimedia, eCommunications (always, even when the
                                  endpoint already is the default)
      ta.ps1 toggle <idA> <idB>   if the eConsole default is idA set idB, else set idA;
                                  prints the target id
      ta.ps1                      usage on stderr, exit 1, module never imported (host floor)
      --timing (anywhere)         phase stamps on stderr: "phase`t<name>`t<us since process creation>"

    Exit codes: 0 OK, 1 usage, 2 COM/module failure ("error: <step> hr=0x........" on stderr),
    3 device not found or not active, 4 no default device.

    Run it with any of the three hosts (see README.md):
      powershell.exe -NoProfile -NonInteractive -File ta.ps1 <command>
      pwsh -NoProfile -NonInteractive -File ta.ps1 <command>
      bin\ta-ps.exe <command>            (ps2exe package of this file, built by build.ps1)

    How the Core Audio calls are made:
      * Enumeration for "list" uses the Get-AudioDevice -List cmdlet, the code path the original
        proof of concept (switch-audio.ps1) used.
      * Everything the cmdlets cannot express exactly is done through the module's own public
        interop classes (CoreAudioApi.MMDeviceEnumerator, CoreAudioApi.PolicyConfigClient), so the
        semantics match the native implementations byte for byte:
          - the cmdlets report eMultimedia as "Default"; the contract uses eConsole;
          - Set-AudioDevice sets only eCommunications and eMultimedia; the contract sets all three
            roles in the order eConsole, eMultimedia, eCommunications;
          - Get-AudioDevice -ID cannot tell "not found" from "not active".
      * COM is already initialized by the PowerShell host (the pipeline thread is STA in all three
        hosts), so there is no explicit CoInitializeEx: the "com_init" phase marks the end of
        Import-Module instead, which is the PowerShell equivalent of getting COM ready.
      * COM objects are runtime callable wrappers (RCWs) owned by the .NET runtime. The module's
        wrapper classes expose no Dispose/Release, so references are simply dropped: no
        deterministic Release/CoUninitialize happens (the CLR may release RCWs on a later GC,
        otherwise they are discarded with the process). This is a deliberate deviation from
        benchmark-method.md section 1.3, see README.md "Known limitations". There are no
        CoTaskMem strings or PROPVARIANTs at this level: the module frees those internally.
      * Set uses only the module's real IPolicyConfig interface. Its other two fallbacks are
        unsafe (wrong Vista vtable layout; an "IPolicyConfig10" declared with IID_IUnknown that
        calls a blind vtable slot), so the script refuses to run without IPolicyConfig.

    Output encoding: when stdout/stderr is redirected (pipe, file, NUL: always the case in the
    benchmarks) the text is written as raw UTF-8 bytes without BOM and with "\n" line endings to
    [Console]::OpenStandardOutput()/OpenStandardError(). This bypasses PowerShell's formatting and
    [Console]::OutputEncoding (code page 950 on a zh-TW system, so CJK endpoint names would come
    out as Big5 bytes, or as "?" for characters outside cp950). [Console]::OutputEncoding is
    deliberately NOT changed: that calls SetConsoleOutputCP and alters the parent console. When a
    stream is an interactive console the text goes through [Console]::Out / [Console]::Error
    instead, so it displays in the console's code page (characters outside it show as "?"; this
    path is never benchmarked).

    The script itself is pure ASCII on purpose: Windows PowerShell 5.1 reads BOM-less scripts in
    the ANSI code page.
#>

# ta.ps1 - toggle-audio benchmark baseline in PowerShell, on top of AudioDeviceCmdlets.
#
# The two timestamp statements below MUST stay the first statements of the script (only comments
# may precede them): they are the "entry" timestamp of the --timing breakdown (everything before
# them is host startup). The help block above is a comment and costs nothing at run time.
$script:EntryTicks = [System.Diagnostics.Stopwatch]::GetTimestamp()
$script:EntryWallUtc = [System.DateTime]::UtcNow

$ErrorActionPreference = 'Stop'      # cmdlet errors become terminating, so try/catch sees them
$ProgressPreference = 'SilentlyContinue'
Set-StrictMode -Version Latest

# ---------------------------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------------------------

Set-Variable -Option Constant -Name EXIT_OK -Value 0
Set-Variable -Option Constant -Name EXIT_USAGE -Value 1
Set-Variable -Option Constant -Name EXIT_COM_FAILURE -Value 2
Set-Variable -Option Constant -Name EXIT_DEVICE_NOT_FOUND -Value 3
Set-Variable -Option Constant -Name EXIT_NO_DEFAULT -Value 4

Set-Variable -Option Constant -Name E_NOTFOUND -Value ([int]0x80070490)    # HRESULT_FROM_WIN32(ERROR_NOT_FOUND)
Set-Variable -Option Constant -Name E_INVALIDARG -Value ([int]0x80070057)
Set-Variable -Option Constant -Name E_NOINTERFACE -Value ([int]0x80004002)
Set-Variable -Option Constant -Name DEVICE_STATE_ACTIVE -Value 1

# ERole values (mmdeviceapi.h). Kept as plain ints so this file parses before the module (which
# defines [CoreAudioApi.ERole]) is loaded; they are cast at the call sites.
Set-Variable -Option Constant -Name ROLE_CONSOLE -Value 0
Set-Variable -Option Constant -Name ROLE_MULTIMEDIA -Value 1
Set-Variable -Option Constant -Name ROLE_COMMUNICATIONS -Value 2

Set-Variable -Option Constant -Name USAGE_TEXT -Value "usage: ta-ps (list | get | set <id> | toggle <idA> <idB>) [--timing]`n"

# ---------------------------------------------------------------------------------------------
# Output buffers and timing (written once, at the end, like the native implementations)
# ---------------------------------------------------------------------------------------------

$script:Out = New-Object System.Text.StringBuilder     # stdout
$script:Err = New-Object System.Text.StringBuilder     # stderr: errors, then timing lines
$script:TimingEnabled = $false
$script:PhaseNames = New-Object 'System.Collections.Generic.List[string]'
$script:PhaseTicks = New-Object 'System.Collections.Generic.List[long]'

# Records a phase timestamp (Stopwatch = QueryPerformanceCounter) when --timing is on.
function Add-Phase([string]$Name) {
    if ($script:TimingEnabled) {
        $script:PhaseNames.Add($Name)
        $script:PhaseTicks.Add([System.Diagnostics.Stopwatch]::GetTimestamp())
    }
}

# Appends "phase`t<name>`t<us since process creation>`n" lines to the stderr buffer.
# Process creation -> entry is measured on the wall clock (Process.StartTime comes from
# GetProcessTimes; DateTime.UtcNow has system-tick granularity on .NET Framework, so this one
# offset may be up to ~1-16 ms coarse under Windows PowerShell / ps2exe). Everything after entry
# is measured with the high-resolution Stopwatch. Values are microseconds with one decimal.
function Format-Timing {
    $creationUtc = [System.Diagnostics.Process]::GetCurrentProcess().StartTime.ToUniversalTime()
    $createToEntryUs = ($script:EntryWallUtc - $creationUtc).Ticks / 10.0
    $ticksPerUs = [System.Diagnostics.Stopwatch]::Frequency / 1000000.0
    $invariant = [System.Globalization.CultureInfo]::InvariantCulture
    for ($i = 0; $i -lt $script:PhaseNames.Count; $i++) {
        $us = $createToEntryUs + ($script:PhaseTicks[$i] - $script:EntryTicks) / $ticksPerUs
        [void]$script:Err.Append('phase').Append("`t").Append($script:PhaseNames[$i]).Append("`t")
        [void]$script:Err.Append($us.ToString('F1', $invariant)).Append("`n")
    }
}

# Writes text to stdout or stderr. Redirected: raw UTF-8 bytes (no BOM) straight to the standard
# handle. Interactive console: the console writer (display only; never benchmarked).
function Write-StdStream([bool]$ToError, [string]$Text) {
    if ($Text.Length -eq 0) { return }
    $redirected = if ($ToError) { [Console]::IsErrorRedirected } else { [Console]::IsOutputRedirected }
    if ($redirected) {
        $bytes = (New-Object System.Text.UTF8Encoding($false)).GetBytes($Text)
        $stream = if ($ToError) { [Console]::OpenStandardError() } else { [Console]::OpenStandardOutput() }
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Flush()     # not disposed: the stream does not own the standard handle
    } else {
        $writer = if ($ToError) { [Console]::Error } else { [Console]::Out }
        $writer.Write($Text)
        $writer.Flush()
    }
}

# ---------------------------------------------------------------------------------------------
# Error helpers
# ---------------------------------------------------------------------------------------------

# Returns the HRESULT carried by an ErrorRecord/exception. PowerShell wraps exceptions thrown by
# .NET methods in MethodInvocationException; the COMException with the real HRESULT is inside.
function Get-HResult($ErrorOrException) {
    $ex = if ($ErrorOrException -is [System.Management.Automation.ErrorRecord]) { $ErrorOrException.Exception } else { $ErrorOrException }
    while ($null -ne $ex) {
        if ($ex -is [System.Runtime.InteropServices.COMException] -or $null -eq $ex.InnerException) { return $ex.HResult }
        $ex = $ex.InnerException
    }
    return [int]0x80004005   # E_FAIL: no exception object at all (should not happen)
}

function Format-Hex32([int]$Value) { '0x{0:X8}' -f $Value }

# "error: <step> hr=0x8007xxxx" and exit code 2.
function Write-StepError([string]$Step, $ErrorOrException) {
    [void]$script:Err.Append("error: $Step hr=$(Format-Hex32 (Get-HResult $ErrorOrException))`n")
    return $EXIT_COM_FAILURE
}

# Friendly names go into a tab-separated line, so tabs and line breaks become spaces.
function ConvertTo-Field([string]$Text) { $Text -replace "[`t`r`n]", ' ' }

function Test-SameId([string]$A, [string]$B) {
    # Endpoint ids are GUID-based: compare ordinally and case-insensitively.
    return ($null -ne $A) -and ($null -ne $B) -and [string]::Equals($A, $B, [System.StringComparison]::OrdinalIgnoreCase)
}

# ---------------------------------------------------------------------------------------------
# Core Audio helpers (module interop classes)
# ---------------------------------------------------------------------------------------------

# Default render endpoint (CoreAudioApi.MMDevice) for a role, or $null when there is none
# (E_NOTFOUND). Any other failure is rethrown for the caller to report.
function Get-DefaultEndpoint($Enumerator, [int]$Role) {
    try {
        return $Enumerator.GetDefaultAudioEndpoint([CoreAudioApi.EDataFlow]::eRender, [CoreAudioApi.ERole]$Role)
    } catch {
        if ((Get-HResult $_) -eq $E_NOTFOUND) { return $null }
        throw
    }
}

# Resolves a user-supplied endpoint id and checks that it is ACTIVE. GetDevice succeeds for
# NOTPRESENT/DISABLED/UNPLUGGED endpoints too, so the state check is mandatory. Returns a hashtable
# @{ ExitCode; Id } where Id is the id as reported by the device itself.
function Resolve-ActiveEndpoint($Enumerator, [string]$Id) {
    try {
        $device = $Enumerator.GetDevice($Id)
    } catch {
        $hr = Get-HResult $_
        if ($hr -eq $E_NOTFOUND -or $hr -eq $E_INVALIDARG) {
            [void]$script:Err.Append("error: device not found: $Id hr=$(Format-Hex32 $hr)`n")
            return @{ ExitCode = $EXIT_DEVICE_NOT_FOUND; Id = $null }
        }
        return @{ ExitCode = (Write-StepError 'GetDevice' $_); Id = $null }
    }
    try {
        $state = [int]$device.State
        $canonicalId = $device.ID
    } catch {
        return @{ ExitCode = (Write-StepError 'GetState' $_); Id = $null }
    }
    if ($state -ne $DEVICE_STATE_ACTIVE) {
        [void]$script:Err.Append("error: device not active: $Id state=$(Format-Hex32 $state)`n")
        return @{ ExitCode = $EXIT_DEVICE_NOT_FOUND; Id = $null }
    }
    return @{ ExitCode = $EXIT_OK; Id = $canonicalId }
}

# Makes the endpoint the default for eConsole, eMultimedia and eCommunications, in that order,
# through the module's PolicyConfigClient. The method throws on a failed HRESULT.
#
# Only the module's IPolicyConfig path is trustworthy (SetDefaultEndpoint at vtable slot 13 of
# IPolicyConfig {f8679f50-...} on CPolicyConfigClient {870af99c-...}). The constructor tries three
# casts in turn, and the two fallbacks are broken (docs/research/core-audio-api.md, Pitfalls
# checklist: "Do not copy AudioDeviceCmdlets"):
#   * IPolicyConfigVista {568b9108-...}: CPolicyConfigClient never answers this QI
#     (E_NOINTERFACE), and the declaration has the wrong layout (the extra ResetDeviceFormat
#     would put SetDefaultEndpoint on the not-implemented stub);
#   * "IPolicyConfig10": declared with IID_IUnknown, so the QI always succeeds and the call then
#     goes to a blind vtable slot of whatever object answered.
# So after construction the private _PolicyConfig field (the IPolicyConfig RCW) is checked; if it
# is null the script reports E_NOINTERFACE (exit 2) instead of ever reaching a fallback.
function Set-DefaultAllRoles([string]$Id) {
    try {
        $policy = New-Object CoreAudioApi.PolicyConfigClient
    } catch {
        return (Write-StepError 'CoCreateInstance(PolicyConfigClient)' $_)
    }
    $policyField = [CoreAudioApi.PolicyConfigClient].GetField('_PolicyConfig', [System.Reflection.BindingFlags]'NonPublic,Instance')
    if ($null -eq $policyField -or $null -eq $policyField.GetValue($policy)) {
        [void]$script:Err.Append("error: QueryInterface(IPolicyConfig) hr=$(Format-Hex32 $E_NOINTERFACE)`n")
        return $EXIT_COM_FAILURE
    }
    $steps = @(
        @{ Role = $ROLE_CONSOLE; Step = 'SetDefaultEndpoint(eConsole)' },
        @{ Role = $ROLE_MULTIMEDIA; Step = 'SetDefaultEndpoint(eMultimedia)' },
        @{ Role = $ROLE_COMMUNICATIONS; Step = 'SetDefaultEndpoint(eCommunications)' }
    )
    foreach ($s in $steps) {
        try {
            $policy.SetDefaultEndpoint($Id, [CoreAudioApi.ERole]$s.Role)
        } catch {
            return (Write-StepError $s.Step $_)
        }
    }
    return $EXIT_OK
}

# ---------------------------------------------------------------------------------------------
# Commands (each returns an exit code and appends to the output buffers)
# ---------------------------------------------------------------------------------------------

function Invoke-List($Enumerator) {
    # No default device is not an error for list: it just means no marker.
    try { $consoleDefault = Get-DefaultEndpoint $Enumerator $ROLE_CONSOLE } catch { return (Write-StepError 'GetDefaultAudioEndpoint(eConsole)' $_) }
    try { $commsDefault = Get-DefaultEndpoint $Enumerator $ROLE_COMMUNICATIONS } catch { return (Write-StepError 'GetDefaultAudioEndpoint(eCommunications)' $_) }
    try {
        $defaultId = if ($null -ne $consoleDefault) { $consoleDefault.ID } else { $null }   # IMMDevice::GetId
        $commsId = if ($null -ne $commsDefault) { $commsDefault.ID } else { $null }
    } catch {
        return (Write-StepError 'GetId' $_)
    }

    # The cmdlet enumerates eAll + DEVICE_STATE_ACTIVE (in endpoint-id order, the same order as
    # EnumAudioEndpoints(eRender)); keep the playback rows. Two fidelity limits of the cmdlet (see
    # README.md "Known limitations"): it wraps an enumeration failure in a plain System.Exception,
    # so the HRESULT printed is COR_E_EXCEPTION (0x80131500) rather than the real one; and a missing
    # PKEY_Device_FriendlyName comes back as the literal "Unknown" (the C reference prints "").
    try { $devices = @(Get-AudioDevice -List) } catch { return (Write-StepError 'Get-AudioDevice -List' $_) }
    foreach ($d in $devices) {
        if ($d.Type -ne 'Playback') { continue }
        $isDefault = Test-SameId $d.ID $defaultId
        $isComms = Test-SameId $d.ID $commsId
        $flags = ''
        if ($isDefault) { $flags += '*' }
        if ($isComms) { $flags += 'c' }
        if (-not $isDefault -and -not $isComms) { $flags = '-' }
        [void]$script:Out.Append($d.ID).Append("`t").Append((ConvertTo-Field $d.Name)).Append("`t").Append($flags).Append("`n")
    }
    return $EXIT_OK
}

function Invoke-Get($Enumerator) {
    try { $device = Get-DefaultEndpoint $Enumerator $ROLE_CONSOLE } catch { return (Write-StepError 'GetDefaultAudioEndpoint(eConsole)' $_) }
    if ($null -eq $device) {
        [void]$script:Err.Append("error: no default playback device`n")
        return $EXIT_NO_DEFAULT
    }
    try { $id = $device.ID } catch { return (Write-StepError 'GetId' $_) }
    try {
        $name = $device.FriendlyName     # PKEY_Device_FriendlyName ({a45c254e-...},14); "Unknown" if absent
    } catch {
        return (Write-StepError 'FriendlyName' $_)
    }
    [void]$script:Out.Append($id).Append("`t").Append((ConvertTo-Field $name)).Append("`n")
    return $EXIT_OK
}

function Invoke-Set($Enumerator, [string]$Id) {
    $resolved = Resolve-ActiveEndpoint $Enumerator $Id
    if ($resolved.ExitCode -ne $EXIT_OK) { return $resolved.ExitCode }
    return (Set-DefaultAllRoles $resolved.Id)
}

function Invoke-Toggle($Enumerator, [string]$IdA, [string]$IdB) {
    # No default at all simply means "not A", so the target is A.
    try { $current = Get-DefaultEndpoint $Enumerator $ROLE_CONSOLE } catch { return (Write-StepError 'GetDefaultAudioEndpoint(eConsole)' $_) }
    try { $currentId = if ($null -ne $current) { $current.ID } else { $null } } catch { return (Write-StepError 'GetId' $_) }
    $target = if (Test-SameId $currentId $IdA) { $IdB } else { $IdA }

    $resolved = Resolve-ActiveEndpoint $Enumerator $target
    if ($resolved.ExitCode -ne $EXIT_OK) { return $resolved.ExitCode }
    $rc = Set-DefaultAllRoles $resolved.Id
    if ($rc -eq $EXIT_OK) { [void]$script:Out.Append($resolved.Id).Append("`n") }
    return $rc
}

# Imports AudioDeviceCmdlets by name (PSModulePath search, as the proof of concept did, minus its
# expensive Get-Module -ListAvailable scan). PowerShell 7 does not search the Windows PowerShell
# per-user module folder (Documents\WindowsPowerShell\Modules) unless an inherited PSModulePath
# already contains it, so fall back to that folder explicitly.
function Import-AudioDeviceCmdlets {
    try {
        Import-Module -Name AudioDeviceCmdlets
    } catch {
        $fallback = Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'WindowsPowerShell\Modules\AudioDeviceCmdlets'
        if (-not (Test-Path -LiteralPath $fallback)) { throw }
        Import-Module -Name $fallback
    }
}

# ---------------------------------------------------------------------------------------------
# Argument handling and main flow
# ---------------------------------------------------------------------------------------------

# argv excludes the program name. Returns the process exit code.
function Invoke-Main([object[]]$Argv) {
    # "--timing" may appear anywhere; everything else is positional.
    #
    # The ps2exe host does not pass argv through verbatim: it turns anything that looks like a
    # PowerShell parameter into a named parameter (PowerShell.AddParameter), and since this script
    # declares no parameters the pair lands in $args again, rewritten:
    #   "--timing <next>"       -> "-timing", "<next>"   (the next word is kept, as its "value")
    #   "--timing" as last word -> "-timing", $true      (a switch: an extra [bool] element)
    # So under ps2exe accept both spellings and drop a [bool] that directly follows the flag.
    # powershell.exe and pwsh -File pass "--timing" through unchanged and never produce [bool]
    # elements, so there only the exact "--timing" counts and "-timing" stays positional (usage
    # error), like every other implementation. The ps2exe host identifies itself as
    # "PSRunspace-Host" (ps2exe 1.0.17); powershell.exe and pwsh report "ConsoleHost".
    $isPs2exeHost = ($Host.Name -ceq 'PSRunspace-Host')
    $positional = New-Object 'System.Collections.Generic.List[string]'
    $previousWasTiming = $false
    foreach ($a in $Argv) {
        if ($previousWasTiming -and $isPs2exeHost -and $a -is [bool]) { $previousWasTiming = $false; continue }
        $previousWasTiming = ($a -is [string]) -and ($a -ceq '--timing' -or ($isPs2exeHost -and $a -ceq '-timing'))
        if ($previousWasTiming) { $script:TimingEnabled = $true } else { $positional.Add([string]$a) }
    }
    if ($script:TimingEnabled) {
        # The "entry" phase is the timestamp taken by the first statement of the script.
        $script:PhaseNames.Add('entry')
        $script:PhaseTicks.Add($script:EntryTicks)
    }

    $badUsage = $true
    if ($positional.Count -ge 1) {
        switch -CaseSensitive ($positional[0]) {
            'list'   { $badUsage = ($positional.Count -ne 1) }
            'get'    { $badUsage = ($positional.Count -ne 1) }
            'set'    { $badUsage = ($positional.Count -ne 2) }
            'toggle' { $badUsage = ($positional.Count -ne 3) }
        }
    }

    # Usage path: the module is never imported, so this measures the host floor.
    if ($badUsage) {
        [void]$script:Err.Append($USAGE_TEXT)
        return $EXIT_USAGE
    }

    try { Import-AudioDeviceCmdlets } catch { return (Write-StepError 'Import-Module AudioDeviceCmdlets' $_) }
    Add-Phase 'com_init'

    try {
        $enumerator = New-Object CoreAudioApi.MMDeviceEnumerator
    } catch {
        return (Write-StepError 'CoCreateInstance(MMDeviceEnumerator)' $_)
    }
    Add-Phase 'enumerator'

    $rc = switch -CaseSensitive ($positional[0]) {
        'list'   { Invoke-List $enumerator }
        'get'    { Invoke-Get $enumerator }
        'set'    { Invoke-Set $enumerator $positional[1] }
        'toggle' { Invoke-Toggle $enumerator $positional[1] $positional[2] }
    }
    if ($rc -eq $EXIT_OK) { Add-Phase 'work_done' }
    return $rc
}

$exitCode = $EXIT_COM_FAILURE
try {
    # Passed as raw objects: under ps2exe $args can contain a [bool] (see Invoke-Main).
    $exitCode = Invoke-Main -Argv ([object[]]@($args))
} catch {
    # Anything not mapped to a specific step above (should not happen).
    $exitCode = Write-StepError 'unexpected' $_
}

Write-StdStream $false $script:Out.ToString()
Add-Phase 'exit'
if ($script:TimingEnabled) { Format-Timing }
Write-StdStream $true $script:Err.ToString()
exit $exitCode
