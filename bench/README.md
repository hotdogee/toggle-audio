# bench/: benchmark implementations and harness

This directory answers one question with reproducible numbers: how fast can a "switch the Windows default playback device" exe be, and does the language matter? The answer is in [`RESULTS.md`](RESULTS.md) (raw numbers) and [`../docs/benchmarks.md`](../docs/benchmarks.md) (write-up). In short, every native language lands at about 40 ms per real toggle, and Windows sets the floor.

## Contents

| Path | What it is |
| --- | --- |
| [`c/`](c/README.md) | `ta.c`, the C (MSVC) **speed reference**, built five ways: `ta-c` (`/MT`, the primary), `ta-c-gui`, `ta-c-md`, `ta-c-delayload` and `ta-c-nocrt`. |
| [`rust/`](rust/README.md) | `ta-rs`, built with the `windows` crate like the product. A standalone Cargo workspace (excluded from the root workspace). |
| [`csharp/`](csharp/README.md) | `ta-cs`, C# on .NET 9 NativeAOT, calling COM through raw vtables. |
| [`go/`](go/README.md) | `ta-go`, pure Go (no cgo) calling COM through vtables. |
| [`zig/`](zig/README.md) | `ta-zig` (native Zig, no libc) and `ta-zigcc` (`c/ta.c` built by `zig cc`). |
| [`powershell/`](powershell/README.md) | The original proof of concept (`switch-audio.ps1`), `ta.ps1` (the bench contract on AudioDeviceCmdlets) and its ps2exe builds `ta-ps.exe` / `ta-ps-anycpu.exe`. |
| [`baseline/`](baseline/README.md) | `nop.exe`, `nop-con.exe` and `nop-con-detached.exe` (empty-process floors), plus `spawnbench.exe`, a launcher that times `CreateProcessW` to exit with chosen console flags. |
| [`common/`](common/) | `detached.manifest` (the `consoleAllocationPolicy=detached` manifest every bench exe embeds) and `vsdevenv.ps1` (imports the MSVC x64 environment). |
| [`run-bench.ps1`](run-bench.ps1) | The harness: builds, runs the correctness gate, hyperfine scenarios, `--timing` phases, launch-style and cold-start measurements, and writes the results. |
| [`RESULTS.md`](RESULTS.md) | The published results, with machine, method, anomalies and recommendation. |
| [`results/`](results/) | Raw data: hyperfine JSON per scenario and round, `timing-raw.tsv`, spawnbench TSVs, `gate.json`, `machine.json` and the generated `summary.json` / `summary.md`. |

Build outputs go to `<lang>/bin/` and `<lang>/obj/` and are not committed.

## The rule: best version per language

Every implementation is kept as **the best-optimized version its language and toolchain allow**: release optimizations, LTO where available, no unnecessary runtime, static linking where it avoids a redistributable, and the same embedded manifest. They must all do the same COM work in the same order, so that a comparison measures the language and toolchain rather than the algorithm. If you can make one faster without breaking the contract, that is a welcome pull request. Include before and after numbers from `run-bench.ps1`. A change to one implementation must not make it do less work than the others.

Each `<lang>/README.md` documents its build flags and why they were chosen, and records language-specific caveats. For example, NativeAOT initializes COM before `Main`, so its `com_init` phase is near zero.

## The bench CLI contract

Every implementation in this directory does exactly this (copied from [`docs/DESIGN.md`](../docs/DESIGN.md) section 12, which supersedes the earlier wording in [`docs/research/benchmark-method.md`](../docs/research/benchmark-method.md) section 1):

| Command | stdout | Exit |
| --- | --- | --- |
| `list` | one line per active render endpoint: `<id>\t<name>\t<flags>`, flags `*` (default), `c` (default communications), `*c` or `-` | 0 |
| `get` | `<id>\t<name>` of the current default (eConsole) | 0, or 4 if none |
| `set <id>` | nothing; sets eConsole, eMultimedia, eCommunications in that order after checking the endpoint is ACTIVE | 0; 3 if unknown/inactive |
| `toggle <idA> <idB>` | the id that was set | 0; 3 if the target is unknown/inactive |
| (no args) | one usage line on stderr, no COM work (runtime floor) | 1 |
| `--timing` | on stderr, `phase\t<name>\t<microseconds since process creation>` for `entry`, `com_init`, `enumerator`, `work_done`, `exit` (`entry` is the create-to-entry interval) | — |

Exit 2 means a COM failure (the HRESULT is printed on stderr). Output is UTF-8 with `\n` line endings when redirected. Every bench exe except `ta-c-gui` is console subsystem with [`common/detached.manifest`](common/detached.manifest) embedded, so shells capture its output and a launch from a GUI program creates no console on Windows 11 24H2+.

This contract is not the product's CLI. `toggle-audio.exe` uses `set` / `toggle` with its own configuration and its own exit codes (see the [README](../README.md#command-line)). The harness accounts for the difference.

## Building

Every directory has a non-interactive, idempotent `build.ps1` for PowerShell 7. CI compiles all of them on every push (`.github/workflows/ci.yml`, job `bench-build`), so you only need the toolchains for the languages you want to run locally.

