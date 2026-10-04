# Toggle Audio: design

Status: approved baseline for v0.1.0, 2026-10-04. Research behind each decision lives in `docs/research/`.

## 1. Problem and goals

A Logitech G HUB key binding launches an executable that must flip the Windows default playback device between two user-chosen endpoints. The PowerShell + AudioDeviceCmdlets proof of concept (packaged with ps2exe) takes about 900 ms per toggle because it boots a PowerShell runtime, imports a module and JITs. Goals, in priority order:

1. Toggle latency close to the process-start floor. Target: under 50 ms end-to-end on the reference machine (research measured ~4 ms COM setup and 5–12 ms per `SetDefaultEndpoint` call, so the floor is Windows, not the language).
2. Zero runtime dependencies: native x64 executables that run on a clean Windows 10/11 install. Static CRT, no VC++ redistributable, no .NET.
3. A small native settings dialog to pick Device 1 / Device 2 from the active playback endpoints, test the toggle, and copy the command for G HUB.
4. Per-machine MSI into `C:\Program Files\Toggle Audio\` with Start Menu entry, upgrade and uninstall.
5. GitHub-ready open source repository (`hotdogee/toggle-audio`, MIT) with CI, docs, reproducible benchmarks and release automation.

Non-goals for v1: tray icon, hotkey registration (G HUB owns the key), recording devices, per-app routing, notifications, dark mode.

## 2. Name

**Toggle Audio** / `toggle-audio` (research: free on crates.io, winget, npm, Chocolatey; the only GitHub collisions are Linux desktop applets). Identifiers: repo `hotdogee/toggle-audio`; crate `toggle-audio`; executables `toggle-audio.exe` and `toggle-audiow.exe`; MSI product "Toggle Audio", manufacturer "Han Lin"; Start Menu "Toggle Audio Settings"; winget id `Hotdogee.ToggleAudio`; tagline "Flip Windows audio output between two devices in milliseconds: one tiny exe, perfect for a hotkey."

## 3. Language

Product: **Rust** (edition 2024, `rust-version = "1.85"`), `windows = "=0.62.2"` + `windows-core = "=0.62.2"` pinned (newer `windows-core` 0.100 has no matching `windows` release). Rationale: the benchmark survey shows startup differences between native languages are a few milliseconds, so the choice is driven by safety for the COM/unsafe surface, the ecosystem for the GUI/build/CI story and single-language maintenance. C stays as the speed reference in `bench/c`.

Benchmark implementations (C, Rust, C# NativeAOT, Go, Zig, PowerShell baselines) are first-class repo content under `bench/<lang>/` (user requirement): each is the best optimized version for its toolchain, with a build script, README and the common bench CLI contract, and CI compiles all of them.

## 4. Executables and console policy

One library crate (`src/lib.rs`) and two thin binaries (python.exe / pythonw.exe convention):

| Binary | Subsystem | Purpose |
| --- | --- | --- |
| `toggle-audio.exe` | Console, with an embedded manifest `consoleAllocationPolicy = detached` | Command-line tool. Shells wait for it, pipes and redirection work. On Windows 11 24H2+ a launch from G HUB/Explorer creates **no** console window. |
| `toggle-audiow.exe` | Windows (GUI) | Same program, never creates a console on any Windows version. Target for hotkeys on Windows 10 / Windows 11 before 24H2, and for the Start Menu shortcut (`toggle-audiow.exe settings`). Uses `AttachConsole(ATTACH_PARENT_PROCESS)` for best-effort output when run from a terminal. |

Both carry the same manifest (common controls v6, PerMonitorV2 DPI, asInvoker, supportedOS) and VERSIONINFO via `assets/app.rc` compiled by `embed-resource` in `build.rs`. The settings dialog shows and copies the path of the exe it is running from, so the user binds whatever they launched Settings with (the Start Menu shortcut gives `toggle-audiow.exe`).

## 5. Command line

Hand-rolled parser (no clap: startup budget and binary size). Subcommands and flags:

| Invocation | Behavior |
| --- | --- |
| `toggle-audio` / `toggle-audio toggle` | Toggle between the configured devices. With no config: open Settings and explain (exit 3 if Settings is cancelled). |
| `toggle-audio list` | Active playback endpoints, one per line: `<id>\t<name>\t<flags>` where flags contain `*` for the default device and `c` for the default communications device. |
| `toggle-audio get` | `<id>\t<name>` of the current default playback device. |
| `toggle-audio set <id-or-name>` | Make the endpoint default for the configured roles. Accepts a full endpoint id, an exact friendly name, or a unique case-insensitive substring of a friendly name. |
| `toggle-audio settings` (aliases `--settings`, `config`, `gui`) | Open the settings dialog. |
| `toggle-audio --help` / `-h`, `--version` / `-V` | Usual. |
| `--timing` | Append per-phase timings (microseconds since process start, QPC based) to stderr. |
| `--comm` / `--no-comm` | Override the configured "also switch Communications" setting for this run. |

Exit codes: 0 success; 1 unexpected/COM error; 2 usage error; 3 no or invalid config; 4 device not found / not active.

Error presentation: if a console or redirected stderr is available, write to stderr. Otherwise (hotkey launch, no console) show a topmost `MessageBoxW` titled "Toggle Audio" so a failed hotkey never fails silently. Success is silent when there is no console.

Output encoding: console handles get UTF-16 via `WriteConsoleW` (CJK names render regardless of code page); redirected handles get UTF-8 with `\n` line endings.

## 6. Toggle semantics (pure function, unit-tested)

```
choose_target(current: Option<&str>, device1, device2, is_active: impl Fn(&str) -> bool)
  preferred, fallback =
      if current == device1.id { (device2, None) }          # on device1 -> go to device2
      else                     { (device1, Some(device2)) } # anywhere else -> go to device1, else device2
  if is_active(preferred.id)                    -> Ok(Target { device: preferred, warning: None })
  else if fallback is active and != current     -> Ok(Target { device: fallback, warning: Some(PreferredUnavailable) })
  else                                          -> Err(ToggleError::NoDeviceAvailable)
