//! `toggle-audio.exe`: the console-subsystem binary.
//!
//! The embedded manifest sets `consoleAllocationPolicy=detached`, so on Windows 11 24H2 and later a
//! launch from G HUB or Explorer creates no console window, while terminals, pipes and
//! redirection behave as usual.

use std::process::ExitCode;

fn main() -> ExitCode {
    toggle_audio::run(std::env::args_os().skip(1).collect(), false)
}
