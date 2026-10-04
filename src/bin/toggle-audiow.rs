//! `toggle-audiow.exe`: the Windows-subsystem binary.
//!
//! Never creates a console window on any Windows version: the hotkey target for Windows 10 and
//! Windows 11 before 24H2, and the target of the Start Menu "Toggle Audio Settings" shortcut.
//! Output still reaches a parent terminal on a best-effort basis (`AttachConsole`).

#![windows_subsystem = "windows"]

use std::process::ExitCode;

fn main() -> ExitCode {
    toggle_audio::run(std::env::args_os().skip(1).collect(), true)
}