```

Mirrors the proof of concept (device1 unless device1 is current) but survives an unplugged device without a silent no-op. `is_active` is `IMMDeviceEnumerator::GetDevice(id)` + `GetState() == DEVICE_STATE_ACTIVE` (GetDevice succeeds for unplugged/disabled devices, so the state check is mandatory).

Roles: `eConsole` and `eMultimedia` always; `eCommunications` when `switch_communications` is true (default true). Order console, multimedia, communications. Skip a role whose current default already is the target.

## 7. Core Audio access (`src/audio.rs`, the only module allowed to touch COM besides the GUI)

- `CoInitializeEx(COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE)` (STA is required by the dialog; latency is identical to MTA).
- Enumerate: `IMMDeviceEnumerator::EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)`, names from `PKEY_Device_FriendlyName` (pid 14; exactly what Sound settings shows, e.g. `喇叭 (FiiO BTA30 PRO)`), ids from `IMMDevice::GetId` (free with `CoTaskMemFree`), `PROPVARIANT` cleared with `PropVariantClear`.
- Defaults: `GetDefaultAudioEndpoint(eRender, role)`; `E_NOTFOUND` means no default device.
- Set: `CoCreateInstance(CLSID {870af99c-171d-4f9e-af0d-e63df40c2bc9})` → `IPolicyConfig` `{f8679f50-850a-41cf-9c72-430f290290c8}`, `SetDefaultEndpoint(PCWSTR, ERole)` at 0-based vtable slot 13 (verified against AudioSes.dll symbols and five sources). Fallback if that fails: CLSID `{294935CE-F637-4E7C-A41B-AB255460B862}` → `IPolicyConfigVista` `{568b9108-44bf-40b4-9006-86afe5b5a620}`, slot 12. Declared with `windows_core::interface` as in `docs/research/core-audio-api.md` §C.2 (do not copy the Vista layouts from SoundSwitch/AudioDeviceCmdlets; they carry an extra method).
- Public surface (sketch): `struct Endpoint { id: String, name: String }`, `struct AudioSystem` (owns the COM init guard and enumerator) with `list_active() -> Result<Vec<Endpoint>>`, `default_for(role) -> Result<Option<Endpoint>>`, `is_active(id) -> Result<bool>`, `name_of(id) -> Result<Option<String>>`, `set_default(id, &[Role]) -> Result<()>`. Errors carry the HRESULT and the failing call.

Hot path for `toggle`: parse args → read config → `AudioSystem::new()` → `default_for(Console)` → `choose_target` with `is_active` → `set_default`. No full enumeration.

## 8. Configuration (`src/config.rs`)

Path: `%APPDATA%\toggle-audio\config.json` (`SHGetKnownFolderPath(FOLDERID_RoamingAppData)`, falling back to the `APPDATA` variable). Written atomically (write `config.json.tmp`, then `MoveFileExW` replace). Unknown fields ignored; `switch_communications` defaults to true; `name` is informational (used in errors and in the dialog when the device is not connected).

```json
{
  "version": 1,
  "device1": { "id": "{0.0.0.00000000}.{739b3554-bfed-4d61-b407-a818b317c991}", "name": "PG42UQ (NVIDIA High Definition Audio)" },
  "device2": { "id": "{0.0.0.00000000}.{30045f40-8cfd-4441-bb89-0d13fc19b589}", "name": "喇叭 (FiiO BTA30 PRO)" },
  "switch_communications": true
}
```

## 9. Settings dialog (`src/gui/`)

`IDD_SETTINGS DIALOGEX` resource in `assets/app.rc` (dialog units, Segoe UI 9 pt, `DS_SETFONT`), run with `DialogBoxParamW`; the dialog manager handles fonts, Tab/mnemonics, Enter/Esc and PerMonitorV2 rescaling. Layout and control ids follow `docs/research/gui.md` (two `CBS_DROPDOWNLIST` combos, communications checkbox, read-only path field + Copy, status text, Test toggle, Save, Cancel). Behavior:

- Combos list active render endpoints (names; the item data indexes a `Vec<Endpoint>`), preselected from config. A configured id that is not currently active is appended as "(not connected) <name>" and kept selectable so the setting is not lost.
- Save validates device1 ≠ device2, writes the config atomically, shows "Saved to <path>" in the status line, and closes.
- Test toggle runs the real toggle logic with the current, unsaved selections and reports the new default in the status line.
- Copy puts the current exe path (quoted when it contains spaces) on the clipboard.
- Dialog is centered on the monitor under the cursor; single instance via a named mutex is optional.

Measured in the prototype: ~60 ms to first paint including enumeration, ~130 KB binary.

## 10. Repository layout

```
toggle-audio/
├── Cargo.toml  Cargo.lock  rust-toolchain.toml  .cargo/config.toml (+crt-static)
├── build.rs                      # embed-resource: assets/app.rc (manifest, icon, dialog, VERSIONINFO)
├── src/
│   ├── lib.rs                    # pub fn run(args) -> ExitCode; module wiring
│   ├── bin/toggle-audio.rs       # console subsystem; calls toggle_audio::run
│   ├── bin/toggle-audiow.rs      # #![windows_subsystem = "windows"]; attaches parent console
│   ├── cli.rs  config.rs  toggle.rs  audio.rs  console.rs  timing.rs  error.rs
│   └── gui/ (mod.rs, dialog.rs, clipboard.rs)
├── assets/ (app.rc, app.manifest, resource.h, icon/toggle-audio.ico, screenshot.png)
├── installer/ (toggle-audio.wxs, build-msi.ps1, License.rtf)
├── bench/ (c/ rust/ csharp/ go/ zig/ powershell/ baseline/ run-bench.ps1 RESULTS.md results/)
├── scripts/ (e2e.ps1 real-device round trip, build-all.ps1)
├── tests/ (integration tests, #[ignore] unless an audio device is present)
├── docs/ (DESIGN.md, research/, benchmarks.md, packaging.md)
├── .github/ (workflows/ci.yml, workflows/release.yml, ISSUE_TEMPLATE/, PULL_REQUEST_TEMPLATE.md, dependabot.yml)
└── README.md LICENSE CHANGELOG.md CONTRIBUTING.md CODE_OF_CONDUCT.md SECURITY.md
    .gitignore .editorconfig .gitattributes rustfmt.toml
```

Cargo: `[workspace] exclude = ["bench/rust"]` so the standalone bench crate keeps its own profile and lock file. Release profile: `opt-level = 3`, `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = true`; `.cargo/config.toml` sets `-C target-feature=+crt-static` for `x86_64-pc-windows-msvc`. Lints: `[lints.rust] unsafe_op_in_unsafe_fn = "warn"`, `[lints.clippy] all = "warn", pedantic = "warn", undocumented_unsafe_blocks = "warn", unwrap_used = "warn"` with targeted allows.

## 11. Quality bar

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` green locally and in CI (windows-latest).
- Unit tests: toggle decision table; config round trip (CJK names, missing/unknown fields, corrupt JSON, atomic write); CLI parser (every subcommand, alias, flag, error); console encoding helpers.
- Integration tests (`#[ignore]`, run with `cargo test -- --ignored` on a machine with audio): enumeration non-empty; `get` equals `GetDefaultAudioEndpoint`; `set` to the current default is a no-op; toggle round trip restores the original default (a guard restores it even on panic).
- `scripts/e2e.ps1`: builds, runs `list`/`get`, toggles PG42UQ ↔ PHL BDM4065 twice and verifies with AudioDeviceCmdlets as an independent oracle, restores the original default.
- `unsafe` only in `audio.rs`, `console.rs`, `gui/`; every `unsafe` block has a `// SAFETY:` comment.
- Reviewed by a panel (COM/unsafe correctness; robustness and UX; code quality and tests) before packaging.
- Hosted CI runners have no audio devices: device access stays behind `AudioSystem` and real-device tests are `#[ignore]`.

## 12. Benchmarks (`bench/`)

Common bench CLI contract (from `docs/research/benchmark-method.md` §1): `list`, `get`, `set <id>`, `toggle <idA> <idB>`, no-args exits 1 without COM work, `--timing` phase stamps on stderr, exit codes 0/1/2/3/4, UTF-8 output. Deviation from that document: bench binaries are built for the **console** subsystem with the `consoleAllocationPolicy=detached` manifest embedded (via `mt.exe` or the toolchain), so hyperfine, output capture and the G HUB no-flash behavior are all covered by one binary per language; the C directory additionally builds `nop.exe`/`nop-con.exe` floors and a GUI-subsystem C variant for comparison. Real-toggle measurements use the two NVIDIA HDMI outputs (PG42UQ ↔ PHL BDM4065) and restore PG42UQ. Results go to `bench/RESULTS.md` and are summarized in the README.

Adopted bench contract (what every implementation in `bench/` actually does; this supersedes the wording in `docs/research/benchmark-method.md` §1 where they differ):

| Command | stdout | Exit |
| --- | --- | --- |
| `list` | one line per active render endpoint: `<id>\t<name>\t<flags>`, flags `*` (default), `c` (default communications), `*c` or `-` | 0 |
| `get` | `<id>\t<name>` of the current default (eConsole) | 0, or 4 if none |
| `set <id>` | nothing; sets eConsole, eMultimedia, eCommunications in that order after checking the endpoint is ACTIVE | 0; 3 if unknown/inactive |
| `toggle <idA> <idB>` | the id that was set | 0; 3 if the target is unknown/inactive |
| (no args) | one usage line on stderr, no COM work (runtime floor) | 1 |
| `--timing` | on stderr, `phase\t<name>\t<microseconds since process creation>` for `entry`, `com_init`, `enumerator`, `work_done`, `exit` (`entry` is the create-to-entry interval) | — |

Exit 2 means a COM failure (HRESULT printed on stderr). Output is UTF-8 with `\n` line endings when redirected. Known per-language caveats are recorded in each `bench/<lang>/README.md` (for example, .NET NativeAOT initializes COM before `Main`, so its `com_init` phase is near zero and the cost shows in `entry`).

## 13. Packaging (`installer/`)

WiX Toolset 7 (dotnet global tool; `wix eula accept wix7` once or `--acceptEula wix7` in CI), hand-written `toggle-audio.wxs` adapted from `docs/research/packaging.md` §4: per-machine x64, `ProgramFiles64Folder\Toggle Audio` (unversioned so the G HUB binding survives upgrades), `MajorUpgrade` scheduled `afterInstallInitialize`, **UpgradeCode `{C82A4013-F2FF-448E-A4AE-63CD73760A63}` never changes**, ProductCode auto-generated per build, component GUIDs derived, files `toggle-audio.exe`, `toggle-audiow.exe`, `LICENSE`, advertised Start Menu shortcut "Toggle Audio Settings" → `toggle-audiow.exe settings`, App Paths registration for both exes, optional PATH feature (selectable, default on), ARP icon/URLs, `WixUI_InstallDir` with License.rtf. `installer/build-msi.ps1` reads the version from Cargo.toml and runs `cargo build --release` + `wix build`. Validation without admin: `wix msi validate`. Real install/upgrade/uninstall test needs UAC (user). Unsigned for v0.x; README documents SmartScreen and SHA256 verification; SignPath Foundation is the future signing route.

## 14. Release and distribution

`ci.yml` on push/PR: fmt, clippy, test, release build, bench-build job (compiles every `bench/<lang>` implementation), upload artifacts. `release.yml` on tag `v*`: verify tag == Cargo version, build exes and MSI, zip portable build, `SHA256SUMS`, GitHub Release. Versioning: start 0.1.0; 1.0.0 once config/CLI are frozen and the MSI upgrade path is proven. Keep a Changelog + SemVer + Conventional Commits. winget manifest after the first public release.

## 15. Migration for the user

Install the MSI, open "Toggle Audio Settings" from the Start Menu, select `PG42UQ (NVIDIA High Definition Audio)` and `喇叭 (FiiO BTA30 PRO)`, Save, Test. Press Copy and bind the command in G HUB (Assignments → System → Launch application) to the G1 key, replacing `C:\bin\Switch-Audio.exe`. Verify once that the G1 press shows no window; if it ever does, bind `toggle-audiow.exe` instead.

## 16. Deviations in the v0.1.0 implementation

Decided during implementation and review; the sections above keep the original text.

- §4: `toggle-audiow.exe` compiles `assets/app-windowed.rc`, which includes `app.rc` with its own `OriginalFilename`. On Windows versions that ignore `consoleAllocationPolicy` (before 11 24H2), `toggle-audio.exe` frees a console that no other process shares (`GetConsoleProcessList` = 1), so a hotkey launch still reports errors in a message box and opens Settings.
- §5: a toggle without a usable configuration (missing, unreadable or invalid) opens Settings only when there is no console; from a terminal it prints the error with a hint to run `settings` and exits 3. After a first-run Save a message box says to press the hotkey again; that press does not toggle. `set` treats a broken configuration as a warning and uses the default roles. `--timing` starts after parsing (the parsed flag is the only source of truth); stamps still count from process creation. Exit 4 also covers a name that matches several devices: `set` lists the matches (name and id) and asks for the full name or the id. A hotkey press while a Settings dialog is already open (also during first-run setup) brings that dialog to the front and exits 0; only Cancel exits 3. `set` prints the endpoint id when the device has no friendly name.
- §6: "nothing to switch to" names the unavailable device and, when that is the reason, the device that already is the default. Before deciding, the toggle looks up both configured ids (`AudioSystem::lookup`: `GetDevice`, `GetId`, `GetState`), so the comparison with the current default uses Windows' own spelling of each id (another case, or a Windows 11 24H2 stable id that `GetDevice` also accepts, still matches); an id Windows does not know counts as not connected.
- §7: only `eRender` endpoints are accepted (`IMMEndpoint::GetDataFlow`); a recording endpoint id behaves like an unknown id, so `set` can never change the default microphone. The `IPolicyConfigVista` fallback also covers a failing `IPolicyConfig::SetDefaultEndpoint`, not only a missing class. The hot path reads only the current default's id (`default_id`), not its name, and passes the console default it read on to `set_default`, which skips reading it again (each read is a call into the audio service, about 1.2 ms).
- §8: the `APPDATA` variable is read first and `SHGetKnownFolderPath` is the fallback: it saves about 2 ms per toggle and lets tests redirect the configuration. The atomic replace uses `std::fs::rename` (`MoveFileExW` with `MOVEFILE_REPLACE_EXISTING`). Whitespace around ids is trimmed on load.
- §9: Save closes the dialog at once, so no "Saved to" status is shown. Copy copies the plain, unquoted exe path for the Path field of G HUB's Launch Application action (its Browse button produces the same form; still to be confirmed against G HUB). A named mutex (`Local\Hotdogee.ToggleAudio.Settings`) keeps one dialog per session and brings an open one to the front. The dialog uses `DS_SETFOREGROUND` plus a topmost round trip, a three-line status, and appends the end of the endpoint id to friendly names that occur twice.
- §10: `assets/app.manifest` carries the version by hand; a unit test fails when it differs from Cargo.toml. `scripts/` holds `e2e.ps1` and `third-party-notices.ps1` (writes `THIRD-PARTY-NOTICES.txt` from `Cargo.lock`; CI runs it with `-Check`); there is no `build-all.ps1` (`cargo build --release` builds both exes, `installer/build-msi.ps1` the MSI).
- §11: `unsafe` also appears in `config.rs` (the known-folder fallback), `timing.rs` (`GetProcessTimes`) and the capture-enumeration helper in `tests/real_device.rs`, each with a `// SAFETY:` comment. The round-trip integration test runs only when `TOGGLE_AUDIO_TEST_DEVICES=<id1>;<id2>` is set, so `cargo test -- --ignored` never leaves a default changed; one of those tests (`policy_config_reasserts_the_current_defaults`) does call `SetDefaultEndpoint`, with each role's current default, through both interfaces. `tests/cli.rs` runs both binaries (usage errors, `--help`, `--version`, `toggle` without a usable configuration, redirected output bytes) without audio devices, so it runs in CI, and kills a run that does not exit within 30 s.
- §13: the installer UI is `WixUI_FeatureTree` rather than `WixUI_InstallDir`, so "Add to PATH" (feature `PathEnv`) can be switched off in the Custom Setup page, which also has the Browse button for the folder; `ARPNOMODIFY` is not set, so Installed apps keeps Modify for changing that choice later. `LICENSE` is installed as `LICENSE.txt`, next to `THIRD-PARTY-NOTICES.txt` (the licenses of the statically linked crates, also in the zip), and `README.md` is installed when it exists at build time. `installer/License.rtf` is generated by `installer/make-license-rtf.ps1`; `build-msi.ps1` fails when it is stale, rejects pre-release versions (MSI versions have no pre-release field), fails when either exe imports the dynamic C runtime (`VCRUNTIME140.dll`, `api-ms-win-crt-*.dll`), runs cargo from the repository root so `.cargo/config.toml` (+crt-static) applies, reads ProductVersion and UpgradeCode back from the package and writes a `.sha256` file. Build, install and winget details: `docs/packaging.md`.
- §14: the checksum file is `SHA256SUMS.txt`. `ci.yml` also runs `cargo check` on the minimum Rust version (1.85, `rust-version` in Cargo.toml) and checks that `THIRD-PARTY-NOTICES.txt` matches `Cargo.lock`. `release.yml` rejects pre-release versions, which the MSI cannot represent.
