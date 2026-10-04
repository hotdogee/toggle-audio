# Benchmarks: how fast can a default-device toggle be?

This page summarizes the benchmark survey that chose the product's language and console strategy. All numbers come from one run on 2026-10-04, recorded in [`bench/RESULTS.md`](../bench/RESULTS.md) with the raw data in [`bench/results/`](../bench/results/). The tables below are copied from that file, not re-measured. The implementations and the harness are described in [`bench/README.md`](../bench/README.md).

## The question

The proof of concept was a PowerShell script built on the AudioDeviceCmdlets module and packaged with ps2exe as `Switch-Audio.exe`. It took about 900 ms per key press. The survey set out to answer three questions:

1. How much of that time is the job itself, and how much is runtime overhead?
2. Does the choice of native language matter? The candidates were C, Rust, C# NativeAOT, Go and Zig.
3. What does a hotkey launcher such as Logitech G HUB add? In particular, what is the cost of a console window, and can it be avoided without giving up a console-friendly CLI?

## Method

**Implementations.** Each language has one implementation under [`bench/<lang>/`](../bench/), and each is the best-optimized build its toolchain allows. All of them follow the same command-line contract ([`bench/README.md`](../bench/README.md#the-bench-cli-contract)) and do the same COM work: `list`, `get`, `set <id>` (always three `SetDefaultEndpoint` calls) and `toggle <idA> <idB>`. They are console-subsystem exes with the `consoleAllocationPolicy=detached` manifest. C is the speed reference. The product (`toggle-audio.exe`, `toggle-audiow.exe`) and the PowerShell baselines were measured with the same harness.

**Harness.** [`bench/run-bench.ps1`](../bench/run-bench.ps1) runs these steps:

| Step | Tool and settings |
| --- | --- |
| Correctness gate | Every implementation must toggle twice (checked against AudioDeviceCmdlets in Windows PowerShell 5.1, for both the playback and the communications role) and produce byte-identical `list` / `get` output to the C reference, including CJK names. It must also return the right exit codes for a bogus id and a usage error. Timing starts only after this gate passes. |
| Scenarios `noargs`, `list`, `get`, `set-noop` | hyperfine 1.20.0, `-N` (no shell), warm-up 10, **200 runs × 3 rounds** in forward / reverse / forward command order, pooled to n = 600. `noargs` is the runtime floor: a usage error that does no COM work (the product uses `--version`). `set-noop` sets PG42UQ, which already is the default. |
| Real toggle | hyperfine `-N`, warm-up 2, 20 runs per implementation (an even count, so each row ends where it started), between two NVIDIA HDMI outputs (PG42UQ ↔ PHL BDM4065). The default device was verified after every row, and PG42UQ was restored at the end. |
| PowerShell baselines | hyperfine `-N`, warm-up 3, 20 runs × 3 rounds. |
| Phases | 30 runs of `<exe> set <PG42UQ id> --timing`, which prints stamps in µs since process creation. The table gives the medians. |
| Launch styles | `spawnbench` (a small C launcher in [`bench/baseline`](../bench/baseline/README.md)) times `CreateProcessW` to exit with chosen console flags. It is started without a console itself to emulate G HUB. |
| Cold first run | A fresh copy of each exe (a few random bytes appended, so the hash is new) is launched once. The table gives the median of 3. |

**Machine.** AMD Ryzen 9 7950X (16 cores / 32 threads) with 128 GB RAM, running Windows 11 Pro for Workstations 26H2, build 10.0.26300.9550 (zh-TW locale). The power plan was "Ultimate Performance" on AC power. Defender real-time protection was on with no exclusions, Smart App Control was off, and the session was not elevated. Toolchains: MSVC 14.44, rustc 1.99.0 with `windows` 0.62.2, .NET SDK 9.0.318 (NativeAOT), Go 1.27.0, Zig 0.17.0, ps2exe 1.0.17, Windows PowerShell 5.1, PowerShell 7.6.6 and AudioDeviceCmdlets 3.1.0.2.

The product ran with `APPDATA` pointed at a temporary directory that held its own configuration, so no user configuration was read or written.

## Results

All times are wall time in ms from `CreateProcess` to process exit, as measured by hyperfine (which includes its own spawn and wait overhead; see the `nop` rows). Each cell gives **mean ± σ (median)**.

| Implementation | What it is | Size (bytes) | noargs (runtime floor) | list | get | set-noop | toggle (real switch) | cold first run, median of 3 |
| --- | --- | ---: | --- | --- | --- | --- | --- | ---: |
| `nop.exe` | floor: GUI subsystem, no CRT, `ExitProcess(0)` | 2,560 | 3.45 ± 0.49 (3.37) | – | – | – | – | – |
| `nop-con.exe` | floor: console subsystem | 2,560 | 3.53 ± 0.47 (3.43) | – | – | – | – | – |
| `nop-con-detached.exe` | floor: console + detached manifest | 4,096 | 3.47 ± 0.53 (3.39) | – | – | – | – | – |
| **`ta-c.exe`** | **C reference**: MSVC `/MT`, console + detached | 116,224 | 7.52 ± 0.80 (7.38) | 17.57 ± 1.01 (17.36) | 13.94 ± 0.76 (13.81) | 38.25 ± 2.53 (37.81) | 41.83 ± 2.83 (41.59) | 43.3 |
| `ta-c-gui.exe` | C, GUI subsystem | 116,224 | 7.60 ± 0.64 (7.52) | 17.67 ± 1.44 (17.39) | 13.81 ± 0.83 (13.62) | 39.18 ± 3.51 (38.54) | 40.59 ± 1.90 (40.58) | 41.4 |
| `ta-c-md.exe` | C, `/MD` (needs the VC++ redistributable) | 23,040 | 8.04 ± 0.81 (7.88) | 17.37 ± 0.97 (17.13) | 14.00 ± 0.98 (13.80) | 38.70 ± 2.84 (38.27) | 40.68 ± 1.95 (40.90) | 34.4 |
| `ta-c-delayload.exe` | C `/MT`, `ole32.dll` delay-loaded | 119,296 | 4.24 ± 0.60 (4.06) | 17.34 ± 1.11 (17.17) | 13.69 ± 0.76 (13.53) | 38.45 ± 2.65 (38.10) | 42.75 ± 4.44 (41.54) | 41.4 |
| `ta-c-nocrt.exe` | C, no CRT (`/ENTRY`, kernel32 + ole32 only) | 20,480 | 7.06 ± 0.78 (6.96) | 17.31 ± 1.09 (17.08) | 13.48 ± 0.78 (13.31) | 38.37 ± 2.73 (37.93) | 40.01 ± 1.92 (40.17) | 32.8 |
| `ta-rs.exe` | Rust, `windows` crate, crt-static | 250,368 | 8.08 ± 0.90 (7.96) | 17.19 ± 1.03 (16.99) | 13.57 ± 0.72 (13.40) | 38.87 ± 2.83 (38.54) | 43.79 ± 2.74 (43.52) | **500.5** (see anomalies) |
| `ta-cs.exe` | C# .NET 9 NativeAOT | 1,001,984 | 11.83 ± 1.27 (11.63) | 18.95 ± 1.15 (18.71) | 14.87 ± 0.80 (14.73) | 39.25 ± 2.66 (38.91) | 43.59 ± 3.63 (42.78) | 42.8 |
| `ta-go.exe` | Go | 1,530,368 | 6.07 ± 0.64 (5.93) | 19.94 ± 1.62 (19.52) | 15.36 ± 0.80 (15.15) | 40.34 ± 2.70 (39.93) | 42.30 ± 2.64 (41.56) | 62.7 |
| `ta-zig.exe` | Zig (native, no libc) | 16,384 | 7.05 ± 0.53 (6.97) | 17.78 ± 1.32 (17.52) | 13.50 ± 0.81 (13.31) | 38.90 ± 3.42 (38.24) | 40.87 ± 2.43 (40.81) | 33.7 |
| `ta-zigcc.exe` | `ta.c` built by `zig cc` (clang + mingw, UCRT) | 88,064 | 7.58 ± 0.51 (7.51) | 18.26 ± 1.94 (17.81) | 13.60 ± 0.69 (13.47) | 38.32 ± 2.54 (37.93) | 40.92 ± 2.36 (41.05) | 37.2 |
| **`toggle-audio.exe`** | **product** (Rust), console + detached; noargs = `--version`, toggle = configured toggle | 521,216 | 9.78 ± 0.78 (9.67) | 19.74 ± 1.24 (19.43) | 15.51 ± 0.85 (15.32) | 19.54 ± 1.07 (19.38) ¹ | 40.91 ± 4.14 (40.05) | 65.3 |
| `toggle-audiow.exe` | product, GUI subsystem | 521,216 | 9.76 ± 0.67 (9.64) | 19.79 ± 1.27 (19.48) | 15.41 ± 0.88 (15.15) | 19.71 ± 1.10 (19.53) ¹ | 42.63 ± 2.60 (42.17) | 66.0 |
| `bin\ta-ps.exe` | ps2exe x64 + detached manifest (`ta.ps1`, Windows PowerShell 5.1 engine) | 48,128 | 207.12 ± 4.74 (206.07) | 551.07 ± 9.79 (546.25) | – | 278.01 ± 30.97 (273.50) | 319.11 ± 10.29 (322.17) | 659.9 |
| `bin\ta-ps-anycpu.exe` | ps2exe defaults, packaged like the original | 48,128 | – | – | – | 279.80 ± 11.14 (277.36) | – | 674.7 |
| `powershell.exe -File ta.ps1` | Windows PowerShell 5.1 | – | 200.96 ± 2.89 (200.23) | 519.83 ± 9.01 (517.35) | – | 266.96 ± 12.31 (263.79) | 289.49 ± 6.34 (289.10) | – |
| `pwsh -File ta.ps1` | PowerShell 7.6.6 | – | 358.29 ± 9.35 (354.43) | 732.70 ± 10.33 (731.54) | – | 419.34 ± 6.30 (418.59) | 437.97 ± 7.74 (435.32) | – |
| `powershell.exe -NoProfile -Command exit` | empty host start | – | 132.98 ± 3.67 (132.43) | – | – | – | – | – |
| `pwsh -NoProfile -Command exit` | empty host start | – | 198.80 ± 5.52 (197.66) | – | – | – | – | – |
| **original `Switch-Audio.exe`** | **the original proof of concept** (ps2exe of `switch-audio.ps1`); toggles S/PDIF ↔ PG42UQ | 27,648 | – | – | – | – | **887.54 ± 10.16 (887.38)** | – |
| `powershell.exe -File switch-audio.ps1` | the same script, without ps2exe | – | – | – | – | – | 858.50 ± 10.26 (858.05) | – |

¹ The product's `set` skips roles whose default already is the target (`docs/DESIGN.md` section 6), so with PG42UQ already the default it makes **no** `SetDefaultEndpoint` calls. The bench contract's `set` always makes three. The product's set-noop is therefore not comparable with the other rows; compare the toggle column, where every row makes three real calls.

Headline: a real toggle takes **~41 ms with every native implementation, including the product (40.9 ms)**, against **887 ms for the original `Switch-Audio.exe`**, about 22× faster. The native languages differ by about 1–4 ms on the toggle, which is inside the spread of the audio service itself (σ is 2–4 ms).

## Phase breakdown

Medians over 30 runs of `<exe> set <PG42UQ id> --timing`, in ms. `entry` covers process creation, the loader and runtime start-up, and the other columns are the deltas between stamps.

| Implementation | entry | entry → com_init (`CoInitializeEx`) | com_init → enumerator (`CoCreateInstance`) | enumerator → work_done (validate + 3 × `SetDefaultEndpoint`) | work_done → exit (release, `CoUninitialize`, output) | exit stamp |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| ta-c | 6.04 | 1.99 | 2.12 | 26.58 | 1.10 | 37.78 |
| ta-c-gui | 6.09 | 2.08 | 2.27 | 24.96 | 1.03 | 36.43 |
| ta-c-md | 6.01 | 2.06 | 2.13 | 25.01 | 1.08 | 36.24 |
| ta-c-delayload | **3.12** | **4.47** | 2.09 | 23.46 | 1.02 | 34.41 |
| ta-c-nocrt | 5.72 | 1.95 | 2.13 | 24.73 | 1.02 | 35.50 |
| ta-rs | 5.69 | 1.93 | 2.12 | 24.31 | 0.98 | 35.15 |
| ta-cs ² | 8.50 | 0.00 | 2.13 | 23.27 | 0.00 | 34.00 |
| ta-go | 5.07 | 3.98 | 2.15 | 23.77 | 1.05 | 36.05 |
| ta-zig | 5.84 | 1.92 | 2.14 | 25.33 | 1.03 | 36.44 |
| ta-zigcc | 5.67 | 1.97 | 2.10 | 23.34 | 1.01 | 33.82 |
| ta.ps1 (powershell.exe) | 134.57 | 60.49 (Import-Module) | 2.28 | 45.39 | 5.25 | 248.64 |
| ta.ps1 (pwsh) | 194.99 | 123.45 (Import-Module + fallback path) | 2.71 | 42.40 | 5.00 | 369.09 |
| ta-ps.exe (ps2exe) | 159.07 | 47.05 (Import-Module) | 2.40 | 45.39 | 5.48 | 259.83 |

² NativeAOT initializes COM before `Main`, so that cost is inside `entry`, and `com_init` is ~0. Its `exit` is stamped before the runtime's own shutdown.

The product's own phases (`toggle-audio.exe set <PG42UQ> --timing`). PG42UQ already is the default, so no `SetDefaultEndpoint` call happens here:

| Exe | start | start → config_loaded | config_loaded → com_ready (COM init + enumerator) | com_ready → target_chosen | target_chosen → set_done (role check, no set needed) | set_done → end |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| toggle-audio | 6.90 | 0.09 | 3.58 | 0.25 | 4.04 | 0.36 |
| toggle-audiow | 6.89 | 0.09 | 3.60 | 0.26 | 4.06 | 0.41 |

### What dominates

For the C reference (41.8 ms hyperfine wall time for a real toggle), the time splits up as follows:

| Cost | Time | Notes |
| --- | ---: | --- |
| Process creation and teardown | ~3.4 ms | The `nop.exe` floor. |
| Loading `ole32.dll` and its dependencies | ~3.3 ms | `ta-c` noargs 7.52 ms against `ta-c-delayload` 4.24 ms. Delay-loading only moves the cost into `com_init`. |
| `CoInitializeEx` | ~2 ms | |
| `CoCreateInstance(MMDeviceEnumerator)` | ~2 ms | |
| Three `SetDefaultEndpoint` calls | ~23–27 ms | About **8 ms each**, an RPC into the Windows Audio service (AudioSrv). This happens even when the endpoint already is the default: set-noop takes 38 ms against 41 ms for a real toggle. |

So about 24 ms of a 40 ms toggle is spent inside AudioSrv, and most of the rest is process start-up and COM initialization. No language can remove either. The one lever left is the number of `SetDefaultEndpoint` calls. The product skips roles that already point at the target, which is why its `set` to the current default takes 19.5 ms instead of 38 ms. Turning off "Also switch the Communications device" saves about another 8 ms per toggle.

## G HUB launch: console allocation

How a GUI launcher such as G HUB starts the exe matters more than the language. For the "G HUB style" rows, spawnbench itself has no console and starts each child with no creation flags, just like G HUB. A console-subsystem exe without the detached manifest then gets a brand-new console window.

| Launch | Exe | Median | Mean | Min – max | Console window |
| --- | --- | ---: | ---: | --- | --- |
| G HUB style (console-less parent, flags 0) | `nop.exe` (GUI subsystem) | 3.02 | 3.05 | 2.80 – 3.40 | none |
| G HUB style | `nop-con.exe` (console, no manifest) | **152.57** | 153.16 | 146.02 – 166.17 | **new window every launch** |
| G HUB style | `nop-con-detached.exe` (console + `consoleAllocationPolicy=detached`) | **2.81** | 2.97 | 2.62 – 4.60 | none |
| G HUB style | `ta-c.exe get` (console + detached) | 14.14 | 14.19 | 13.73 – 14.83 | none |
| G HUB style | `ta-c-gui.exe get` (GUI) | 14.34 | 14.46 | 13.79 – 15.65 | none |
| G HUB style | `toggle-audio.exe get` (product, console + detached) | 15.80 | 15.90 | 14.79 – 17.09 | none |
| G HUB style | `toggle-audiow.exe get` (product, GUI) | 16.30 | 16.47 | 15.39 – 17.80 | none |
| explicit `CREATE_NEW_CONSOLE` | `nop-con.exe` | 153.36 | 162.72 | 142.60 – 207.26 | new window |
| explicit `CREATE_NEW_CONSOLE` | `nop-con-detached.exe` | 2.90 | 2.95 | 2.67 – 3.36 | none (see observations) |

Inherited console and no-console floors (spawnbench, 200 runs, warm-up 10; median / mean ms): `nop.exe` 2.92 / 3.14 (inherit) and 3.17 / 3.21 (`DETACHED_PROCESS`); `nop-con.exe` 2.86 / 2.95 and 2.74 / 2.82; `nop-con-detached.exe` 2.81 / 2.92 and 2.56 / 2.63.

A plain console exe costs **about 150 ms extra and flashes a window** on every key press (152.57 ms against 2.81 ms). The `consoleAllocationPolicy=detached` manifest removes both, so the exe starts as fast as a GUI-subsystem exe (2.81 against 3.02 ms). Having a manifest costs nothing measurable (`nop-con-detached` 3.47 ms against `nop-con` 3.53 ms under hyperfine). On this build, the detached policy even suppressed an explicit `CREATE_NEW_CONSOLE` (2.90 ms, no window).

The policy only works on Windows 11 24H2 and later, which is why the product also ships `toggle-audiow.exe` (GUI subsystem). Both product exes are equally fast from a G HUB-style launch: 15.80 and 16.30 ms for `get`.

## Per-language notes

| Language | Notes |
| --- | --- |
| **C (MSVC)** | `/MT` (static CRT) is the shippable build, because `/MD` (23 KB) needs the VC++ redistributable and is no faster (8.04 against 7.52 ms noargs). Removing the CRT entirely (`ta-c-nocrt`, 20 KB) saves only about 0.5 ms. Most of the C floor is the load-time import of `ole32.dll` (about 3.3 ms), not the CRT. Delay-loading `ole32` speeds up only the no-COM path, and every real command uses COM. |
| **Rust** | The bench build (`windows` crate, `crt-static`, fat LTO, `panic = "abort"`) is 250 KB, and within noise of C on every command: −0.4 to +0.6 ms on `list`, `get` and set-noop, and +2 ms on the toggle, inside the toggle σ. Its runtime floor is 8.08 ms, about 0.6 ms above C. The product is 521 KB and has a 9.78 ms floor (console setup, argument parsing, JSON configuration, dialog code), and its real toggle (40.05 ms median) matches C. |
| **C# NativeAOT** | This is true AOT: a single 1 MB exe with no managed DLLs and no JIT. The runtime initializes COM **before `Main`**, so `com_init` is ~0 and the cost moves into `entry` (8.5 ms). It has the highest floor of the native builds (11.83 ms, about 8.4 ms above `nop`), but on commands it is only 0.9–1.4 ms behind C. |
| **Go** | Pure Go, with no cgo and COM called through vtables. It is the largest exe (1.5 MB). The Go runtime starts quickly, and because `ole32.dll` is loaded lazily, its no-COM floor (6.07 ms) is *below* C. It pays for that in `com_init` (4.0 against 2.0 ms) and lands 1.4–2.4 ms behind C on `list`, `get` and set-noop, and about 0.5 ms (mean) behind on the toggle. It is built with `GOAMD64=v3`, so the bench binary needs an AVX2 CPU. |
| **Zig** | The native build (`ta-zig`, 16 KB, no libc, imports only `ntdll`, `kernel32` and `ole32`) has the same floor as no-CRT C (7.05 against 7.06 ms). `ta-zigcc` compiles the unchanged C source with `zig cc` (Clang + MinGW-w64, UCRT) and is within 0.9 ms of MSVC in every scenario. |
| **PowerShell / ps2exe** | ps2exe wraps the script in a small .NET Framework exe that hosts the **Windows PowerShell 5.1** engine (never PowerShell 7). An empty host takes 133 ms to start (199 ms for `pwsh`). `Import-Module AudioDeviceCmdlets` adds 47–60 ms. `Get-AudioDevice -List` takes about 300 ms per call because it reads every property of every playback and recording endpoint to find each name, and the original script called it twice. That accounts for the 887 ms. The `SetDefaultEndpoint` calls themselves take ~45 ms, close to native. x64 and AnyCPU ps2exe builds perform the same (273.5 against 277.4 ms set-noop). Under PowerShell 7 everything is about 1.5× slower. |

## Correctness gate

| Implementation | toggle 1 (→ PHL) | toggle 2 (→ PG42UQ) | list flags | list == C | get | bogus id | usage | Result |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| ta-c | ok | ok | ok | yes | ok | 3 | 1 | **PASS** |
| ta-c-gui | ok | ok | ok | yes | ok | 3 | 1 | **PASS** |
| ta-c-md | ok | ok | ok | yes | ok | 3 | 1 | **PASS** |
| ta-c-delayload | ok | ok | ok | yes | ok | 3 | 1 | **PASS** |
| ta-c-nocrt | ok | ok | ok | yes | ok | 3 | 1 | **PASS** |
| ta-rs | ok | ok | ok | yes | ok | 3 | 1 | **PASS** |
| ta-cs | ok | ok | ok | yes | ok | 3 | 1 | **PASS** |
| ta-go | ok | ok | ok | yes | ok | 3 | 1 | **PASS** |
| ta-zig | ok | ok | ok | yes | ok | 3 | 1 | **PASS** |
| ta-zigcc | ok | ok | ok | yes | ok | 3 | 1 | **PASS** |
| toggle-audio (product) | ok | ok | ok | yes | ok | 4 | 2 | **PASS** |
| toggle-audiow (product) | ok | ok | ok | yes | ok | 4 | 2 | **PASS** |
| ta.ps1 (powershell.exe) | ok | ok | ok | yes | ok | 3 | 1 | **PASS** |
| ta.ps1 (pwsh) | ok | ok | ok | yes | ok | 3 | 1 | **PASS** |
| ta-ps.exe | ok | ok | ok | yes | ok | 3 | 1 | **PASS** |
| original Switch-Audio.exe | → S/PDIF ok | → PG42UQ ok | – | – | – | – | – | **PASS** |

## Sanity check against the expected ranges

The survey plan ([`research/benchmark-method.md`](research/benchmark-method.md) section 4) predicted ranges before anything was measured:

| Row | Expected | Measured | Verdict |
| --- | --- | --- | --- |
| `nop.exe` | 1–4 ms | 3.45 ms | in range |
| `nop-con.exe`, inherited console | nop + 0–1 ms | 3.53 ms (+0.08) | in range |
| `nop-con.exe`, new console | +10–100 ms | 153 ms (about +150 ms over `nop-con-detached`) | **above range.** On this machine the new console opens as a Windows Terminal window (seen in the E2E pass), which probably costs more than a bare conhost. It only strengthens the case for the detached manifest |
| C runtime-only (no args) | nop + 0–0.5 ms | 7.52 ms (+4.1) | **above range.** The load-time import of `ole32.dll` costs about 3.3 ms; with ole32 delay-loaded it is +0.8 ms |
| C `list` | 5–15 ms | 17.6 ms | **slightly above range**, below the 40 ms "investigate" mark. All native builds land at 17.2–19.9 ms, so the cost is in the OS (endpoint enumeration and property stores), not the code |
| Rust | C + 0–2 ms | +0.6 (noargs), −0.4 (`list`), +0.6 (set-noop) | in range |
| zig cc | ≈ C | within 0.9 ms in every scenario | in range |
| Go | C + 2–10 ms | −1.5 (noargs, lazy ole32), +2.4 (`list`), +1.4 (`get`), +2.1 (set-noop) | in range for real commands; the floor is lower because ole32 loads lazily |
| NativeAOT | C + 3–15 ms, < 60 ms | +4.3 (noargs), +1.4 (`list`), +1.0 (set-noop) | in range at start-up and faster than expected on commands. It is real AOT: `bin\` holds only `ta-cs.exe` (1 MB), no managed DLLs |
| COM init + enumerator | 1–10 ms | about 4 ms (2 + 2) | in range |
| Enumerate endpoints + names | 1–20 ms | about 3.6 ms (`list` minus `get`) | in range |
| set-noop, 3 × `SetDefaultEndpoint` | 1–30 ms | 23–27 ms | in range, near the top |
| real toggle | 10–100+ ms | 40–44 ms (native) | in range |
| empty host | 150–500 ms | 133 ms (powershell.exe), 199 ms (pwsh) | Windows PowerShell is **slightly faster** than the range |
| ps2exe `Switch-Audio.exe` toggle | 700–1200 ms (about 900 ms seen before) | 887.5 ms | in range; matches the author's earlier observation of about 900 ms |

## Conclusion

- **The operating system sets the floor, not the language.** All ten native builds land within 40.0–43.8 ms for a real toggle and 17.2–19.9 ms for `list`. Those differences are inside the jitter of the audio service (σ 2–4 ms at n = 20).
- **Rust was chosen for the product**, and its real toggle (40.05 ms median) is identical to the C reference (41.59 ms) within noise. That is **about 22× faster** than the original `Switch-Audio.exe` (887.38 ms). Its extra start-up cost over C (about 2 ms on the floor) is about 5% of a toggle and invisible to a user. Rewriting it in C, Zig or a no-CRT build would save at most 2–4 ms of a ~40 ms action dominated by AudioSrv. The decision therefore rests on what the numbers leave open: memory safety around the COM and `unsafe` surface, the `windows` crate, and single-language maintenance of the CLI, dialog, tests and build.
- **Console subsystem plus the detached manifest** is the primary exe. It costs nothing measurable, it gives shells a normal console program, and on Windows 11 24H2+ it never flashes a window from G HUB. `toggle-audiow.exe` covers older Windows versions.
- **The build profile needs no change.** Delay-loading `ole32` would not help, because every real command uses COM. Delay-loading the DLLs that only the settings dialog and the known-folder fallback use (`shell32`, `oleaut32`, `comctl32`) measured about 1.2–1.8 ms faster per run in a later experiment; it is not adopted in 0.1.0.

## How to reproduce

Requirements: PowerShell 7, hyperfine, the toolchains listed in [`bench/README.md`](../bench/README.md#building), AudioDeviceCmdlets in Windows PowerShell 5.1 (used as the oracle), and at least two active playback devices. **The real-toggle scenario changes your default playback device.** The harness restores it in a `finally` block, but the endpoint ids of the toggle pair and of the restore target are constants at the top of `run-bench.ps1`, so set them for your machine first.

```powershell
pwsh -NoProfile -File bench\run-bench.ps1                          # build everything, gate, all scenarios (~17 min)
pwsh -NoProfile -File bench\run-bench.ps1 -SkipBuild -SkipToggle   # never toggles
```

Parameters and their defaults: `-Runs 200 -Warmup 10 -Rounds 3 -ToggleRuns 20 -PsRuns 20 -PsWarmup 3 -TimingRuns 30 -SpawnRuns 10 -ColdCopies 3 -Scenarios noargs,list,get,set-noop -OutDir bench\results`. The switches are `-SkipBuild -SkipGate -SkipToggle -SkipPowerShell -SkipTiming -SkipSpawn -SkipCold`. The harness measures the product from `target\release-build\release\` (`-ProductDir`) and builds it there with `CARGO_TARGET_DIR`. Results go to `bench\results\` as hyperfine JSON, TSV and a generated `summary.json` / `summary.md`.

## Caveats

- **One machine, one run.** Absolute numbers depend on the CPU, the Windows build, the audio drivers and Defender. Treat the ratios and the phase structure as the result, not the milliseconds.
- **Small toggle samples.** n = 20 per toggle row, with σ 2–4 ms, so the order within the native toggle column means nothing. Between rounds, one implementation's median moved by up to 1.8 ms (noargs), 1.5 ms (`list`), 0.6 ms (`get`) and 2.4 ms (set-noop). Treat differences under about 2 ms as noise.
- **A desktop, not a lab.** Chrome, Edge, SignalRGB and Parsec were running, and whether audio was playing was not checked. hyperfine flagged outliers in 22 blocks, and the worst single runs reached about twice the median. Means and medians are both reported.
- **hyperfine includes its own spawn and wait overhead.** Compare rows with each other and with the `nop` floors, not with the stamps from `--timing`.
- **The product's set-noop is not comparable** with the bench implementations: it skips roles that are already set (footnote ¹). Compare the toggle column instead.
- **First launch of a new exe is slower**, because Defender scans each new file hash. Native exes took 33–69 ms on their first launch, the ps2exe exes about 660 ms, and the Rust *bench* binary consistently 457–605 ms, which is probably a deeper Defender scan of that particular image (diagnosing it needs admin rights). The product's first run was 64–69 ms. This is why every scenario uses warm-up runs.
- **The product build changed after the measurement.** The rows come from the 20:30 build. A later change touched only the error message boxes and kept the size; a quick re-run of the harness showed the same numbers.
- **No ETW / WPR traces** (they need admin rights). hyperfine's user and system columns (15.6 ms granularity on Windows) were ignored.
- **Toggle direction** (PG42UQ → PHL against PHL → PG42UQ) changed the means by at most 1.8 ms for native rows. Toggles ran back to back, so notifications from the previous switch may still have been in flight. This is realistic for repeated key presses and the same for every row.
