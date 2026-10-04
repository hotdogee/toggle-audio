# C (MSVC) reference implementation

`ta.c` is the speed reference for the toggle-audio benchmarks: a plain C program that talks to
Windows Core Audio through raw COM vtables, uses no CRT functions, and imports only `kernel32.dll`
and `ole32.dll`. Every other implementation in `bench/` is compared against it, both for speed
and, byte for byte, for the output of `list` and `get`.

One source file builds five ways (all in `bin\`, gitignored):

| Exe | What differs | Why it exists |
| --- | --- | --- |
| `ta-c.exe` | **primary**: `/MT` static CRT, console subsystem, `detached.manifest` embedded | The configuration the project would ship: no VC++ redistributable, no console window from G HUB/Explorer on Windows 11 24H2+, shells wait and capture output. |
| `ta-c-gui.exe` | Windows (GUI) subsystem, `/ENTRY:wmainCRTStartup` | Compares the console + detached-manifest approach with the classic GUI-subsystem trick. Writes to redirected stdout/stderr when a parent passes handles (hyperfine, spawnbench, `cmd /c "... > file"`, pipes). |
| `ta-c-md.exe` | `/MD` dynamic CRT | Shows the cost of loading `vcruntime140.dll` + the UCRT API sets instead of linking the CRT in. Needs the VC++ redistributable, so it is not shippable. |
| `ta-c-delayload.exe` | `ole32.dll` delay-loaded (`/DELAYLOAD:ole32.dll delayimp.lib`) | Isolates the load-time cost of the `ole32` import (it pulls in `combase.dll`, `rpcrt4.dll`, ...). Only the no-arguments scenario benefits; every real command needs COM anyway. |
| `ta-c-nocrt.exe` | no CRT at all: `/DTA_NOCRT /Oi /Zl`, `/NODEFAULTLIB /ENTRY:entry` | Measures what the static CRT's startup (heap, environment, locale, atexit tables) costs. |

All five are built with the same compiler flags otherwise and all carry the same embedded
manifest, so the manifest itself is never the variable (its cost is measured separately by the
`nop-con` / `nop-con-detached` pair in [`../baseline`](../baseline/README.md)).

## Command-line contract

Common to every `bench/<lang>` implementation. It is `docs/research/benchmark-method.md` section 1
plus the console-subsystem decision in `docs/DESIGN.md` section 12, with the output-format changes
listed under [Deviations from benchmark-method.md section 1](#deviations-from-benchmark-methodmd-section-1)
below; where the two disagree, this table is what every implementation actually emits:

| Command | Behaviour | stdout | Exit |
| --- | --- | --- | --- |
| `ta-c list` | Active render endpoints in enumeration order | `<id>\t<name>\t<flags>` per line; flags `*` default, `c` default communications, `*c` both, `-` neither | 0 |
| `ta-c get` | Default render endpoint (`eConsole`) | `<id>\t<name>` | 0, or 4 if there is no default |
| `ta-c set <id>` | `GetDevice(id)` + `GetState() == ACTIVE`, then `SetDefaultEndpoint` for `eConsole`, `eMultimedia`, `eCommunications`, always (even when it already is the default: that is the "set-noop" scenario) | nothing | 0, 3 if not found / not active |
| `ta-c toggle <idA> <idB>` | If the `eConsole` default is `idA` set `idB`, otherwise set `idA` (same validation and three roles) | the target id | 0, 3 |
| `ta-c` (no or bad arguments) | One usage line on stderr; **no COM work** (the "runtime floor" scenario) | nothing | 1 |
| `--timing` anywhere | Phase stamps on stderr (below) | unchanged | unchanged |

Exit code 2 is any COM/Win32 failure, reported as `error: <step> hr=0x8XXXXXXX` on stderr. If the
endpoint disappears between validation and `SetDefaultEndpoint` (which then returns `E_NOTFOUND`,
`0x80070490`), `set`/`toggle` exit with 3, like any other "device not found", not 2. stdout is
all-or-nothing: when a command fails, nothing is written to stdout (no partial `list` or `get`
output), only the error line on stderr.
Endpoint ids are compared ordinally and case-insensitively (they are GUID based). The id passed
to `SetDefaultEndpoint` and printed by `toggle` is the canonical one returned by
`IMMDevice::GetId`, not the user's spelling.

### Deviations from benchmark-method.md section 1

These were adopted by the orchestrator for all languages (the correctness gate byte-compares `list`
and `get` output against this C reference, so every implementation must match them):

| Item | benchmark-method.md section 1 | Implemented contract |
| --- | --- | --- |
| `get` output | `<id>` | `<id>\t<name>` |
| `list` flags | `*` (default) or `-` | `*` default, `c` default communications, `*c` both, `-` neither |
| `--timing` line | `timing\t<phase>\t<us>` | `phase\t<name>\t<us>` |
| `--timing` reference point | microseconds since entry, plus a separate `create_to_entry` line | microseconds since **process creation**, so `entry` itself is the create-to-entry time |
| `--timing` phases | `entry`, `com_init`, `enumerator`, `default_got`, `enumerated`, `names_read`, `device_validated`, `policy_created`, `set_*`, `released`, `exit` | `entry`, `com_init`, `enumerator`, `work_done`, `exit` |
| Subsystem | GUI (`/SUBSYSTEM:WINDOWS`) | console plus the embedded `consoleAllocationPolicy=detached` manifest (`docs/DESIGN.md` section 12) |

### `--timing`

After the program has finished (so the I/O does not disturb the measurement) it writes, to stderr
and in one write call:

```
phase	entry	2509.0
phase	com_init	4391.2
phase	enumerator	6402.7
phase	work_done	8530.6
phase	exit	9002.4
```

Each value is microseconds since **process creation**, one decimal. `entry` is
`GetSystemTimePreciseAsFileTime()` at the first statement of `wmain`/`entry` minus the creation
time from `GetProcessTimes`, i.e. kernel process setup + loader + CRT startup. The later phases
add `QueryPerformanceCounter` deltas measured from that same first statement. `com_init` follows
`CoInitializeEx`, `enumerator` follows `CoCreateInstance(MMDeviceEnumerator)`, `work_done` follows
the command's COM work (after `SetDefaultEndpoint` for `set`/`toggle`), and `exit` is taken after
COM is released, `CoUninitialize` has run and stdout has been written. The usage path prints only
`entry` and `exit`.

## How the COM calls are made in C

- **Interfaces.** `#define COBJMACROS` makes the SDK headers emit C wrapper macros such as
  `IMMDeviceEnumerator_GetDefaultAudioEndpoint(p, ...)`, which expand to
  `p->lpVtbl->GetDefaultAudioEndpoint(p, ...)`.
- **GUIDs.** In C, `mmdeviceapi.h` only *declares* `CLSID_MMDeviceEnumerator` and
  `IID_IMMDeviceEnumerator`, and no SDK library defines them. `ta.c` includes `<initguid.h>`
  (which turns `DEFINE_GUID` / `DEFINE_PROPERTYKEY` into definitions for this translation unit)
  and then defines those two and the undocumented policy-config GUIDs with `DEFINE_GUID`.
  `PKEY_Device_FriendlyName` comes from `functiondiscoverykeys_devpkey.h` the same way. GUIDs the
  program never references are removed by `/Gw` + `/OPT:REF`.
- **IPolicyConfig.** This interface is undocumented, so `ta.c` declares it by hand: a struct whose
  first member points to a vtable of 15 pointer-sized slots. Slots that are never called are `void *`
  placeholders, and `SetDefaultEndpoint(LPCWSTR, ERole)` sits at **slot 13**. That layout was checked
  against Microsoft's public PDB symbols for `AudioSes.dll` (`docs/research/core-audio-api.md`
  section B.2). If `CoCreateInstance(CLSID_CPolicyConfigClient, IID_IPolicyConfig)` fails, the
  program falls back to `IPolicyConfigVista` on `CPolicyConfigVistaClient`, where
  `SetDefaultEndpoint` is slot 12. Do not copy the Vista layout from SoundSwitch or
  AudioDeviceCmdlets: theirs has an extra method. On Windows 7, 8, and Windows 10 1607 and later
  (including 11) the `IPolicyConfig` request succeeds. Windows 10 1507 and 1511 used a different
  IID (`docs/research/core-audio-api.md` section B.1); there the fallback is the path taken
  (untested).
