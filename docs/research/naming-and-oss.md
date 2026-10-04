# Naming and open-source conventions: switch-audio rewrite

Researched 2026-10-04. Name checks were run against the live crates.io API, the npm registry, the GitHub repository search API, `winget search` (source `winget`), and web search.

---

## TL;DR: decisions

| Item | Decision |
|---|---|
| **Project name** | **Toggle Audio** (`toggle-audio`). The user's proposal holds up. Keep it. |
| GitHub repo | `hotdogee/toggle-audio` |
| Executable | `toggle-audio.exe`: toggles and exits; this is what G HUB binds. `toggle-audio.exe --settings` (or a sibling `toggle-audio-settings.exe` if the GUI ends up as a separate binary) opens the configuration window. |
| VERSIONINFO | `FileDescription = "Toggle Audio"`, `ProductName = "Toggle Audio"`, `CompanyName = "Han Lin"`, `OriginalFilename = "toggle-audio.exe"`. Task Manager and the G HUB app picker show FileDescription, so the user sees "Toggle Audio". |
| MSI product name | `Toggle Audio` (Publisher `Han Lin`). Install directory `C:\Program Files\Toggle Audio\`. |
| Start Menu entries | `Toggle Audio Settings` (runs the settings GUI) and optionally `Toggle Audio` (runs one toggle). Put them in the Start Menu root, not in a folder; a folder for two shortcuts is noise. |
| Crate name | `toggle-audio` (binary crate; crates.io returns 404, so the name is free). `[[bin]] name = "toggle-audio"`. |
| winget id | `Hotdogee.ToggleAudio` (format `Publisher.Package`) |
| Config | `%APPDATA%\toggle-audio\config.toml` (per-user and roaming; Program Files is read-only for users) |
| Tagline | **"Flip Windows audio output between two devices in milliseconds: one tiny exe, perfect for a hotkey."** |
| License | **MIT** (single), `Copyright (c) 2026 Han Lin` |
| First release | **v0.1.0**. Go to 1.0.0 once the config format and CLI flags are stable and the winget package is accepted. |
| Commits | Conventional Commits 1.0 (`feat:`, `fix:`, `perf:`, `docs:`, `build:`, `ci:`, `chore:`) |

---

## Part 1: Naming

### 1.1 Criteria
1. **Clarity**: does the name immediately say "switch the default audio output"?
2. **Collision**: is there an existing Windows audio tool or package with the same or a confusingly similar name?
3. **CLI typeability**: short, lowercase, hyphenated.
4. **How it reads in Task Manager and G HUB**: G HUB's "Launch Application" action shows the exe's name or file description, so the name should read as an action ("Toggle Audio") on a key label.
5. **Searchability**: people search "toggle audio output windows hotkey".

### 1.2 Raw collision data

**crates.io** (`/api/v1/crates/<name>`) returned **404 (free)** for every candidate: toggle-audio, audio-toggle, toggleaudio, audiotoggle, audioflip, audio-flip, flipaudio, soundflip, outflip, audio-swap, speakerswap, toggleout, audiohop, outswap, soundhop, sinkswap.
**npm**: all free except `audiotoggle` (taken; not relevant to a Windows exe).
**winget** (`winget search`): no package named AudioFlip, ToggleAudio, "Audio Toggle", SpeakerSwap, SoundFlip or AudioHop. Existing nearby ids are `AntoineAflalo.SoundSwitch`, `sirWest.AudioSwitch`, `Esquillax.AudioSwitcher` and `FortyOneLtd.AudioSwitcher`, which is why every *Switch*/*Switcher* name is crowded. That also rules out reusing "switch-audio".
**Chocolatey**: no `audioflip` package.

GitHub repository-name search (`in:name`, total count, then the top hits):

| Query | Count | Notable hits |
|---|---|---|
| toggle-audio / audio-toggle | 106 | Blackstareye/toggleAudio (GNOME extension, 8 stars), zonaston/toggle-audio (Cinnamon applet, 1 star), jrunning/Toggle-Audio-Devices (macOS Automator workflow, 2010), olQwQlo/Audio-Toggle (Windows PowerShell over SoundVolumeView, 0 stars), capacitor audio-toggle plugins (mobile). Mostly Linux desktop applets or mobile plugins; **no notable Windows exe named toggle-audio**. |
| audioflip | 8 | **Cyp9715/AudioFlip: a C# Windows tool, "Quickly switch Windows playback, communication, and recording devices with hotkeys"**, plus the **AudioFlip macOS app** that cycles output devices with shortcuts. This is a direct collision. |
| flipaudio | 0 | none |
| soundflip | 7 | small unrelated repos, plus soundflip.github.io |
| outflip | 1 | kakaoenterprise/OutFlip (an NLP research project) |
| audio-swap | 48 | isofew/AudioSwap (video audio-track swapping), many small repos |
| speakerswap | 0 | none |
| toggleout | 1 | unrelated |
| audiohop (own idea) | 3 | isIbra/AudioHop, a **macOS menu-bar audio device switcher** (collision in concept) |
| outswap (own idea) | 2 | camdoherty/outswap (display-switcher CLI) |
| soundhop (own idea) | 11 | small unrelated repos |

Web search also turned up close-concept Windows projects that are not name collisions: caendesilva/WindowsToggleAudioDevice (a PowerShell script built on AudioDeviceCmdlets), bendiknesbo/AudioDefaultDeviceSwitcher (an ini-driven two-device toggle exe, the closest in spirit), valleyman86/DefaultAudioSwitcher, yan0lovesha/AudioSwitch (Rust), irasychan/simple-audio-switch (CLI using IPolicyConfig), nixpare/AudioSwitch (Go), luizbossoi/windows-audio-switcher-standalone, and "Audio Device Switch" on itch.io (a tray app).

### 1.3 Candidate evaluation

| Candidate | Clarity | Collision risk | CLI | G HUB / Task Manager | Verdict |
|---|---|---|---|---|---|
| **toggle-audio** | High: verb + object, says exactly what one keypress does | Low. Same-named repos are Linux applets; no Windows exe, crate, winget or choco package uses it | `toggle-audio`, 12 chars | "Toggle Audio" reads like a key label | **Pick** |
| audio-toggle | High | Low (same pool) | fine | "Audio Toggle" reads as a noun or widget, not an action | Second choice |
| ToggleAudio / AudioToggle | same as above | same | CamelCase is awkward at a CLI | fine | Use only as the display form "Toggle Audio" |
| audioflip / AudioFlip | Medium ("flip" also suggests L/R channel flip; web results were literally about flipping channels) | **High**: a Windows C# hotkey device switcher *and* a macOS app share the name | good | good | Reject |
| FlipAudio | Medium (same L/R ambiguity) | Low | ok | ok | Reject on clarity |
| SoundFlip | Medium | Low-medium | ok | ok | Reject |
| outflip | Low (opaque) | kakaoenterprise/OutFlip | short | "outflip" means nothing to a user | Reject |
| audio-swap | Medium-high | Medium (AudioSwap = swapping audio tracks in video) | ok | ok | Reject |
| SpeakerSwap | Medium. Misleading, because the main use case is speakers to **headphones** | None | ok | ok | Reject |
| ToggleOut | Low (out of what?) | None | ok | ambiguous | Reject |
| audiohop (own) | Medium | macOS AudioHop exists | short | ok | Reject |
| outswap (own) | Low | display-switcher repo | short | opaque | Reject |
| soundhop (own) | Low-medium | small | short | ok | Reject |

**Why toggle-audio wins.** It is the most literal name, so it is the best for search and for a key label. It is free on every package registry that matters for a Windows tool, and its GitHub namesakes are on other platforms. A distinctive brand name (AudioFlip, AudioHop) buys nothing here: the good ones are already taken by direct competitors, and the free ones are opaque.

**Disambiguation tactic.** Use the GitHub description "Toggle the Windows default audio output between two devices, instantly", and the topics `windows`, `audio`, `default-audio-device`, `hotkey`, `logitech-g-hub`, `rust`, `core-audio`, `bluetooth-headphones`, so it doesn't get confused with the GNOME and Cinnamon applets.

### 1.4 Adjacent tools (for the README "Why another one?" section)

- **SoundSwitch** (https://github.com/Belphemur/SoundSwitch): a full-featured C#/.NET tray app that cycles devices with its own global hotkeys. Excellent, but it is a resident process and needs the .NET runtime.
- **AudioSwitcher** (https://github.com/xenolightning/AudioSwitcher): a .NET tray app plus the popular `AudioSwitcher.AudioApi` library. The app is largely unmaintained.
- **EarTrumpet** (https://github.com/File-New-Project/EarTrumpet): a per-app volume mixer that replaces the tray volume flyout, and can change the default device from it. It is mouse-driven; there is no "toggle two devices" exe.
- **NirCmd `setdefaultsounddevice`** (https://www.nirsoft.net/utils/nircmd.html): freeware, closed source. It sets one device by (locale-dependent) name, so toggling needs a wrapper script to track state.
- **SoundVolumeView** (https://www.nirsoft.net/utils/sound_volume_view.html): freeware, closed source GUI and CLI. `/SwitchDefault` can alternate between two devices, which is the closest equivalent. It has no installer and no settings UI for the pair.
- **AudioDeviceCmdlets** (https://github.com/frgnca/AudioDeviceCmdlets): a PowerShell module and the basis of the current POC. Starting the PowerShell host and loading the module (or a ps2exe wrapper) costs about 1 s per toggle.
- Others in the same niche: bendiknesbo/AudioDefaultDeviceSwitcher (ini-configured toggle exe, C#), yan0lovesha/AudioSwitch (Rust CLI), irasychan/simple-audio-switch (CLI).

**Honest positioning (README copy):**
> Toggle Audio does one thing: each run flips the Windows default playback device between the two outputs you picked, then exits. It is a single native exe with no runtime, no tray icon and no background process. It finishes in a few milliseconds (see Benchmarks), so it works well behind a macro key (Logitech G HUB, Razer Synapse, Stream Deck, AutoHotkey, a desktop shortcut). If you want a tray app with profiles and per-app routing, use SoundSwitch or EarTrumpet. They're great.

Also state that the default-device switch uses the undocumented `IPolicyConfig` COM interface, the same one SoundSwitch, AudioDeviceCmdlets and NirSoft tools use, because Windows has no public API for it.

---

## Part 2: OSS conventions for a small Rust Windows utility

### 2.1 Recommended repository layout
```
toggle-audio/
  .github/
    ISSUE_TEMPLATE/ bug_report.yml, feature_request.yml, config.yml
    PULL_REQUEST_TEMPLATE.md
    dependabot.yml
    workflows/ ci.yml, release.yml
  assets/          icon.ico, demo.gif, screenshot-settings.png
  bench/           C reference + other-language impls + harness (excluded from crate)
  installer/       WiX source (Package.wxs), license.rtf, banner bitmaps
  src/             main.rs, audio.rs (COM), config.rs, gui/
  build.rs         embeds icon + VERSIONINFO + app manifest (winresource / embed-resource)
  Cargo.toml, Cargo.lock (commit the lock file: binary crate)
  rust-toolchain.toml (optional: channel = "stable", components = rustfmt, clippy)
  rustfmt.toml, .editorconfig, .gitattributes, .gitignore
  README.md, CHANGELOG.md, LICENSE, CONTRIBUTING.md, CODE_OF_CONDUCT.md, SECURITY.md
