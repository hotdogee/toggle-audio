# Benchmark results: toggle-audio implementations

Date: 2026-10-04 (main run 20:30 to 20:47 +08:00; `get` pass at 20:47)  |  Harness: [`run-bench.ps1`](run-bench.ps1) with hyperfine 1.20.0 (`-N`, warm-up 10, 200 runs × 3 rounds, forward / reverse / forward order, pooled) and `spawnbench`
Machine: AMD Ryzen 9 7950X (16 cores / 32 threads), 128 GB RAM, Windows 11 Pro for Workstations 26H2 build 10.0.26300.9550 (zh-TW), power plan "Ultimate Performance", AC power
Defender real-time protection: ON (AMProductVersion 4.18.26080.4; no exclusions, because adding one needs admin) | Smart App Control: off | Session not elevated | Desktop apps left running (Chrome, Edge, SignalRGB, Parsec); no builds or other benchmarks during the run; whether audio was playing was not checked
Toolchains: MSVC 14.44.35207 (cl 19.44.35228, VS 2022 17.14.40, Windows SDK 10.0.26100) | rustc 1.99.0, `windows` / `windows-core` 0.62.2 | .NET SDK 9.0.318 (ILCompiler 9.0.20, NativeAOT) | Go 1.27.0 (GOAMD64=v3) | Zig 0.17.0 | ps2exe 1.0.17 on Windows PowerShell 5.1.26100.9549 | PowerShell 7.6.6 | AudioDeviceCmdlets 3.1.0.2
Endpoints: the set-noop target is PG42UQ `{0.0.0.00000000}.{739b3554-bfed-4d61-b407-a818b317c991}` (the current default); the toggle pair is PG42UQ ↔ PHL BDM4065 `{0.0.0.00000000}.{5b124733-5d8f-428c-b83c-ee05ce6467fb}` (both NVIDIA HDMI). The legacy rows toggle S/PDIF ↔ PG42UQ. PG42UQ was verified as the default for playback and communications after every toggle row and at the end of the run.

All times are wall time in ms from `CreateProcess` to process exit, as measured by hyperfine (which includes its own spawn and wait overhead; see the `nop` rows). Each cell gives **mean ± σ (median)**. Native rows pool n = 600 runs (200 × 3 rounds); toggle rows have n = 20; PowerShell rows have n = 60 (20 × 3 rounds, warm-up 3).

## Summary table

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

Medians over 30 runs of `<exe> set <PG42UQ id> --timing`, launched by .NET `Process.Start` from pwsh. Absolute stamps are µs since process creation, in ms here. The deltas are medians of the per-run differences. `entry` covers process creation, the loader and runtime start-up. The launcher hardly matters: a verification re-run (30 runs each, after the main run) with `cmd.exe` as the parent gave `entry` medians of 5.56 ms (`ta-c`) and 3.19 ms (`ta-c-delayload`), against 5.85 and 3.36 ms through `Process.Start` in the same session. So `entry` is comparable with the hyperfine floors to within about 0.3 ms; the delay-load build's `entry` of 3.1 ms fits its 4.2 ms noargs wall time.

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

Product (`toggle-audio.exe set <PG42UQ> --timing`, phases from `src/lib.rs`; no `SetDefaultEndpoint` calls happen here, see ¹):

| Exe | start | start → config_loaded | config_loaded → com_ready (COM init + enumerator) | com_ready → target_chosen | target_chosen → set_done (role check, no set needed) | set_done → end |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| toggle-audio | 6.90 | 0.09 | 3.58 | 0.25 | 4.04 | 0.36 |
| toggle-audiow | 6.89 | 0.09 | 3.60 | 0.26 | 4.06 | 0.41 |

Where the time goes in a native toggle (C reference, hyperfine wall time 41.8 ms):
- Process creation and teardown floor: ~3.4 ms (`nop`).
- Loading `ole32.dll` and its dependencies at start-up: ~3.3 ms (`ta-c` noargs 7.52 ms against `ta-c-delayload` 4.24 ms). Delay-loading only moves this cost into `com_init` (4.5 ms instead of 2.0 ms).
- `CoInitializeEx` ~2 ms, plus `CoCreateInstance(MMDeviceEnumerator)` ~2 ms.
- Three `SetDefaultEndpoint` RPCs into AudioSrv: ~23–27 ms (about 8 ms each), **even when the endpoint already is the default** (set-noop 38 ms against a real toggle of 41 ms).

## Correctness gate

Run by `run-bench.ps1` before any timing. For each implementation: `toggle` → the oracle (AudioDeviceCmdlets `Get-AudioDevice -Playback` and `-PlaybackCommunication` in Windows PowerShell 5.1) must report PHL BDM4065 for both, and stdout must be the PHL id; `toggle` again → PG42UQ for both, and stdout the PG42UQ id; `list` → three tab-separated fields, PG42UQ flagged `*c`, every other line `-`; `get` → exactly `<PG42UQ id>\tPG42UQ (NVIDIA High Definition Audio)\n`; `set <bogus id>` → exit 3; no arguments → exit 1. The product (`toggle` uses its config; stdout not checked) expects exit 4 for the bogus id and exit 2 for a usage error (`--no-such-flag`), because for the product no arguments means toggle. "list == C" means byte-identical to `ta-c.exe list` (UTF-8, `\n`, including the CJK names `喇叭 (...)`).

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

