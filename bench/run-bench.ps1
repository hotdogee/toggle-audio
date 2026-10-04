#Requires -Version 7.0

<#
.SYNOPSIS
    Reproducible benchmark harness for every bench/<lang> implementation, the product and the
    PowerShell baselines. Writes raw hyperfine JSON and a generated summary to -OutDir.

.DESCRIPTION
    Steps (each can be skipped where noted):
      1. Build (unless -SkipBuild): every bench\<lang>\build.ps1 and the product
         (cargo build --release with CARGO_TARGET_DIR=target\release-build).
      2. Discover the executables and record their sizes.
      3. Correctness gate (unless -SkipGate): per implementation, toggle twice (verified after each
         step with the AudioDeviceCmdlets oracle in Windows PowerShell 5.1, playback AND
         communications), list (flags), get, set <bogus id> (exit 3), no args (exit 1). The
         toggle steps are skipped with -SkipToggle.
      4. hyperfine -N scenarios, -Rounds rounds alternating forward / reverse command order:
           noargs   runtime floor (<exe> with no args, exit 1; the product uses --version; nop*.exe)
           list     <exe> list
           get      <exe> get
           set-noop <exe> set <PG42UQ id>   (PG42UQ is the default, so nothing changes)
         PowerShell baselines (unless -SkipPowerShell) with -PsRuns/-PsWarmup: empty host start,
         ta.ps1 under powershell.exe and pwsh, bin\ta-ps.exe (ps2exe).
      5. --timing phase capture (unless -SkipTiming): -TimingRuns runs of "<exe> set <PG42UQ id> --timing" per exe,
         median per phase (stderr parsed; bench lines "phase\t..", product lines "timing\t..").
      6. Real toggle scenario (unless -SkipToggle): hyperfine --warmup 2 --runs -ToggleRuns (even)
         of "<exe> toggle <PG42UQ> <PHL BDM4065>"; the product toggles with its own config
         (APPDATA points at a temp dir); the legacy -LegacyExe (when given) and
         bench\powershell\switch-audio.ps1 toggle S/PDIF <-> PG42UQ (even counts only). The
         default device is verified before and after every toggle row and PG42UQ is restored.
      7. spawnbench (unless -SkipSpawn): nop floors in inherit / noconsole mode, explicit
         CREATE_NEW_CONSOLE, and a G HUB style launch: spawnbench itself is started WITHOUT a
         console (DETACHED_PROCESS), so its children are created with no flags by a console-less
         parent, exactly like a GUI launcher. Console windows that stay open are closed.
      8. Cold first run (unless -SkipCold): each exe is copied to a fresh file with a few random
         bytes appended (new hash, so Defender has never seen it) and launched once with "list".
      9. Summary: summary.json and summary.md in -OutDir.

    A finally block always restores PG42UQ as the default for all roles and verifies it.
    Never touches the user's real toggle-audio config (APPDATA is redirected for the product).

.PARAMETER Runs
    hyperfine runs per command and round in the main scenarios (default 200).

.PARAMETER Warmup
    hyperfine warm-up runs per command in the main scenarios (default 10).

.PARAMETER Rounds
    Rounds per scenario; odd rounds run the commands forward, even rounds in reverse (default 3).

.PARAMETER ToggleRuns
    Runs per real toggle row (step 6). Must be even, so every row ends on the device it started
    from (default 20).

.PARAMETER PsRuns
    hyperfine runs for the PowerShell baselines (default 20).

.PARAMETER PsWarmup
    hyperfine warm-up runs for the PowerShell baselines (default 3).

.PARAMETER TimingRuns
    Runs per executable for the --timing phase capture (step 5, default 30).

.PARAMETER SpawnRuns
    Runs per spawnbench row that opens a console window or emulates the G HUB launch (step 7,
    default 10). The inherit / noconsole floors use -Runs and -Warmup.

.PARAMETER ColdCopies
    Fresh copies per executable for the cold first run (step 8, default 3).

.PARAMETER Scenarios
    The hyperfine scenarios to run: any of noargs, list, get, set-noop (default all four).

.PARAMETER SkipTiming
    Skip the --timing phase capture (step 5).

.PARAMETER SkipToggle
    Never change the default device: skip the toggle steps of the gate and the real toggle
    scenario (step 6).

.PARAMETER SkipBuild
    Use the executables already built instead of running every build script (step 1).

.PARAMETER SkipGate
    Skip the correctness gate (step 3).

.PARAMETER SkipPowerShell
    Skip the PowerShell baselines in step 4.

.PARAMETER SkipSpawn
    Skip spawnbench (step 7).

.PARAMETER SkipCold
    Skip the cold first run (step 8).

.PARAMETER OutDir
    Where the raw results, run-bench.log and the summary are written (default bench\results).

.PARAMETER Hyperfine
    Path of hyperfine.exe. Default: hyperfine on PATH, then the WinGet links and packages folders.

.PARAMETER ProductDir
    Folder holding toggle-audio.exe and toggle-audiow.exe (default target\release-build\release,
    where step 1 builds them).

.PARAMETER LegacyExe
    Path of the original Switch-Audio.exe proof of concept (a ps2exe build of
    bench\powershell\switch-audio.ps1 that toggles S/PDIF <-> PG42UQ by name). Its rows are
    left out when this is not given.

.EXAMPLE
    pwsh -NoProfile -File bench\run-bench.ps1 -SkipBuild

.EXAMPLE
    pwsh -NoProfile -File bench\run-bench.ps1 -Runs 50 -Rounds 1 -SkipToggle -SkipPowerShell
