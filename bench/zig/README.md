# bench/zig: Zig implementations

Two executables that implement the common bench CLI contract
([`docs/research/benchmark-method.md`](../../docs/research/benchmark-method.md) section 1, with the
console-subsystem deviation from [`docs/DESIGN.md`](../../docs/DESIGN.md) section 12):

| Exe | Source | What it measures |
| --- | --- | --- |
| `bin/ta-zig.exe` (primary) | [`ta.zig`](ta.zig) | Native Zig 0.17: hand-declared Win32 imports and COM vtables, no libc, no allocator. About 16 KB; imports only `ntdll`, `kernel32` and `ole32`. |
| `bin/ta-zigcc.exe` (secondary) | [`../c/ta.c`](../c/ta.c), unchanged, via [`ta-mingw.c`](ta-mingw.c) | The MSVC C reference compiled by `zig cc` (Clang + MinGW-w64 with UCRT). It compares Clang/MinGW with MSVC on identical source. |

**Name mapping to benchmark-method.md §1.6/§3.5:** that document calls the `zig cc` build of the C
source `ta-zig.exe` and the native build `ta-zig-native.exe`. Here the native build is the primary
and is named `ta-zig.exe`; the `zig cc` build is `ta-zigcc.exe`. When filling RESULTS.md, record
this directory's `ta-zig.exe` as `ta-zig-native` and `ta-zigcc.exe` as `ta-zig`.

Both are console-subsystem (PE subsystem 3) executables with
[`../common/detached.manifest`](../common/detached.manifest) embedded as `RT_MANIFEST` #1
(`consoleAllocationPolicy=detached`). On Windows 11 24H2+, a launch from G HUB or Explorer creates
no console window, while shells still wait for the program and capture its output.

```
ta-zig list                 <id>\t<name>\t<flags> per ACTIVE render endpoint; flags * (default), c (communications), *c, -
ta-zig get                  <id>\t<name> of the default (eConsole) render endpoint
ta-zig set <id>             check the endpoint exists and is ACTIVE, then SetDefaultEndpoint for eConsole, eMultimedia, eCommunications
ta-zig toggle <idA> <idB>   if the eConsole default is idA, set idB; otherwise set idA. Prints the target id
ta-zig                      usage on stderr, exit 1, no COM work (runtime floor)
--timing (anywhere)         stderr: "phase\t<name>\t<microseconds since process creation>" for entry, com_init, enumerator, work_done, exit
```

Exit codes: 0 OK, 1 usage, 2 COM failure (`error: <step> hr=0x...`), 3 device not found or not active,
4 no default device. `list`/`get` output is byte-identical to `ta-c.exe` (checked with md5 on this
machine, including the CJK name `喇叭 (FiiO BTA30 PRO)` = `E5 96 87 E5 8F AD ...`).

## How the COM calls are made in Zig

Zig has no COM support in its standard library, and `zigwin32` trails Zig releases. So `ta.zig`
declares everything it needs itself, in the same way as the C `lpVtbl` style:

- **Imports**: `extern "kernel32" fn ... callconv(.winapi)` and `extern "ole32" fn ...`
  (`CoInitializeEx`, `CoCreateInstance`, `CoTaskMemFree`, `PropVariantClear`, ...). Zig generates
  the import libraries from its bundled MinGW-w64 `.def` files, so neither the Windows SDK nor libc
  is needed. `callconv(.winapi)` is the spelling Zig 0.17's own std uses (see `lib/std/os/windows.zig`).
  On x64 it is the Microsoft x64 convention.
