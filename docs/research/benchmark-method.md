# Benchmark method and fast-startup playbook

Who reads this: five implementer agents (C, Rust, C# NativeAOT, Go, Zig), who each build a minimal "set default audio device" tool, and one measuring agent, who benchmarks those tools and the PowerShell baselines.
Machine: Windows 11 Pro for Workstations 10.0.26300 (zh-TW locale), AMD Ryzen 9 7950X, not elevated, Defender real-time protection ON (AMProductVersion 4.18.26080.4).

Root: `S = C:\Users\Hotdogee\AppData\Local\Temp\claude\I--Projects-switch-audio\c5969902-99b1-4723-bb94-f401b9680340\scratchpad`
- Sources: `S\bench\<lang>\` (c, rust, cs, go, zig, harness)
- Each `S\bench\<lang>\` folder has a `build.cmd` that rebuilds from scratch without asking for input.
- Built binaries are copied to `S\bench\bin\` (see 1.6 for names).
- Results go to `S\bench\results\` (`*.json`, `*.md`, `RESULTS.md`)
- Do not create anything under `I:\Projects`. Language toolchains that are not installed (Go, Zig, hyperfine) go in `S\tools\` as portable zips, because the MSI/Program Files installers need admin, which we do not have.

---

## 0. COM facts every implementation needs (verified on this PC)

| Item | Value |
|---|---|
| CLSID_MMDeviceEnumerator | `{BCDE0395-E52F-467C-8E3D-C4579291692E}`. In-proc server `%SystemRoot%\System32\MMDevApi.dll`, ThreadingModel `both` (checked in the registry on this PC) |
| IID_IMMDeviceEnumerator | `{A95664D2-9614-4F35-A746-DE8DB63617E6}`. vtable after IUnknown(0-2): EnumAudioEndpoints=3, GetDefaultAudioEndpoint=4, GetDevice=5, RegisterEndpointNotificationCallback=6, Unregister...=7 |
| IMMDeviceCollection | GetCount=3, Item=4 |
| IMMDevice | Activate=3, OpenPropertyStore=4, GetId=5, GetState=6 |
| IPropertyStore | GetCount=3, GetAt=4, GetValue=5, SetValue=6, Commit=7 |
| PKEY_Device_FriendlyName | fmtid `{A45C254E-DF1C-4EFD-8020-67D146A850E0}`, pid 14 (VT_LPWSTR; `pwszVal` is at byte offset 8 of the 16-byte x64 PROPVARIANT; free it with `PropVariantClear` from ole32) |
| CLSID_PolicyConfigClient (undocumented) | `{870AF99C-171D-4F9E-AF0D-E63DF40C2BC9}`. In-proc server `C:\Windows\System32\AudioSes.dll`, ThreadingModel `Both` (checked in the registry on this PC) |
| IID_IPolicyConfig (Win7+, undocumented) | `{F8679F50-850A-41CF-9C72-430F290290C8}`. vtable: GetMixFormat=3, GetDeviceFormat=4, ResetDeviceFormat=5, SetDeviceFormat=6, GetProcessingPeriod=7, SetProcessingPeriod=8, GetShareMode=9, SetShareMode=10, GetPropertyValue=11, SetPropertyValue=12, **SetDefaultEndpoint=13** `HRESULT(PCWSTR deviceId, ERole role)`, SetEndpointVisibility=14 |
| Constants | eRender=0; eConsole=0, eMultimedia=1, eCommunications=2; DEVICE_STATE_ACTIVE=1; STGM_READ=0; CLSCTX_INPROC_SERVER=1 (CLSCTX_ALL=0x17 also works) |
| Endpoint id format | `{0.0.0.00000000}.{guid}`, e.g. PG42UQ = `{0.0.0.00000000}.{739b3554-bfed-4d61-b407-a818b317c991}` |

References: IMMDeviceEnumerator https://learn.microsoft.com/windows/win32/api/mmdeviceapi/nn-mmdeviceapi-immdeviceenumerator ; PKEY_Device_FriendlyName https://learn.microsoft.com/windows/win32/coreaudio/pkey-device-friendlyname ; IPolicyConfig as used by SoundSwitch / AudioDeviceCmdlets / EarTrumpet (https://github.com/Belphemur/SoundSwitch , https://github.com/frgnca/AudioDeviceCmdlets , https://github.com/File-New-Project/EarTrumpet `Interop/IPolicyConfig.cs`).

Apartment: call `CoInitializeEx(NULL, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE)` on the thread that makes all the COM calls. Both servers are `Both`, so MTA also works. Record which apartment you used.
- Go: the COM work must run on a locked OS thread (`runtime.LockOSThread()` in `init()`).
- C#: put `[STAThread]` on `Main`. If `CoInitializeEx` returns `RPC_E_CHANGED_MODE` (0x80010106), treat it as success: the runtime already initialized COM.

---

## 1. Common CLI contract (all bench implementations)

One harness drives every implementation, so they must behave identically.

### 1.1 Commands

| Command | Behaviour | stdout | Exit |
|---|---|---|---|
| `<exe> list` | Enumerate `eRender` + `DEVICE_STATE_ACTIVE`. For each endpoint, in enumeration order, print `<id>\t<friendly name>\t<marker>\n`. `<marker>` is `*` if the id equals the current default for (eRender, eConsole), otherwise `-` | the lines | 0 |
| `<exe> get` | `GetDefaultAudioEndpoint(eRender, eConsole)` | `<id>\n` | 0, or 4 if there is no default |
| `<exe> set <id>` | Call `IMMDeviceEnumerator::GetDevice(id)` and check `GetState == ACTIVE` (exit 3 if not). Then call `IPolicyConfig::SetDefaultEndpoint(id, r)` for r = eConsole, eMultimedia, eCommunications, in that order. Do this even when id is already the default; the "set-noop" scenario measures exactly that | nothing | 0 |
| `<exe> toggle <idA> <idB>` | The product's hot path: get the eConsole default. If it equals idA, target = idB, otherwise target = idA. Then run the same validate + 3-role set as `set` | `<target id>\n` | 0 |
| no arguments / unknown | usage message on stderr, **no COM work** (this is the "runtime-only" scenario) | - | 1 |

Exit codes:
- 0: OK
- 1: usage
- 2: COM/Win32 failure. Print `error: <step> hr=0x%08X` on stderr.
- 3: device not found or not active
- 4: no default device

Global flag `--timing`, accepted anywhere in argv: write phase timestamps to **stderr** (see 1.3). stdout must stay byte-identical with or without `--timing`.

### 1.2 Output encoding and format
- Redirected stdout (pipe, file, NUL): **UTF-8 bytes, no BOM, `\n` line endings, no trailing spaces**. On this zh-TW machine the console code page is 950, so anything that depends on the ANSI or console code page is a bug. Check: `喇叭` must come out as bytes `E5 96 87 E5 8F AD`.
- stdout attached to a real console (`GetConsoleMode` succeeds): use `WriteConsoleW` with UTF-16 so the CJK text displays correctly. This is a nice-to-have; the benchmarks always redirect.
- Rust std and Go `os.Stdout` already behave this way. In C, use `WideCharToMultiByte(CP_UTF8)` + `WriteFile`. In C#, do not use `Console.Out`: its encoding comes from `GetConsoleOutputCP` and would be cp950. Write `new UTF8Encoding(false).GetBytes(...)` to `Console.OpenStandardOutput()`. Do not set `Console.OutputEncoding`: that calls `SetConsoleOutputCP`, which changes the parent console and costs time.
- Command-line parsing: parse `GetCommandLineW()` (or the language runtime's argv). **Do not use `CommandLineToArgvW`**: it pulls in shell32.dll and adds a DLL load. A 20-line whitespace/quote splitter is enough; ids contain no spaces.
- Correctness gate before any timing: run `list` and `get` for every implementation, redirect to files, and byte-compare against the C reference (`fc /b`). Any mismatch disqualifies that implementation until it is fixed.

### 1.3 `--timing` format
Each line is `timing\t<phase>\t<microseconds since entry, 1 decimal>`, written to stderr.
- "Entry" = first statement of `main` (or of the custom entry point), timestamped with `QueryPerformanceCounter`.
- One extra line, `timing\tcreate_to_entry\t<us>`, holds `GetSystemTimePreciseAsFileTime()` at entry minus the creation time from `GetProcessTimes(GetCurrentProcess())`, converted from 100 ns units to µs.
  - This captures loader + runtime init (CRT, Go runtime, .NET AOT runtime). It is the only way to see language-runtime startup from inside the process.
  - The creation timestamp may be coarse. If the values are negative or cluster on multiples of ~15.6 ms, mark `create_to_entry` as unreliable.

Phases. Print the ones that apply to the command, in order:

| Phase | Taken when |
|---|---|
| `entry` | 0 |
| `com_init` | after CoInitializeEx |
| `enumerator` | after CoCreateInstance(MMDeviceEnumerator) |
| `default_got` | after GetDefaultAudioEndpoint + GetId (list/get/toggle) |
| `enumerated` | after EnumAudioEndpoints + GetCount (list) |
| `names_read` | after all ids, names and outputs are formatted (list) |
| `device_validated` | after GetDevice + GetState (set/toggle) |
| `policy_created` | after CoCreateInstance(PolicyConfigClient) |
| `set_console` / `set_multimedia` / `set_communications` | after each SetDefaultEndpoint |
| `released` | after Release of all objects + CoUninitialize |
| `exit` | just before returning, or before ExitProcess |

Buffer the timing lines and write them in one call at the end, so the I/O does not distort the phases. All implementations must release their objects and call CoUninitialize, so the comparison is fair. An optional `fast-exit` variant (skip cleanup, call ExitProcess) may be reported as an extra row.

### 1.4 No console window flash
G HUB launches the exe from a GUI process. If the exe is a **console-subsystem** exe, Windows creates a new console for it. On Win11 that can be Windows Terminal if it is the default terminal, which means a visible window and +10-100 ms. **Every bench binary is therefore built for the WINDOWS (GUI) subsystem.**
- A GUI-subsystem exe still writes to inherited std handles when the parent redirects them. hyperfine and the spawnbench launcher do redirect; pwsh does too when piped (`ta-c.exe list | Out-String`).
- With no redirection, the output is simply lost. That is acceptable for the bench. The product's GUI does not need it, and its CLI can call `AttachConsole(ATTACH_PARENT_PROCESS)` lazily.

| Language | How |
|---|---|
| C (MSVC) | `/link /SUBSYSTEM:WINDOWS /ENTRY:mainCRTStartup` (keeps a normal `int main(void)`). With no CRT: `/ENTRY:entry` |
| Zig cc | `-Wl,--subsystem,windows` (or `-mwindows`). Verify with dumpbin |
| Rust | `#![windows_subsystem = "windows"]` at the top of main.rs. std writes to an invalid/null handle are silently ignored |
| Go | `-ldflags "-s -w -H windowsgui"` |
| C# | `<OutputType>WinExe</OutputType>` |

Every implementer verifies the result with `dumpbin /headers <exe> | findstr /i subsystem`, which must show `2 subsystem (Windows GUI)`.
For process-start comparison, the C implementer also builds console-subsystem copies `ta-c-con.exe` and `nop-con.exe`.

### 1.5 Baseline "empty process"
The C implementer also builds `nop.exe` (GUI) and `nop-con.exe` (console). Each is no-CRT, its entry is just `ExitProcess(0)`, and it imports kernel32 only. This gives the process-creation floor that every number is compared against, and it measures harness overhead.

### 1.6 Binary names (in `S\bench\bin\`)
- `ta-c.exe` (MSVC /MT, GUI)
- `ta-c-md.exe` (/MD)
- `ta-c-nocrt.exe`
- `ta-c-con.exe`
- `nop.exe`, `nop-con.exe`
- `ta-rust.exe` (`windows` crate)
- `ta-rust-sys.exe` (optional: windows-sys + hand vtables)
- `ta-cs.exe` (NativeAOT)
- `ta-go.exe`
- `ta-zig.exe` (`zig cc` of the C source)
- `ta-zig-native.exe` (optional)

Never rebuild or touch a binary between measurement rounds. A new hash triggers a fresh Defender scan.

---

## 2. Measurement methodology

### 2.1 Primary tool: hyperfine (no shell)

Install without admin. Use either of:
- `winget install --id sharkdp.hyperfine -e --scope user --accept-source-agreements --accept-package-agreements`. This is a portable package; the shim lands in `%LOCALAPPDATA%\Microsoft\WinGet\Links`.
- The release zip from https://github.com/sharkdp/hyperfine/releases (v1.20.0 at the time of writing), unzipped to `S\tools\hyperfine`.
- Do not use `cargo install`: hyperfine now needs a newer Rust than 1.72, so rustup would have to be updated first.

Canonical invocation for one scenario:
```
hyperfine -N --warmup 10 --runs 200 --output=null --time-unit millisecond ^
  --export-json results\list-r1.json --export-markdown results\list-r1.md ^
  --command-name nop  "C:/.../bench/bin/nop.exe" ^
  --command-name c    "C:/.../bench/bin/ta-c.exe list" ^
  --command-name rust "C:/.../bench/bin/ta-rust.exe list"  ...
```
- `-N` / `--shell=none`: start the process directly. With the default `cmd.exe` shell, a ~5-15 ms shell spawn is added and then subtracted by an estimate, and that estimate is noisy at the 1 ms scale. Docs: https://github.com/sharkdp/hyperfine
- `-N` splits the command with POSIX shell-word rules, so **backslash is an escape character**. Use forward slashes (`C:/Users/...`) or single-quote the whole path.
- `{...}` is hyperfine's parameter syntax. Endpoint ids contain braces, so do not combine id arguments with `-P`/`-L`. Use plain commands.
- `--warmup 10` is mandatory. The first run of a new exe triggers a Defender scan and possibly a cloud lookup ("block at first sight"), which can take 100 ms to several seconds. Warm-up also brings the image into the file cache and the prefetcher.
- `--output=null` (the default) redirects child stdout/stderr to NUL. The children are GUI-subsystem, so no console attaches. Measure the console variants (`*-con.exe`) separately and say so in the report: launched from a console parent they inherit the console (cheap), but launched from G HUB they would create one (expensive, see 2.2).
- Limitations on Windows:
  - Wall time is measured with `Instant` (QueryPerformanceCounter, sub-µs). It is reliable.
  - The "User"/"System" columns come from `GetProcessTimes`, which is effectively 15.6 ms-granular for short processes. **Ignore them.**
  - hyperfine's own `CreateProcessW` + `WaitForSingleObject` overhead (~0.1-0.3 ms) is included equally for every command. Subtracting `nop.exe` gives "time attributable to the program".
  - Outliers come from Defender scans, the indexer, the audio service and the CPU boost/park state. hyperfine warns about statistical outliers. Re-run if the warning appears for native implementations; report median and min alongside mean.
- Interleaving: hyperfine runs commands one after another, not interleaved. Repeat each scenario **3 rounds (forward order, reverse order, forward order)**, then pool the `times` arrays from the three JSON files. The headline is mean ± σ; also report median and min.
- The runtime-only scenario exits 1 on purpose, so pass `-i` / `--ignore-failure` for it.

### 2.2 Precise alternative: `spawnbench.exe` (C, ~60 lines, built by the measuring agent in `S\bench\harness\`)
This launcher removes hyperfine as a variable and can emulate how G HUB launches a process (`CREATE_NEW_CONSOLE` for console exes). Build it with `cl /nologo /O2 /W4 spawnbench.c`.
```c
#define UNICODE
#define _UNICODE
#include <windows.h>
#include <stdio.h>
#include <stdlib.h>
#include <math.h>
static int cmpd(const void*a,const void*b){double x=*(const double*)a,y=*(const double*)b;return (x>y)-(x<y);}
int wmain(int argc, wchar_t **argv){
  if(argc<5){fwprintf(stderr,L"usage: spawnbench <runs> <warmup> <inherit|newconsole|nowindow|detached> \"<cmdline>\"\n");return 1;}
  int runs=_wtoi(argv[1]), warm=_wtoi(argv[2]); DWORD cf=0;
  if(!wcscmp(argv[3],L"newconsole")) cf=CREATE_NEW_CONSOLE;
  else if(!wcscmp(argv[3],L"nowindow")) cf=CREATE_NO_WINDOW;
  else if(!wcscmp(argv[3],L"detached")) cf=DETACHED_PROCESS;
  SECURITY_ATTRIBUTES sa={sizeof sa,NULL,TRUE};
  HANDLE nul=CreateFileW(L"NUL",GENERIC_WRITE,FILE_SHARE_WRITE,&sa,OPEN_EXISTING,0,NULL);
  LARGE_INTEGER f,t0,t1; QueryPerformanceFrequency(&f);
  double *ms=(double*)malloc(sizeof(double)*runs); int fails=0;
  for(int i=-warm;i<runs;i++){
    wchar_t cmd[2048]; wcsncpy_s(cmd,2048,argv[4],_TRUNCATE);   /* CreateProcessW may modify the buffer */
    STARTUPINFOW si={sizeof si}; PROCESS_INFORMATION pi;
    si.dwFlags=STARTF_USESTDHANDLES; si.hStdInput=NULL; si.hStdOutput=nul; si.hStdError=nul;
    QueryPerformanceCounter(&t0);
    if(!CreateProcessW(NULL,cmd,NULL,NULL,TRUE,cf,NULL,NULL,&si,&pi)){fwprintf(stderr,L"CreateProcess %lu\n",GetLastError());return 2;}
    WaitForSingleObject(pi.hProcess,INFINITE);
    QueryPerformanceCounter(&t1);
    DWORD ec=0; GetExitCodeProcess(pi.hProcess,&ec); if(ec) fails++;
    CloseHandle(pi.hThread); CloseHandle(pi.hProcess);
    if(i>=0) ms[i]=(double)(t1.QuadPart-t0.QuadPart)*1000.0/(double)f.QuadPart;
  }
  double s=0,ss=0; for(int i=0;i<runs;i++) s+=ms[i]; double m=s/runs;
  for(int i=0;i<runs;i++) ss+=(ms[i]-m)*(ms[i]-m);
  qsort(ms,runs,sizeof(double),cmpd);
  wprintf(L"mean_ms\tstd_ms\tmedian_ms\tmin_ms\tp95_ms\tmax_ms\tfailures\n%.3f\t%.3f\t%.3f\t%.3f\t%.3f\t%.3f\t%d\n",
    m, runs>1?sqrt(ss/(runs-1)):0.0, ms[runs/2], ms[0], ms[(int)(runs*0.95)], ms[runs-1], fails);
  return 0;
}
```
- Mode `inherit` matches the hyperfine numbers.
- Mode `newconsole` on a `*-con.exe` shows what G HUB would pay for a console-subsystem tool (conhost/OpenConsole startup). Use it **only for the `nop-con.exe` and `ta-c-con.exe` rows**. If Windows Terminal is the default terminal, each run may open a WT tab or window, so run at most 5 iterations and close what opens. If the cost is obviously large, report it and skip the rest.
- The `failures` column counts nonzero exits. For the runtime-only scenario it should equal `runs`.

### 2.3 PowerShell Stopwatch loop (fallback; not for sub-10 ms comparisons)
```powershell
$sw=[Diagnostics.Stopwatch]::new(); $t=foreach($i in 1..50){ $sw.Restart(); & $exe list > $null; $sw.Elapsed.TotalMilliseconds }
```
- pwsh's native-command invocation adds ~2-6 ms with ±1-2 ms jitter.
- `Start-Process -Wait` is worse: ~15-40 ms of overhead, because it goes through ShellExecuteEx and job waiting.
- `[Diagnostics.Process]::Start()` + `WaitForExit()` sits in between.
- If you use this, calibrate by timing `nop.exe` the same way and subtract. Use it only to sanity-check hyperfine or for the slow PowerShell baselines.

### 2.4 ETW / WPR (optional, not available here)
Kernel process and image-load tracing needs admin: `wpr -start CPU`, xperf, and Process Monitor all do. The session is not elevated, so **skip it** and note it in RESULTS.md. The in-process `--timing` phases plus the `nop.exe` baseline cover the same breakdown well enough.

### 2.5 Separating the costs
- Process creation floor = `nop.exe` time.
- Loader + language runtime init = (implementation's runtime-only time, i.e. no args, exit 1) minus nop. Cross-check with `create_to_entry` from `--timing`.
- COM cost = `com_init` + `enumerator` deltas. This is where combase, rpcrt4 and MMDevApi load.
- Enumeration and property cost = `enumerated` → `names_read`.
- SetDefaultEndpoint cost = `policy_created` → `set_communications` (an RPC into AudioSrv per role). Report the per-role numbers.
- How to collect: run each exe 30× with `--timing` from a small script (stderr to file) after the hyperfine rounds, and report the median of each phase delta.

### 2.6 Hygiene checklist (record it in RESULTS.md)
- AC power. Record the power plan (`powercfg /getactivescheme`, from cmd or pwsh, not Git Bash).
- Close heavy apps and do not watch video. Note whether audio was playing: an active stream makes endpoint switches costlier.
- Defender: real-time protection is ON. Exclusions need admin (`Add-MpPreference`), so they cannot be added; that is realistic for the end user anyway. Also record the Smart App Control state.
- Keep every exe in `S\bench\bin` (the same folder for all), never in Downloads.
- Report the **first-ever run** of each exe separately ("cold") by copying it to a fresh name and running it once: `copy ta-c.exe ta-c-cold1.exe`, then time one launch. That is what the user sees once after install.
- Use the same, freshly re-read device ids for all implementations. Verify the default device before and after each scenario.

### 2.7 Benchmark matrix

| # | Scenario | Command | Warm-up / runs × rounds | Side effect |
|---|---|---|---|---|
| 0 | process floor | `nop.exe`, `nop-con.exe` | 10 / 200 × 3 | none |
| R | runtime-only | `<exe>` (no args, exit 1, use `-i`) | 10 / 200 × 3 | none |
| a | list | `<exe> list` | 10 / 200 × 3 | none |
| b | set-noop | `<exe> set <current default id>` (PG42UQ) | 10 / 200 × 3 | none (already the default; still does 3 RPCs) |
| c | real toggle | `<exe> toggle <PG42UQ id> <PHL BDM4065 id>` | 2 / 20 × 1 (even counts) | **changes the default device** |
| P | PowerShell baselines | see 2.8 | 2 / 20 × 1 | (c) rules apply to the toggling rows |

Scenario (c):
- Only run it if the orchestrator has authorized changing the default device.
- Use the two NVIDIA HDMI endpoints (PG42UQ `{...739b3554...}`, PHL BDM4065 `{...5b124733...}`). Do not use the Bluetooth BTA30: BT endpoints can be disconnected, and A2DP reconfiguration adds 100s of ms that have nothing to do with the tool. Bluetooth timing can be an optional, separately labelled row.
- Even run counts plus `--conclude "C:/.../bin/ta-c.exe set {0.0.0.00000000}.{739b3554-bfed-4d61-b407-a818b317c991}"` guarantee that PG42UQ ends as the default. The braces are fine because no `-P`/`-L` is used.
- Afterwards, verify with `ta-c.exe get`.
- Successive toggles come back-to-back. That is acceptable, but note it: AudioSrv notifications from the previous switch may still be in flight.

### 2.8 PowerShell baselines (show where ~900 ms goes)
AudioDeviceCmdlets 3.1.0.2 is installed at `C:\Users\Hotdogee\Documents\WindowsPowerShell\Modules`, which both powershell.exe and pwsh can see.

| Row | Command (hyperfine -N) |
|---|---|
| ps-host-empty | `powershell.exe -NoLogo -NoProfile -NonInteractive -Command exit` |
| pwsh-host-empty | `pwsh.exe -NoLogo -NoProfile -NonInteractive -Command exit` |
| ps-list | `powershell.exe -NoProfile -NonInteractive -Command "Import-Module AudioDeviceCmdlets; Get-AudioDevice -List \| Out-Null"` |
| pwsh-list | the same command with pwsh.exe |
| ps-set-noop | `powershell.exe -NoProfile -NonInteractive -Command "Import-Module AudioDeviceCmdlets; Set-AudioDevice -ID '<PG42UQ id>' \| Out-Null"` |
| ps-script | `powershell.exe -NoProfile -ExecutionPolicy Bypass -File I:/Projects/switch-audio/switch-audio.ps1`. This **toggles**, so only run it under (c) rules. Note that it toggles **S/PDIF ↔ PG42UQ**; restore PG42UQ afterwards |
| ps2exe | `C:/bin/Switch-Audio.exe`. This also toggles S/PDIF ↔ PG42UQ, so the (c) rules apply: even runs, then restore |

Notes:
- The quoting inside hyperfine `-N` uses shell-words. Test each line once by hand first.
- If shell-words quoting becomes painful, put the PowerShell lines in `S\bench\harness\*.ps1` files and call them with `-File`. File-based invocation adds no measurable overhead.

---

## 3. Per-language fast-startup and size playbook

General rules, the same for all languages:
- Static linking of runtime pieces wherever possible. **The product must not depend on the VC++ redistributable** (`vcruntime140.dll` is not part of Windows; `ucrtbase.dll` is).
- No reflection, no shell32, no user32 unless needed, no global constructors.
- Only ole32/combase + kernel32 (+ oleaut32 if unavoidable).
- Check imports with `dumpbin /dependents <exe>`. Fewer DLLs means fewer loader mappings: each non-KnownDLL costs roughly 50-300 µs.

### 3.1 C (MSVC 14.44), the speed reference
- Source: plain C11 with `#define COBJMACROS`, `UNICODE`, `_UNICODE`, `WIN32_LEAN_AND_MEAN`.
  - Define all GUIDs as `static const GUID` locally rather than relying on `initguid.h` tricks.
  - Use the `IMMDeviceEnumerator_GetDevice(p, ...)` macros. Define IPolicyConfig yourself as a struct with a vtable of 15 function pointers.
  - Entry `int main(void)`, with args parsed from `GetCommandLineW()`. That keeps one source buildable by MSVC and `zig cc`.
- Environment: `call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat"` inside `build.cmd`.
- `ta-c.exe` (primary):
  `cl /nologo /O2 /GL /Gy /Gw /GS- /MT /W4 /DUNICODE /D_UNICODE ta.c /link /LTCG /OPT:REF /OPT:ICF /SUBSYSTEM:WINDOWS /ENTRY:mainCRTStartup ole32.lib`
  - /O2 is speed. /GL + /LTCG is whole-program optimization. /Gy + /Gw put functions and globals in COMDATs, so /OPT:REF,ICF can strip and fold them.
  - /GS- skips the stack cookie; it is negligible, but it is needed for no-CRT anyway.
  - /MT links the CRT statically: no vcruntime140 dependency, ~100-130 KB.
- `ta-c-md.exe`: the same with `/MD`. That is ~10 KB, but it needs vcruntime140.dll, an extra DLL load. Expect +0.2-1 ms; measure it.
- `ta-c-con.exe`: `/SUBSYSTEM:CONSOLE`, for console-attach comparison only.
- `ta-c-nocrt.exe`: same source, `#ifdef NOCRT`.
  - Entry `int WINAPI entry(void){ ExitProcess(run()); }`.
  - Compile with `/GS- /Oi /Zl`; link with `/NODEFAULTLIB /ENTRY:entry /SUBSYSTEM:WINDOWS kernel32.lib ole32.lib`.
  - Provide `memset`/`memcpy`: `#pragma function(memset)` with a simple loop. Keep stack buffers under 4 KB to avoid `__chkstk`.
  - Do any UTF-8 conversion with `WideCharToMultiByte` and decimal formatting by hand (no printf). Expected size: 3-8 KB.
  - Expected gain over /MT: static CRT init (heap, environment, locale, atexit tables) is roughly 50-300 µs. That is probably inside run-to-run noise (σ ≈ 0.2-0.5 ms).
  - Adopt no-CRT for the product only if pooled medians differ by more than 0.5 ms **and** the difference is larger than 2× the standard error. Otherwise prefer the maintainable build.
- `nop.exe` / `nop-con.exe`: `void WINAPI entry(void){ExitProcess(0);}` linked with `/NODEFAULTLIB /ENTRY:entry kernel32.lib`, with the GUI and console subsystems respectively.

### 3.2 Rust (update the toolchain first: `rustup update stable`; this is user scope, and 1.72 is too old for current `windows` crates)
`Cargo.toml`:
```toml
[profile.release]
opt-level = 3          # also try "z" for a size-only comparison; startup difference expected negligible
lto = "fat"
codegen-units = 1
panic = "abort"
strip = true
debug = false
incremental = false
```
`.cargo/config.toml` (static CRT: no vcruntime140.dll dependency; https://doc.rust-lang.org/reference/linkage.html#static-and-dynamic-c-runtimes):
```toml
[target.x86_64-pc-windows-msvc]
rustflags = ["-C", "target-feature=+crt-static"]
```
- `#![windows_subsystem = "windows"]` at the top of `main.rs`.
- Hot path:
  - Write with `std::io::stdout().lock().write_all(&bytes)` into a single prebuilt `Vec<u8>`; avoid `println!`/`format!` per line. The fmt machinery is fine for `--timing` and errors.
  - No `clap`. Parse args with `std::env::args_os()`.
  - Convert UTF-16 with `String::from_utf16_lossy`.
- Crate choice:
  - **`windows`** crate (https://docs.rs/windows): typed COM interfaces for `IMMDeviceEnumerator`, `IMMDevice`, `IPropertyStore`, `CoCreateInstance`, and RAII Release on drop.
    - Declare IPolicyConfig with `#[windows::core::interface("f8679f50-850a-41cf-9c72-430f290290c8")] unsafe trait IPolicyConfig: IUnknown { fn GetMixFormat(&self, ...) -> HRESULT; ... /* 10 placeholders in order */ fn SetDefaultEndpoint(&self, id: PCWSTR, role: ERole) -> HRESULT; fn SetEndpointVisibility(&self, id: PCWSTR, visible: i32) -> HRESULT; }`.
    - Features needed (check exact names on docs.rs for the version you pin): `Win32_Foundation`, `Win32_Media_Audio`, `Win32_System_Com`, `Win32_System_Com_StructuredStorage`, `Win32_UI_Shell_PropertiesSystem`, `Win32_Devices_FunctionDiscovery` (PKEY_Device_FriendlyName), `Win32_System_Variant` (if PROPVARIANT lives there in your version).
    - Thanks to LTO and generic-free bindings, the size cost is small: expect ~150-300 KB stripped. Compile time is the main cost.
  - **`windows-sys`** (https://docs.rs/windows-sys) has functions, constants and structs only, **no COM interface vtables**. You would hand-write `#[repr(C)]` vtables, as in C. That is lighter on compile time, but unsafe and verbose.
  - **Decision: the bench uses `windows` (`ta-rust.exe`), because that is what the product would ship.** `ta-rust-sys.exe` is optional, just to show that the crate choice does not matter at runtime: both import the same DLLs.
  - The product uses `windows` (+ `windows-sys` for the few plain Win32 calls if desired).
- `build-std` / `panic_immediate_abort`: not needed (nightly only, saves only KBs). Rust std's startup on Windows is tiny: a stack guard and the main thread name, roughly 50-200 µs. Since Rust 1.78, std imports `bcryptprimitives.dll` (ProcessPrng) for HashMap seeds, which is one extra small DLL; note it in `dumpbin /dependents`.

### 3.3 C# .NET 9 NativeAOT (SDK 9.0.318; requires the VS C++ toolset, which is present)
`ta-cs.csproj`:
```xml
<PropertyGroup>
  <OutputType>WinExe</OutputType>
  <TargetFramework>net9.0</TargetFramework>
  <RuntimeIdentifier>win-x64</RuntimeIdentifier>
  <PublishAot>true</PublishAot>
  <AllowUnsafeBlocks>true</AllowUnsafeBlocks>
  <Nullable>enable</Nullable>
  <InvariantGlobalization>true</InvariantGlobalization>   <!-- no ICU load -->
  <UseSystemResourceKeys>true</UseSystemResourceKeys>     <!-- no resource strings -->
  <OptimizationPreference>Speed</OptimizationPreference>  <!-- also try Size for the size column -->
  <StackTraceSupport>false</StackTraceSupport>            <!-- .NET 8+ name; older docs: IlcGenerateStackTraceData=false -->
  <DebuggerSupport>false</DebuggerSupport>
  <EventSourceSupport>false</EventSourceSupport>
  <UseNativeHttpHandler>false</UseNativeHttpHandler>
  <IlcFoldIdenticalMethodBodies>true</IlcFoldIdenticalMethodBodies>
  <StripSymbols>true</StripSymbols>                      <!-- symbols go to a separate .pdb; exclude it from size -->
  <TrimMode>full</TrimMode>                              <!-- default under PublishAot -->
  <SatelliteResourceLanguages>en</SatelliteResourceLanguages>
  <BuiltInComInteropSupport>false</BuiltInComInteropSupport>
</PropertyGroup>
```
Publish with `dotnet publish -c Release -r win-x64 -o out`. Docs: https://learn.microsoft.com/dotnet/core/deploying/native-aot/optimizing and https://learn.microsoft.com/dotnet/core/deploying/trimming/trimming-options

COM under AOT: classic `[ComImport]` interop is **unsupported** in NativeAOT. There are two options:
1. **Raw vtable calls with function pointers.** This is recommended for the bench: smallest, no marshalling, no ComWrappers init.
   - Declare `[LibraryImport("ole32.dll")] static partial int CoCreateInstance(in Guid clsid, nint outer, uint ctx, in Guid iid, out nint ppv);` (likewise CoInitializeEx, CoUninitialize, CoTaskMemFree, PropVariantClear).
   - Call through the vtable: `var vtbl = *(nint**)p; int hr = ((delegate* unmanaged[Stdcall]<nint, char*, int, int>)vtbl[13])(p, idPtr, role);`
   - Release is `((delegate* unmanaged<nint, uint>)vtbl[2])(p)`.
   - PROPVARIANT: a 16-byte `stackalloc`; `pwszVal` is at offset 8.
2. **Source-generated COM**: `[GeneratedComInterface]` + `[Guid]` on partial interfaces, and `StrategyBasedComWrappers` (`new StrategyBasedComWrappers().GetOrCreateObjectForComInstance(ptr, CreateObjectFlags.None)`). This is AOT-safe and readable, but adds the ComWrappers runtime (~100-300 KB, small init cost). Suitable for a C# product; reference https://learn.microsoft.com/dotnet/standard/native-interop/comwrappers-source-generation

Other rules:
- `Main(string[] args)` with `[STAThread]`.
- Output as UTF-8 bytes via `Console.OpenStandardOutput()` (see 1.2). Do not use string interpolation/LINQ in the hot path.
- Verify it is really native: there must be no managed `*.dll` beside the exe, and no `coreclr.dll` in `dumpbin /dependents`.

Expected: 1.0-2.0 MB exe. Startup = AOT runtime init (GC heap reservation, type system, main-thread setup), roughly 3-10 ms over C on this CPU.

### 3.4 Go (portable install: download `go1.xx.windows-amd64.zip` from https://go.dev/dl/ into `S\tools\go`; set `GOROOT`, `PATH`, `GOPATH=S\tools\gopath`, `GOCACHE=S\tools\gocache`)
- COM without cgo (`CGO_ENABLED=0`). Two options:
  - **Raw**: use `golang.org/x/sys/windows` and `windows.NewLazySystemDLL("ole32.dll").NewProc("CoCreateInstance")`, then call vtable entries with `syscall.SyscallN(vtbl[i], this, args...)`. Here `vtbl := *(*[16]uintptr)(unsafe.Pointer(*(*uintptr)(unsafe.Pointer(this))))`, and strings come from `windows.UTF16PtrFromString`. `NewLazySystemDLL` restricts loading to System32 (DLL-hijack safe). Recommended: it has the fewest dependencies.
  - **go-ole** (https://github.com/go-ole/go-ole): convenient, but still needs custom vtables for MMDevice and IPolicyConfig, and adds code. Optional.
- `func init(){ runtime.LockOSThread() }`. Do all COM work on main.
- Build: `set CGO_ENABLED=0&& set GOAMD64=v3&& go build -trimpath -ldflags "-s -w -H windowsgui" -o ta-go.exe`
  - `-s -w` strips the symbol table and DWARF. `-H windowsgui` selects the GUI subsystem.
- Expected: ~1.5-2.5 MB. Runtime startup on Windows means creating sysmon and GC worker threads, loading winmm/ws2_32 lazily, etc., roughly 2-8 ms over C. Do not use UPX: it brings AV false positives and slower startup.

### 3.5 Zig (portable: zip from https://ziglang.org/download/ into `S\tools\zig`; use the latest stable)
- **Primary (`ta-zig.exe`)**: compile the *same C source* with `zig cc -target x86_64-windows-gnu -O2 -s -Wl,--subsystem,windows ta.c -lole32 -o ta-zig.exe`.
  - This compares MinGW-w64 CRT + Clang/LLVM against MSVC on identical code. Expect parity with `ta-c.exe` within noise, and ~10-60 KB.
  - The C source must not use `wmain` (MinGW needs `-municode` for it) or MSVC-only intrinsics. `int main(void)` + `GetCommandLineW` keeps it portable.
  - Check that `--subsystem` took effect with dumpbin.
- **Optional (`ta-zig-native.exe`)**: native Zig with hand-declared `extern "ole32" fn CoCreateInstance(...) callconv(.winapi)` and `extern struct` vtables.
  - Prefer hand declarations over **zigwin32** (https://github.com/marlersoft/zigwin32): zigwin32 lags Zig releases and API churn is frequent. Use it only if it compiles with the installed Zig.
  - Build with `zig build-exe -O ReleaseFast -fstrip --subsystem windows` (or `ReleaseSmall` for the size column). Expected: same speed as C, 5-20 KB.

### 3.6 PowerShell baselines: why ~900 ms
The `C:\bin\Switch-Audio.exe` (27,648 bytes) is a ps2exe wrapper. It does the following:
1. Starts the .NET Framework 4.x CLR (mscoree → clr.dll, ~20-60 ms warm).
2. Loads System.Management.Automation and creates a runspace with the default InitialSessionState. That loads built-in cmdlets, type and format data, and the providers: hundreds of ms.
3. Runs the script. `Get-Module -ListAvailable -Name AudioDeviceCmdlets` **scans every module path on disk**, which is expensive. `Import-Module` then loads and JIT-compiles the C# cmdlet assembly.
4. Calls `Get-AudioDevice -List` **twice**, which means two full enumerations with property reads, plus pipeline/Where-Object overhead and JIT for each new code path.
5. `Set-AudioDevice`, then `Write-Host` formatting.

Only step 5's COM calls (~tens of ms) are inherent to the job. Everything else is host overhead that the native rewrite removes. The `ps-host-empty`/`pwsh-host-empty` rows isolate the host share (expect powershell.exe ~150-300 ms, pwsh ~250-500 ms). The `ps-list` row adds module import + JIT.

---

## 4. Expected results (sanity ranges for this 7950X, warm, Defender on)

| Row | Expected mean | Red flags |
|---|---|---|
| nop.exe (GUI) | 1-4 ms | > 8 ms: Defender is scanning each run (check that warm-up happened and the file is not changing), or there is harness overhead |
| nop-con.exe, inherited console | nop + 0-1 ms | - |
| nop-con.exe, newconsole | +10-100 ms (conhost/WT) | this is why GUI subsystem is mandatory |
| C runtime-only (no args) | ≈ nop + 0-0.5 ms | - |
| C list | 5-15 ms | < nop + 1 ms means COM probably was not called; > 40 ms needs investigation |
| Rust | C + 0-2 ms (10-20 ms ballpark) | > C + 5 ms: check crt-static, a debug build, or fmt in loops |
| Zig cc | ≈ C | - |
| Go | C + 2-10 ms (20-40 ms ballpark) | - |
| NativeAOT | C + 3-15 ms (20-40 ms ballpark) | > 60 ms: check that it is really AOT (no managed `*.dll` beside the exe, no coreclr.dll in dependents) |
| COM init + enumerator creation (phases) | 1-10 ms | - |
| Enumerate 5 endpoints + names | 1-20 ms | - |
| set-noop: 3 × SetDefaultEndpoint | 1-30 ms total | < 0.2 ms means it is not being called |
| real toggle | 10-100+ ms (driver/AudioSrv dependent; HDMI endpoints can be slow) | Same for all languages: it is OS-bound. Differences between languages are only the startup delta |
| powershell.exe / pwsh empty host | 150-500 ms | - |
| ps2exe Switch-Audio.exe toggle | 700-1200 ms (previously measured ~900 ms) | - |

How to read the results:
- If every native implementation lands within a few ms of each other and well under 50 ms, process start is no longer the bottleneck. The language choice should then rest on product concerns (GUI, MSI, maintainability), not on startup speed.
- What the user perceives is dominated by the SetDefaultEndpoint round-trip and by how fast Windows and apps re-route audio, not by the exe.

---

## 5. Reporting template: `S\bench\results\RESULTS.md`

```markdown
# Benchmark results: toggle-audio implementations
Date: YYYY-MM-DD  |  Harness: hyperfine vX.Y.Z (-N, warmup 10, runs 200 x 3 rounds interleaved fwd/rev/fwd) + spawnbench
Machine: AMD Ryzen 9 7950X, <RAM>, Windows 11 Pro for Workstations 10.0.26300 (zh-TW), power plan <...>, AC power
Defender RTP: ON (no exclusions possible, non-admin) | Smart App Control: <state> | Audio playing during test: yes/no
Toolchains: MSVC 14.44 (VS 17.14) | rustc <ver>, windows <ver> | .NET SDK 9.0.318 | Go <ver> | Zig <ver>
Endpoints: set-noop target PG42UQ {..739b3554..}; toggle pair PG42UQ <-> PHL BDM4065 {..5b124733..}

## Summary table (ms, pooled mean ± σ; median; min)
| Implementation | Subsystem | Binary size (bytes) | DLL deps | runtime-only | list | set-noop | toggle | cold first run | Notes |
|---|---|---|---|---|---|---|---|---|---|
| nop.exe (floor) | GUI | | kernel32 | - | - | - | - | | |
| C /MT | GUI | | | | | | | | |
| C /MD | GUI | | | | | | | | needs vcruntime140 |
| C no-CRT | GUI | | | | | | | | |
| C console (inherit / newconsole) | CUI | | | | | | | | |
| Rust (windows crate) | GUI | | | | | | | | |
| Rust (windows-sys) [opt] | GUI | | | | | | | | |
| Zig cc (C source) | GUI | | | | | | | | |
| Zig native [opt] | GUI | | | | | | | | |
| Go | GUI | | | | | | | | |
| C# NativeAOT | GUI | | | | | | | | |
| powershell.exe empty host | - | - | - | - | - | - | - | | |
| pwsh.exe empty host | - | - | - | - | - | - | - | | |
| powershell + AudioDeviceCmdlets | - | - | - | - | | | - | | |
| ps2exe C:\bin\Switch-Audio.exe | - | 27,648 | | - | - | - | | | toggles S/PDIF<->PG42UQ |

## Phase breakdown (median µs over 30 `--timing` runs, `set` command)
| Impl | create_to_entry | com_init | enumerator | device_validated | policy_created | set_console | set_multimedia | set_communications | released | total in-process |
|---|---|---|---|---|---|---|---|---|---|---|

## Correctness gate
list/get byte-identical to C reference: C ok | Rust ok | ... (UTF-8 check of 喇叭 = E5 96 87 E5 8F AD)

## Observations and anomalies
- outlier warnings, Defender effects, cold-run numbers, anything not reproducible; ETW skipped (needs admin)

## Recommendation
- Language for the product + rationale (startup delta vs. maintainability/GUI/MSI)
- Build profile to adopt (e.g. /MT vs no-CRT; Rust opt-level)

## Raw data
results/*.json (hyperfine), results/timing-*.tsv, spawnbench outputs
```

---

## 6. Hand-off checklist per implementer
1. Source in `S\bench\<lang>\`, plus a non-interactive `build.cmd`. Copy the binary into `S\bench\bin\`.
2. Implements `list`, `get`, `set`, `toggle`, the no-args exit 1, `--timing`, and exit codes 0/1/2/3/4. GUI subsystem (verified with dumpbin). UTF-8 output.
3. Self-test only side-effect-free commands: `list`, `get`, `set <current default id>`, no args, a bad id (expect exit 3). **Do not run `toggle` or `set` to a different device** unless the orchestrator explicitly allows it.
4. Report the binary size, `dumpbin /dependents` output, toolchain version, and exact build flags in the final return message.