#>
[CmdletBinding()]
param(
    [ValidateRange(2, 100000)][int]$Runs = 200,
    [ValidateRange(0, 1000)][int]$Warmup = 10,
    [ValidateRange(1, 20)][int]$Rounds = 3,
    [ValidateRange(2, 1000)][int]$ToggleRuns = 20,
    [ValidateRange(2, 1000)][int]$PsRuns = 20,
    [ValidateRange(0, 100)][int]$PsWarmup = 3,
    [ValidateRange(1, 1000)][int]$TimingRuns = 30,
    [ValidateRange(1, 20)][int]$SpawnRuns = 10,
    [ValidateRange(1, 10)][int]$ColdCopies = 3,
    [ValidateSet('noargs', 'list', 'get', 'set-noop')][string[]]$Scenarios = @('noargs', 'list', 'get', 'set-noop'),
    [switch]$SkipTiming,
    [switch]$SkipToggle,
    [switch]$SkipBuild,
    [switch]$SkipGate,
    [switch]$SkipPowerShell,
    [switch]$SkipSpawn,
    [switch]$SkipCold,
    [string]$OutDir = (Join-Path $PSScriptRoot 'results'),
    [string]$Hyperfine,
    [string]$ProductDir = (Join-Path (Split-Path -Parent $PSScriptRoot) 'target\release-build\release'),
    [string]$LegacyExe
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 3.0

if ($ToggleRuns % 2) { throw '-ToggleRuns must be even, so every toggle row ends on the device it started from.' }

$Bench = $PSScriptRoot
$Repo = Split-Path -Parent $Bench
$Pg = '{0.0.0.00000000}.{739b3554-bfed-4d61-b407-a818b317c991}'      # PG42UQ (NVIDIA High Definition Audio)
$Phl = '{0.0.0.00000000}.{5b124733-5d8f-428c-b83c-ee05ce6467fb}'     # PHL BDM4065 (NVIDIA High Definition Audio)
$Spdif = '{0.0.0.00000000}.{0b317ab3-7e08-4d56-975a-f33b90d5b57a}'   # Digital Audio (S/PDIF), legacy script only
$Bogus = '{0.0.0.00000000}.{00000000-0000-0000-0000-000000000000}'
$Legacy = $LegacyExe
$TaPs1 = Join-Path $Bench 'powershell\ta.ps1'
$LegacyPs1 = Join-Path $Bench 'powershell\switch-audio.ps1'
$ToggleWarmup = 2

New-Item -ItemType Directory -Force $OutDir | Out-Null
$OutDir = (Resolve-Path $OutDir).Path
$LogFile = Join-Path $OutDir 'run-bench.log'
"run-bench started $(Get-Date -Format o)" | Set-Content -Encoding utf8 $LogFile

function Write-Log([string]$Text, [string]$Color = 'Gray') {
    Write-Host $Text -ForegroundColor $Color
    Add-Content -Encoding utf8 -LiteralPath $LogFile -Value $Text
}
function Write-Step([string]$Text) { Write-Log "`n== $Text" 'Cyan' }
function Fwd([string]$Path) { return ($Path -replace '\\', '/') }
function Test-SameId([string]$A, [string]$B) { return [string]::Equals($A, $B, [StringComparison]::OrdinalIgnoreCase) }

# ---------------------------------------------------------------------------------------------
# Native helpers: detached launcher (G HUB emulation) and console-window cleanup.
# ---------------------------------------------------------------------------------------------
if (-not ('TaBenchNative' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class TaBenchNative {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct STARTUPINFO {
        public int cb; public string lpReserved; public string lpDesktop; public string lpTitle;
        public int dwX, dwY, dwXSize, dwYSize, dwXCountChars, dwYCountChars, dwFillAttribute, dwFlags;
        public short wShowWindow, cbReserved2; public IntPtr lpReserved2, hStdInput, hStdOutput, hStdError;
    }
    [StructLayout(LayoutKind.Sequential)]
    struct PROCESS_INFORMATION { public IntPtr hProcess, hThread; public int dwProcessId, dwThreadId; }
    [StructLayout(LayoutKind.Sequential)]
    struct SECURITY_ATTRIBUTES { public int nLength; public IntPtr lpSecurityDescriptor; public int bInheritHandle; }

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern bool CreateProcessW(string app, StringBuilder cmd, IntPtr pa, IntPtr ta, bool inherit,
        uint flags, IntPtr env, string cwd, ref STARTUPINFO si, out PROCESS_INFORMATION pi);
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern IntPtr CreateFileW(string name, uint access, uint share, ref SECURITY_ATTRIBUTES sa,
        uint disposition, uint flags, IntPtr template);
    [DllImport("kernel32.dll")] static extern uint WaitForSingleObject(IntPtr h, uint ms);
    [DllImport("kernel32.dll")] static extern bool GetExitCodeProcess(IntPtr h, out uint code);
    [DllImport("kernel32.dll")] static extern bool TerminateProcess(IntPtr h, uint code);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);

    delegate bool EnumWindowsProc(IntPtr hwnd, IntPtr lParam);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumWindowsProc cb, IntPtr lParam);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassNameW(IntPtr hwnd, StringBuilder sb, int max);
    [DllImport("user32.dll")] static extern bool PostMessageW(IntPtr hwnd, uint msg, IntPtr w, IntPtr l);

    // Starts cmdline with DETACHED_PROCESS (no console at all, like a GUI parent such as G HUB),
    // stdin = NUL, stdout/stderr = outFile. Waits up to timeoutMs. Returns the exit code
    // (or -1 on timeout, after terminating the process).
    public static int RunDetached(string cmdline, string outFile, string cwd, uint timeoutMs) {
        var sa = new SECURITY_ATTRIBUTES { nLength = Marshal.SizeOf(typeof(SECURITY_ATTRIBUTES)), bInheritHandle = 1 };
        IntPtr hOut = CreateFileW(outFile, 0x40000000, 3, ref sa, 2, 0x80, IntPtr.Zero);
        if (hOut == new IntPtr(-1)) throw new System.ComponentModel.Win32Exception();
        IntPtr hIn = CreateFileW("NUL", 0x80000000, 3, ref sa, 3, 0, IntPtr.Zero);
        try {
            var si = new STARTUPINFO();
            si.cb = Marshal.SizeOf(typeof(STARTUPINFO));
            si.dwFlags = 0x100; // STARTF_USESTDHANDLES
            si.hStdInput = hIn; si.hStdOutput = hOut; si.hStdError = hOut;
            PROCESS_INFORMATION pi;
            if (!CreateProcessW(null, new StringBuilder(cmdline), IntPtr.Zero, IntPtr.Zero, true,
                    0x00000008 /* DETACHED_PROCESS */, IntPtr.Zero, cwd, ref si, out pi))
                throw new System.ComponentModel.Win32Exception();
            try {
                if (WaitForSingleObject(pi.hProcess, timeoutMs) != 0) { TerminateProcess(pi.hProcess, 99); return -1; }
                uint code; GetExitCodeProcess(pi.hProcess, out code); return (int)code;
            } finally { CloseHandle(pi.hThread); CloseHandle(pi.hProcess); }
        } finally { CloseHandle(hOut); CloseHandle(hIn); }
    }

    static readonly string[] ConsoleClasses = { "ConsoleWindowClass", "CASCADIA_HOSTING_WINDOW_CLASS", "PseudoConsoleWindow" };

    public static long[] ConsoleWindows() {
        var list = new List<long>();
        EnumWindows((h, l) => {
            var sb = new StringBuilder(256); GetClassNameW(h, sb, 256);
            if (IsWindowVisible(h) && Array.IndexOf(ConsoleClasses, sb.ToString()) >= 0) list.Add(h.ToInt64());
            return true;
        }, IntPtr.Zero);
        return list.ToArray();
    }

    // Posts WM_CLOSE to visible console / terminal windows that are not in 'before'.
    public static int CloseNewConsoleWindows(long[] before) {
        int n = 0;
        foreach (long h in ConsoleWindows())
            if (Array.IndexOf(before, h) < 0) { PostMessageW(new IntPtr(h), 0x0010, IntPtr.Zero, IntPtr.Zero); n++; }
        return n;
    }
}
'@
}

# ---------------------------------------------------------------------------------------------
# Process helpers
# ---------------------------------------------------------------------------------------------
$script:AppData = Join-Path ([IO.Path]::GetTempPath()) ('toggle-audio-bench-' + [guid]::NewGuid().ToString('N'))

# Runs a program with stdin closed (EOF; ps2exe reads stdin to EOF when it is redirected),
# stdout/stderr captured as UTF-8. -Product points APPDATA at the bench configuration.
function Invoke-Capture([string]$File, [string[]]$Arguments = @(), [switch]$Product, [int]$TimeoutMs = 120000) {
    $info = [Diagnostics.ProcessStartInfo]::new($File)
    foreach ($a in $Arguments) { $info.ArgumentList.Add($a) }
    if ($Product) { $info.Environment['APPDATA'] = $script:AppData }
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $false
    $info.RedirectStandardInput = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.StandardOutputEncoding = [Text.UTF8Encoding]::new($false)
    $info.StandardErrorEncoding = [Text.UTF8Encoding]::new($false)
    $p = [Diagnostics.Process]::Start($info)
    $p.StandardInput.Close()
    $errTask = $p.StandardError.ReadToEndAsync()
    $outTask = $p.StandardOutput.ReadToEndAsync()
    if (-not $p.WaitForExit($TimeoutMs)) { $p.Kill($true); throw "timeout: $File $($Arguments -join ' ')" }
    $p.WaitForExit()
    return [pscustomobject]@{ ExitCode = $p.ExitCode; Stdout = $outTask.Result; Stderr = $errTask.Result }
}