- **Apartment.** The program calls `CoInitializeEx(NULL, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE)`.
  STA is what the product's settings dialog needs, and both COM classes are registered
  `ThreadingModel=Both`, so there is no proxy and no marshaling cost. If the call returns
  `S_FALSE`, the program still calls `CoUninitialize`; if it returns `RPC_E_CHANGED_MODE`, it
  continues without one. Objects are created with `CLSCTX_INPROC_SERVER`.
- **Call sequence per command.**
  - `list`: `GetDefaultAudioEndpoint` for `eConsole` and for `eCommunications`, each followed by
    `GetId` (`E_NOTFOUND` only means no marker). Then `EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)`
    and `GetCount`. For each item: `Item`, `GetId`, `OpenPropertyStore(STGM_READ)`, and
    `GetValue(PKEY_Device_FriendlyName)`. The name is only used when the value is `VT_LPWSTR`.
  - `get`: `GetDefaultAudioEndpoint(eRender, eConsole)`, then `GetId` and the friendly name.
  - `set`/`toggle`: `GetDevice(id)`. `E_NOTFOUND` or `E_INVALIDARG` gives exit 3. Then `GetState`
    must be `DEVICE_STATE_ACTIVE`, because `GetDevice` also succeeds for unplugged or disabled
    endpoints. Then `GetId` (canonical id), `CoCreateInstance(CPolicyConfigClient)`, and
    `SetDefaultEndpoint` three times. `E_NOTFOUND` from `SetDefaultEndpoint` (the endpoint vanished
    after validation) also gives exit 3.
