# Contributing to Toggle Audio

Thanks for your interest in improving Toggle Audio. Bug reports, device compatibility notes, benchmark results from other machines and pull requests are all welcome.

By participating you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md). Security issues go through the private process in [SECURITY.md](SECURITY.md), not public issues. There is no CLA or DCO: contributions are accepted under the project's [MIT License](LICENSE).

## Where the design lives

- [`docs/DESIGN.md`](docs/DESIGN.md) is the binding specification: command line, exit codes, toggle semantics, Core Audio access, configuration format, settings dialog, repository layout, quality bar, packaging and release plan. Read the relevant section before changing behavior, and update it in the same pull request when behavior changes.
- [`docs/research/`](docs/research/) holds the background research behind each decision (Core Audio and `IPolicyConfig`, GUI, packaging, benchmark method, naming). These notes record why things are the way they are; they are not kept in sync with the code.

For anything larger than a bug fix, please open an issue first so we can agree on the approach.

## Development setup (Windows)

Toggle Audio is a Windows-only project. Development needs Windows 10 or 11 on x64.

### Required

1. **Rust stable** through [rustup](https://rustup.rs/). The repository's `rust-toolchain.toml` selects the stable channel with `rustfmt` and `clippy`; the minimum supported Rust version is 1.85 (edition 2024).

   ```powershell
   winget install Rustlang.Rustup
   rustup update stable
   ```

2. **Visual Studio 2022 Build Tools** with the **Desktop development with C++** workload (MSVC compiler and linker) and a **Windows 10/11 SDK** (provides `rc.exe`, which `build.rs` uses to compile `assets/app.rc`: manifest, icon, dialog and version info).

   ```powershell
   winget install Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
   ```

   The full Visual Studio 2022 IDE with the same workload works too.

Then build and run:

```powershell
cargo build --release
.\target\release\toggle-audio.exe list
.\target\release\toggle-audio.exe settings
```

### Optional: benchmark toolchains (`bench/`)

Only needed to build or run the benchmark implementations. CI compiles all of them, so you can also rely on CI for languages you do not have installed.

| Tool | Used by | Install |
| --- | --- | --- |
| PowerShell 7 | every `build.ps1` and `bench/run-bench.ps1` | `winget install Microsoft.PowerShell` |
| MSVC and the Windows SDK (from the Build Tools above) | `bench/c`, `bench/baseline`, the NativeAOT linker, `mt.exe` for Go and PowerShell | already installed |
| Go 1.21 or later | `bench/go` | `winget install GoLang.Go` |
| Zig 0.17.x | `bench/zig` | `winget install zig.zig` |
| .NET 9 SDK (NativeAOT) | `bench/csharp` | `winget install Microsoft.DotNet.SDK.9` |
| ps2exe 1.0.17 | `bench/powershell` | `Install-Module ps2exe -Scope CurrentUser` (in Windows PowerShell) |
| AudioDeviceCmdlets | `bench/powershell` and the oracle in `bench/run-bench.ps1` and `scripts/e2e.ps1` | `Install-Module AudioDeviceCmdlets -Scope CurrentUser` (in Windows PowerShell) |
| hyperfine | `bench/run-bench.ps1` | `winget install sharkdp.hyperfine` or `cargo install hyperfine` |

Each `bench/<lang>/` directory has its own README and `build.ps1` (for example `pwsh -File bench\c\build.ps1`). `pwsh -NoProfile -File bench\run-bench.ps1` builds and runs the whole suite and writes raw results to `bench/results/`. See [`bench/README.md`](bench/README.md) for the parameters, [`bench/RESULTS.md`](bench/RESULTS.md) for the published numbers and [`docs/benchmarks.md`](docs/benchmarks.md) for the write-up. The real-toggle scenarios change your default playback device. The harness restores it afterwards, but the device ids are constants at the top of `run-bench.ps1`, so set them for your machine or pass `-SkipToggle`.

### Optional: installer (`installer/`)

The MSI is built with **WiX Toolset 7**, installed as a .NET global tool (needs a .NET SDK, for example the .NET 9 SDK above):

```powershell
dotnet tool install --global wix --version 7.0.0
wix eula accept wix7
wix extension add -g WixToolset.UI.wixext/7.0.0
wix extension add -g WixToolset.Util.wixext/7.0.0
.\installer\build-msi.ps1
```

`build-msi.ps1` reads the version from `Cargo.toml`, runs `cargo build --release` and `wix build`, validates the package with `wix msi validate` (no administrator rights needed) and writes the MSI and its SHA256 to `installer/out/`. Installing, upgrading and uninstalling the MSI needs UAC, so test those on a machine or VM you do not mind changing. [`docs/packaging.md`](docs/packaging.md) covers what the MSI installs, silent installs, upgrades, the GUID rules and the winget plan.

## Checks before you push

CI runs these on `windows-latest`; run them locally first:

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
pwsh scripts/third-party-notices.ps1 -Check   # after a Cargo.lock change
cargo +1.85 check --all-targets --locked      # minimum Rust version (rust-version in Cargo.toml)
```

Expectations:

- **Formatting:** `cargo fmt --all` with the repository's `rustfmt.toml`. No manual style debates.
- **Lints:** clippy (including the `pedantic` group configured in `Cargo.toml`) must be clean with `-D warnings`. If you need to silence a lint, use `#[expect(..., reason = "...")]` (it warns once the lint no longer fires) and keep it as narrow as possible.
- **Unsafe code:** `unsafe` is allowed only where the Win32 and COM calls live: `src/audio.rs`, `src/console.rs`, `src/gui/`, the known-folder fallback in `src/config.rs`, `src/timing.rs` and the enumeration helper in `tests/real_device.rs`. Every `unsafe` block needs a `// SAFETY:` comment stating the invariant it relies on.
- **Tests:** new logic comes with unit tests. Keep pure logic (toggle decision, config parsing, CLI parsing, encoding helpers) separate from COM so it can be tested on CI runners, which have no audio devices.
- **Dependencies:** the startup-time budget and the zero-runtime-dependency goal matter. Discuss any new crate in an issue before adding it, and regenerate `THIRD-PARTY-NOTICES.txt` (`pwsh scripts/third-party-notices.ps1`) whenever `Cargo.lock` changes.

### Real-device tests

Tests that touch real audio endpoints live in `tests/real_device.rs` and are marked `#[ignore]`, because hosted CI has no audio devices. Run them on a machine with at least one active playback device. By default they never change a default device: the only `set` calls target the device that already is the default, and one test re-asserts each role's current default through `SetDefaultEndpoint`.

```powershell
cargo test -- --ignored
```

The round-trip test really toggles, so it runs only when you name two active playback endpoints (ids from `toggle-audio list`). A guard restores every role's original default afterwards, even when an assertion fails:

```powershell
$env:TOGGLE_AUDIO_TEST_DEVICES = '{0.0.0.00000000}.{...};{0.0.0.00000000}.{...}'
cargo test --test real_device -- --ignored toggle_round_trip
```

`scripts/e2e.ps1` is the end-to-end check. It builds the release binaries and runs `--version`, `--help`, usage errors, `list`, `get`, `set` and `--timing` through both exes, comparing the results with an independent oracle (the AudioDeviceCmdlets module). It then toggles between two devices and verifies every step. It points `APPDATA` at a scratch directory, so your real configuration is never touched, and it restores the original default devices at the end. The default `-Device1` / `-Device2` ids belong to the reference machine, so pass your own:

```powershell
pwsh scripts/e2e.ps1 -SkipToggle                                    # read-only checks only
pwsh scripts/e2e.ps1 -Device1 '{0.0.0.00000000}.{...}' -Device2 '{0.0.0.00000000}.{...}' -Rounds 2
pwsh scripts/e2e.ps1 -SkipBuild                                     # test the existing release build (honors CARGO_TARGET_DIR)
pwsh scripts/e2e.ps1 -Exe path\to\toggle-audio.exe                 # test specific binaries (toggle-audiow.exe next to it)
```

Please mention in your pull request whether you ran the real-device tests and on which Windows build.

## Commit messages

This project uses [Conventional Commits 1.0](https://www.conventionalcommits.org/en/v1.0.0/):

```text
type(optional scope): short summary in the imperative
```

- Types: `feat`, `fix`, `perf`, `refactor`, `test`, `docs`, `build`, `ci`, `chore`.
- Common scopes: `audio`, `cli`, `config`, `gui`, `installer`, `bench`.
- Mark breaking changes with `!` (for example `feat(cli)!: rename --comm`) or a `BREAKING CHANGE:` footer.

Examples: `fix(audio): fall back to IPolicyConfigVista when SetDefaultEndpoint fails`, `perf: skip roles that already point at the target`, `docs: document the G HUB binding`.

Pull requests are squash-merged, so the pull request title becomes the commit message on `main` and should follow the same format.

## Pull request checklist

- [ ] The change is focused; unrelated refactors are in a separate pull request.
- [ ] `cargo fmt --all -- --check`, `cargo clippy --all-targets --locked -- -D warnings` and `cargo test --locked` pass locally.
- [ ] New or changed behavior has tests, or the pull request explains why it cannot be tested.
- [ ] Real-device tests (`cargo test -- --ignored`, `scripts/e2e.ps1`) were run if the change touches `audio.rs`, the toggle path or the GUI, with the Windows build noted.
- [ ] Every new `unsafe` block has a `// SAFETY:` comment.
- [ ] `docs/DESIGN.md`, `README.md` and `--help` text are updated if user-visible behavior, the CLI or the config format changed.
- [ ] `CHANGELOG.md` has an entry under `[Unreleased]` for user-visible changes.
- [ ] If `Cargo.lock` changed, `THIRD-PARTY-NOTICES.txt` is regenerated with `pwsh scripts/third-party-notices.ps1` (CI checks it).
- [ ] If the change affects startup or toggle latency, before and after numbers (for example from `--timing` or hyperfine) are included.
- [ ] The pull request title follows Conventional Commits.

## Releases (maintainer notes)

Versions follow [Semantic Versioning](https://semver.org/) and the changelog follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). To release: move the `[Unreleased]` entries into a new version section with the release date, bump `version` in `Cargo.toml` and the `assemblyIdentity` version in `assets/app.manifest` (a unit test fails when they differ), commit, and push a `vMAJOR.MINOR.PATCH` tag. The release workflow checks that the tag matches `Cargo.toml`, builds the executables and the MSI, and, once code signing is configured, submits the MSI to SignPath and waits: approve the signing request in the SignPath web app (you get an email; the job gives up after an hour). It then builds the portable zip and `SHA256SUMS.txt` from the signed files and publishes the GitHub Release. The step-by-step procedure, the dry run and the SignPath set-up are in [`docs/signing.md`](docs/signing.md). The MSI `UpgradeCode` must never change.

Changes to `.github/workflows/release.yml`, `build.rs`, `assets/*.rc`, `installer/*.ps1`, `installer/toggle-audio.wxs` and `installer/signpath/` affect what gets signed, so review them like code (part of the [code signing policy](docs/signing.md#code-signing-policy)).