- **Interfaces**: each interface is an `extern struct { vtbl: *const Vtbl }`, and `Vtbl` is an
  `extern struct` of function pointers whose first parameter is the object itself:

  ```zig
  const IMMDevice = extern struct {
      vtbl: *const Vtbl,
      const Vtbl = extern struct {
          unknown: IUnknownVtbl,  // slots 0-2
          Activate: Slot,         // 3 (unused: opaque pointer placeholder)
          OpenPropertyStore: *const fn (self: *IMMDevice, access: DWORD, out: *?*IPropertyStore) callconv(.winapi) HRESULT,
          GetId: *const fn (self: *IMMDevice, out: *?LPWSTR) callconv(.winapi) HRESULT,
          GetState: *const fn (self: *IMMDevice, state: *DWORD) callconv(.winapi) HRESULT,
      };
  };
  ```

  Methods the program never calls are `*const anyopaque` placeholders, so every slot index stays
  exact. `comptime` asserts pin the important offsets: `IPolicyConfig.SetDefaultEndpoint` at slot
  13, `IPolicyConfigVista.SetDefaultEndpoint` at slot 12, `GUID` = 16 bytes,
  `PROPERTYKEY` = 20 bytes and `PROPVARIANT` = **24** bytes on x64.
- **IPolicyConfig**: CLSID `{870af99c-...}` / IID `{f8679f50-...}`, `SetDefaultEndpoint` at slot 13
  (verified against the AudioSes.dll PDB symbols, see
  [`core-audio-api.md`](../../docs/research/core-audio-api.md) B.2). If that fails, the fallback is
  `CPolicyConfigVistaClient` / `IPolicyConfigVista`, slot 12. Both reduce to "object pointer + function
  pointer", so the calling code is the same either way.
- **Errors and cleanup**: every HRESULT goes through `check()`/`checkOut()`. These turn a failure into
  `error.ComFailure` and record the step name and HRESULT. `checkOut` also rejects a successful call
  that returns a null interface. Cleanup is plain Zig `defer`: every interface is `Release`d, every
  `GetId` string is passed to `CoTaskMemFree` (`OwnedId.deinit`), and every `PROPVARIANT` gets
  `PropVariantClear`. `CoUninitialize` is the outermost `defer`, so it runs after the last `Release`.
  `RPC_E_CHANGED_MODE` from `CoInitializeEx` is accepted and is not balanced with `CoUninitialize`.
- **Apartment**: STA (`COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE`), the same as the product and
  the C reference.
- **Output**: text accumulates as UTF-16 in static buffers. At exit it is written once:
  - to a file or pipe: converted to UTF-8 (in-house encoder, with surrogate pairs and U+FFFD for
    lone surrogates) and written with `WriteFile`;
  - to a real console: written with `WriteConsoleW`.

  No code-page-dependent API is involved.
- **Arguments**: `GetCommandLineW()` is split by a small splitter (whitespace, double quotes). This
  avoids `CommandLineToArgvW`, which would load shell32.dll, and keeps the file independent of the
  `std.process` argument APIs, which changed in recent Zig releases. It is deliberately simpler than
  ta.c's `TA_NOCRT` splitter: whitespace and double quotes only, no backslash escapes and no
  doubled `""` inside quotes.
- **Timing**: `QueryPerformanceCounter` and `GetSystemTimePreciseAsFileTime` are read at the first
  statement of `main`. `GetProcessTimes` (creation time) is read only when the report is formatted,
  as in the C reference. Each phase is printed as creation→entry (wall clock) plus the QPC delta since
  entry, in µs with one decimal.

## Build flags and why

`build.ps1` runs, in effect:

```
zig build-exe ta.zig ..\common\detached.manifest -target x86_64-windows-gnu -O ReleaseFast -fstrip -fsingle-threaded --subsystem console --name ta-zig -femit-bin=bin\ta-zig.exe
zig cc -target x86_64-windows-gnu -O2 -s -municode -Wall ta-mingw.c ..\common\detached.manifest -lole32 -luuid -o bin\ta-zigcc.exe
```