- **Cleanup discipline.** Every HRESULT is checked. Every interface is `Release`d before
  `CoUninitialize`, every `GetId` string is freed with `CoTaskMemFree`, and every `PROPVARIANT` is
  `PropVariantInit`ed and `PropVariantClear`ed (both come from ole32, so `propsys.lib` is not
  needed).
- **Output.** stdout and stderr are each collected in a growable UTF-16 buffer (on the process
  heap) and written once at the end. stdout is written only when the command succeeded:
  - When the handle is a console (`GetConsoleMode` succeeds), the buffer is written with
    `WriteConsoleW`, so CJK names such as `喇叭 (FiiO BTA30 PRO)` display correctly whatever the
    console code page is (950 on the reference machine).
  - Otherwise (pipe, file, NUL) it is converted with `WideCharToMultiByte(CP_UTF8)` and written with
    `WriteFile`: UTF-8, no BOM, `\n` line endings. `喇叭` comes out as `E5 96 87 E5 8F AD`.
  - Tabs and newlines inside a friendly name are replaced with spaces, so one endpoint is always
    exactly one line.
- **Arguments.** CRT builds use `wmain`'s `argv`. The no-CRT build splits `GetCommandLineW()`
  itself (same backslash/quote rules as the CRT). `CommandLineToArgvW` is never used, because it
  would load `shell32.dll`.

## Build flags and why

`build.ps1` compiles `ta.c` once per variant:

```
cl /nologo /W4 /utf-8 /O2 /GL /Gy /Gw /GS- /DUNICODE /D_UNICODE <variant flags> ta.c
   /link /LTCG /OPT:REF /OPT:ICF /MANIFEST:EMBED /MANIFESTINPUT:..\common\detached.manifest
         /MANIFESTUAC:NO <variant link flags> ole32.lib
```

