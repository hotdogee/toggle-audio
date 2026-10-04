# ta-cs: C# .NET 9 NativeAOT benchmark implementation

`ta-cs.exe` is the C# entry in the toggle-audio benchmark. It implements the common bench CLI
(`docs/research/benchmark-method.md` section 1, with the console-subsystem deviation from
`docs/DESIGN.md` section 12) and is compiled ahead of time by NativeAOT into one native x64
executable. It needs no .NET runtime on the target machine and has no JIT.

```
ta-cs list                  # "<id>\t<name>\t<flags>" per ACTIVE render endpoint; flags: * default, c communications, *c both, - neither
ta-cs get                   # "<id>\t<name>" of the default (eConsole) endpoint
ta-cs set <id>              # check the endpoint exists and is ACTIVE, then SetDefaultEndpoint for eConsole, eMultimedia, eCommunications
ta-cs toggle <idA> <idB>    # if the default is idA set idB, otherwise set idA; prints the target id (canonical form, from IMMDevice::GetId)
ta-cs                       # usage line on stderr, exit 1, no COM work (runtime floor)
--timing                    # anywhere: "phase\t<name>\t<us since process creation>" lines on stderr
```

Exit codes: 0 OK, 1 usage, 2 COM failure (`error: <step> hr=0xXXXXXXXX` on stderr),
3 device not found or not active, 4 no default device.

## Source layout

| File | Contents |
| --- | --- |
| `Program.cs` | `Main`, argument parsing, the four commands, error reporting |
| `CoreAudio.cs` | GUIDs, constants, raw vtable wrappers for `IMMDeviceEnumerator`, `IMMDeviceCollection`, `IMMDevice`, `IPropertyStore`, `IPolicyConfig`, and small helpers (`GetId`, `GetFriendlyName`, `TryGetDefaultId`, `CreatePolicyConfig`) |
| `Native.cs` | `LibraryImport` declarations (ole32, kernel32), `PROPVARIANT`, `PROPERTYKEY` |
| `Output.cs` | `TextBuffer`: UTF-16 buffer flushed once as UTF-8 (`WriteFile`) or UTF-16 (`WriteConsoleW`) |
| `Timing.cs` | `--timing` phase stamps |
| `ta-cs.csproj` | NativeAOT and trimming settings (explained below) |
| `build.ps1` | Non-interactive build that produces `bin\ta-cs.exe` |
| `global.json` | Pins the .NET SDK (9.0.318, latest patch of that band) |

## How COM is called

NativeAOT does not support the built-in COM interop (`[ComImport]` interfaces, RCWs), so ta-cs
calls COM the way C does:

1. `CoCreateInstance` is a `LibraryImport` from ole32.dll that returns a raw `void*` interface
   pointer.
2. A method call reads the vtable (`*(void***)obj`), takes the function pointer at the method's
   0-based slot (IUnknown occupies 0 to 2) and calls it through an unmanaged function pointer
   with the object as the first argument:

   ```csharp
   // IMMDeviceEnumerator::GetDefaultAudioEndpoint, slot 4
   ((delegate* unmanaged[Stdcall]<void*, int, int, void**, int>)Vtbl.Slot(self, 4))(self, flow, role, &device);
   ```

3. `Release` (slot 2) runs in a `finally` for every pointer obtained, strings from
   `IMMDevice::GetId` are freed with `CoTaskMemFree`, and every `PROPVARIANT` is cleared with
   `PropVariantClear`. All objects are released before `CoUninitialize`.

There is no marshalling anywhere: every P/Invoke signature is blittable,
`[assembly: DisableRuntimeMarshalling]` is set, and BOOL results are declared as `int`. The
calls are bound statically through `<DirectPInvoke>`, so `CoCreateInstance` and the other
functions appear in the PE import table instead of being resolved with
`LoadLibrary`/`GetProcAddress` on first use.

