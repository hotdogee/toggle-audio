# ta-rs: Rust bench implementation

`ta-rs.exe` is the Rust entry in the toggle-audio benchmark. It implements the common bench CLI contract (`docs/research/benchmark-method.md` section 1, with the console-subsystem change from `docs/DESIGN.md` section 12) using the [`windows`](https://crates.io/crates/windows) crate, which is the same crate the product uses. Every bench implementation (C, Rust, C#, Go, Zig) does the same COM work, so the timings compare the language runtimes and toolchains rather than different algorithms.

This crate stands alone as a Cargo workspace. It has its own `Cargo.lock` and release profile, its `Cargo.toml` contains an empty `[workspace]` table, and the root workspace lists it in `exclude`. It is not fully independent of the repository, though: rustup applies the repository's `rust-toolchain.toml` (stable, with clippy and rustfmt) to it, and Cargo also reads the repository root's `.cargo\config.toml` (see [Build](#build)).

## Command-line contract

| Command | Output (stdout, UTF-8, `\n`) | Exit |
| --- | --- | --- |
| `ta-rs list` | One line per ACTIVE render endpoint, in enumeration order: `<id>\t<name>\t<flags>`. `flags` is `*` for the default (eConsole) device, `c` for the default communications device, `*c` when it is both, and `-` when it is neither. | 0 |
| `ta-rs get` | `<id>\t<name>` of the eConsole default | 0, or 4 if there is no default |
| `ta-rs set <id>` | Nothing. Resolves the endpoint with `GetDevice` and requires a render endpoint (`eRender`) in `DEVICE_STATE_ACTIVE` (exit 3 otherwise). Then calls `SetDefaultEndpoint` for eConsole, eMultimedia and eCommunications, in that order, even when the device is already the default. | 0 |
| `ta-rs toggle <idA> <idB>` | The target id. The target is B if the eConsole default is A, and A in every other case. The target then goes through the same checks and three role calls as `set`. | 0 |
| `ta-rs` (no arguments, or bad arguments) | One usage line on stderr. No COM work happens, so this measures the runtime floor. | 1 |
| `--timing` (allowed anywhere) | Adds `phase\t<name>\t<µs since process creation>` lines on stderr for `entry`, `com_init`, `enumerator`, `work_done` and `exit`. stdout does not change. | unchanged |

Exit codes:
- 0: OK
- 1: usage error
- 2: a COM call failed. stderr gets `error: <call> hr=0xXXXXXXXX`.
- 3: the device was not found, is a capture (recording) endpoint, or is not active
- 4: there is no default device

Names are `PKEY_Device_FriendlyName`, the exact string that Windows Sound settings shows (for example `喇叭 (FiiO BTA30 PRO)`, bytes `E5 96 87 E5 8F AD ...`).

## How the COM calls are made

The `windows` crate generates typed wrappers for the documented interfaces. Every interface is a smart pointer, and its `Drop` calls `Release`.

| Step | Call |
| --- | --- |
| Apartment | `CoInitializeEx(COINIT_APARTMENTTHREADED \| COINIT_DISABLE_OLE1DDE)`, wrapped in `ComApartment`, whose `Drop` calls `CoUninitialize`. `RPC_E_CHANGED_MODE` is accepted and is then not uninitialised. |
| Enumerator | `CoCreateInstance::<IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER)` |
| Enumerate | `EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)`, then `GetCount` and `Item(i)` |
| Id | `IMMDevice::GetId`. The `CoTaskMemAlloc` string is owned by `CoTaskString`, whose `Drop` calls `CoTaskMemFree`. |
| Name | `OpenPropertyStore(STGM_READ)` and `GetValue(&PKEY_Device_FriendlyName)`. `pwszVal` is read only when `vt == VT_LPWSTR`, and `PropVariantClear` runs right after. propsys.dll is not used. |
| Default | `GetDefaultAudioEndpoint(eRender, role)`. `E_NOTFOUND` (`0x80070490`) means there is no default. |
| Validate | `GetDevice(id)` (`E_NOTFOUND`/`E_INVALIDARG` lead to exit 3), then `cast::<IMMEndpoint>()` and `GetDataFlow() == eRender`, then `GetState() == DEVICE_STATE_ACTIVE`. `GetDevice` also resolves capture ids, and `SetDefaultEndpoint` would switch the default recording device for them, so the flow check is needed. `GetDevice` also succeeds for unplugged and disabled endpoints, which is why the state check is needed. |
| Set | Undocumented `IPolicyConfig` `{f8679f50-850a-41cf-9c72-430f290290c8}` on `CPolicyConfigClient` `{870af99c-171d-4f9e-af0d-e63df40c2bc9}`, declared with `#[windows_core::interface]`. `SetDefaultEndpoint` is vtable slot 13, verified against AudioSes.dll PDB symbols. If that CLSID/IID cannot be created, `IPolicyConfigVista` `{568b9108-44bf-40b4-9006-86afe5b5a620}` on `{294935ce-f637-4e7c-a41b-ab255460b862}` is tried, where the slot is 12. See `src/policy.rs`. |

Release order before `CoUninitialize`:
- The borrow checker enforces it for the enumerator: `AudioSystem<'com>` holds a `&'com ComApartment`.
- Every other interface (`IMMDevice`, `IMMDeviceCollection`, `IMMEndpoint`, `IPropertyStore`, `IPolicyConfig`) is a local inside an `AudioSystem` method and is dropped before that method returns. None of them is returned from `src/audio.rs`; callers only get the plain-data `EndpointId` and `Endpoint`. `PolicyConfig::create` takes a `&ComApartment`.

Every `HRESULT` is checked, and failures carry the name of the call that failed (`src/error.rs`).

Output goes straight to the standard handles (`src/output.rs`):
- Redirected handles (file, pipe, NUL) get the UTF-8 bytes via `WriteFile`.
- A real console (`GetConsoleMode` succeeds) gets UTF-16 via `WriteConsoleW`, so CJK names display correctly on the code-page-950 console.

Nothing depends on the ANSI or console code page. `std::io::stdout` and `println!` are not used. Output is built in a `String` with `push_str`, and `format!` appears only on the error and `--timing` paths.

Arguments come from `std::env::args_os()`, which parses `GetCommandLineW` in std with no `shell32`/`CommandLineToArgvW`. They are converted to UTF-16 losslessly with `OsStrExt::encode_wide`. Id comparisons ignore ASCII case. The id passed to `SetDefaultEndpoint` (and printed by `toggle`) is the canonical one returned by `GetId`.

## Build flags and why

`Cargo.toml` `[profile.release]`:

| Setting | Why |
| --- | --- |
| `opt-level = 3` | Optimise for speed. Size is secondary; the loader and COM dominate startup. |
| `lto = "fat"` | Whole-program LTO across std and the `windows` crate. It inlines the thin wrappers and drops unused code. |
| `codegen-units = 1` | LLVM sees the whole crate at once, which gives the best inlining and the smallest output. |
| `panic = "abort"` | No unwind tables or landing pads. Runtime errors never panic; they map to exit codes. |
| `strip = true`, `debug = false` | No symbols or debug info in the exe. |
| `incremental = false` | Reproducible, fully optimised release builds. |

`.cargo/config.toml` (`x86_64-pc-windows-msvc`):

| Flag | Why |
| --- | --- |
| `-C target-feature=+crt-static` | Links the C runtime statically, so the exe does not need the VC++ redistributable (`vcruntime140.dll`). |
| `--cfg windows_slim_errors` | A `windows-rs` option. `windows::core::Error` stores only the HRESULT and does not capture `IErrorInfo`. This removes the `oleaut32.dll` import (`GetErrorInfo`), leaving one less DLL to map at startup. We only print HRESULTs anyway. |

`Cargo.toml` `[lints.clippy]` enables `pedantic` (plus `undocumented_unsafe_blocks`, `unwrap_used`, `expect_used`), so the two targeted `#[allow]`s on lossless-in-practice casts in `src/timing.rs` are meaningful. `cargo clippy --all-targets -- -D warnings` is clean.

`build.rs` passes these linker arguments:

| Linker argument | Why |
| --- | --- |
| `/MANIFEST:EMBED`, `/MANIFESTINPUT:<repo>\bench\common\detached.manifest` | Embeds the shared bench manifest as `RT_MANIFEST` #1. The manifest sets `consoleAllocationPolicy=detached`, `asInvoker` and supportedOS Windows 10/11. |
| `/MANIFESTUAC:NO` | Stops the linker from adding a second `trustInfo`; ours already declares `asInvoker`. |

The exe is console subsystem (there is no `#![windows_subsystem]` attribute):
- Shells wait for it, and redirection and capture work, including PowerShell `$x = & ta-rs.exe list`.
- On Windows 11 24H2+, a launch from a GUI process (G HUB, Explorer) gets no console window because of the manifest.

Dependencies are `windows = "=0.62.2"` and `windows-core = "=0.62.2"`, both pinned exactly:
- `#[interface]` expands to `::windows_core` paths, so `windows-core` must be a direct dependency.
- `windows-core` 0.100 has no matching `windows` release.

Only the needed Win32 features are enabled; each one is commented in `Cargo.toml`.

## Prerequisites

- Rust stable with the `x86_64-pc-windows-msvc` toolchain. Edition 2024 needs Rust 1.85 or later; 1.99.0 was used.
- The MSVC linker: Visual Studio 2022 or Build Tools with the C++ workload. Cargo finds `link.exe` itself, so vcvars is not needed.
- PowerShell 7 for `build.ps1`.
- Optional: the Windows SDK and the MSVC tools (already required for the linker). `build.ps1` uses `mt.exe` and `dumpbin.exe` to confirm the manifest, subsystem and imports.

## Build

```powershell
pwsh -NoProfile -File bench\rust\build.ps1
```

`build.ps1` does the following:
1. Refuses to run when `RUSTFLAGS` or `CARGO_ENCODED_RUSTFLAGS` is set, because either one replaces the config-file flags.
2. Runs `cargo build --release --locked --target-dir bench\rust\target --message-format=json-render-diagnostics` from `bench\rust`. Cargo reads `.cargo\config.toml` from the current directory and every parent directory. Run from `bench\rust`, both `bench\rust\.cargo\config.toml` (`windows_slim_errors` and `+crt-static`) and the repository root's `.cargo\config.toml` (`+crt-static` again, which is harmless) apply. Built from elsewhere with `--manifest-path`, `windows_slim_errors` is lost (the `oleaut32.dll` import comes back), and outside the repository `+crt-static` is lost too. The pinned `--target-dir` keeps `CARGO_TARGET_DIR` or a user-level `build.target-dir` from redirecting the output.
3. Takes the exe path from Cargo's `compiler-artifact` JSON message rather than assuming `target\release`, so `CARGO_BUILD_TARGET` (output in `target\<triple>\release`) cannot make it copy a stale binary.
4. Deletes `bin\ta-rs.exe` and copies the fresh exe there, so a failed copy never leaves an old binary behind.
5. Checks the result: the embedded manifest must contain `consoleAllocationPolicy` = `detached` (`mt.exe`), the PE subsystem must be 3, Windows CUI (`dumpbin /headers`), and there must be no `vcruntime140.dll`, `ucrtbase.dll`, `api-ms-win-crt-*` or `oleaut32.dll` import (`dumpbin /dependents`). `mt.exe` is found under the newest Windows 10/11 SDK, and `dumpbin.exe` under the newest MSVC toolset of the VS install that vswhere reports (or the default VS 2022 folders). A missing tool produces a warning; a failed check fails the build.
6. Prints the size.

Manual equivalent and checks:

```powershell
cd bench\rust
cargo build --release --locked
cargo test                       # parser, toggle decision, exit codes, list flags
cargo clippy --all-targets -- -D warnings
dumpbin /headers target\release\ta-rs.exe | findstr /i subsystem      # 3 subsystem (Windows CUI)
mt -nologo "-inputresource:target\release\ta-rs.exe;#1" -out:check.manifest
```

Build facts on the reference machine (rustc 1.99.0, windows 0.62.2):
- Size: 250,368 bytes.
- Imports: `kernel32`, `ole32`, `combase`, `ntdll` and `api-ms-win-core-synch-l1-2-0` (std's `WaitOnAddress`).
- No `vcruntime140.dll`, `oleaut32.dll` or `propsys.dll`.

## Results

_Measured 2026-10-04 by `bench/run-bench.ps1` on the reference machine (7950X, Windows 11 26300.9550, Defender on). Full tables, the method and the anomalies are in [`bench/RESULTS.md`](../RESULTS.md); raw data in [`bench/results/`](../results/)._

Method: hyperfine `-N`, warm-up 10, 200 runs × 3 rounds (toggle: warm-up 2 + 20 runs). Times in ms; each cell is mean ± σ (median, min).

| Scenario | Command | ta-rs | C reference (`ta-c`) | Δ mean vs C |
| --- | --- | --- | --- | --- |
| runtime-only | `ta-rs.exe` (exit 1) | 8.08 ± 0.90 (7.96, min 6.35) | 7.52 ± 0.80 (7.38, min 6.18) | +0.56 |
| list | `ta-rs.exe list` | 17.19 ± 1.03 (16.99, min 15.67) | 17.57 ± 1.01 (17.36, min 15.97) | -0.38 |
| get | `ta-rs.exe get` | 13.57 ± 0.72 (13.40, min 12.36) | 13.94 ± 0.76 (13.81, min 12.66) | -0.37 |
| set-noop | `ta-rs.exe set <PG42UQ id>` | 38.87 ± 2.83 (38.54, min 33.24) | 38.25 ± 2.53 (37.81, min 33.24) | +0.61 |
| toggle | `ta-rs.exe toggle <PG42UQ id> <PHL BDM4065 id>` | 43.79 ± 2.74 (43.52, min 38.68) | 41.83 ± 2.83 (41.59, min 37.57) | +1.96 |
| cold first run | fresh copy (new hash), one `list` launch | 530 / 457 / 501 | 43 / 42 / 50 | see note |

Phase medians in µs since process creation, over 30 `--timing` runs launched from pwsh `Process.Start`:

| Command | entry (create→entry) | com_init | enumerator | work_done | exit |
| --- | --- | --- | --- | --- | --- |
| `list` | not captured (the harness captures `set` only) | | | | |
| `set` (no-op) | 5,692 | 7,669 | 9,766 | 34,037 | 35,148 |
| `set` (no-op), `ta-c` for comparison | 6,044 | 8,107 | 10,266 | 36,727 | 37,784 |

- ta-rs is within noise of C in every warm scenario; one implementation's median moves by up to about 2 ms from round to round. Its runtime floor is about 0.5 ms above C's.
- **Cold-start anomaly:** each fresh copy of `ta-rs.exe` (new hash) took about 0.46–0.6 s on its first launch, in 5 of 5 tries. The second launch took 9–10 ms. The other native exes took 33–68 ms on their first launch, including the product `toggle-audio.exe` (64–69 ms), which is also Rust and uses the same `windows` crate. This looks like a deeper Defender scan of this particular image. It could not be investigated further without admin. It happens once per new binary, not on every toggle.
- Passed the correctness gate: a real toggle verified with AudioDeviceCmdlets, `list` byte-identical to C, `get`, a bogus id (exit 3) and the usage error (exit 1).

## Known limitations

- **IPolicyConfig is undocumented.** It works on Windows 7 through Windows 11 26H2 (10.0.26300), and every audio switcher relies on it, but Microsoft could change it. Failing HRESULTs are reported, not hidden.
- **The no-console-window behaviour needs Windows 11 24H2+.** Earlier Windows versions ignore `consoleAllocationPolicy`, so a launch from G HUB or Explorer would flash a console window. The product ships a GUI-subsystem twin for those versions; this bench binary does not.
- If the launcher passes `CREATE_NEW_CONSOLE` explicitly, a console can still appear. The policy only changes the default.
- **`set` always makes three `SetDefaultEndpoint` calls**, even for roles that already point at the target. The contract requires this so that the set-noop scenario measures them. The product skips roles that are already set.
- **`toggle` decides from the eConsole default.** It does not fall back to B when A is unplugged: an inactive target exits with code 3, as in the contract.
- **`create_to_entry` uses two clocks.** It is `GetSystemTimePreciseAsFileTime` at entry minus the creation time from `GetProcessTimes`, so it includes kernel process creation, image mapping and any Defender scan, not just Rust startup. Treat negative values, or values clustered on ~15.6 ms steps, as unreliable. All other phases add a QPC delta to it.
- **The three `SetDefaultEndpoint` calls are not atomic.** If the eMultimedia or eCommunications call fails, the process exits 2 after the earlier roles were already switched, leaving the roles split between two devices. The stderr message names the roles that were already switched, for example `error: SetDefaultEndpoint(eMultimedia) (eConsole was already switched) hr=0x...`. Running `set` again repairs it.
- **Only render endpoints.** Capture devices are out of scope. `set` and `toggle` reject a capture id with exit 3 (`error: not a render device: <id>`).
- **Build location matters.** Run Cargo from `bench\rust` (or use `build.ps1`): `windows_slim_errors` lives only in `bench\rust\.cargo\config.toml`, and an environment `RUSTFLAGS` or `CARGO_ENCODED_RUSTFLAGS` overrides all config-file flags.