| Implementation | Build | Prerequisites | Output |
| --- | --- | --- | --- |
| C | `pwsh -File bench\c\build.ps1` | Visual Studio 2022 or Build Tools with the C++ workload, Windows SDK | `c\bin\ta-c.exe`, `ta-c-gui.exe`, `ta-c-md.exe`, `ta-c-delayload.exe`, `ta-c-nocrt.exe` |
| Baselines | `pwsh -File bench\baseline\build.ps1` | As for C | `baseline\bin\nop.exe`, `nop-con.exe`, `nop-con-detached.exe`, `spawnbench.exe` |
| Rust | `pwsh -File bench\rust\build.ps1` | Rust stable 1.85+ (MSVC toolchain) and the MSVC linker; `mt.exe` / `dumpbin.exe` optional, for verification | `rust\bin\ta-rs.exe` |
| C# | `pwsh -File bench\csharp\build.ps1` | .NET 9 SDK 9.0.318 or a later 9.0.3xx (pinned by `global.json`), MSVC C++ workload (the NativeAOT linker) | `csharp\bin\ta-cs.exe` |
| Go | `pwsh -File bench\go\build.ps1` | Go 1.21+ (1.27.0 was measured), `mt.exe` from the Windows SDK | `go\bin\ta-go.exe` |
| Zig | `pwsh -File bench\zig\build.ps1` | Zig 0.17.x (the script refuses other versions); `mt.exe` / `dumpbin.exe` optional | `zig\bin\ta-zig.exe`, `ta-zigcc.exe` |
| PowerShell | `pwsh -File bench\powershell\build.ps1` | Windows PowerShell 5.1, ps2exe 1.0.17, `mt.exe`; AudioDeviceCmdlets 3.1.0.2 to run the result | `powershell\bin\ta-ps.exe`, `ta-ps-anycpu.exe` |

Install commands for the toolchains are in [CONTRIBUTING.md](../CONTRIBUTING.md#optional-benchmark-toolchains-bench). The harness also builds the product (`cargo build --release` with `CARGO_TARGET_DIR=target\release-build`).

## Running the harness

Requirements: PowerShell 7, [hyperfine](https://github.com/sharkdp/hyperfine) (on `PATH`, installed with winget, or given with `-Hyperfine <path>`), the AudioDeviceCmdlets module in Windows PowerShell 5.1 (the independent oracle for the gate and the toggle checks), and at least two active playback devices for the toggle scenarios.

> [!WARNING]
> The gate and the real-toggle scenario **change your default playback device**. A `finally` block restores the restore target for all roles and verifies it, but the toggle pair and the restore target are endpoint-id constants at the top of `run-bench.ps1` (`$Pg`, `$Phl`, `$Spdif`), set for the reference machine. Edit them for yours, using the ids from `toggle-audio list`, or run with `-SkipToggle`.

```powershell
pwsh -NoProfile -File bench\run-bench.ps1                          # build everything, gate, all scenarios (~17 min)
pwsh -NoProfile -File bench\run-bench.ps1 -SkipBuild -SkipToggle   # never toggles
pwsh -NoProfile -File bench\run-bench.ps1 -Runs 50 -Rounds 1 -SkipToggle -SkipPowerShell   # quick look
```

| Parameter | Default | Meaning |
| --- | --- | --- |
| `-Runs`, `-Warmup`, `-Rounds` | 200, 10, 3 | hyperfine runs and warm-up per scenario. Rounds alternate forward and reverse command order and are pooled. |
| `-Scenarios` | `noargs,list,get,set-noop` | Which hyperfine scenarios to run. |
| `-ToggleRuns` | 20 | Runs per real-toggle row (must be even, so every row ends where it started). |
| `-PsRuns`, `-PsWarmup` | 20, 3 | Runs and warm-up for the PowerShell baselines. |
| `-TimingRuns` | 30 | `--timing` runs per exe for the phase breakdown. |
| `-SpawnRuns`, `-ColdCopies` | 10, 3 | spawnbench runs per launch style, and fresh copies for the cold first-run measurement. |
| `-SkipBuild`, `-SkipGate`, `-SkipToggle`, `-SkipPowerShell`, `-SkipTiming`, `-SkipSpawn`, `-SkipCold` | off | Skip a step. |
| `-OutDir` | `bench\results` | Where results go. |
| `-ProductDir` | `target\release-build\release` | Where the product exes are. |
| `-LegacyExe` | none | Path of the original `Switch-Audio.exe` proof of concept; its gate and toggle rows are left out when it is not given. |

The product is always run with `APPDATA` pointed at a temporary directory that holds its own configuration, so your real `%APPDATA%\toggle-audio\config.json` is never read or written.

## Where the results live

- `results/` (committed) has everything the harness writes: `{noargs,list,get,set-noop}-r{1,2,3}.json`, `ps-*.json` and `toggle-*.json` (hyperfine exports), `timing-raw.tsv`, `spawn-*.tsv` and `spawn.json`, `gate.json`, `machine.json`, and the generated `summary.json` / `summary.md`. `run-bench.log` is written there too but is gitignored.
- [`RESULTS.md`](RESULTS.md) is the curated report, recomputed from the raw `times` arrays. Update it by hand after a full run and keep the machine and toolchain description accurate.
- [`../docs/benchmarks.md`](../docs/benchmarks.md) is the write-up, and the README's Performance table summarizes it. Both copy their numbers from `RESULTS.md`.