Slots used: `IMMDeviceEnumerator` EnumAudioEndpoints=3, GetDefaultAudioEndpoint=4,
GetDevice=5; `IMMDeviceCollection` GetCount=3, Item=4; `IMMDevice` OpenPropertyStore=4,
GetId=5, GetState=6; `IPropertyStore` GetValue=5; undocumented `IPolicyConfig`
(`{F8679F50-...}` on CLSID `{870AF99C-...}`) SetDefaultEndpoint=13, with a fallback to
`IPolicyConfigVista` (`{568B9108-...}` on CLSID `{294935CE-...}`) slot 12. The names are
`PKEY_Device_FriendlyName` (`{A45C254E-...}`, pid 14), the string Windows Sound settings shows.

### COM apartment

The NativeAOT startup code initializes COM on the main thread before `Main` runs (MTA by
default, STA when `Main` has `[STAThread]`). This cannot be turned off from the project file.
ta-cs uses `[STAThread]` so the apartment matches the other implementations (STA). Its own
`CoInitializeEx(COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE)` then returns `S_FALSE` and
is paired with `CoUninitialize`. As a result:

- the real COM initialization cost is inside `create_to_entry` (runtime startup), and the
  `com_init` phase is only a few microseconds;
- even the no-argument usage path pays for COM initialization, because the runtime does it
  before `Main`.

`RPC_E_CHANGED_MODE` is handled as the contract requires: it is treated as success and is not
paired with `CoUninitialize`.

### Output

`Console.Out` encodes with the console output code page (950 on this zh-TW machine), which
would mangle `喇叭 (FiiO BTA30 PRO)`. ta-cs collects output in a UTF-16 buffer and writes it
once at exit with `GetStdHandle` and:

- `WriteFile` of UTF-8 bytes (no BOM, `\n` line endings) when the handle is redirected (pipe,
  file, NUL). This is what the benchmarks and the correctness gate see.
- `WriteConsoleW` when the handle is a real console (`GetConsoleMode` succeeds), so CJK
  names display correctly in a terminal whatever the code page.

Friendly names have `\t`, `\r` and `\n` replaced with a space, so a renamed endpoint cannot
break the one-line, tab-separated format (the same rule as the C reference). For `set` and
`toggle`, the id passed to `SetDefaultEndpoint` and printed by `toggle` is the canonical one
read back with `IMMDevice::GetId` from the validated device, not the argument as typed
(`GetDevice` matches ids case-insensitively).

`Console.OutputEncoding` is never set, because that would call `SetConsoleOutputCP` on the
parent's console. `Main(string[] args)` receives the arguments from the NativeAOT bootstrapper.
No `CommandLineToArgvW` or shell32 is involved.

### --timing

