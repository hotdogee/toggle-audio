# ta-go: Go implementation of the toggle-audio bench CLI

`ta-go.exe` is the Go entry in the toggle-audio benchmark: a tiny command-line tool that lists Windows playback devices, reads the default device, and sets or toggles the default device. It implements the common bench contract (`docs/research/benchmark-method.md` §1, with the console-subsystem deviation from `docs/DESIGN.md` §12) so it can be compared directly with the C, Rust, C# NativeAOT and Zig versions.

It is written in pure Go:

- no cgo (`CGO_ENABLED=0`, so no C compiler and no libc);
- no third-party modules (only `os`, `runtime`, `strconv`, `strings`, `syscall` and `unsafe` from the standard library);
- COM is called directly through vtables.

## Usage

| Command | stdout | Exit |
| --- | --- | --- |
| `ta-go list` | One line per ACTIVE render endpoint: `<id>\t<friendly name>\t<flags>`. `flags` is `*` for the default (eConsole) device, `c` for the default communications device, `*c` for both and `-` for neither. | 0 |
| `ta-go get` | `<id>\t<friendly name>` of the default (eConsole) device | 0, or 4 if there is no default |
| `ta-go set <id>` | nothing. Checks that the endpoint exists and is ACTIVE, then calls `SetDefaultEndpoint` with the endpoint's canonical id (as `IMMDevice::GetId` reports it) for eConsole, eMultimedia and eCommunications, in that order. It does this even when the endpoint already is the default. | 0, 2 or 3 |
| `ta-go toggle <idA> <idB>` | The canonical id of the endpoint it set. If the eConsole default is A it sets B, otherwise A (with the same checks as `set`). | 0, 2 or 3 |
| `ta-go` (or any usage error) | One usage line on stderr. COM is never touched (no ole32.dll load), so this measures the Go runtime floor. | 1 |