After every hyperfine toggle row (warm-up 2 + 20 runs, so an even count), the oracle reported PG42UQ for playback and communications (17 of 17 rows; see `results/summary.json` → `toggle_verify`). No implementation was excluded.

## G HUB launch: console allocation (no-flash benefit)

`spawnbench` times `CreateProcessW` → process exit. For the "G HUB style" rows, `run-bench.ps1` starts spawnbench itself with `DETACHED_PROCESS`, so the launcher has no console, just like a GUI program such as G HUB, and spawnbench starts each child with no creation flags. A console-subsystem child without the detached manifest then gets a brand-new console: a conhost window appears on screen. Ten runs per row after one warm-up; median (mean; min–max) in ms.

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

So a console-subsystem tool without the manifest costs **~150 ms extra and flashes a window** on every key press. The detached manifest removes both (2.8 ms, the same as a GUI-subsystem exe), and having a manifest costs nothing measurable (`nop-con-detached` 3.47 ms against `nop-con` 3.53 ms under hyperfine). Every console window that opened also closed by itself when the child exited; the harness found no window left over to close.

## Observations and anomalies

- **The OS sets the floor, not the language.** All ten native builds land within 38.2–40.3 ms for set-noop and 40.0–43.8 ms for the real toggle, and within 17.2–19.9 ms for `list`. The order of the toggle column is not meaningful at n = 20 with σ ≈ 2–4 ms.
- **Runtime floors** (noargs minus `nop` at 3.45 ms): Go +2.6 ms (it loads ole32 lazily, so it pays for that in `com_init`: 4.0 ms against 2.0 ms), C delay-load +0.8, C no-CRT +3.6, Zig +3.6, C `/MT` +4.1, zig cc +4.1, Rust +4.6, C `/MD` +4.6, product +6.3 (521 KB image, console setup, parsing), C# NativeAOT +8.4 (runtime start-up including COM initialization). Most of the C floor is the load-time import of `ole32.dll` (about 3.3 ms), not the CRT (no-CRT saves about 0.5 ms).
- **SetDefaultEndpoint is expensive even as a no-op**: about 8 ms per role, through an RPC into AudioSrv. The product skips roles that already match, so its `set` to the current default takes 19.5 ms against 38–40 ms for the bench `set`. On a real toggle all three roles change, and the product matches C (40.9 against 41.8 ms).
- **Toggle direction does not matter**: the mean of the PG42UQ → PHL runs and of the PHL → PG42UQ runs differ by at most 1.8 ms for every native implementation (largest: the product, 40.0 against 41.8 ms) and by 0.8–6.7 ms for the PowerShell and legacy rows, which is within their σ of 6–10 ms. The toggles ran back to back, so AudioSrv notifications from the previous switch may still have been in flight. That is realistic for repeated key presses and the same for every row.
- **Cold first run, Rust bench binary**: each fresh copy of `ta-rs.exe` (a new hash, 16 random bytes appended) took **457–605 ms** on its first launch, in 5 of 5 tries, including 2 extra probes with no arguments. The second launch took 9–10 ms. Every other native exe took 33–68 ms on its first launch, **including the product `toggle-audio.exe` (Rust, 64–69 ms)**. This is consistent with Defender doing a deeper (cloud or emulation) scan of that particular image. It cannot be diagnosed without admin (no Defender logs or ETW), and it does not affect the product. The ps2exe exes take about 660 ms cold, against about 550 ms warm for `list`.
- **Product build measured**: the product rows were measured on the build that existed at 20:30. The E2E pass later changed `src/console.rs` (error message boxes only) and rebuilt both exes at 21:05; the size is unchanged (521,216 bytes). A quick re-run of the harness (`-SkipToggle -Runs 5 -Rounds 1`) with the new build gave the same picture (noargs 10.3 / 9.6 ms, `get` 15.6 / 15.6 ms), and the new build passes the gate except for the toggle steps, which that mode skips. Review changes made after this run (the toggle reuses the console default it has just read and looks up the configured ids first; `docs/DESIGN.md` section 16) grew both exes to 523,776 bytes and were not re-measured. The ps2exe exes measured 48,128 bytes because `ta.ps1` had LF line endings at the time; a Git checkout (CRLF, as `.gitattributes` asks) builds them at 48,640 bytes.
- **Explicit `CREATE_NEW_CONSOLE` is also suppressed by the detached policy** on this build (26300): `nop-con-detached.exe` launched with that flag took 2.9 ms and created no console, while `nop-con.exe` took 153 ms. `bench/baseline/README.md` expected the opposite ("the detached manifest does not stop an explicit `CREATE_NEW_CONSOLE`"). The measurement shows the policy wins on this build, so even a launcher that passes the flag would not flash a window.
- **Where the original's 887 ms goes**: the Windows PowerShell host start (133 ms empty, about 200 ms to the first script statement through ps2exe), `Import-Module AudioDeviceCmdlets` (47–60 ms), and above all `Get-AudioDevice -List`, which `switch-audio.ps1` calls twice (`ta.ps1 list` alone takes 520 ms). Under PowerShell 7 everything is about 1.5× slower (host start 199 ms; `Import-Module` 123 ms including the module-path fallback).
- In the main run's log, hyperfine flagged statistical outliers in 22 benchmark blocks (18 native, 3 PowerShell, 1 toggle; the separate `get` pass is not in that log), plus one "first run was slower" warning. The worst single runs reached about twice the median (72 ms against 38.5 ms for `ta-c-gui` set-noop, 509 ms against 274 ms for `ta-ps.exe` set-noop), probably from the background desktop apps and audio-service jitter. Medians and means are reported together. One implementation's median can move between rounds by up to 1.8 ms (noargs), 1.5 ms (list), 0.6 ms (get) and 2.4 ms (set-noop), so treat differences under about 2 ms between implementations as noise.
- The User/System columns of hyperfine (15.6 ms granularity on Windows) were ignored. ETW/WPR was not used (needs admin).
- `get` was added to the harness after the main run, so it was measured in a second pass right afterwards with the same parameters (`-Scenarios get`, 200 × 3 rounds), and its `get-r*.json` files were copied into `results/`.