```
Either keep the PowerShell POC as `legacy/toggle-audio.ps1` or drop it and mention it in the CHANGELOG. The Rust toolchain on this machine (1.72) is too old for the `[lints]` table (stable since 1.74) and edition 2024 (1.85). Run `rustup update stable` first.

### 2.2 README.md outline
1. `# Toggle Audio` and the tagline, then badges: CI status (`https://github.com/hotdogee/toggle-audio/actions/workflows/ci.yml/badge.svg`), latest release (shields.io `github/v/release/hotdogee/toggle-audio`), license MIT, downloads (shields.io `github/downloads/hotdogee/toggle-audio/total`), and winget (shields.io `winget/v/Hotdogee.ToggleAudio`) once it is published.
2. Hero: `assets/demo.gif` (G1 press → the Windows volume flyout shows the device change), with a `<!-- TODO: hero GIF -->` placeholder until it is recorded, plus a settings-window screenshot.
3. **Features**: switches in a few ms; native, small exe with no runtime; no background process; settings GUI listing active playback devices by friendly name (Unicode/CJK-safe); stores stable endpoint IDs rather than names; sets all roles (Console, Multimedia, Communications); MSI installer into Program Files.
4. **Why another one?** (the positioning paragraph and alternatives list from 1.4)
5. **Install**
   - MSI: `toggle-audio-x.y.z-x64.msi` from Releases. Silent: `msiexec /i toggle-audio-x.y.z-x64.msi /qn`.
   - winget (once accepted): `winget install Hotdogee.ToggleAudio`
   - cargo: `cargo install toggle-audio`
   - Portable zip, plus checking it against `SHA256SUMS` (`Get-FileHash`).
   - SmartScreen note: until the binaries are code-signed, click More info, then Run anyway.