| Flag | Why |
| --- | --- |
| `ta.zig ..\common\detached.manifest` | Zig compiles a `.manifest` input file into an `RT_MANIFEST` resource with ID 1 and links it in, the same as MSVC `/MANIFEST:EMBED`. No `mt.exe` post-step is needed. The build checks the result with `mt.exe -inputresource:<exe>;#1`. `zig cc` accepts the same input. |
| `-target x86_64-windows-gnu` | Makes the build reproducible on any host. An explicit target also means a **baseline x86-64 CPU** (no `-mcpu=native`), so the exe runs on any x64 machine. `gnu` is Zig's Windows ABI; `ta-zig` does not link libc at all, so the choice only affects `ta-zigcc` (MinGW-w64 + UCRT). |
| `-O ReleaseFast` | Optimizes for speed and turns off runtime safety checks, matching MSVC `/O2`. For reference, without the manifest `ReleaseSmall` is 11,264 bytes and `ReleaseFast` is 14,848 bytes. ReleaseFast is the bench variant because the contract asks for speed, and at this size neither page count nor imports differ. |
| `-fstrip` | No debug info and no PDB; the exe holds only code, data, imports and the manifest. |
| `-fsingle-threaded` | The program creates no threads (COM's own threads are unaffected). Zig drops its thread-local-storage setup and threading support code, which saves about 0.5 KB. |
| `--subsystem console` | Console (CUI) subsystem as decided in DESIGN.md §12. The detached manifest prevents the console window. |
| (no `-lc`) | `ta-zig` links no C runtime. Its imports are only `ntdll` (Zig start code: `RtlExitUserProcess`), `kernel32` and `ole32`. |
| `zig cc -O2 -s` | The same optimization level as MSVC `/O2`, with symbols stripped. |
| `zig cc -municode` | `ta.c`'s CRT entry point is `wmain`, and MinGW needs `-municode` to use it. |
| `zig cc -lole32 -luuid` | COM imports. `-luuid` is harmless: every GUID the program uses is defined in the source. |

Source-level choices that affect the binary:

- **No large stack frames.** The 4 KB UTF-8 staging buffer and the argument storage are static (and
  zero-initialised, so they live in `.bss`). A frame over one page makes LLVM emit a `__chkstk`
  stack probe, which pulled all of Zig's `compiler_rt` (about 45 KB of `.text`) into the first
  build.
- **Zero-initialised globals.** The text buffers take the std handle id as a `comptime` parameter
  rather than a field, so the whole global is zero and goes to `.bss` instead of 64 KB of `.data`.
- **`noinline` flush.** The UTF-8 encoder and write path are kept out of every append call site,
  which ReleaseFast would otherwise inline many times over.
- **`pub fn main() u8` without parameters** is Zig 0.17's lightest start path: no allocator, no
  `std.Io`, no environment map. A test exe with a hand-written `wWinMainCRTStartup` came out the same size as
  one using `main`.

## ta-zigcc and the `ta-mingw.c` wrapper

`ta.c` is compiled unchanged. With `<initguid.h>` in effect, MinGW-w64's `<mmdeviceapi.h>` already
*defines* `CLSID_MMDeviceEnumerator` and `IID_IMMDeviceEnumerator`. The Windows SDK header only declares
them, which is why `ta.c` defines them, and here that would be a redefinition error. `ta-mingw.c`
includes the headers first, then renames `ta.c`'s two GUID objects with `#define`, and then does
`#include "../c/ta.c"`. The values are identical, and `ta.c` stays the single source of truth.
Differences from `ta-c.exe`:

- MinGW CRT startup instead of the MSVC CRT.
- UCRT is linked dynamically through the `api-ms-win-crt-*` API sets (part of Windows 10+, nothing to
  redistribute), so there are 9 imported DLLs instead of 2.
- The usage line still says `ta-c`, because it comes from `ta.c`.

## Building

Prerequisites:

- PowerShell 7.
- Zig **0.17.0** (`winget install zig.zig`). `build.ps1` finds `zig.exe` from `-Zig <path>`,
  `$env:ZIG`, `PATH` or the WinGet package directory.
- `bench/c/ta.c` for the secondary exe (it is skipped with a warning if missing).
- Optional, used only for verification: the Windows SDK `mt.exe` (manifest extraction) and MSVC
  `dumpbin.exe` (import list, found with vswhere). Without them the script falls back to a byte scan
  for the manifest. It always parses the PE header itself for the subsystem check.

```powershell
pwsh -File bench\zig\build.ps1                 # both executables
pwsh -File bench\zig\build.ps1 -Only ta-zig    # primary only
```

The script is non-interactive and idempotent. It deletes and rebuilds `bin\<name>.exe`, keeps Zig's
per-compilation cache in `obj\zig-cache` (`--cache-dir` for `build-exe`, `ZIG_LOCAL_CACHE_DIR` for
`zig cc`; the prebuilt MinGW CRT stays in Zig's global cache), refuses any Zig other than 0.17.x,
checks subsystem = 3 and the detached manifest, and prints a size/imports
table, for example:

```
Exe          Bytes Subsystem   Manifest          Imports
ta-zig.exe   16384 3 (console) detached (mt.exe) ntdll.dll, KERNEL32.dll, ole32.dll
ta-zigcc.exe 88064 3 (console) detached (mt.exe) ole32.dll, api-ms-win-crt-*.dll (7), KERNEL32.dll
```

## Results

_Measured 2026-10-04 by `bench/run-bench.ps1` on the reference machine (7950X, Windows 11 26300.9550, Defender on). Full tables, the method and the anomalies are in [`bench/RESULTS.md`](../RESULTS.md); raw data in [`bench/results/`](../results/)._

Cells are mean ± σ (median), ms. hyperfine `-N`, warm-up 10, 200 runs × 3 rounds (toggle: warm-up 2 + 20 runs).

| Scenario | ta-zig.exe (ms) | ta-zigcc.exe (ms) | ta-c.exe (ms, reference) |
| --- | --- | --- | --- |
| no args (runtime floor) | 7.05 ± 0.53 (6.97) | 7.58 ± 0.51 (7.51) | 7.52 ± 0.80 (7.38) |
| `get` | 13.50 ± 0.81 (13.31) | 13.60 ± 0.69 (13.47) | 13.94 ± 0.76 (13.81) |
| `list` | 17.78 ± 1.32 (17.52) | 18.26 ± 1.94 (17.81) | 17.57 ± 1.01 (17.36) |
| `set <current default>` (no-op) | 38.90 ± 3.42 (38.24) | 38.32 ± 2.54 (37.93) | 38.25 ± 2.53 (37.81) |
| `toggle` (real switch) | 40.87 ± 2.43 (40.81) | 40.92 ± 2.36 (41.05) | 41.83 ± 2.83 (41.59) |
| `create_to_entry` median (µs, `--timing` `entry`) | 5,836 | 5,672 | 6,044 |
| cold first run (fresh copy, `list`, ms) | 33.7 / 36.4 / 31.7 | 35.4 / 37.2 / 37.3 | 43.3 / 41.6 / 49.9 |
| Binary size (bytes) | 16,384 | 88,064 | 116,224 |

Both Zig builds are within noise of the MSVC reference in every scenario. `ta-zig` (no libc, 16 KB) has the same runtime floor as `ta-c-nocrt`, about 7.0 ms. Both passed the correctness gate, with `list` byte-identical to C.

## Known limitations

- **Tied to Zig 0.17.** Zig's language and std change between releases. `ta.zig` keeps its std
  usage minimal (`std.debug.assert` at comptime and `std.math.maxInt`) and declares all Win32/COM
  types itself, but spellings such as `callconv(.winapi)`, `@splat` initialisers and
  `-fsingle-threaded` are 0.17 syntax.
- **No console window only on Windows 11 24H2+.** Older Windows ignores `consoleAllocationPolicy`,
  so a launch from a GUI process briefly shows a console there.
- **Simplified argument splitting.** Quotes group characters but backslash escapes are not
  interpreted. Endpoint ids contain neither, so this does not matter here. More than 8 positional
  arguments, or more than 4096 UTF-16 units of argument text (including one terminator per
  argument), is a usage error (exit 1); parsing stops at the buffer end instead of writing past it.
- **Case-insensitive id matching.** `toggle` compares the current default with `idA` ignoring ASCII
  case, the same as the C reference's `CompareStringOrdinal(..., TRUE)`.
- **Mutating commands not exercised by the implementer.** Only `list`, `get`, `set <current default>`
  (a no-op re-assert), the usage path and `--timing` were run while building. A real `toggle` is
  left to the measuring agent.
- **Correction to the research notes.** `PROPVARIANT` is 24 bytes on x64, not 16 as stated in
  benchmark-method.md §0. The `pwszVal` member is at offset 8, as stated there, and the comptime
  size assert guards the declaration.
