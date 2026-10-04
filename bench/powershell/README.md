# PowerShell baselines (`ta.ps1`, `bin\ta-ps.exe`)

This directory holds the PowerShell starting point of Toggle Audio and a PowerShell implementation of the common bench CLI contract, so the numbers that motivated the native rewrite can be reproduced with the same harness as every other implementation.

| File | What it is |
| --- | --- |
| `switch-audio.ps1` | The original proof of concept, unchanged. It toggles between two devices chosen by **name** (S/PDIF and PG42UQ). |
| `ta.ps1` | The bench contract (`list`, `get`, `set`, `toggle`, no-args, `--timing`) on top of the AudioDeviceCmdlets module. It runs under Windows PowerShell 5.1, PowerShell 7 and ps2exe. |
| `build.ps1` | Packages `ta.ps1` with ps2exe into `bin\ta-ps.exe` (and `bin\ta-ps-anycpu.exe`), embeds the bench manifest and verifies the result. |

## The original setup

The user's G HUB key ran `C:\bin\Switch-Audio.exe`, a 27,648-byte exe that ps2exe built from `switch-audio.ps1` with default settings. The file shows those settings:

- PE32, machine x86, CLR header "IL only" (AnyCPU, so it runs as a 64-bit process).
- Console subsystem (3).
- ps2exe's default `asInvoker` manifest.
- STA, and no `.exe.config`.

Each key press took about **900 ms** before the audio moved. The script:

1. Checks `Get-Module -ListAvailable -Name AudioDeviceCmdlets` and installs the module if it is missing.
2. Runs `Import-Module AudioDeviceCmdlets`.
3. Runs `Get-AudioDevice -List | Where-Object { $_.Name -eq ... }` **twice**, once for each device.
4. Gets the current device with `Get-AudioDevice -Playback`.
5. Calls `Set-AudioDevice -ID ...`, then prints a line with `Write-Host`.

## Where the ~900 ms goes

These are single `--timing` runs of `ta.ps1` plus a small in-process Stopwatch probe of the proof-of-concept steps, warm, on the reference machine (7950X, Windows 11 26300, Defender on). They are indicative only. Benchmark numbers belong in the Results table below and in `bench/RESULTS.md`.

| Cost | Approx. on this PC | Notes |
| --- | --- | --- |
| Process creation and hosting the PowerShell runtime, up to the first script statement | powershell.exe ~145-155 ms, pwsh 7 ~205-230 ms, ps2exe exe ~160-185 ms | The `create_to_entry` value (the `entry` phase). It covers CLR start, loading System.Management.Automation, building the default InitialSessionState (built-in cmdlets, providers, type and format data), opening a runspace and parsing the script. See benchmark-method.md §3.6 steps 1-2 and §2.8 (`ps-host-empty`, `pwsh-host-empty`). |
| Script body before any audio work (function definitions, first cmdlet invocations) | ~35-120 ms | This is `entry` → `exit` on the usage path, where the module is never imported. |
| `Get-Module -ListAvailable -Name AudioDeviceCmdlets` | ~25 ms | Scans every directory in PSModulePath and parses module manifests. Only the proof of concept does this; `ta.ps1` does not. |
| `Import-Module AudioDeviceCmdlets` | ~50-130 ms (~10 ms right after a `-ListAvailable` scan) | Module discovery plus loading and JIT-compiling the binary module (`com_init` phase). |
| **`Get-AudioDevice -List`** | **~280-310 ms per call, every call (not just the first)** | The cmdlet enumerates **all** active endpoints, playback and recording. For each endpoint's `FriendlyName`, the module's wrapper opens the property store and walks every property key twice (`PropertyStore.Contains`, then the indexer) to find one value. That costs ~40 ms per endpoint here, against about 1 ms for all names in C. It also calls `GetDefaultAudioEndpoint` several times per endpoint for the Default/DefaultCommunication flags. The proof of concept pays this **twice** (~580 ms). The `list` command of `ta.ps1` pays it once. |
| `Get-AudioDevice -Playback` | ~37 ms | One default lookup, plus `FindIndex`, which enumerates again. |
| `SetDefaultEndpoint` × 3 roles through the module's `PolicyConfigClient` | ~40-45 ms (`enumerator` → `work_done` of `set`) | This is the only part inherent to the job. Native implementations spend 5-12 ms per call, and most of that is the RPC into AudioSrv. |
| Shutdown after the last statement | not captured by the phases | Covers runspace close and CLR shutdown. Compare the hyperfine wall time with the `exit` phase to see it. |