6. **Usage**
   - First run, or `toggle-audio --settings`: pick Device 1 and Device 2.
   - CLI table (adjust to the final CLI): `toggle-audio` (toggle), `--settings`, `--list` (devices and IDs), `--version`, `--help`; exit codes.
   - **Logitech G HUB binding**: G HUB → select the keyboard → Assignments → System → "Launch Application" (create a new one) → browse to `C:\Program Files\Toggle Audio\toggle-audio.exe` → name it "Toggle Audio" → drag it onto G1. Add one-liners for Stream Deck ("Open"), AutoHotkey (`F13::Run "C:\Program Files\Toggle Audio\toggle-audio.exe"`), and a Windows shortcut with a "Shortcut key".
7. **Configuration**: the `%APPDATA%\toggle-audio\config.toml` path, an annotated example (`device1 = "{0.0.0.00000000}.{...}"`, `device2 = ...`, optional cached display names), and the behavior when a device is missing (e.g. Bluetooth transmitter unplugged: what happens and which exit code).
8. **Benchmarks**: a table of median and p95 ms per toggle for the PowerShell POC (ps2exe), C, Rust and the other implementations; methodology (warm-up, N runs, tool, Ryzen 9 7950X, Windows 11 build 26300); a link to `bench/README.md`. Be candid that audio-service propagation time is outside the process.
9. **How it works**: about 5 lines on MMDevice enumeration, IPolicyConfig, and why there is no tray or background process.
10. **Building from source**: prerequisites (Rust stable ≥ MSRV, MSVC Build Tools, Windows SDK; WiX for the MSI), `cargo build --release`, `cargo test`, the MSI build command.
11. **Contributing**: link to CONTRIBUTING.md and the Code of Conduct.
12. **License**: MIT © 2026 Han Lin. **Acknowledgements**: AudioDeviceCmdlets, SoundSwitch, windows-rs.