# Independent oracle: AudioDeviceCmdlets in Windows PowerShell 5.1.
function Get-Oracle {
    $out = & powershell.exe -NoProfile -NonInteractive -Command 'Import-Module AudioDeviceCmdlets; (Get-AudioDevice -Playback).ID; (Get-AudioDevice -PlaybackCommunication).ID; (Get-AudioDevice -Playback).Name'
    $lines = @($out | ForEach-Object { "$_".Trim() } | Where-Object { $_ })
    if ($lines.Count -lt 2) { throw "oracle failed: $out" }
    return [pscustomobject]@{ Playback = $lines[0]; Communication = $lines[1]; Name = ($lines | Select-Object -Skip 2) -join ' ' }
}
function Test-OracleIs([string]$Id) {
    $o = Get-Oracle
    return [pscustomobject]@{ Ok = ((Test-SameId $o.Playback $Id) -and (Test-SameId $o.Communication $Id)); Oracle = $o }
}

# Makes PG42UQ the default for console, multimedia and communications and verifies it.
function Restore-Pg {
    $setters = @()
    foreach ($e in @((Join-Path $Bench 'c\bin\ta-c.exe'), (Join-Path $Bench 'rust\bin\ta-rs.exe'))) { if (Test-Path $e) { $setters += , @($e, $false) } }
    $prod = Join-Path $ProductDir 'toggle-audio.exe'
    if (Test-Path $prod) { $setters += , @($prod, $true) }
    foreach ($s in $setters) {
        try { $null = Invoke-Capture -File $s[0] -Arguments @('set', $Pg) -Product:$s[1] } catch { Write-Log "restore via $($s[0]) failed: $_" 'Yellow' }
        $t = Test-OracleIs $Pg
        if ($t.Ok) { return $t.Oracle }
    }
    # Last resort: AudioDeviceCmdlets itself (multimedia + communications).
    & powershell.exe -NoProfile -NonInteractive -Command "Import-Module AudioDeviceCmdlets; Set-AudioDevice -ID '$Pg' | Out-Null" | Out-Null
    $t = Test-OracleIs $Pg
    if (-not $t.Ok) { Write-Log "COULD NOT RESTORE PG42UQ: playback=$($t.Oracle.Playback) comm=$($t.Oracle.Communication)" 'Red' }
    return $t.Oracle
}