For the proof of concept: ~165 ms (ps2exe host) + ~35 ms (`-ListAvailable` + import) + ~580 ms (two `-List` calls) + ~37 ms (`-Playback`) + the set and `Write-Host` ≈ **850-900 ms**. That matches the observed 900 ms. Only the set calls (tens of ms) are real work. Everything else is host start-up and module overhead, which the native rewrite removes. See `docs/research/benchmark-method.md` §2.8 for the hyperfine rows that isolate the host (`ps-host-empty`, `pwsh-host-empty`) and the cmdlet paths (`ps-list`, `ps-set-noop`), and §3.6 for the step-by-step analysis.

## How `ta.ps1` makes the Core Audio calls

`ta.ps1` sits on top of AudioDeviceCmdlets 3.1.0.2. It uses the cmdlets where they match the contract exactly, and the module's own public interop classes everywhere else. That way its output is byte-identical to `bench/c` (checked for `list` and `get` on all three hosts).

| Step | Call | Why |
| --- | --- | --- |
| COM init | none in the script | All three hosts run the script on an STA thread that is already COM-initialized. The `com_init` phase marks the end of `Import-Module AudioDeviceCmdlets`, the PowerShell equivalent of getting COM ready. |
| Enumerator | `New-Object CoreAudioApi.MMDeviceEnumerator` | This is the module's wrapper around `CoCreateInstance(CLSID_MMDeviceEnumerator)` (`enumerator` phase). |
| `list` rows | `Get-AudioDevice -List`, keeping rows with `Type -eq 'Playback'` | This is the cmdlet path the proof of concept used. Its order (by endpoint id) matches `EnumAudioEndpoints(eRender, ACTIVE)`. |
| `*` / `c` flags, `get`, `toggle` current device | `MMDeviceEnumerator.GetDefaultAudioEndpoint(eRender, eConsole / eCommunications)` | The cmdlets report **eMultimedia** as "Default". The contract uses eConsole. |
| `set` / `toggle` validation | `MMDeviceEnumerator.GetDevice(id)` + `State == DEVICE_STATE_ACTIVE` | `Get-AudioDevice -ID` cannot tell "not found" (`E_NOTFOUND`/`E_INVALIDARG`) from "not active". Both exit 3, with distinct messages. |
| Set | `CoreAudioApi.PolicyConfigClient.SetDefaultEndpoint(id, role)` for eConsole, eMultimedia, eCommunications, in that order | `Set-AudioDevice -ID` sets only **eCommunications then eMultimedia** and never eConsole. The module class throws on a failed HRESULT. Only its `IPolicyConfig` path (`{f8679f50-…}` on `CPolicyConfigClient` `{870af99c-…}`, SetDefaultEndpoint at slot 13) is trustworthy. The constructor's two fallbacks are broken: its `IPolicyConfigVista` declaration has the wrong layout (an extra ResetDeviceFormat puts SetDefaultEndpoint on the not-implemented stub) and can never be QI'd on this CLSID anyway, and its "IPolicyConfig10" is declared with `IID_IUnknown`, so that QI always succeeds and the call goes to a blind vtable slot. `ta.ps1` therefore reads the class's private `_PolicyConfig` field right after construction and exits 2 with `error: QueryInterface(IPolicyConfig) hr=0x80004002` if it is null, so a fallback is never reached. See the Pitfalls checklist in `docs/research/core-audio-api.md`. |

COM lifetime: every COM object is a .NET runtime callable wrapper (RCW). The module's wrappers expose no `Dispose`, so `ta.ps1` drops its references. No deterministic `Release` or `CoUninitialize` happens: the CLR may release RCWs on a later GC, otherwise they are discarded with the process. This deviates from benchmark-method.md §1.3 (see Known limitations). CoTaskMem ids and PROPVARIANTs are freed inside the module.

Error handling: every call sits in its own `try`/`catch`. PowerShell wraps exceptions from .NET methods in `MethodInvocationException`. The script walks the inner exceptions to the `COMException` and prints `error: <step> hr=0x8007xxxx`, using the same step names as the C reference. One exception: `Get-AudioDevice -List` wraps an enumeration failure in a plain `System.Exception`, discarding the HRESULT, so that step reports `hr=0x80131500` (COR_E_EXCEPTION). The exit code is still 2.

### Output encoding

