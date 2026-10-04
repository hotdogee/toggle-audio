# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Nothing yet.

## [0.1.0] - 2026-10-04

Initial native release; replaces the PowerShell proof of concept (`Switch-Audio.exe`, built with ps2exe, about 900 ms per toggle).

### Added

- Instant toggle: `toggle-audio` flips the Windows default playback device between two configured endpoints in milliseconds. Sets the Console and Multimedia roles, plus Communications unless disabled (`switch_communications`, `--comm` / `--no-comm`). Falls back to the other device when the preferred one is unplugged, and reports an error instead of a silent no-op when neither is available.
- Two native x64 executables with a static CRT and no runtime dependencies: `toggle-audio.exe` (console subsystem, `consoleAllocationPolicy = detached`, so no console window flashes on Windows 11 24H2 and later) and `toggle-audiow.exe` (GUI subsystem, never creates a console; for hotkeys on older Windows builds).
- Command-line interface: `list` (active playback endpoints with id, name and default flags), `get` (current default device), `set <id-or-name>` (by endpoint id, exact friendly name or a part of a name that matches one device; an ambiguous name exits with code 4 and lists the matches), `settings`, `--help`, `--version` and `--timing` (per-phase timings on stderr). Documented exit codes 0 to 4. Unicode-safe output: UTF-16 to the console, UTF-8 when redirected.
- Errors are shown in a message box when there is no console, so a failed hotkey press never fails silently.
- Settings dialog (`toggle-audio settings`, Start Menu "Toggle Audio Settings"): pick Device 1 and Device 2 from the active playback devices, keep a disconnected device selectable, test the toggle before saving, and copy the program path for Logitech G HUB or any other launcher.
- Per-user configuration in `%APPDATA%\toggle-audio\config.json`, stored by stable endpoint id and written atomically.
- Per-machine MSI installer (WiX Toolset 7) into `C:\Program Files\Toggle Audio\` with a Start Menu shortcut, App Paths registration, an optional PATH entry, in-place upgrades and clean uninstall.
- Reproducible benchmarks under `bench/`: optimized implementations in C, Rust, C# NativeAOT, Go and Zig plus PowerShell baselines, a shared CLI contract, a hyperfine harness and published results (`bench/RESULTS.md`, summarized in `docs/benchmarks.md`).
- `THIRD-PARTY-NOTICES.txt` with the licenses of the statically linked crates, installed by the MSI and included in the zip.
- Documentation: README with install, hotkey set-up (G HUB and other launchers), command-line and configuration reference, compatibility and troubleshooting notes; `docs/benchmarks.md`; `docs/packaging.md`.
- Continuous integration (format, lint, tests, release build with a static C runtime check, benchmark builds, MSI build and validation) and tag-driven release automation with the MSI, a portable zip and `SHA256SUMS.txt`.

[Unreleased]: https://github.com/hotdogee/toggle-audio/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/hotdogee/toggle-audio/releases/tag/v0.1.0