### 2.3 LICENSE
- The Rust ecosystem convention for **libraries** is `MIT OR Apache-2.0` (Rust API Guidelines C-PERMISSIVE, https://rust-lang.github.io/api-guidelines/necessities.html). It exists so downstream crates can combine code freely, and for Apache's patent grant.
- This is an **end-user application** that nobody depends on as a library. **Recommend plain MIT**: one `LICENSE` file, one license page in the MSI dialog (`installer/license.rtf`), one badge. `Cargo.toml`: `license = "MIT"`.
- If a reusable library crate is ever split out, switch to dual `MIT OR Apache-2.0` (`LICENSE-MIT` + `LICENSE-APACHE`) at that point.
- Text: the standard MIT text with `Copyright (c) 2026 Han Lin`.
- Optional: generate `THIRD-PARTY-NOTICES` with `cargo about` in release.yml and ship it in the MSI (windows-rs is MIT/Apache-2.0).

### 2.4 CHANGELOG.md
Use Keep a Changelog 1.1.0 (https://keepachangelog.com/en/1.1.0/) with SemVer 2.0.0 (https://semver.org/). Sections are `## [Unreleased]` and `## [0.1.0] - YYYY-MM-DD`, with the groups Added / Changed / Deprecated / Removed / Fixed / Security, and compare links at the bottom. First entry: "Initial native release; replaces the PowerShell POC (switch-audio)." release.yml extracts the matching section as the release notes.

### 2.5 CONTRIBUTING.md
Prerequisites; exactly the commands CI runs (`cargo fmt --all -- --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`); tests that touch real audio devices are `#[ignore]` and run manually with `cargo test -- --ignored` because they change the default device; Conventional Commits; small PRs; update CHANGELOG `[Unreleased]`; how to run benches and build the MSI locally; no CLA or DCO.

### 2.6 CODE_OF_CONDUCT.md
Contributor Covenant **2.1** verbatim (https://www.contributor-covenant.org/version/2/1/code_of_conduct/). Leave the enforcement contact as a placeholder for the user to fill (an email address or GitHub private reporting). Do not fill in his personal email without his decision.

### 2.7 SECURITY.md
- Supported versions: latest minor only.
- Report via **GitHub Private Vulnerability Reporting** (enable it in Settings → Code security), not public issues. Acknowledgment within 7 days.
- Scope: the app runs unelevated, has no network access, and only reads and writes its own config file. The MSI is per-machine (UAC at install time only).

### 2.8 .gitignore
```gitignore
# Rust (Cargo.lock IS committed: binary crate)
/target/
**/*.rs.bk

# Windows
Thumbs.db
ehthumbs.db
Desktop.ini
$RECYCLE.BIN/

# Visual Studio / MSVC (bench/c)
.vs/
*.obj
*.pdb
*.ilk
*.exp
*.lib
*.user
*.suo
x64/
[Dd]ebug/
[Rr]elease/

# .NET (bench/csharp)
bin/
obj/
*.nupkg

# WiX / installer output
*.wixobj
*.wixpdb
*.msi
*.cab

# Bench output
bench/results/
bench/out/

# Editors
.vscode/*
!.vscode/extensions.json
.idea/
*.swp
```

### 2.9 .editorconfig
```ini
root = true

[*]
charset = utf-8
end_of_line = lf
insert_final_newline = true
trim_trailing_whitespace = true
indent_style = space
indent_size = 4

[*.{yml,yaml,json,toml,md}]
indent_size = 2

[*.md]
trim_trailing_whitespace = false

[*.{ps1,psm1,bat,cmd,rc,wxs,wxl,sln,vcxproj,csproj}]
end_of_line = crlf

[*.{ps1,psm1}]
# Windows PowerShell 5.1 misreads BOM-less UTF-8 (CJK device names)
charset = utf-8-bom
```

### 2.10 .gitattributes
```gitattributes
* text=auto eol=lf

# Windows-native formats keep CRLF
*.ps1     text eol=crlf
*.psm1    text eol=crlf
*.bat     text eol=crlf
*.cmd     text eol=crlf
*.rc      text eol=crlf
*.wxs     text eol=crlf
*.wxl     text eol=crlf
*.sln     text eol=crlf
*.vcxproj text eol=crlf
*.csproj  text eol=crlf

# Binaries
*.ico binary
*.png binary
*.gif binary
*.bmp binary
*.rtf binary
*.msi binary
*.exe binary

# Keep bench code out of GitHub language stats
bench/** linguist-vendored
```
A hand-written `.rc` should be UTF-8 with `#pragma code_page(65001)` so non-ASCII strings (©, CJK) compile correctly. It is better to generate resources from `build.rs` (`winresource` or `embed-resource` crate).

### 2.11 rustfmt.toml
Keep it minimal and stable-only (no nightly options):
```toml
edition = "2024"
newline_style = "Unix"
use_field_init_shorthand = true
use_try_shorthand = true
```

### 2.12 Cargo.toml metadata and `[lints]`
```toml
[package]
name = "toggle-audio"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"   # raise to whatever the chosen windows-rs version requires
description = "Toggle the Windows default audio output between two devices instantly"
license = "MIT"
repository = "https://github.com/hotdogee/toggle-audio"
readme = "README.md"
keywords = ["windows", "audio", "toggle", "hotkey", "default-device"]
categories = ["command-line-utilities", "multimedia::audio"]
exclude = ["bench/", "installer/", "assets/*.gif", ".github/"]

[package.metadata.docs.rs]
targets = ["x86_64-pc-windows-msvc"]

[lints.rust]
unsafe_op_in_unsafe_fn = "deny"
unused_qualifications = "warn"

[lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
module_name_repetitions = "allow"
missing_errors_doc = "allow"
missing_panics_doc = "allow"
undocumented_unsafe_blocks = "warn"   # every COM unsafe block gets a // SAFETY: comment
unwrap_used = "warn"

[profile.release]
lto = true
codegen-units = 1
panic = "abort"
strip = true
```
CI turns these warnings into failures with `-D warnings`. The `priority = -1` on lint groups is required so individual overrides take precedence.

### 2.13 Issue templates (YAML issue forms)
- `bug_report.yml` fields:
  - Toggle Audio version (required)
  - Windows version/build (required)
  - Install method (dropdown: MSI, winget, cargo, portable)
  - Launched from (dropdown: G HUB, other macro software, shortcut, terminal)
  - Devices: paste `toggle-audio --list` output
  - What happened vs. expected (required)
  - Config file (`render: toml`)
  - Checkbox: "I searched existing issues"
- `feature_request.yml` fields: problem, proposed solution, alternatives considered, "willing to send a PR?".
- `config.yml`: `blank_issues_enabled: false`; contact_links point to Discussions (Q&A) and to the security policy.

### 2.14 PULL_REQUEST_TEMPLATE.md
Summary; `Closes #`; change type (feat/fix/perf/docs/build/ci/refactor). Checklist:
- fmt, clippy and test pass
- CHANGELOG `[Unreleased]` updated
- tested on real hardware (which devices)
- screenshots for GUI changes

### 2.15 dependabot.yml
```yaml
version: 2
updates:
  - package-ecosystem: cargo
    directory: /
    schedule:
      interval: weekly
    groups:
      windows-rs:
        patterns: ["windows*"]
      minor-and-patch:
        update-types: [minor, patch]
    commit-message:
      prefix: build
  - package-ecosystem: github-actions
    directory: /
    schedule:
      interval: monthly
    commit-message:
      prefix: ci
```

### 2.16 CI workflow (`.github/workflows/ci.yml`)
- Triggers: push to `main`, `pull_request`, `workflow_dispatch`. Set `permissions: contents: read`, and use `concurrency` with cancel-in-progress.
- Job `check` on `windows-latest`:
  1. `actions/checkout` (current major)
  2. `dtolnay/rust-toolchain@stable` with `components: rustfmt, clippy`
  3. `Swatinem/rust-cache@v2`
  4. `cargo fmt --all -- --check`
  5. `cargo clippy --all-targets --locked -- -D warnings`
  6. `cargo test --locked`. Hosted runners have **no audio endpoints**, so put device access behind a trait and unit-test config parsing and next-device selection with a fake; real-device tests stay `#[ignore]`.
  7. `cargo build --release --locked`, and upload the exe as a workflow artifact
  8. Optional: build the MSI on pushes to `main` to catch installer breakage early.
- Optional `msrv` job: install the toolchain at `rust-version` and run `cargo check --locked`.
- Hardening option: pin third-party actions by commit SHA; Dependabot keeps the SHAs current.

### 2.17 Release workflow (`.github/workflows/release.yml`)
- Trigger: `push: tags: ['v*']`. Set `permissions: contents: write`, plus `id-token: write` and `attestations: write` if using provenance.
- Steps on `windows-latest`:
  1. Checkout, toolchain, cache.
  2. Fail fast if the tag does not equal `v` + the Cargo.toml version.
  3. `cargo build --release --locked`
  4. Build the MSI. Install WiX as a pinned .NET global tool (`dotnet tool install --global wix --version X`), then run `wix build`.
     - **WiX v7 enforces its Open Source Maintenance Fee EULA**: commands are blocked until the EULA is accepted (a `wix eula accept ...` step or an equivalent flag; verify the exact syntax for the pinned version).
     - The fee is required only when WiX is used to generate revenue; an individual's free OSS project is not paying (https://docs.firegiant.com/wix/osmf/).
     - Pinning **WiX v5** avoids the EULA step entirely.
     - `cargo-wix` (https://github.com/volks73/cargo-wix) is an optional wrapper.
  5. Package `toggle-audio-<ver>-x64.msi` and `toggle-audio-<ver>-x64.zip` (exe + LICENSE + README).
  6. Write `SHA256SUMS` (sha256sum format, one line per artifact) with PowerShell `Get-FileHash -Algorithm SHA256`, lowercased, with ASCII encoding.
  7. Optional: `actions/attest-build-provenance` for the exe and msi (verify with `gh attestation verify`).
  8. Create the release with the preinstalled GitHub CLI, using the CHANGELOG section for `<ver>` as notes. This avoids a third-party release action; `softprops/action-gh-release@v2` is the common alternative.
     ```
     gh release create $TAG *.msi *.zip SHA256SUMS --title "Toggle Audio <ver>" --notes-file notes.md
     ```
  9. Optional: `cargo publish` (needs a `CARGO_REGISTRY_TOKEN` secret).
- **Code signing** (recommended before promoting widely, optional for winget): unsigned binaries trigger SmartScreen. Free route for OSS: SignPath Foundation (https://signpath.org). Paid route: Azure Trusted Signing (about $10/month).

### 2.18 winget-pkgs submission (later)
- Guide: https://github.com/microsoft/winget-pkgs/blob/master/doc/Authoring.md
- Manifest fields:
  - PackageIdentifier `Hotdogee.ToggleAudio`; Publisher `Han Lin`; License `MIT`; tags audio, toggle, default-device, headphones, hotkey.
  - InstallerType `wix` (or `msi`), Architecture `x64`, Scope `machine`.
  - InstallerUrl is the immutable GitHub Release asset URL, and InstallerSha256 must match it.
- **Correction to the brief: "stable ProductCode/UpgradeCode" is half wrong.** The MSI **UpgradeCode must stay constant forever**. The **ProductCode should change every version**: in WiX, leave it auto-generated together with `<MajorUpgrade>` so each install cleanly replaces the previous one. The manifest lists the per-version `ProductCode` (and the `UpgradeCode` under `AppsAndFeaturesEntries`) so winget can correlate installed versions.
- Silent install: for msi/wix installer types, winget supplies the msiexec quiet and passive switches itself, so no `InstallerSwitches` are needed. The MSI must complete without required UI input and without a forced reboot.
- Keep the ARP DisplayVersion equal to the package version (the MSI ProductVersion is `major.minor.build`, so 0.1.0 works).
- Tooling: `wingetcreate new <msi-url>` (Microsoft) or `komac` generates the manifest and the PR. Do the first submission manually. Later releases can be automated with the `vedantmgoyal9/winget-releaser` action and a PAT that can push to the user's winget-pkgs fork.
- Validation installs the package in a VM and scans it with Defender. Unsigned installers are accepted.

### 2.19 cargo-dist (`dist`) as an alternative
cargo-dist (axodotdev, https://github.com/axodotdev/cargo-dist; v0.32, May 2026) generates a release workflow with archives, checksums, shell and PowerShell installers, and an optional MSI built with **WiX v3** (end-of-life).

**Verdict: not worth it here.**
- There is one target (x86_64-pc-windows-msvc).
- The MSI needs custom content: Program Files layout, a "Toggle Audio Settings" Start Menu shortcut, maybe an autostart option.
- dist's generated workflow is large and opinionated, made for multi-platform CLIs.

A ~60-line hand-written release.yml plus a WiX v5/v7 `.wxs` is easier to own. Reconsider only if non-Windows targets ever appear, which is unlikely for a Core Audio tool.

### 2.20 Versioning and commits
- First release **0.1.0**. It is a rewrite with a new config format and CLI, and a 0.x version honestly says "the interface may still change". Release **1.0.0** once the config format and CLI are frozen, the MSI upgrade path (0.x → 1.0) is verified, and the winget package is live.
- Tags are `vMAJOR.MINOR.PATCH`; pushing a tag triggers release.yml.
- **Conventional Commits 1.0** (https://www.conventionalcommits.org/):
  - Types and scopes: `feat(gui):`, `fix(audio):`, `perf:`, `build(installer):`, `ci:`, `docs:`, `refactor:`, `test:`, `chore:`.
  - Mark breaking changes with `!` or a `BREAKING CHANGE:` footer.
  - Squash-merge PRs so the PR title becomes the commit message.
  - `git-cliff` can draft CHANGELOG entries, but hand-curated Keep a Changelog is fine at this size.
- Repo settings:
  - Protect `main` (require CI).
  - Enable Discussions and Private Vulnerability Reporting.
  - Set the description and topics (1.3) and a social preview image.

---

## Sources
- Live queries 2026-10-04: crates.io API, npm registry, GitHub search API, `winget search`, Chocolatey OData API.
- Cyp9715/AudioFlip: https://github.com/Cyp9715/AudioFlip
- AudioFlip (macOS): https://alternativeto.net/software/audioflip/about/
- Name-collision repos:
  - https://github.com/Blackstareye/toggleAudio-blackeyeprojects.de
  - https://github.com/zonaston/toggle-audio
  - https://github.com/jrunning/Toggle-Audio-Devices
  - https://github.com/olQwQlo/Audio-Toggle
  - https://github.com/isIbra/AudioHop
  - https://github.com/camdoherty/outswap
  - https://github.com/kakaoenterprise/OutFlip
- Similar Windows tools:
  - https://github.com/bendiknesbo/AudioDefaultDeviceSwitcher
  - https://github.com/yan0lovesha/AudioSwitch
  - https://github.com/irasychan/simple-audio-switch
  - https://github.com/valleyman86/DefaultAudioSwitcher
  - https://github.com/caendesilva/WindowsToggleAudioDevice
  - https://dev.to/emmadscodes/toggle-windows-audio-output-devices-from-the-command-line-1gk3
- Established tools:
  - SoundSwitch: https://github.com/Belphemur/SoundSwitch
  - AudioSwitcher: https://github.com/xenolightning/AudioSwitcher
  - EarTrumpet: https://github.com/File-New-Project/EarTrumpet
  - NirCmd: https://www.nirsoft.net/utils/nircmd.html
  - SoundVolumeView: https://www.nirsoft.net/utils/sound_volume_view.html
  - AudioDeviceCmdlets: https://github.com/frgnca/AudioDeviceCmdlets
- Rust API Guidelines (licensing): https://rust-lang.github.io/api-guidelines/necessities.html
- Conventions:
  - Keep a Changelog: https://keepachangelog.com/en/1.1.0/
  - SemVer: https://semver.org/
  - Conventional Commits: https://www.conventionalcommits.org/
  - Contributor Covenant 2.1: https://www.contributor-covenant.org/version/2/1/code_of_conduct/
- winget-pkgs authoring: https://github.com/microsoft/winget-pkgs/blob/master/doc/Authoring.md
- WiX and packaging:
  - WiX OSMF: https://docs.firegiant.com/wix/osmf/
  - WiX releases: https://github.com/wixtoolset/wix/releases
  - cargo-wix: https://github.com/volks73/cargo-wix
  - cargo-dist: https://github.com/axodotdev/cargo-dist