When stdout or stderr is redirected (pipe, file, NUL), as it always is in the benchmarks, the text is encoded with `UTF8Encoding($false)`, which gives no BOM and `\n` line endings. The raw bytes go to `[Console]::OpenStandardOutput()` / `OpenStandardError()`. This bypasses PowerShell's formatter and `[Console]::OutputEncoding`, which is code page 950 on this zh-TW machine: `喇叭` would otherwise come out as Big5 bytes instead of `E5 96 87 E5 8F AD`. The script does **not** set `[Console]::OutputEncoding`, because that calls `SetConsoleOutputCP` and changes the parent console for everyone.

Caveat: on an interactive console (not redirected), the text goes through `[Console]::Out`, so it is encoded in the console's code page. CJK names display correctly in a cp950 console. Characters outside the console code page show as `?`. The benchmarks never use this path.

The script file itself is pure ASCII. Windows PowerShell 5.1 reads BOM-less scripts in the ANSI code page, so any non-ASCII literal would be mis-decoded.

### `--timing`

The first statement of the script takes `Stopwatch.GetTimestamp()` (QueryPerformanceCounter) and `DateTime.UtcNow`. Phases are stamped with the Stopwatch. When the script finishes, `Process.StartTime` (from `GetProcessTimes`) supplies the creation time, and every phase is printed as `phase\t<name>\t<µs since process creation>`, with the same phase names as `bench/c` (`entry com_init enumerator work_done exit`). The lines are buffered and written to stderr in one write after stdout.

Caveat: on .NET Framework (powershell.exe, ps2exe), `DateTime.UtcNow` has system-tick granularity, so the creation→entry offset (and therefore every value) can be off by up to one tick (1-16 ms). Differences between phases are exact. PowerShell 7 (.NET) uses the precise clock.

### PowerShell 7

pwsh 7 can load AudioDeviceCmdlets 3.1.0.2, a .NET Framework 4 binary module, and all commands work. pwsh does **not** search the Windows PowerShell per-user module folder (`Documents\WindowsPowerShell\Modules`), where the module is installed, unless an inherited `PSModulePath` already contains it. A pwsh started from Explorer, G HUB or a clean environment does not see it. So `ta.ps1` first tries `Import-Module AudioDeviceCmdlets` by name and, if that fails, imports it from `[Environment]::GetFolderPath('MyDocuments')\WindowsPowerShell\Modules\AudioDeviceCmdlets`. Under pwsh, measure in a clean environment so the failed lookup is included consistently, or install the module for pwsh with `Install-Module AudioDeviceCmdlets -Scope CurrentUser` from pwsh.

## Building `bin\ta-ps.exe`

Prerequisites:

- Windows PowerShell 5.1 (part of Windows).
- PowerShell 7 to run `build.ps1`.
- ps2exe **1.0.17** (`Install-Module ps2exe -Scope CurrentUser` in Windows PowerShell).
- A Windows 10/11 SDK for `mt.exe` (found automatically under `Windows Kits\10\bin\<newest>\x64`).
- AudioDeviceCmdlets 3.1.0.2 to run the result (`Install-Module AudioDeviceCmdlets -Scope CurrentUser`).

```powershell
pwsh -NoProfile -File bench\powershell\build.ps1
```

The build is non-interactive and idempotent: it deletes and rebuilds the two exes in `bin\` every time. It prints a verification table and the size of `bin\ta-ps.exe`.

| Output | ps2exe flags | Post-processing | Why |
| --- | --- | --- | --- |
| `bin\ta-ps.exe` (primary, ~48 KB) | `-x64 -noConsole:$false` | `mt.exe -manifest ..\common\detached.manifest -outputresource:ta-ps.exe;#1` | Bench contract: 64-bit, console subsystem (`-noConsole:$false`, so `/target:exe`), and the `consoleAllocationPolicy=detached` manifest (DESIGN.md §12), so a G HUB launch on Windows 11 24H2+ creates no console. ps2exe has no option for a custom manifest, so `mt.exe` replaces the default manifest that csc embeds. |
| `bin\ta-ps-anycpu.exe` | none (defaults) | none | Reproduces the user's original packaging flag for flag (PE32 AnyCPU, console, STA, default manifest, no `.config`), so the cost of ps2exe as originally used can be compared with the bench build. |

Flags that both builds leave at the ps2exe defaults, on purpose:

- **STA** (`[STAThread]` on the generated `Main`). It is ps2exe's default and the original's setting, and the apartment Core Audio callers normally use.
- **No `-configFile`**, as in the original.
- **No `-UNICODEEncoding`**. That flag sets `Console.OutputEncoding`, which would change the console code page. `ta.ps1` writes raw UTF-8 bytes instead.
- **No `-conHost`**, which would force a conhost window.

Which PowerShell the exe embeds: ps2exe always compiles in Windows PowerShell, even when called from pwsh, because it relaunches `powershell.exe`. The exe targets the .NET Framework 4 CLR (v4.0.30319) and references `System.Management.Automation 3.0.0.0`. At run time that resolves from the GAC to the **Windows PowerShell 5.1** engine (`GAC_MSIL\System.Management.Automation\v4.0_3.0.0.0__31bf3856ad364e35`, file version 10.0.26100.9549 on this PC). It never hosts PowerShell 7. `build.ps1` reads this reference from the built exe's metadata and prints it.

Verified on the reference machine:

- `dumpbin /headers`: `ta-ps.exe` is machine 8664 (x64) and `ta-ps-anycpu.exe` is 14C, both with subsystem 3 (Windows CUI).
- `dumpbin /clrheader`: IL only.
- `mt.exe -inputresource:bin\ta-ps.exe;#1`: the extracted manifest contains `consoleAllocationPolicy` `detached`.

## Running the baselines

```powershell
cd bench\powershell
# Windows PowerShell 5.1
powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File ta.ps1 list
# PowerShell 7
pwsh -NoProfile -NonInteractive -File ta.ps1 get --timing
# ps2exe package (Windows PowerShell 5.1 engine)
bin\ta-ps.exe set "{0.0.0.00000000}.{739b3554-bfed-4d61-b407-a818b317c991}"
```

Commands: `list`, `get`, `set <id>`, `toggle <idA> <idB>`, no arguments (usage, exit 1, module never imported), and `--timing` anywhere. Exit codes: 0 OK, 1 usage, 2 COM/module failure, 3 device not found or not active, 4 no default device.

hyperfine (`-N`, forward slashes, see benchmark-method.md §2.1):

```
hyperfine -N --warmup 3 --runs 30 ^
  --command-name ps-ta-list   "powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File I:/Projects/toggle-audio/bench/powershell/ta.ps1 list" ^
  --command-name pwsh-ta-list "pwsh.exe -NoProfile -NonInteractive -File I:/Projects/toggle-audio/bench/powershell/ta.ps1 list" ^
  --command-name ps2exe-list  "I:/Projects/toggle-audio/bench/powershell/bin/ta-ps.exe list"
```

`pwsh.exe` resolves through the App Execution Alias in `%LOCALAPPDATA%\Microsoft\WindowsApps` (measured: same time as the full path). To call the package directly instead, quote the program path *inside* the command string, because `hyperfine -N` splits the string with shell-words rules and would cut an unquoted path at the space in `Program Files`:

```
--command-name pwsh-ta-list "'C:/Program Files/WindowsApps/Microsoft.PowerShell_7.6.6.0_x64__8wekyb3d8bbwe/pwsh.exe' -NoProfile -NonInteractive -File I:/Projects/toggle-audio/bench/powershell/ta.ps1 list"
```

That path contains the Store package version and changes with every pwsh update. Look it up with `(Get-AppxPackage Microsoft.PowerShell).InstallLocation`. (`(Get-Command pwsh).Source` returns the alias, not the package path.)

Notes for measuring:

- **ps2exe rewrites arguments.** The generated host turns anything that looks like a PowerShell parameter into a named parameter, so `--timing` reaches the script as `-timing`. That is followed by the next word or, at the end of the line, by an extra `[bool] $true`. `ta.ps1` handles both cases, so the contract holds for `bin\ta-ps.exe`. It accepts the single-dash `-timing` (and drops the trailing `[bool]`) only under the ps2exe host (`$Host.Name` is `PSRunspace-Host`). Under `powershell.exe -File` and `pwsh -File` only the exact `--timing` counts, and `-timing` is a positional argument (usage, exit 1), as in every other implementation. Ids (`{...}.{...}`) pass through unchanged. ps2exe also reserves `-wait`, `-extract:<file>`, `-end`, `-?` and `-debug`.
- **ps2exe reads stdin when it is redirected.** It reads until EOF before it runs the script. hyperfine's default null stdin is fine. An open pipe that never closes would hang the exe.
- **Real toggles.** `toggle` and `set` to another id change the default device. Follow the scenario (c) rules in benchmark-method.md §2.7, and use the original `C:\bin\Switch-Audio.exe` row only under those rules: it toggles S/PDIF ↔ PG42UQ by name.