| Flag | Reason |
| --- | --- |
| `/O2` | Optimize for speed. The program is COM/IO bound, so `/O1` would measure the same; `/O2` is the conventional "fast" setting. |
| `/GL` + `/LTCG` | Whole-program optimization of `ta.c` (cross-function inlining). The prebuilt CRT libraries are not `/GL` objects, so LTCG does not touch them; unused static-CRT code is removed by `/OPT:REF`. |
| `/Gy` `/Gw` | Put every function (`/Gy`) and global (`/Gw`) in its own COMDAT so that `/OPT:REF` can drop unreferenced ones and `/OPT:ICF` can fold identical ones. |
| `/GS-` | No per-function stack-cookie checks in `ta.c` (negligible cost either way). The CRT startup still initialises the cookie (`__security_init_cookie`) in the `/MT` and `/MD` builds; only `ta-c-nocrt` skips it. Required for the no-CRT build, which has no `__security_check_cookie`. The program has no attacker-controlled buffers (all text goes through bounds-checked heap buffers). |
| `/MT` | Links the CRT statically: no `vcruntime140.dll` dependency (that DLL is not part of Windows). `ta-c-md` uses `/MD` for comparison. |
| `/W4` | High warning level. The build is warning-free, and `build.ps1` prints any diagnostic it sees. |
| `/utf-8` | Source and execution character sets are UTF-8, whatever the system code page (950 here). The source is ASCII, so this is a safeguard. |
| `/DUNICODE /D_UNICODE` | Wide-character Win32 APIs. |
| `/SUBSYSTEM:CONSOLE` | Console subsystem (`docs/DESIGN.md` section 12): shells wait for the process and redirection works, including PowerShell `>` and `$x = & exe`. |
| `/MANIFEST:EMBED /MANIFESTINPUT:...` | Embeds `bench/common/detached.manifest` as `RT_MANIFEST` #1 with the linker itself, so `mt.exe` is not needed. `consoleAllocationPolicy=detached` means a launch from a GUI process (G HUB, Explorer) gets **no** console window on Windows 11 24H2+, while a launch from a terminal still inherits that terminal. |
| `/MANIFESTUAC:NO` | The input manifest already has `trustInfo` (`asInvoker`). Without this flag the linker would merge in a second copy. |
| `/OPT:REF /OPT:ICF` | Remove unreferenced code and data (including unused static-CRT functions), and fold identical COMDATs. |
| `ole32.lib` | `CoInitializeEx`, `CoCreateInstance`, `CoTaskMemFree`, `PropVariantClear`. |

Variant-specific flags:

- `ta-c-gui`: `/SUBSYSTEM:WINDOWS /ENTRY:wmainCRTStartup`, which keeps `wmain` in a GUI-subsystem
  exe.
- `ta-c-md`: `/MD` in place of `/MT`.
- `ta-c-delayload`: `delayimp.lib /DELAYLOAD:ole32.dll`.
- `ta-c-nocrt`: `/DTA_NOCRT /Oi /Zl` and `/NODEFAULTLIB /ENTRY:entry kernel32.lib`.
  - `/Zl` keeps default-library records out of the object file.
  - `ta.c` supplies `memset` and `memcpy` (`#pragma function` + `__stosb`/`__movsb`), because
    `PropVariantInit` and struct initializers need them.
  - The program uses no floating point and keeps its stack frames small, so neither `_fltused`
    nor `__chkstk` is pulled in.

Security-relevant linker defaults stay on: `/DYNAMICBASE`, `/HIGHENTROPYVA`, `/NXCOMPAT`.

## Building

Prerequisites:
- Windows 10/11 x64.
- Visual Studio 2022 or Build Tools with the "Desktop development with C++" workload (MSVC 14.44
  and Windows SDK 10.0.26100 were used).
- PowerShell 7.

```powershell
pwsh -File bench\c\build.ps1                 # all variants
pwsh -File bench\c\build.ps1 -Only ta-c      # just the primary exe
```

The script is non-interactive and idempotent:
1. It imports the x64 developer environment via `vcvars64.bat`, which it locates with `vswhere`
   (or, without `vswhere`, by probing the default VS 2022 Build Tools / Community / Professional /
   Enterprise folders). The helper is `bench/common/vsdevenv.ps1` and is shared with `../baseline`.
   This step is skipped only when an x64-targeting MSVC environment is already active
   (`VSCMD_ARG_TGT_ARCH` and `VSCMD_ARG_HOST_ARCH` both `x64`). The stock "Developer PowerShell"
   and "Developer Command Prompt" target x86, and their environment is deliberately not reused.
2. It rebuilds every variant from scratch into `bin\`, with intermediates in `obj\<variant>\`.
3. It verifies each exe:
   - the machine type, read with `dumpbin /headers` (must be `8664 machine (x64)`; a 32-bit
     WOW64 build would have entirely different startup costs);
   - the PE subsystem, read with `dumpbin /headers` (3 = console, 2 = GUI);
   - the embedded manifest, extracted with `mt.exe -inputresource:<exe>;#1` and checked for
     `consoleAllocationPolicy` = `detached`;
   - the imported DLLs, listed with `dumpbin /dependents`; any `vcruntime`, `ucrtbase` or
     `api-ms-win-crt-*` import fails the build except in `ta-c-md`.
