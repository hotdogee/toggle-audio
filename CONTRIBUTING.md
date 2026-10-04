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
| MSVC (from the Build Tools above) | `bench/c` | already installed |
| Go | `bench/go` | `winget install GoLang.Go` |
| Zig | `bench/zig` | `winget install zig.zig` |
| .NET 9 SDK (NativeAOT) | `bench/csharp` | `winget install Microsoft.DotNet.SDK.9` |
| hyperfine | `bench/run-bench.ps1` | `winget install sharkdp.hyperfine` or `cargo install hyperfine` |

Each `bench/<lang>/` directory has its own README and build script; `bench/run-bench.ps1` runs the whole suite and writes raw results under `bench/results/`. See [`bench/RESULTS.md`](bench/RESULTS.md) for methodology and published numbers. Benchmarks that really toggle change your default playback device; the harness restores it afterwards.

### Optional: installer (`installer/`)

The MSI is built with **WiX Toolset 7**, installed as a .NET global tool (needs a .NET SDK, for example the .NET 9 SDK above):

```powershell
dotnet tool install --global wix --version 7.*
wix eula accept wix7
.\installer\build-msi.ps1
```

`build-msi.ps1` reads the version from `Cargo.toml`, runs `cargo build --release` and `wix build`, and writes the MSI to `installer/out/`. `wix msi validate` checks the package without administrator rights. Installing, upgrading and uninstalling the MSI needs UAC, so test those on a machine or VM you do not mind changing.

## Checks before you push

CI runs these on `windows-latest`; run them locally first:

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

Expectations:

- **Formatting:** `cargo fmt --all` with the repository's `rustfmt.toml`. No manual style debates.
- **Lints:** clippy (including the `pedantic` group configured in `Cargo.toml`) must be clean with `-D warnings`. If you need an `#[allow(...)]`, keep it as narrow as possible and add a comment explaining why.
- **Unsafe code:** `unsafe` is allowed only in `src/audio.rs`, `src/console.rs` and `src/gui/`. Every `unsafe` block needs a `// SAFETY:` comment stating the invariant it relies on.
- **Tests:** new logic comes with unit tests. Keep pure logic (toggle decision, config parsing, CLI parsing, encoding helpers) separate from COM so it can be tested on CI runners, which have no audio devices.
- **Dependencies:** the startup-time budget and the zero-runtime-dependency goal matter. Discuss any new crate in an issue before adding it.

### Real-device tests

Tests that touch real audio endpoints are marked `#[ignore]` because hosted CI has no audio devices and because they **change your default playback device** (they restore the original one afterwards, even on panic). Run them on a machine with at least two active playback devices:

```powershell
cargo test -- --ignored
```

The end-to-end script builds the release binaries, runs `list` and `get`, toggles back and forth, checks the result against an independent oracle (the AudioDeviceCmdlets PowerShell module) and restores the original default:

```powershell
.\scripts\e2e.ps1
```

Read the parameters at the top of `scripts/e2e.ps1` first and point it at two devices on your machine. Please mention in your pull request whether you ran the real-device tests and on which Windows build.

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
- [ ] If the change affects startup or toggle latency, before and after numbers (for example from `--timing` or hyperfine) are included.
- [ ] The pull request title follows Conventional Commits.

## Releases (maintainer notes)

Versions follow [Semantic Versioning](https://semver.org/) and the changelog follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). To release: move the `[Unreleased]` entries into a new version section with the release date, bump `version` in `Cargo.toml`, commit, and push a `vMAJOR.MINOR.PATCH` tag. The release workflow checks that the tag matches `Cargo.toml`, builds the executables, MSI, portable zip and `SHA256SUMS`, and publishes the GitHub Release. The MSI `UpgradeCode` must never change.