## Sanity check against the expected ranges (`docs/research/benchmark-method.md` section 4)

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

## Recommendation

- **Keep Rust for the product.** Its real toggle (40.9 ms) is identical to the C reference (41.8 ms) within noise, and about 22× faster than the original (887 ms). Its extra start-up cost of about 2 ms over C (9.8 against 7.5 ms runtime floor) is about 5% of a toggle and invisible to a user. A rewrite in C, Zig or a no-CRT build would save at most about 2–4 ms of a roughly 40 ms action dominated by AudioSrv.
- **Keep the console subsystem plus the detached manifest** (`toggle-audio.exe`) as the primary binary. It costs nothing measurable, a G HUB-style launch creates no console (2.8 ms against 153 ms with a window for a plain console exe), and on this build it even overrides an explicit `CREATE_NEW_CONSOLE`. `toggle-audiow.exe` remains the fallback for Windows versions before 11 24H2.
- **The remaining lever is the number of `SetDefaultEndpoint` calls** (about 8 ms each): the product already skips roles that are already set, and users who turn off "also switch Communications" save another ~8 ms per toggle. The build profile (LTO, `panic = "abort"`, crt-static) needs no change; delay-loading `ole32` would not help, because every real command uses COM. Delay-loading the DLLs that only the settings dialog and the known-folder fallback use (`shell32`, `oleaut32`, `comctl32`) measured about 1.2–1.8 ms faster per run in a later experiment; it is not adopted in 0.1.0.

## How to reproduce

```powershell
pwsh -NoProfile -File bench\run-bench.ps1               # builds everything, gate, all scenarios (~17 min)
pwsh -NoProfile -File bench\run-bench.ps1 -SkipBuild -SkipToggle   # never toggles; only re-asserts PG42UQ (set to the current default)
```

Parameters: `-Runs 200 -Warmup 10 -Rounds 3 -ToggleRuns 20 -PsRuns 20 -PsWarmup 3 -TimingRuns 30 -SpawnRuns 10 -ColdCopies 3 -Scenarios noargs,list,get,set-noop -OutDir bench\results -LegacyExe <path of the original Switch-Audio.exe>`, and the switches `-SkipBuild -SkipGate -SkipToggle -SkipPowerShell -SkipTiming -SkipSpawn -SkipCold`. The toggle pair and the restore target (PG42UQ) are constants at the top of the script, so edit them for another machine. The product is run with `APPDATA` pointed at a temporary directory that holds a config with device1 = PG42UQ, device2 = PHL BDM4065 and `switch_communications = true`, so the user's real configuration is never read or written. A `finally` block always restores PG42UQ for all roles and verifies it with the oracle.

## Raw data

`results/` (committed):
- `{noargs,list,get,set-noop}-r{1,2,3}.json` and `ps-{noargs,list,set-noop}-r{1,2,3}.json`: hyperfine exports, one per scenario and round.
- `toggle-*.json`: one hyperfine export per toggle row.
- `timing-raw.tsv`: every `--timing` stamp (implementation, run, phase, µs).
- `spawn-*.tsv`: raw spawnbench output; `spawn.json` has their parsed summaries.
- `gate.json`: correctness gate; `machine.json`: machine information.
- `summary.json` / `summary.md`: pooled statistics generated by `run-bench.ps1`. `summary.json` rounds to 3 decimals and `summary.md` rounds that again to 2, so nine `summary.md` cells differ from the tables here by 0.01 ms. The tables in this file were recomputed from the raw `times` arrays (sample σ).
- `run-bench.log`: the full console log (gitignored by `*.log`).