function Find-Hyperfine {
    if ($Hyperfine) { return $Hyperfine }
    $c = Get-Command hyperfine -ErrorAction SilentlyContinue
    if ($c) { return $c.Source }
    foreach ($p in @("$env:LOCALAPPDATA\Microsoft\WinGet\Links\hyperfine.exe",
            (Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages\sharkdp.hyperfine*\*\hyperfine.exe" -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty FullName))) {
        if ($p -and (Test-Path $p)) { return $p }
    }
    throw 'hyperfine not found (pass -Hyperfine <path>)'
}

function Find-Pwsh {
    $pkg = Get-AppxPackage Microsoft.PowerShell -ErrorAction SilentlyContinue | Sort-Object Version -Descending | Select-Object -First 1
    if ($pkg -and (Test-Path (Join-Path $pkg.InstallLocation 'pwsh.exe'))) { return (Join-Path $pkg.InstallLocation 'pwsh.exe') }
    foreach ($p in @("$env:ProgramFiles\PowerShell\7\pwsh.exe")) { if (Test-Path $p) { return $p } }
    return (Get-Command pwsh -ErrorAction Stop).Source
}

# hyperfine -N command string: shell-words split, so quote any path with a space.
function Q([string]$Path) { $f = Fwd $Path; if ($f -match '\s') { return "'$f'" } return $f }

# ---------------------------------------------------------------------------------------------
# Statistics
# ---------------------------------------------------------------------------------------------
function Get-Stats([double[]]$Ms) {
    $n = $Ms.Count
    if ($n -eq 0) { return $null }
    $sorted = [double[]]($Ms | Sort-Object)
    $mean = ($Ms | Measure-Object -Average).Average
    $sd = 0.0
    if ($n -gt 1) { $sd = [math]::Sqrt((($Ms | ForEach-Object { ($_ - $mean) * ($_ - $mean) }) | Measure-Object -Sum).Sum / ($n - 1)) }
    $median = if ($n % 2) { $sorted[[int](($n - 1) / 2)] } else { ($sorted[[int]($n / 2) - 1] + $sorted[[int]($n / 2)]) / 2 }
    return [pscustomobject]@{ n = $n; mean = [math]::Round($mean, 3); sd = [math]::Round($sd, 3); median = [math]::Round($median, 3); min = [math]::Round($sorted[0], 3); max = [math]::Round($sorted[-1], 3) }
}
function Get-Median([double[]]$V) { $s = Get-Stats $V; if ($null -eq $s) { return [double]::NaN } return $s.median }

# ---------------------------------------------------------------------------------------------
# 1. Build
# ---------------------------------------------------------------------------------------------
$langs = 'baseline', 'c', 'rust', 'csharp', 'go', 'zig', 'powershell'
$buildStatus = [ordered]@{}
if (-not $SkipBuild) {
    Write-Step 'Build'
    foreach ($l in $langs) {
        $sw = [Diagnostics.Stopwatch]::StartNew()
        $blog = Join-Path ([IO.Path]::GetTempPath()) "toggle-audio-build-$l.log"
        & pwsh -NoProfile -NonInteractive -File (Join-Path $Bench "$l\build.ps1") *> $blog
        $buildStatus[$l] = if ($LASTEXITCODE -eq 0) { 'ok' } else { "FAILED (exit $LASTEXITCODE, log $blog)" }
        Write-Log ("  {0,-11} {1} ({2:n1} s)" -f $l, $buildStatus[$l], $sw.Elapsed.TotalSeconds)
    }
    $saved = $env:CARGO_TARGET_DIR
    try {
        $env:CARGO_TARGET_DIR = Join-Path $Repo 'target\release-build'
        Push-Location $Repo
        & cargo build --release 2>&1 | Out-Null
        $buildStatus['product'] = if ($LASTEXITCODE -eq 0) { 'ok' } else { "FAILED (exit $LASTEXITCODE)" }
    } finally { Pop-Location; $env:CARGO_TARGET_DIR = $saved }
    Write-Log ("  {0,-11} {1}" -f 'product', $buildStatus['product'])
}

# ---------------------------------------------------------------------------------------------
# 2. Discover
# ---------------------------------------------------------------------------------------------
Write-Step 'Discover executables'
$hf = Find-Hyperfine
$pwshExe = Find-Pwsh
$psExe = (Get-Command powershell.exe -ErrorAction Stop).Source

# Kind: nop (floor only), bench (common contract), product (toggle-audio CLI), ps (PowerShell rows)
$impls = [Collections.Generic.List[object]]::new()
function Add-Impl([string]$Name, [string]$Path, [string]$Kind, [string]$Note) {
    if (-not (Test-Path $Path)) { Write-Log "  missing: $Name ($Path), excluded" 'Yellow'; return }
    $impls.Add([pscustomobject]@{ Name = $Name; Path = (Resolve-Path $Path).Path; Kind = $Kind; Size = (Get-Item $Path).Length; Note = $Note })
}
Add-Impl 'nop' "$Bench\baseline\bin\nop.exe" 'nop' 'GUI subsystem, no CRT, ExitProcess(0)'
Add-Impl 'nop-con' "$Bench\baseline\bin\nop-con.exe" 'nop' 'console subsystem, no manifest'
Add-Impl 'nop-con-detached' "$Bench\baseline\bin\nop-con-detached.exe" 'nop' 'console + detached manifest'
Add-Impl 'ta-c' "$Bench\c\bin\ta-c.exe" 'bench' 'MSVC /MT, console + detached (primary C)'
Add-Impl 'ta-c-gui' "$Bench\c\bin\ta-c-gui.exe" 'bench' 'MSVC /MT, GUI subsystem'
Add-Impl 'ta-c-md' "$Bench\c\bin\ta-c-md.exe" 'bench' 'MSVC /MD (needs VC++ redist)'
Add-Impl 'ta-c-delayload' "$Bench\c\bin\ta-c-delayload.exe" 'bench' 'MSVC /MT, ole32 delay-loaded'
Add-Impl 'ta-c-nocrt' "$Bench\c\bin\ta-c-nocrt.exe" 'bench' 'MSVC, no CRT'
Add-Impl 'ta-rs' "$Bench\rust\bin\ta-rs.exe" 'bench' 'Rust, windows crate, crt-static'
Add-Impl 'ta-cs' "$Bench\csharp\bin\ta-cs.exe" 'bench' 'C# .NET 9 NativeAOT'
Add-Impl 'ta-go' "$Bench\go\bin\ta-go.exe" 'bench' 'Go, GOAMD64=v3'
Add-Impl 'ta-zig' "$Bench\zig\bin\ta-zig.exe" 'bench' 'Zig native'
Add-Impl 'ta-zigcc' "$Bench\zig\bin\ta-zigcc.exe" 'bench' 'ta.c via zig cc (clang + mingw, UCRT)'
Add-Impl 'toggle-audio' "$ProductDir\toggle-audio.exe" 'product' 'product, console + detached'
Add-Impl 'toggle-audiow' "$ProductDir\toggle-audiow.exe" 'product' 'product, GUI subsystem'
Add-Impl 'ta-ps' "$Bench\powershell\bin\ta-ps.exe" 'ps2exe' 'ps2exe x64 + detached manifest (WinPS 5.1 engine)'
Add-Impl 'ta-ps-anycpu' "$Bench\powershell\bin\ta-ps-anycpu.exe" 'ps2exe' 'ps2exe defaults, like the original'
if ($Legacy) { Add-Impl 'Switch-Audio (legacy)' $Legacy 'legacy' 'original ps2exe proof of concept, toggles S/PDIF <-> PG42UQ' }
foreach ($i in $impls) { Write-Log ("  {0,-22} {1,10:n0} B  {2}" -f $i.Name, $i.Size, $i.Path) }
Write-Log "  hyperfine: $hf ($(& $hf --version))"
Write-Log "  pwsh:      $pwshExe"
$byName = @{}; foreach ($i in $impls) { $byName[$i.Name] = $i }

# Machine info
$machine = [ordered]@{
    date        = (Get-Date -Format 'yyyy-MM-dd HH:mm:ss zzz')
    cpu         = (Get-CimInstance Win32_Processor | Select-Object -First 1).Name.Trim()
    logical     = (Get-CimInstance Win32_ComputerSystem).NumberOfLogicalProcessors
    ram_gb      = [math]::Round((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory / 1GB, 1)
    os          = (Get-CimInstance Win32_OperatingSystem).Caption + ' ' + [Environment]::OSVersion.Version.ToString()
    ubr         = (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion').UBR
    power_plan  = ((powercfg /getactivescheme) -join ' ').Trim()
    hyperfine   = (& $hf --version)
    pwsh        = $PSVersionTable.PSVersion.ToString()
    defender_rtp = $(try { (Get-MpComputerStatus).RealTimeProtectionEnabled } catch { 'unknown' })
    smart_app_control = $(try { (Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\CI\Policy' -ErrorAction Stop).VerifiedAndReputablePolicyState } catch { 'unknown' })
}
$machine | ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path $OutDir 'machine.json')

# Product config in a temp APPDATA
New-Item -ItemType Directory -Force (Join-Path $script:AppData 'toggle-audio') | Out-Null
$cfg = [ordered]@{
    version = 1
    device1 = [ordered]@{ id = $Pg; name = 'PG42UQ (NVIDIA High Definition Audio)' }
    device2 = [ordered]@{ id = $Phl; name = 'PHL BDM4065 (NVIDIA High Definition Audio)' }
    switch_communications = $true
}
[IO.File]::WriteAllText((Join-Path $script:AppData 'toggle-audio\config.json'), ($cfg | ConvertTo-Json), [Text.UTF8Encoding]::new($false))

# Per-implementation arguments for each scenario, as one space-separated string ('' = no
# arguments, $null = not applicable). Endpoint ids contain no spaces.
function Get-Args([object]$Impl, [string]$Scenario) {
    $bench = @{ 'noargs' = ''; 'list' = 'list'; 'get' = 'get'; 'set-noop' = "set $Pg"; 'toggle' = "toggle $Pg $Phl"; 'bogus' = "set $Bogus"; 'timing' = "set $Pg --timing" }
    # Product: no args = toggle, so its runtime floor is --version; toggle uses the bench config via APPDATA.
    $product = @{ 'noargs' = '--version'; 'list' = 'list'; 'get' = 'get'; 'set-noop' = "set $Pg"; 'toggle' = 'toggle'; 'bogus' = "set $Bogus"; 'timing' = "set $Pg --timing" }
    switch ($Impl.Kind) {
        'nop' { if ($Scenario -eq 'noargs') { return '' } }
        'bench' { return $bench[$Scenario] }
        'product' { return $product[$Scenario] }
    }
    return $null
}
function Split-Args([string]$A) { return [string[]]@($A -split ' ' | Where-Object { $_ }) }
function Get-Cmd([object]$Impl, [string]$A) { return ("$(Q $Impl.Path) $A").Trim() }

$results = [ordered]@{ machine = $machine; build = $buildStatus; sizes = [ordered]@{}; gate = @(); hyperfine = [ordered]@{}; timing = [ordered]@{}; toggle_verify = @(); spawn = @(); cold = @() }
foreach ($i in $impls) { $results.sizes[$i.Name] = $i.Size }
$savedAppData = $env:APPDATA

try {
    Write-Step 'Initial state'
    $o = Restore-Pg
    Write-Log "  oracle: playback=$($o.Playback) comm=$($o.Communication) ($($o.Name))"

    # -----------------------------------------------------------------------------------------
    # 3. Correctness gate
    # -----------------------------------------------------------------------------------------
    if (-not $SkipGate) {
        Write-Step 'Correctness gate'
        $ref = Invoke-Capture -File $byName['ta-c'].Path -Arguments @('list')
        $gateTargets = [Collections.Generic.List[object]]::new()
        foreach ($i in $impls | Where-Object { $_.Kind -in 'bench', 'product' }) {
            $gateTargets.Add([pscustomobject]@{ Name = $i.Name; File = $i.Path; Pre = @(); Kind = $i.Kind; Impl = $i })
        }
        if (-not $SkipPowerShell) {
            $gateTargets.Add([pscustomobject]@{ Name = 'ta.ps1 (powershell.exe)'; File = $psExe; Pre = @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', $TaPs1); Kind = 'bench'; Impl = $null })
            $gateTargets.Add([pscustomobject]@{ Name = 'ta.ps1 (pwsh)'; File = $pwshExe; Pre = @('-NoProfile', '-NonInteractive', '-File', $TaPs1); Kind = 'bench'; Impl = $null })
            if ($byName.ContainsKey('ta-ps')) { $gateTargets.Add([pscustomobject]@{ Name = 'ta-ps'; File = $byName['ta-ps'].Path; Pre = @(); Kind = 'bench'; Impl = $null }) }
        }
        $benchLike = [pscustomobject]@{ Kind = 'bench' }
        $prodLike = [pscustomobject]@{ Kind = 'product' }
        foreach ($t in $gateTargets) {
            $shape = if ($t.Kind -eq 'product') { $prodLike } else { $benchLike }
            $isProd = $t.Kind -eq 'product'
            $run = { param([string]$Sc) Invoke-Capture -File $t.File -Arguments ([string[]]@($t.Pre) + (Split-Args (Get-Args $shape $Sc))) -Product:$isProd }
            $g = [ordered]@{ impl = $t.Name; toggle1 = 'skipped'; toggle2 = 'skipped'; list = $null; list_eq_c = $null; get = $null; bogus = $null; noargs = $null; detail = @() }
            try {
                if (-not $SkipToggle) {
                    $null = Restore-Pg
                    $r1 = & $run 'toggle'; $v1 = Test-OracleIs $Phl
                    $out1ok = $isProd -or (Test-SameId $r1.Stdout.Trim() $Phl)
                    $g.toggle1 = ($r1.ExitCode -eq 0 -and $v1.Ok -and $out1ok)
                    if (-not $g.toggle1) { $g.detail += "toggle1 exit=$($r1.ExitCode) out=[$($r1.Stdout.Trim())] err=[$($r1.Stderr.Trim())] oracle=$($v1.Oracle.Playback)/$($v1.Oracle.Communication)" }
                    $r2 = & $run 'toggle'; $v2 = Test-OracleIs $Pg
                    $out2ok = $isProd -or (Test-SameId $r2.Stdout.Trim() $Pg)
                    $g.toggle2 = ($r2.ExitCode -eq 0 -and $v2.Ok -and $out2ok)
                    if (-not $g.toggle2) { $g.detail += "toggle2 exit=$($r2.ExitCode) out=[$($r2.Stdout.Trim())] err=[$($r2.Stderr.Trim())] oracle=$($v2.Oracle.Playback)/$($v2.Oracle.Communication)" }
                    if (-not $v2.Ok) { $null = Restore-Pg }
                }
                $l = & $run 'list'
                $lines = @($l.Stdout -split "`n" | Where-Object { $_ })
                $pgLine = $lines | Where-Object { $_.Split("`t")[0] -eq $Pg }
                $others = @($lines | Where-Object { $_.Split("`t")[0] -ne $Pg -and $_.Split("`t")[-1] -ne '-' })
                $g.list = ($l.ExitCode -eq 0 -and $pgLine -and $pgLine.Split("`t")[-1] -eq '*c' -and $others.Count -eq 0 -and @($lines | Where-Object { $_.Split("`t").Count -ne 3 }).Count -eq 0)
                if (-not $g.list) { $g.detail += "list exit=$($l.ExitCode) out=[$($l.Stdout)] err=[$($l.Stderr.Trim())]" }
                $g.list_eq_c = ($l.Stdout -ceq $ref.Stdout)
                $gt = & $run 'get'
                $g.get = ($gt.ExitCode -eq 0 -and $gt.Stdout -ceq "$Pg`tPG42UQ (NVIDIA High Definition Audio)`n")
                if (-not $g.get) { $g.detail += "get exit=$($gt.ExitCode) out=[$($gt.Stdout)]" }
                $b = & $run 'bogus'
                $expBogus = if ($isProd) { 4 } else { 3 }
                $g.bogus = ($b.ExitCode -eq $expBogus)
                if (-not $g.bogus) { $g.detail += "bogus exit=$($b.ExitCode) (expected $expBogus)" }
                if ($isProd) {
                    $n = Invoke-Capture -File $t.File -Arguments @('--no-such-flag') -Product
                    $g.noargs = ($n.ExitCode -eq 2)   # product: no args = toggle, so test a usage error (exit 2)
                } else {
                    $n = Invoke-Capture -File $t.File -Arguments @($t.Pre)
                    $g.noargs = ($n.ExitCode -eq 1)
                }
                if (-not $g.noargs) { $g.detail += "noargs/usage exit=$($n.ExitCode)" }
            } catch {
                $g.detail += "exception: $_"
                $null = Restore-Pg
            }
            $pass = @(@($g.toggle1, $g.toggle2, $g.list, $g.get, $g.bogus, $g.noargs) | Where-Object { $_ -is [bool] -and -not $_ })
            $g.pass = ($pass.Count -eq 0)
            Write-Log ("  {0,-24} toggle={1}/{2} list={3} (==C:{4}) get={5} bogus={6} usage={7} => {8}" -f $t.Name, $g.toggle1, $g.toggle2, $g.list, $g.list_eq_c, $g.get, $g.bogus, $g.noargs, $(if ($g.pass) { 'PASS' } else { 'FAIL' })) $(if ($g.pass) { 'Green' } else { 'Red' })
            foreach ($d in $g.detail) { Write-Log "      $d" 'Yellow' }
            $results.gate += [pscustomobject]$g
        }
        if (-not $SkipToggle -and $byName.ContainsKey('Switch-Audio (legacy)')) {
            $null = Restore-Pg
            $r1 = Invoke-Capture -File $Legacy; $v1 = Get-Oracle
            $r2 = Invoke-Capture -File $Legacy; $v2 = Get-Oracle
            $ok = ($r1.ExitCode -eq 0 -and (Test-SameId $v1.Playback $Spdif) -and $r2.ExitCode -eq 0 -and (Test-SameId $v2.Playback $Pg))
            Write-Log ("  {0,-24} toggle pair S/PDIF->PG42UQ: after1={1} after2={2} => {3}" -f 'Switch-Audio (legacy)', $v1.Name, $v2.Name, $(if ($ok) { 'PASS' } else { 'FAIL' })) $(if ($ok) { 'Green' } else { 'Red' })
            $results.gate += [pscustomobject]@{ impl = 'Switch-Audio (legacy)'; toggle1 = (Test-SameId $v1.Playback $Spdif); toggle2 = (Test-SameId $v2.Playback $Pg); list = $null; list_eq_c = $null; get = $null; bogus = $null; noargs = $null; detail = @(); pass = $ok }
            $null = Restore-Pg
        }
        $results.gate | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 (Join-Path $OutDir 'gate.json')
    }

    # -----------------------------------------------------------------------------------------
    # 4. hyperfine scenarios
    # -----------------------------------------------------------------------------------------
    function Invoke-Hf([string]$Scenario, [int]$Round, [object[]]$Cmds, [int]$R, [int]$W, [switch]$IgnoreFailure) {
        $json = Join-Path $OutDir "$Scenario-r$Round.json"
        $a = @('-N', '--warmup', $W, '--runs', $R, '--output=null', '--time-unit', 'millisecond', '--style', 'basic', '--export-json', $json)
        if ($IgnoreFailure) { $a += '-i' }
        foreach ($c in $Cmds) { $a += @('--command-name', $c.Name, $c.Cmd) }
        Write-Log "  hyperfine $Scenario round $Round ($($Cmds.Count) commands, $W + $R runs each)"
        & $hf @a 2>&1 | ForEach-Object { Add-Content -Encoding utf8 -LiteralPath $LogFile -Value "$_" }
        if ($LASTEXITCODE -ne 0) { Write-Log "  hyperfine exit $LASTEXITCODE for $Scenario r$Round" 'Red' }
    }
    function Invoke-Rounds([string]$Scenario, [object[]]$Cmds, [int]$R, [int]$W, [switch]$IgnoreFailure) {
        for ($k = 1; $k -le $Rounds; $k++) {
            $ordered = if ($k % 2) { $Cmds } else { [object[]]($Cmds[($Cmds.Count - 1)..0]) }
            Invoke-Hf $Scenario $k $ordered $R $W -IgnoreFailure:$IgnoreFailure
        }
    }

    Write-Step 'hyperfine: native scenarios'
    $env:APPDATA = $script:AppData   # only the product reads it
    try {
        foreach ($sc in $Scenarios) {
            $cmds = foreach ($i in $impls | Where-Object { $_.Kind -in 'nop', 'bench', 'product' }) {
                $a = Get-Args $i $sc
                if ($null -ne $a) { [pscustomobject]@{ Name = $i.Name; Cmd = (Get-Cmd $i $a) } }
            }
            Invoke-Rounds $sc @($cmds) $Runs $Warmup -IgnoreFailure:($sc -eq 'noargs')
        }
    } finally { $env:APPDATA = $savedAppData }
    $o = Test-OracleIs $Pg; if (-not $o.Ok) { Write-Log '  default changed during set-noop! restoring' 'Red'; $null = Restore-Pg }

    if (-not $SkipPowerShell) {
        Write-Step 'hyperfine: PowerShell baselines'
        $ps = "$(Q $psExe) -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $(Q $TaPs1)"
        $pw = "$(Q $pwshExe) -NoProfile -NonInteractive -File $(Q $TaPs1)"
        $tps = if ($byName.ContainsKey('ta-ps')) { Q $byName['ta-ps'].Path } else { $null }
        $tpa = if ($byName.ContainsKey('ta-ps-anycpu')) { Q $byName['ta-ps-anycpu'].Path } else { $null }
        $psNo = @(
            [pscustomobject]@{ Name = 'ps-host-empty'; Cmd = "$(Q $psExe) -NoLogo -NoProfile -NonInteractive -Command exit" }
            [pscustomobject]@{ Name = 'pwsh-host-empty'; Cmd = "$(Q $pwshExe) -NoLogo -NoProfile -NonInteractive -Command exit" }
            [pscustomobject]@{ Name = 'ps-ta (usage)'; Cmd = $ps }
            [pscustomobject]@{ Name = 'pwsh-ta (usage)'; Cmd = $pw }
        )
        if ($tps) { $psNo += [pscustomobject]@{ Name = 'ta-ps (usage)'; Cmd = $tps } }
        $psList = @(
            [pscustomobject]@{ Name = 'ps-ta list'; Cmd = "$ps list" }
            [pscustomobject]@{ Name = 'pwsh-ta list'; Cmd = "$pw list" }
        )
        if ($tps) { $psList += [pscustomobject]@{ Name = 'ta-ps list'; Cmd = "$tps list" } }
        $psSet = @(
            [pscustomobject]@{ Name = 'ps-ta set-noop'; Cmd = "$ps set $Pg" }
            [pscustomobject]@{ Name = 'pwsh-ta set-noop'; Cmd = "$pw set $Pg" }
        )
        if ($tps) { $psSet += [pscustomobject]@{ Name = 'ta-ps set-noop'; Cmd = "$tps set $Pg" } }
        if ($tpa) { $psSet += [pscustomobject]@{ Name = 'ta-ps-anycpu set-noop'; Cmd = "$tpa set $Pg" } }
        Invoke-Rounds 'ps-noargs' $psNo $PsRuns $PsWarmup -IgnoreFailure
        Invoke-Rounds 'ps-list' $psList $PsRuns $PsWarmup
        Invoke-Rounds 'ps-set-noop' $psSet $PsRuns $PsWarmup
        $o = Test-OracleIs $Pg; if (-not $o.Ok) { Write-Log '  default changed during PowerShell set-noop! restoring' 'Red'; $null = Restore-Pg }
    }

    # -----------------------------------------------------------------------------------------
    # 5. --timing phases
    # -----------------------------------------------------------------------------------------
    if (-not $SkipTiming) {
    Write-Step "--timing phase capture ($TimingRuns runs of set <PG42UQ> --timing)"
    $timingTargets = [Collections.Generic.List[object]]::new()
    foreach ($i in $impls | Where-Object { $_.Kind -in 'bench', 'product' }) {
        $timingTargets.Add([pscustomobject]@{ Name = $i.Name; File = $i.Path; Args = (Split-Args (Get-Args $i 'timing')); Product = ($i.Kind -eq 'product') })
    }
    if (-not $SkipPowerShell) {
        $timingTargets.Add([pscustomobject]@{ Name = 'ta.ps1 (powershell.exe)'; File = $psExe; Args = @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', $TaPs1, 'set', $Pg, '--timing'); Product = $false })
        $timingTargets.Add([pscustomobject]@{ Name = 'ta.ps1 (pwsh)'; File = $pwshExe; Args = @('-NoProfile', '-NonInteractive', '-File', $TaPs1, 'set', $Pg, '--timing'); Product = $false })
        if ($byName.ContainsKey('ta-ps')) { $timingTargets.Add([pscustomobject]@{ Name = 'ta-ps'; File = $byName['ta-ps'].Path; Args = @('set', $Pg, '--timing'); Product = $false }) }
    }
    $raw = [Text.StringBuilder]::new()
    [void]$raw.AppendLine("impl`trun`tphase`tus")
    foreach ($t in $timingTargets) {
        $phases = [ordered]@{}
        for ($k = -3; $k -lt $TimingRuns; $k++) {
            $r = Invoke-Capture -File $t.File -Arguments $t.Args -Product:$t.Product
            if ($k -lt 0) { continue }
            foreach ($line in ($r.Stderr -split "`r?`n")) {
                if ($line -match '^(phase|timing)\t(\S+)\t([0-9.]+)$') {
                    if (-not $phases.Contains($Matches[2])) { $phases[$Matches[2]] = [Collections.Generic.List[double]]::new() }
                    $phases[$Matches[2]].Add([double]$Matches[3])
                    [void]$raw.AppendLine("$($t.Name)`t$k`t$($Matches[2])`t$($Matches[3])")
                }
            }
        }
        $med = [ordered]@{}
        foreach ($p in $phases.Keys) { $med[$p] = [math]::Round((Get-Median $phases[$p].ToArray()), 1) }
        $results.timing[$t.Name] = $med
        Write-Log ("  {0,-24} {1}" -f $t.Name, (($med.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join ' '))
    }
    [IO.File]::WriteAllText((Join-Path $OutDir 'timing-raw.tsv'), $raw.ToString(), [Text.UTF8Encoding]::new($false))
    }

    # -----------------------------------------------------------------------------------------
    # 6. Real toggle
    # -----------------------------------------------------------------------------------------
    if (-not $SkipToggle) {
        Write-Step "Real toggle (warmup $ToggleWarmup + $ToggleRuns runs per row, even)"
        $toggleRows = [Collections.Generic.List[object]]::new()
        foreach ($i in $impls | Where-Object { $_.Kind -in 'bench', 'product' }) {
            $toggleRows.Add([pscustomobject]@{ Name = $i.Name; Cmd = (Get-Cmd $i (Get-Args $i 'toggle')); Product = ($i.Kind -eq 'product'); Back = $Pg })
        }
        if (-not $SkipPowerShell) {
            if ($byName.ContainsKey('ta-ps')) { $toggleRows.Add([pscustomobject]@{ Name = 'ta-ps'; Cmd = "$(Q $byName['ta-ps'].Path) toggle $Pg $Phl"; Product = $false; Back = $Pg }) }
            $toggleRows.Add([pscustomobject]@{ Name = 'ps-ta'; Cmd = "$(Q $psExe) -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $(Q $TaPs1) toggle $Pg $Phl"; Product = $false; Back = $Pg })
            $toggleRows.Add([pscustomobject]@{ Name = 'pwsh-ta'; Cmd = "$(Q $pwshExe) -NoProfile -NonInteractive -File $(Q $TaPs1) toggle $Pg $Phl"; Product = $false; Back = $Pg })
            if ($byName.ContainsKey('Switch-Audio (legacy)')) { $toggleRows.Add([pscustomobject]@{ Name = 'Switch-Audio (legacy)'; Cmd = (Q $Legacy); Product = $false; Back = $Pg }) }
            $toggleRows.Add([pscustomobject]@{ Name = 'switch-audio.ps1 (legacy)'; Cmd = "$(Q $psExe) -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $(Q $LegacyPs1)"; Product = $false; Back = $Pg })
        }
        foreach ($row in $toggleRows) {
            $pre = Test-OracleIs $Pg
            if (-not $pre.Ok) { $null = Restore-Pg }
            $safe = ($row.Name -replace '[^A-Za-z0-9_.-]+', '_').Trim('_')
            $json = Join-Path $OutDir "toggle-$safe.json"
            if ($row.Product) { $env:APPDATA = $script:AppData }
            try {
                & $hf -N --warmup $ToggleWarmup --runs $ToggleRuns --output=null --time-unit millisecond --style basic --export-json $json --command-name $row.Name $row.Cmd 2>&1 |
                    ForEach-Object { Add-Content -Encoding utf8 -LiteralPath $LogFile -Value "$_" }
                $hfExit = $LASTEXITCODE
            } finally { $env:APPDATA = $savedAppData }
            $post = Get-Oracle
            $ok = (Test-SameId $post.Playback $Pg) -and (Test-SameId $post.Communication $Pg)
            Write-Log ("  {0,-26} hyperfine exit={1}; after: playback={2} comm={3} => {4}" -f $row.Name, $hfExit, $post.Playback.Substring(18, 8), $post.Communication.Substring(18, 8), $(if ($ok) { 'PG42UQ ok' } else { 'NOT PG42UQ, restoring' })) $(if ($ok) { 'Green' } else { 'Yellow' })
            $results.toggle_verify += [pscustomobject]@{ impl = $row.Name; hyperfine_exit = $hfExit; playback_after = $post.Playback; communication_after = $post.Communication; back_on_pg = $ok }
            $null = Restore-Pg
        }
    }

    # -----------------------------------------------------------------------------------------
    # 7. spawnbench: floors and the G HUB launch (console allocation)
    # -----------------------------------------------------------------------------------------
    if (-not $SkipSpawn -and (Test-Path "$Bench\baseline\bin\spawnbench.exe")) {
        Write-Step 'spawnbench'
        $sb = (Resolve-Path "$Bench\baseline\bin\spawnbench.exe").Path
        function ConvertFrom-Spawn([string]$Text) {
            $s = [ordered]@{}
            foreach ($line in ($Text -split "`r?`n")) { if ($line -match '^stat\t(\S+)\t(\S+)') { $s[$Matches[1]] = [double]$Matches[2] } }
            return $s
        }
        function Add-Spawn([string]$Label, [string]$Launcher, [string]$Mode, [string]$Exe, [string[]]$A, [int]$N, [int]$W) {
            $before = [TaBenchNative]::ConsoleWindows()
            $safe = ("spawn-$Launcher-$Mode-" + [IO.Path]::GetFileNameWithoutExtension($Exe) + ($(if ($A) { '-' + ($A -join '-') } else { '' }))) -replace '[^A-Za-z0-9_.-]+', '_'
            $outFile = Join-Path $OutDir "$safe.tsv"
            if ($Launcher -eq 'console') {
                $text = (& $sb -m $Mode -n $N -w $W -- $Exe @A) -join "`n"
            } else {
                # G HUB emulation: spawnbench has NO console (DETACHED_PROCESS); its children get flags 0.
                $cl = "`"$sb`" -m $Mode -n $N -w $W -- `"$Exe`" " + ($A -join ' ')
                $code = [TaBenchNative]::RunDetached($cl, $outFile, $Repo, 300000)
                if ($code -ne 0) { Write-Log "  detached spawnbench exit $code for $Label" 'Red' }
                $text = (Get-Content -Raw $outFile) -replace "`r`n", "`n"
            }
            # The header line names the exe; keep this machine's checkout path out of the results.
            $text = $text.Replace("$Repo\", '<repo>\').TrimEnd("`n")
            [IO.File]::WriteAllText($outFile, $text + "`n", [Text.UTF8Encoding]::new($false))
            Start-Sleep -Milliseconds 300
            $closed = [TaBenchNative]::CloseNewConsoleWindows($before)
            $s = ConvertFrom-Spawn $text
            $rec = [pscustomobject]@{ label = $Label; launcher = $Launcher; mode = $Mode; exe = [IO.Path]::GetFileName($Exe); args = ($A -join ' '); runs = $s['runs']; failures = $s['failures']; median_ms = [math]::Round($s['median_us'] / 1000, 3); mean_ms = [math]::Round($s['mean_us'] / 1000, 3); min_ms = [math]::Round($s['min_us'] / 1000, 3); p95_ms = [math]::Round($s['p95_us'] / 1000, 3); max_ms = [math]::Round($s['max_us'] / 1000, 3); windows_left_open_closed = $closed }
            Write-Log ("  {0,-58} median {1,8:n2} ms  mean {2,8:n2}  min {3,8:n2}  max {4,8:n2}  fail {5}  closed {6}" -f $Label, $rec.median_ms, $rec.mean_ms, $rec.min_ms, $rec.max_ms, $rec.failures, $closed)
            $results.spawn += $rec
        }
        $nop = $byName['nop'].Path; $nopc = $byName['nop-con'].Path; $nopd = $byName['nop-con-detached'].Path
        foreach ($e in $nop, $nopc, $nopd) {
            Add-Spawn "inherit    $([IO.Path]::GetFileName($e))" 'console' 'inherit' $e @() $Runs $Warmup
            Add-Spawn "noconsole  $([IO.Path]::GetFileName($e))" 'console' 'noconsole' $e @() $Runs $Warmup
        }
        # Explicit CREATE_NEW_CONSOLE (a window per run)
        Add-Spawn 'newconsole nop-con.exe' 'console' 'newconsole' $nopc @() $SpawnRuns 1
        Add-Spawn 'newconsole nop-con-detached.exe' 'console' 'newconsole' $nopd @() $SpawnRuns 1
        # G HUB style: console-less parent, no creation flags.
        $ghub = @(
            @($nop, @()), @($nopc, @()), @($nopd, @()),
            @($byName['ta-c'].Path, @('get')), @($byName['ta-c-gui'].Path, @('get'))
        )
        if ($byName.ContainsKey('toggle-audio')) { $ghub += , @($byName['toggle-audio'].Path, @('get')) }
        if ($byName.ContainsKey('toggle-audiow')) { $ghub += , @($byName['toggle-audiow'].Path, @('get')) }
        $env:APPDATA = $script:AppData
        try {
            foreach ($g in $ghub) {
                Add-Spawn ("G HUB-style (no-console parent) $([IO.Path]::GetFileName($g[0])) $($g[1] -join ' ')") 'detached-parent' 'inherit' $g[0] $g[1] $SpawnRuns 1
            }
        } finally { $env:APPDATA = $savedAppData }
        $results.spawn | ConvertTo-Json -Depth 4 | Set-Content -Encoding utf8 (Join-Path $OutDir 'spawn.json')
    }

    # -----------------------------------------------------------------------------------------
    # 8. Cold first run (new hash each copy)
    # -----------------------------------------------------------------------------------------
    if (-not $SkipCold -and (Test-Path "$Bench\baseline\bin\spawnbench.exe")) {
        Write-Step "Cold first run ($ColdCopies fresh copies per exe, one 'list' launch each)"
        $sb = (Resolve-Path "$Bench\baseline\bin\spawnbench.exe").Path
        $env:APPDATA = $script:AppData
        try {
            foreach ($i in $impls | Where-Object { $_.Kind -in 'bench', 'product', 'ps2exe' }) {
                $times = @()
                for ($k = 1; $k -le $ColdCopies; $k++) {
                    $copy = Join-Path (Split-Path $i.Path) ("cold-" + [guid]::NewGuid().ToString('N').Substring(0, 8) + '-' + [IO.Path]::GetFileName($i.Path))
                    Copy-Item $i.Path $copy
                    $bytes = [byte[]]::new(16); [Security.Cryptography.RandomNumberGenerator]::Fill($bytes)
                    $fs = [IO.File]::Open($copy, 'Append'); $fs.Write($bytes, 0, 16); $fs.Close()
                    try {
                        $text = (& $sb -q -n 1 -w 0 -- $copy list) -join "`n"
                        $m = [regex]::Match($text, 'stat\tmedian_us\t(\S+)')
                        if ($m.Success) { $times += [double]$m.Groups[1].Value / 1000 }
                    } finally { Start-Sleep -Milliseconds 200; Remove-Item $copy -Force -ErrorAction SilentlyContinue }
                }
                $results.cold += [pscustomobject]@{ impl = $i.Name; runs_ms = ($times | ForEach-Object { [math]::Round($_, 2) }); median_ms = [math]::Round((Get-Median $times), 2) }
                Write-Log ("  {0,-22} {1}" -f $i.Name, (($times | ForEach-Object { '{0:n1}' -f $_ }) -join ' / '))
            }
        } finally { $env:APPDATA = $savedAppData }
    }
} finally {
    $env:APPDATA = $savedAppData
    Write-Step 'Final restore'
    $final = Restore-Pg
    Write-Log "  oracle: playback=$($final.Playback) comm=$($final.Communication) ($($final.Name))"
    $results.final_oracle = $final
    Remove-Item -Recurse -Force $script:AppData -ErrorAction SilentlyContinue
}

# ---------------------------------------------------------------------------------------------
# 9. Summary
# ---------------------------------------------------------------------------------------------
Write-Step 'Summary'
function Get-Pooled([string]$Prefix) {
    $pool = [ordered]@{}
    foreach ($f in Get-ChildItem $OutDir -Filter "$Prefix-r*.json" | Sort-Object Name) {
        $j = Get-Content -Raw $f.FullName | ConvertFrom-Json
        foreach ($r in $j.results) {
            if (-not $pool.Contains($r.command)) { $pool[$r.command] = [Collections.Generic.List[double]]::new() }
            foreach ($t in $r.times) { $pool[$r.command].Add([double]$t * 1000) }
        }
    }
    $out = [ordered]@{}
    foreach ($k in $pool.Keys) { $out[$k] = Get-Stats $pool[$k].ToArray() }
    return $out
}
foreach ($sc in 'noargs', 'list', 'get', 'set-noop', 'ps-noargs', 'ps-list', 'ps-set-noop') { $results.hyperfine[$sc] = Get-Pooled $sc }
$tg = [ordered]@{}
foreach ($f in Get-ChildItem $OutDir -Filter 'toggle-*.json' -ErrorAction SilentlyContinue) {
    $j = Get-Content -Raw $f.FullName | ConvertFrom-Json
    foreach ($r in $j.results) { $tg[$r.command] = Get-Stats ([double[]]($r.times | ForEach-Object { $_ * 1000 })) }
}
$results.hyperfine['toggle'] = $tg
$results | ConvertTo-Json -Depth 8 | Set-Content -Encoding utf8 (Join-Path $OutDir 'summary.json')

function Fmt($s) { if ($null -eq $s) { return 'n/a' } return ('{0:n2} ± {1:n2} ({2:n2})' -f $s.mean, $s.sd, $s.median) }
$md = [Text.StringBuilder]::new()
[void]$md.AppendLine("# Generated summary ($($machine.date))")
[void]$md.AppendLine('')
[void]$md.AppendLine('mean ± σ (median), ms; hyperfine -N, pooled over rounds.')
[void]$md.AppendLine('')
[void]$md.AppendLine('| Implementation | Size (bytes) | noargs | list | get | set-noop | toggle |')
[void]$md.AppendLine('| --- | ---: | --- | --- | --- | --- | --- |')
foreach ($i in $impls | Where-Object { $_.Kind -in 'nop', 'bench', 'product' }) {
    [void]$md.AppendLine("| $($i.Name) | $('{0:n0}' -f $i.Size) | $(Fmt $results.hyperfine['noargs'][$i.Name]) | $(Fmt $results.hyperfine['list'][$i.Name]) | $(Fmt $results.hyperfine['get'][$i.Name]) | $(Fmt $results.hyperfine['set-noop'][$i.Name]) | $(Fmt $tg[$i.Name]) |")
}
[void]$md.AppendLine('')
[void]$md.AppendLine('| PowerShell row | Scenario | mean ± σ (median) |')
[void]$md.AppendLine('| --- | --- | --- |')
foreach ($sc in 'ps-noargs', 'ps-list', 'ps-set-noop') { foreach ($k in $results.hyperfine[$sc].Keys) { [void]$md.AppendLine("| $k | $sc | $(Fmt $results.hyperfine[$sc][$k]) |") } }
foreach ($k in $tg.Keys) { if (-not $byName.ContainsKey($k) -or $byName[$k].Kind -notin 'bench', 'product') { [void]$md.AppendLine("| $k | toggle | $(Fmt $tg[$k]) |") } }
[void]$md.AppendLine('')
[void]$md.AppendLine('Phase medians (µs since process creation), set <PG42UQ> --timing:')
[void]$md.AppendLine('')
foreach ($k in $results.timing.Keys) { [void]$md.AppendLine("- $k : " + (($results.timing[$k].GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join ', ')) }
[IO.File]::WriteAllText((Join-Path $OutDir 'summary.md'), $md.ToString(), [Text.UTF8Encoding]::new($false))
Write-Log $md.ToString()
Write-Log "done $(Get-Date -Format o)"