## Results

Placeholder for the measuring agent (hyperfine `-N`, warm; ms; see `bench/RESULTS.md` for the full tables).

| Row | Command | Mean ± σ | Median | Min | Notes |
| --- | --- | --- | --- | --- | --- |
| ps-host-empty | `powershell.exe -NoLogo -NoProfile -NonInteractive -Command exit` | | | | |
| pwsh-host-empty | `pwsh.exe -NoLogo -NoProfile -NonInteractive -Command exit` | | | | |
| ps-ta-usage | `powershell.exe ... -File ta.ps1` (exit 1) | | | | |
| ps-ta-list | `powershell.exe ... -File ta.ps1 list` | | | | |
| ps-ta-set-noop | `powershell.exe ... -File ta.ps1 set <PG42UQ>` | | | | |
| pwsh-ta-list | `pwsh.exe ... -File ta.ps1 list` | | | | |
| pwsh-ta-set-noop | `pwsh.exe ... -File ta.ps1 set <PG42UQ>` | | | | |
| ps2exe-usage | `bin\ta-ps.exe` (exit 1) | | | | |
| ps2exe-list | `bin\ta-ps.exe list` | | | | |
| ps2exe-set-noop | `bin\ta-ps.exe set <PG42UQ>` | | | | |
| ps2exe-anycpu-set-noop | `bin\ta-ps-anycpu.exe set <PG42UQ>` | | | | |
| ps2exe-toggle | `bin\ta-ps.exe toggle <PG42UQ> <PHL BDM4065>` | | | | scenario (c) rules |
| original | `C:\bin\Switch-Audio.exe` | | | | toggles S/PDIF ↔ PG42UQ; scenario (c) rules |

Phase medians (`--timing`, µs since process creation):

| Host | entry | com_init | enumerator | work_done | exit |
| --- | --- | --- | --- | --- | --- |
| powershell.exe | | | | | |
| pwsh | | | | | |
| bin\ta-ps.exe | | | | | |

## Known limitations

- These are baselines, not products: start-up is dominated by hosting a PowerShell runtime (hundreds of ms) and cannot be optimized away from inside the script.
- `ta.ps1` mixes cmdlets (`Get-AudioDevice -List`) with the module's interop classes so the output matches the contract. A pure-cmdlet version (the proof of concept) would use eMultimedia for "default" and would never set eConsole.
- The `toggle` path and the not-found/not-active exits (3) were not exercised during development, because the toggle and foreign-id `set` commands were off-limits on this machine. They share the validation and set code that `set <current id>` exercises. The error mapping (`E_NOTFOUND` → exit 3, `DEVICE_STATE_NOTPRESENT` → exit 3) was checked separately against the module's `GetDevice` behaviour.
- **Deviation from benchmark-method.md §1.3** ("All implementations must release their objects and call CoUninitialize"): `ta.ps1` drops its references and never calls `Release` or `CoUninitialize`, because the module's wrappers offer no `Dispose`. The CLR may release RCWs on a later GC, otherwise the in-process objects are discarded with the process (.NET Framework does not finalize reachable objects at shutdown, and .NET never runs finalizers on exit). The native rows pay for Release and CoUninitialize; the PowerShell rows do not. That cost is microseconds against hundreds of milliseconds of host start-up, so it does not change any conclusion.
- Error fidelity: when `Get-AudioDevice -List` fails to enumerate, the cmdlet wraps the error in a plain `System.Exception`, so `ta.ps1` prints `hr=0x80131500` (COR_E_EXCEPTION) instead of the real HRESULT. The exit code (2) is still correct.
- An endpoint without `PKEY_Device_FriendlyName` is printed with the name `Unknown` (the module's substitute, in both `list` and `get`), where `bench/c` prints an empty field. Mapping it back would cost another walk of the property store per endpoint, so it is documented rather than fixed.
- `toggle` and `set` rely only on the module's `IPolicyConfig` path. If a future Windows stopped answering that QI, `ta.ps1` exits 2 (`QueryInterface(IPolicyConfig) hr=0x80004002`) instead of using the module's broken fallbacks.
- Under Windows PowerShell and ps2exe, the absolute `--timing` values can be off by up to one system tick (see above).
- `bin\ta-ps.exe` requires the AudioDeviceCmdlets module on the machine. ps2exe embeds only the script, not the module.