4. It prints a size table.

Sizes from MSVC 14.44.35207 on the reference machine:

| Exe | Bytes | Imports |
| --- | ---: | --- |
| `ta-c.exe` | 116,224 | ole32, kernel32 |
| `ta-c-gui.exe` | 116,224 | ole32, kernel32 |
| `ta-c-md.exe` | 23,040 | ole32, kernel32, vcruntime140, api-ms-win-crt-* |
| `ta-c-delayload.exe` | 119,296 | kernel32 (+ ole32 delay-loaded) |
| `ta-c-nocrt.exe` | 20,480 | kernel32, ole32 |

## Results

The measuring agent fills in this section. Numbers are wall time in ms, measured with hyperfine
`-N` (warm-up 10, 200 runs × 3 rounds, pooled), reference machine (7950X, Windows 11 26300,
Defender on). Scenario definitions are in `docs/research/benchmark-method.md` section 2.7.

| Exe | runtime floor (no args) | `list` | `set` no-op | real `toggle` | `--timing` entry (µs) |
| --- | ---: | ---: | ---: | ---: | ---: |
| `nop-con-detached.exe` (baseline) | _tbd_ | n/a | n/a | n/a | n/a |
| `ta-c.exe` | _tbd_ | _tbd_ | _tbd_ | _tbd_ | _tbd_ |
| `ta-c-gui.exe` | _tbd_ | _tbd_ | _tbd_ | _tbd_ | _tbd_ |
| `ta-c-md.exe` | _tbd_ | _tbd_ | _tbd_ | _tbd_ | _tbd_ |
| `ta-c-delayload.exe` | _tbd_ | _tbd_ | _tbd_ | _tbd_ | _tbd_ |
| `ta-c-nocrt.exe` | _tbd_ | _tbd_ | _tbd_ | _tbd_ | _tbd_ |

Informal implementer smoke numbers, **not** results: `spawnbench`, inherit mode, 200 runs, median.
- `nop-con-detached` 3.4 ms; `ta-c` with no arguments 7.6 ms; `ta-c-delayload` with no arguments
  3.9 ms. So most of the C "runtime floor" is the load-time import of `ole32.dll` and its
  dependencies, not the CRT. Keep this in mind when comparing the no-arguments scenario across
  languages: a runtime that loads ole32 lazily (Go's `syscall.NewLazyDLL`, for example) skips that
  cost there, but pays it in every real command.
- `ta-c list` 18.7 ms; `ta-c set <current default>` 41 to 52 ms (three `SetDefaultEndpoint` RPCs).

## Known limitations

- **`toggle` has not been run on real devices by the implementer.** A hard rule for this pass was
  never to change the default device. `toggle` shares its validation and set path with `set`,
  which was exercised as a no-op re-assert of the current default. The measuring agent runs the
  real toggles.
- **The detached console policy requires Windows 11 24H2 or later.** On older builds the manifest
  element is ignored, and a launch from a GUI process flashes a console window; use
  `ta-c-gui.exe` there. If G HUB passes `CREATE_NEW_CONSOLE` explicitly, the policy (which only
  sets defaults) may not apply. Verify once with the real key binding.
- **`ta-c-gui.exe` has no console of its own.** Without redirection its output is lost (it does not
  `AttachConsole`). PowerShell cannot capture a GUI-subsystem exe with `>` or `$x = & exe`, but
  pipes and `cmd /c "... > file"` work.
- **`entry` (create to entry) depends on the parent.** Launched from PowerShell, `ta-c` shows about
  4.7 ms; launched from Git Bash (MSYS), about 7 ms, because the MSYS runtime does extra work
  between creating the child and letting it run. Compare these values only within one launcher.
- **`IPolicyConfig` is undocumented.** A future Windows release could change it. Every failing
  HRESULT is reported, and the Vista interface is a fallback.
- **One role decides `toggle`.** Only the `eConsole` default is checked, and all three roles are
  always set. The product (`src/`) instead skips roles that already match and makes
  communications optional.