The first statements of `Main` read `QueryPerformanceCounter` and
`GetSystemTimePreciseAsFileTime`. At exit, `GetProcessTimes` provides the creation time, and
each phase is printed as `create_to_entry + (QPC - QPC_entry)` in microseconds with one
decimal, so `entry` is the create-to-entry interval (loader plus NativeAOT runtime
startup, which includes COM initialization, see above). Phases, in order: `entry`,
`com_init`, `enumerator`, `work_done` (after list/get/set/toggle, including the release of
the command's own objects; stamped only when the command succeeded, as in the C reference),
`exit` (after stdout is written, before the stderr write). All lines are buffered and written
in one call at the end. Repeating `--timing` has no extra effect.

## Build

Prerequisites:

- Windows 10/11 x64
- .NET 9 SDK 9.0.318 or a later 9.0.3xx patch (pinned by `global.json`). The ILCompiler pack
  and the runtime pack are pinned to 9.0.20 in `ta-cs.csproj` (`TaIlcVersion`), so a newer
  SDK still generates the same code; NuGet downloads 9.0.20 if it is not cached. To move to a
  newer toolchain, bump `global.json` and `TaIlcVersion` together and re-measure.
- Visual Studio 2022 or Build Tools with the "Desktop development with C++" workload
  (NativeAOT links with the MSVC linker and the Windows SDK; tested with MSVC 14.44 and
  SDK 10.0.26100)
- PowerShell 7

```powershell
pwsh -NoProfile -File bench\csharp\build.ps1
```

The script:

1. Prepends `C:\Program Files (x86)\Microsoft Visual Studio\Installer` to `PATH`. NativeAOT
   finds the linker through `vcvarsall.bat`, which calls `vswhere.exe` unqualified. Without
   this directory on `PATH`, vswhere's error text ends up in the linker path and the link step
   fails with MSB3073.
2. Runs `dotnet publish ta-cs.csproj -c Release -r win-x64 -o obj\publish` from the project
   folder (so `global.json` applies), after printing the SDK and ILCompiler versions.
3. Copies the exe to `bin\ta-cs.exe`, but only when its SHA-256 differs from the existing file.
4. Extracts the embedded manifest with `mt.exe -inputresource:...;#1` and checks it for
   `consoleAllocationPolicy`. If the manifest is missing, it embeds
   `..\common\detached.manifest` with `mt.exe -outputresource`. With SDK 9.0.318 the
   manifest comes through `<ApplicationManifest>` and this fallback is not needed.
5. Prints the size and the SHA-256.

It is idempotent and the output is reproducible: rebuilding unchanged sources with the same
toolchain gives a byte-identical exe (verified across incremental and clean `obj\` rebuilds),
and `bin\ta-cs.exe` is then left untouched. ILC itself still runs on every publish
("Generating native code"); only its output is stable. This matters for the benchmark: a new
file hash makes Defender scan the exe again on its next launch (`benchmark-method.md` 1.6).
Intermediate files go to `obj\` (the IL build output is redirected to `obj\out\` so `bin\`
contains only `ta-cs.exe`).

### Project settings and why

| Setting | Why |
| --- | --- |
| `OutputType=Exe` | Console subsystem (PE subsystem 3). Shells wait for the exe and capture its output. |
| `ApplicationManifest=..\common\detached.manifest` | `consoleAllocationPolicy=detached`: on Windows 11 24H2 or later, a launch from G HUB or Explorer creates no console window. The SDK embeds it in the IL assembly and ILC copies the Win32 resources into the native image. The manifest also carries `asInvoker` and `supportedOS`. |
| `PublishAot=true` | Ahead-of-time native code: no JIT, no runtime install, fast startup. |
| `TrimMode=full` | Whole-program trimming (the default under `PublishAot`, stated explicitly). |
| `OptimizationPreference=Speed` | Tells ILC to favour speed over size. This is a startup benchmark. |
| `IlcFoldIdenticalMethodBodies=true` | Merges identical machine code. Smaller image, no runtime cost. |
| `StackTraceSupport=false`, `IlcGenerateStackTraceData=false` | No stack-trace metadata (method names). Smaller image. ta-cs never prints stack traces. |
| `StripSymbols=true` | Native symbols go to a separate `.pdb` instead of the exe. |
| `InvariantGlobalization=true`, `InvariantTimezone=true` | No ICU or time-zone data loading. All formatting in ta-cs is culture-independent anyway. |
| `UseSystemResourceKeys=true` | Exception messages become resource keys instead of embedded English strings. |
| `DebuggerSupport`, `EventSourceSupport`, `MetricsSupport`, `UseNativeHttpHandler`, `HttpActivityPropagationSupport`, `NullabilityInfoContextSupport`, `EnableUnsafeBinaryFormatterSerialization`, `EnableUnsafeUTF7Encoding` = `false` | Feature switches that let the trimmer drop framework code that is never needed here. |
| `BuiltInComInteropSupport=false` | NativeAOT does not support it anyway. COM is called through vtables. |
| `AllowUnsafeBlocks=true` | Needed for pointers and unmanaged function pointers. |
| `DirectPInvoke` ole32, kernel32 | Static imports instead of lazy binding on first call. |
| `[SuppressGCTransition]` on QPC and `GetSystemTimePreciseAsFileTime` | These never block, so the GC mode switch around them can be skipped. This keeps the timing stamps cheap. |
| `[assembly: DisableRuntimeMarshalling]` | Guarantees that no marshalling stubs are generated. |
| `LinkerArg /Brepro` | link.exe writes a content hash instead of the build time into the PE `TimeDateStamp` and debug directory, so the native image is reproducible. `Deterministic=true` alone only covers the IL assembly. |
| `TaIlcVersion=9.0.20` (sets `RuntimeFrameworkVersion` and overrides the SDK's `KnownILCompilerPack` entry) | Pins the code generator and the compiled framework, so the numbers do not shift with whichever SDK is installed. An explicit `PackageReference` to `Microsoft.DotNet.ILCompiler` would also work but triggers a warning from the ILCompiler targets. |

Not used: `OptimizationPreference=Size` would save a few tens of KB at some speed cost.
`IlcInstructionSet` (for example `x86-x64-v3`) is left at the portable default so the exe
runs on any x64 CPU.

## Verification done by the implementer

On Windows 11 10.0.26300, zh-TW, code page 950:

- `dumpbin /headers`: `3 subsystem (Windows CUI)`, machine x64.
- `mt.exe -inputresource:bin\ta-cs.exe;#1`: the manifest contains `consoleAllocationPolicy`
  `detached`, `asInvoker` and `supportedOS`.
- `dumpbin /dependents`: KERNEL32, ole32, ADVAPI32, bcrypt and the UCRT API sets
  (`api-ms-win-crt-*`, which resolve to `ucrtbase.dll`, part of Windows). There is no
  `vcruntime140.dll`, no `coreclr.dll` and no managed DLL next to the exe.
  `dumpbin /imports` lists `CoCreateInstance`, `CoInitializeEx`, `CoTaskMemFree` and
  `PropVariantClear` from ole32 (direct P/Invoke).
- `list` (5 active endpoints, `喇叭` encoded as `E5 96 87 E5 8F AD`, no `\r`), `get`,
  `set <current default id>` (no-op re-assert, exit 0), no arguments (exit 1), unknown id
  (exit 3), a NOTPRESENT id (exit 3, `state=0x00000004`), and `--timing`. PowerShell capture
  (`$x = & ta-cs.exe list`) receives the UTF-8 names correctly.
- Two rebuilds (incremental, and after deleting `obj\`) produce the same SHA-256.
- `toggle` and `set` to a different device were **not** run by the implementer, by rule. The
  measuring agent exercises them.

## Results

To be filled in by the measuring agent (hyperfine `-N --warmup 10 --runs 200`, 3 rounds,
pooled; see `bench/RESULTS.md`).

| Scenario | Mean ± σ (ms) | Median (ms) | Min (ms) | Notes |
| --- | --- | --- | --- | --- |
| no args (runtime floor) | | | | Includes the runtime's own `CoInitializeEx` (before `Main`); not COM-free like the other languages |
| `list` | | | | |
| `get` | | | | |
| `set` (no-op, current default) | | | | |
| `toggle` (real switch) | | | | |

| Item | Value |
| --- | --- |
| Binary size | 1,001,984 bytes (0.96 MB) with SDK 9.0.318 / ILCompiler 9.0.20 |
| `--timing` median `entry` (create to entry, µs) | (includes COM initialization, see "COM apartment") |
| `--timing` median `com_init` / `enumerator` / `work_done` (µs) | (`com_init` is near zero because COM is already initialized; compare `entry` instead) |

## Known limitations

- **Runtime startup cost.** NativeAOT still initializes a GC heap, the type system, the main
  thread and COM before `Main`. Expect the no-argument floor to be several milliseconds above
  the C implementation. The binary is about 1 MB, compared with tens of KB for C.
- **COM is initialized before `Main`** by the runtime, even on the usage path (see "COM
  apartment"). The `com_init` phase therefore does not show the true `CoInitializeEx` cost.
  Compare `entry` instead.
- **No console flash only on Windows 11 24H2 or later.** Earlier Windows versions ignore
  `consoleAllocationPolicy`, so a console window appears when the exe is launched from a GUI
  process. A `WinExe` (GUI subsystem) build would avoid that at the cost of PowerShell output
  capture.
- `IPolicyConfig` is undocumented. The Vista fallback is implemented but untested, because
  the primary interface works on every Windows version from 7 to 11.
- `create_to_entry` relies on the process creation timestamp from `GetProcessTimes`. If the
  values cluster on ~15.6 ms steps or go negative, treat them as unreliable.
- The `toggle` decision reads only the eConsole default, and `set` always writes all three
  roles even when they already match. Both are deliberate, to keep the work identical across
  implementations. The product skips roles that are already set.