- **Errors.** COM failures print `error: <step> hr=0x%08X` on stderr and exit with code 2. An unknown, malformed or empty id, or an endpoint that is not ACTIVE, exits with code 3 (`GetDevice` returning `E_NOTFOUND` or `E_INVALIDARG` counts as not found). If ole32.dll or one of its exports cannot be resolved, the reported HRESULT is `HRESULT_FROM_WIN32` of the loader error.
- **Output encoding.** stdout is UTF-8 with `\n` line endings, independent of the console code page. For example, `喇叭` is written as `E5 96 87 E5 8F AD`.
- **`--timing`.** The flag is accepted anywhere in the arguments and never changes stdout. At exit it writes one line per phase to stderr, in a single write:

  ```
  phase	entry	7643.3
  phase	com_init	11372.0
  phase	enumerator	13315.1
  phase	work_done	17804.2
  phase	exit	18313.5
  ```

  Each value is microseconds since process creation, with one decimal:
  - `entry` is the create-to-entry time. It is `GetSystemTimePreciseAsFileTime` at the start of `main` minus the creation time from `GetProcessTimes`, so it covers the loader plus Go runtime start-up.
  - Every later phase is `entry` plus the `QueryPerformanceCounter` delta since entry.
  - `com_init` is taken after `CoInitializeEx`, and `enumerator` after `CoCreateInstance(MMDeviceEnumerator)`. Because ole32.dll is loaded lazily, `com_init` also includes the `LoadLibrary` of ole32.dll and resolving its exports; implementations that import ole32 statically (C, Rust, C#) pay that cost in the loader, before `entry`.
  - `work_done` is taken after the list, get, set or toggle work, before Release and `CoUninitialize`.
  - `exit` is taken after all cleanup and output, just before returning.
  - Without a command, only `entry` and `exit` are printed.

## How the COM calls are made in Go

| Piece | Where | Notes |
| --- | --- | --- |
| DLL entry points | `com.go` | `syscall.NewLazyDLL("ole32.dll").NewProc(...)` for `CoInitializeEx`, `CoCreateInstance`, `CoTaskMemFree`, `PropVariantClear` and `CoUninitialize`. The DLL is loaded on first use, so the no-args path never maps ole32. ole32.dll is a KnownDLL, so it always comes from System32. |
| Interface calls | `comObject.call(slot, args...)` | A COM interface pointer is a pointer to a pointer to the vtable. Go mirrors that with `type comObject struct{ vtbl *[32]uintptr }` and calls `syscall.SyscallN(o.vtbl[slot], this, args...)`. The slot numbers come from `docs/research/core-audio-api.md`: `EnumAudioEndpoints`=3, `GetDefaultAudioEndpoint`=4, `GetDevice`=5, `GetCount`=3, `Item`=4, `OpenPropertyStore`=4, `GetId`=5, `GetState`=6, `IPropertyStore::GetValue`=5, `IPolicyConfig::SetDefaultEndpoint`=13 (`IPolicyConfigVista` fallback: 12). |
| Out-parameters | `//go:uintptrescapes` on `callProc` and `comObject.call` | Go stacks can move. The directive makes the compiler heap-allocate any variable whose address is converted to `uintptr` in the call's argument list, and keep it alive for the call. This is the same mechanism `syscall.LazyProc.Call` uses. Callers therefore always write `uintptr(unsafe.Pointer(&out))` inline. |
| Structures | `guid`, `propertyKey`, `propVariant` | These match the x64 Win32 layouts (16, 20 and 24 bytes; `pwszVal` at offset 8). `cli_test.go` asserts the sizes and offsets, and checks every GUID literal against its registry string. |
| Strings | `utf16PtrToString`, `syscall.UTF16PtrFromString` | Ids and names arrive as CoTaskMem UTF-16 strings and are copied into Go strings (UTF-8). |
| Activation | `coCreateInstance` | `CLSCTX_INPROC_SERVER`, like the C reference: both classes are in-proc servers, and no out-of-proc activation is ever attempted. |
| Apartment | `init()` and `openAudioSession` | `runtime.LockOSThread()` in `init` pins the main goroutine to the main OS thread. Then `CoInitializeEx(COINIT_APARTMENTTHREADED \| COINIT_DISABLE_OLE1DDE)`, the same STA the product uses. `RPC_E_CHANGED_MODE` is tolerated and not paired with `CoUninitialize`. |
| Lifetimes | `audio.go` | Every HRESULT is checked. Every interface is released with `defer x.release()` right after it is obtained. Every `GetId` string goes through `CoTaskMemFree`, and every `PROPVARIANT` through `PropVariantClear`. All of this happens before `CoUninitialize`. |

`golang.org/x/sys/windows` was not needed: `syscall` already provides `NewLazyDLL`, `SyscallN`, `UTF16PtrFromString`, `UTF16ToString` and `GetProcessTimes`. The few HRESULT and GUID constants are spelled out in `com.go`.

Source layout:
- `main.go`: entry point, flow and output.
- `cli.go`: pure helpers for argument parsing, the toggle decision and formatting.
- `com.go`: COM plumbing.
- `audio.go`: the four commands.
- `timing.go`: `--timing`.
- `cli_test.go`: unit tests.

## Build

Prerequisites:

- Windows 10/11 x64 with PowerShell 7 (`pwsh`).
- Go 1.21 or newer (`go.mod` declares `go 1.21`; the code needs only `syscall.SyscallN` and `unsafe.Slice`/`unsafe.Add`). The published numbers were built with go1.27.0. Any install works, including the portable zip. `build.ps1` finds `go.exe` in this order: the `-GoExe` parameter, `%GO_EXE%`, `PATH`, `%LOCALAPPDATA%\Programs\go\bin\go.exe`, `%ProgramFiles%\Go\bin\go.exe`.
- `mt.exe` from the Windows 10/11 SDK. `build.ps1` finds it in this order: the `-MtExe` parameter, `%MT_EXE%`, `PATH`, then the newest `C:\Program Files (x86)\Windows Kits\10\bin\<version>\x64\mt.exe`.

```powershell
pwsh -NoProfile -File bench\go\build.ps1              # vet + test + build + embed manifest
pwsh -NoProfile -File bench\go\build.ps1 -SkipTests   # build only
pwsh -NoProfile -File bench\go\build.ps1 -GoAmd64 v1  # baseline x64 instead of v3
```

The script produces `bench\go\bin\ta-go.exe` and prints its size. It is non-interactive and idempotent, and with the same toolchain it produces byte-identical output. It runs these commands:

```powershell
# Environment: GOTOOLCHAIN=local GOFLAGS=-mod=readonly GOOS=windows GOARCH=amd64 GOAMD64=v3 CGO_ENABLED=0
go vet .
go test -count=1 .
go build -trimpath -buildvcs=false "-ldflags=-s -w -buildid=" -o bin\ta-go.exe .
mt.exe -nologo -manifest ..\common\detached.manifest "-outputresource:bin\ta-go.exe;#1"
mt.exe -nologo "-inputresource:bin\ta-go.exe;#1" -out:<temp>   # verify consoleAllocationPolicy is present
```

### Flags and why

| Flag | Why |
| --- | --- |
| `CGO_ENABLED=0` | Pure Go: no C toolchain, no libc or msvcrt import, and the binary is statically linked except for system DLLs. COM needs no cgo. |
| `GOAMD64=v3` | Lets the compiler use x86-64-v3 instructions (AVX2, BMI2, FMA; Haswell, Zen 1 and newer). The reference 7950X supports it. The effect on a start-up-bound tool is negligible; it is kept to match the benchmark plan. **The binary will not start on CPUs without AVX2**, so pass `-GoAmd64 v1` for a binary that runs on any x64 CPU. |
| `-trimpath` | Removes local file-system paths from the binary, which makes it reproducible and keeps the user name out. |
| `-buildvcs=false` | No VCS stamping. The build works outside a git checkout, and the output does not change with the commit. |
| `-ldflags "-s -w"` | Strips the symbol table and DWARF, which makes the file smaller (less to read and map). Panics still print function names, because the runtime keeps `pclntab`. |
| `-ldflags "-buildid="` | Empty Go build id, so repeated builds give identical bytes (Defender rescans only a new hash). |
| *no* `-H windowsgui` | Console subsystem (dumpbin: `3 subsystem (Windows CUI)`), per DESIGN.md §12. Shells wait for the process and capture its output, and pwsh `> file` and `$x = & ta-go list` work. |
| Embedded `detached.manifest` | `consoleAllocationPolicy=detached`. On Windows 11 24H2+, a launch from a GUI process (G HUB, Explorer) creates no console window. Launches from a terminal inherit the terminal as usual. The manifest also declares `asInvoker` and Windows 10/11 `supportedOS`. |
| `GOTOOLCHAIN=local`, `GOFLAGS=-mod=readonly` | Never download a different toolchain, and never let the go command rewrite `go.mod` or `go.sum` (the build fails instead), which keeps the build reproducible. The module has no dependencies, so there is no `go.sum`. |

**Manifest alternatives.** The Go linker cannot embed a manifest by itself, so `mt.exe` adds it after the link as resource `RT_MANIFEST #1` (the C build uses `link /MANIFEST:EMBED /MANIFESTINPUT`; Zig embeds it natively). A pure-Go alternative is [go-winres](https://github.com/tc-hib/go-winres) (or the older `github.com/akavel/rsrc`):
- `go-winres make --in winres.json` writes `rsrc_windows_amd64.syso`.
- `go build` then links it automatically, so no SDK is needed.

It was not used here, to keep the repo free of extra tools.

### Verify

```powershell
$exe = 'bench\go\bin\ta-go.exe'
dumpbin /headers $exe | findstr /i subsystem       # 3 subsystem (Windows CUI)
dumpbin /dependents $exe                            # kernel32.dll only (the runtime loads the rest dynamically)
mt.exe -nologo "-inputresource:$exe;#1" -out:check.manifest   # contains consoleAllocationPolicy
& $exe list; & $exe get; & $exe; $LASTEXITCODE      # usage, exit 1
```

Unit tests (`go test .`) cover:
- argument parsing for every command, `--timing` in any position, and arity errors (an empty id is accepted by the parser and rejected later by `GetDevice`, exit 3);
- the toggle decision table;
- the list flags;
- UTF-8 and LF output with a CJK name;
- the `--timing` line format;
- HRESULT formatting and `HRESULT_FROM_WIN32`;
- the ABI layouts and the GUID literals.

They do not need audio hardware.

## Results

_Measured 2026-10-04 by `bench/run-bench.ps1` on the reference machine (7950X, Windows 11 26300.9550, Defender on). Full tables, the method and the anomalies are in [`bench/RESULTS.md`](../RESULTS.md); raw data in [`bench/results/`](../results/)._

hyperfine `-N --warmup 10 --runs 200`, 3 rounds pooled (toggle: warm-up 2 + 20 runs).

| Scenario | Command | Mean ± σ (ms) | Median (ms) | Min (ms) | Notes |
| --- | --- | --- | --- | --- | --- |
| Runtime floor | `ta-go.exe` (exit 1) | 6.07 ± 0.64 | 5.93 | 4.91 | C: 7.52 |
| List | `ta-go.exe list` | 19.94 ± 1.62 | 19.52 | 17.60 | C: 17.57 |
| Get | `ta-go.exe get` | 15.36 ± 0.80 | 15.15 | 14.23 | C: 13.94 |
| Set (no-op) | `ta-go.exe set <current id>` | 40.34 ± 2.70 | 39.93 | 34.34 | C: 38.25 |
| Toggle (real) | `ta-go.exe toggle <A> <B>` | 42.30 ± 2.64 | 41.56 | 37.53 | C: 41.83 |

| Phase (median µs since creation, `--timing`) | entry | com_init | enumerator | work_done | exit |
| --- | --- | --- | --- | --- | --- |
| `set <current id>` | 5,071 | 9,029 | 11,224 | 35,005 | 36,046 |
| `ta-c` for comparison | 6,044 | 8,107 | 10,266 | 36,727 | 37,784 |

Binary: 1,530,368 bytes (go1.27.0, GOAMD64=v3, manifest embedded). Cold first run (fresh copy, `list`): 62.7 / 65.8 / 62.5 ms.

After the C delay-load build, Go has the lowest runtime floor of the bench builds, because it loads `ole32.dll` lazily (`NewLazySystemDLL`). That cost comes back in `com_init` (about 4.0 ms against 2.0 ms for C), so `list` and `get` end up about 1.4–2.4 ms slower than C. `set` and `toggle` are within noise. Passed the correctness gate.

## Known limitations

- **Go runtime start-up.** Before `main` runs, the runtime creates its threads (sysmon, GC workers) and initializes the heap and scheduler. That cost appears in `entry` (create-to-entry). Informal smoke runs on the reference machine showed about 6.5–8 ms, which is not a benchmark result. This is the main gap to C and Rust, and no build flag removes it. UPX is deliberately not used: it causes antivirus false positives and slows start-up.
- **Binary size.** About 1.5 MB, compared with tens of KB for C. This is the Go runtime plus `os` and `strconv`. The `fmt` package is avoided on purpose.
- **Windows 11 24H2+ for no console flash.** On earlier Windows the manifest element is ignored, and a launch from a GUI process would flash a console. For those systems, build a GUI variant with `-ldflags "-s -w -H windowsgui"`; output then only reaches redirected handles.
- **`GOAMD64=v3`** requires AVX2 (see above).
- **Undocumented API.** `IPolicyConfig` is undocumented. The program falls back to `IPolicyConfigVista` (slot 12) if the first `CoCreateInstance` fails, and reports every failing HRESULT.
- **Case-insensitive toggle comparison.** `toggle` compares the current default with `idA` ignoring ASCII case, so hand-typed ids still match. Both `set` and `toggle` hand Windows the canonical id from `IMMDevice::GetId`, never the typed string, and `toggle` prints that canonical id, so the output matches the C reference byte for byte.
- **The bench contract is not the product CLI.** There is no config file, name lookup or skipping of roles that are already set. `set` always calls all three roles, to measure `SetDefaultEndpoint` honestly.
