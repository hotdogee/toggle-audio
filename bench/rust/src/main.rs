//! `ta-rs`: the Rust implementation of the toggle-audio bench CLI contract
//! (docs/research/benchmark-method.md section 1, as amended by DESIGN.md
//! section 12).
//!
//! ```text
//! ta-rs list                  <id>\t<name>\t<flags> per active render endpoint
//!                             flags: "*" default, "c" default communications, "*c" both, "-" neither
//! ta-rs get                   <id>\t<name> of the default (eConsole) endpoint
//! ta-rs set <id>              validate ACTIVE, then SetDefaultEndpoint for eConsole, eMultimedia, eCommunications
//! ta-rs toggle <idA> <idB>    if the default is A set B, else set A; prints the target id
//! ta-rs                       usage on stderr, exit 1, no COM work (runtime floor)
//! --timing (anywhere)         phase\t<name>\t<us since process creation> lines on stderr
//! ```
//!
//! Exit codes: 0 OK, 1 usage, 2 COM failure, 3 device not found / not active,
//! 4 no default device.
//!
//! Console subsystem with the `consoleAllocationPolicy=detached` manifest
//! embedded by build.rs: no console window when launched from G HUB/Explorer on
//! Windows 11 24H2+, normal waiting and redirection in shells.

mod audio;
mod cli;
mod error;
mod output;
mod policy;
mod timing;

use std::ffi::OsStr;
use std::process::ExitCode;

use windows::Win32::Media::Audio::{eCommunications, eConsole};

use crate::audio::{AudioSystem, ComApartment, EndpointId};
use crate::cli::Command;
use crate::error::{EXIT_OK, EXIT_USAGE, Failure};
use crate::timing::Timing;

fn main() -> ExitCode {
    // First statement: the `entry` stamp.
    let mut timing = Timing::start();
    let invocation = cli::parse(std::env::args_os().skip(1));

    let code = match &invocation.command {
        // No COM work at all on the usage path: this measures the runtime floor.
        None => {
            output::stderr(cli::USAGE);
            EXIT_USAGE
        }
        Some(command) => match execute(command, &mut timing) {
            Ok(text) => {
                output::stdout(&text);
                EXIT_OK
            }
            Err(failure) => {
                output::stderr(&failure.message());
                failure.exit_code()
            }
        },
    };

    timing.mark("exit");
    if invocation.timing {
        output::stderr(&timing.report());
    }
    ExitCode::from(code)
}

/// Runs one command inside a COM apartment and returns the text for stdout.
///
/// Phases: `com_init` after `CoInitializeEx`, `enumerator` after the device
/// enumerator exists, `work_done` after the command's COM work and output
/// formatting. All interfaces are released and COM is uninitialised before
/// this function returns (on success and on every error path, by drop order).
fn execute(command: &Command, timing: &mut Timing) -> Result<String, Failure> {
    let apartment = ComApartment::init()?;
    timing.mark("com_init");

    let audio = AudioSystem::new(&apartment)?;
    timing.mark("enumerator");

    let text = match command {
        Command::List => list(&audio)?,
        Command::Get => get(&audio)?,
        Command::Set { id } => {
            set(&audio, id)?;
            String::new()
        }
        Command::Toggle { a, b } => toggle(&audio, a, b)?,
    };
    timing.mark("work_done");

    drop(audio); // Release the enumerator...
    drop(apartment); // ...then CoUninitialize.
    Ok(text)
}

/// `list`: one `<id>\t<name>\t<flags>\n` line per active render endpoint.
fn list(audio: &AudioSystem<'_>) -> Result<String, Failure> {
    let endpoints = audio.active_endpoints()?;
    let console = audio.default_id(eConsole)?;
    let communications = audio.default_id(eCommunications)?;

    let is = |default: &Option<EndpointId>, id: &EndpointId| {
        default.as_ref().is_some_and(|d| d.same_as(id))
    };
    let mut out = String::with_capacity(endpoints.len() * 96);
    for endpoint in &endpoints {
        let flags = list_flags(
            is(&console, &endpoint.id),
            is(&communications, &endpoint.id),
        );
        out.push_str(&endpoint.id.to_utf8());
        out.push('\t');
        out.push_str(&endpoint.name);
        out.push('\t');
        out.push_str(flags);
        out.push('\n');
    }
    Ok(out)
}

/// `get`: `<id>\t<name>\n` of the default eConsole endpoint, exit 4 if none.
fn get(audio: &AudioSystem<'_>) -> Result<String, Failure> {
    let endpoint = audio
        .default_endpoint(eConsole)?
        .ok_or(Failure::NoDefault)?;
    let id = endpoint.id.to_utf8();
    let mut out = String::with_capacity(id.len() + endpoint.name.len() + 2);
    out.push_str(&id);
    out.push('\t');
    out.push_str(&endpoint.name);
    out.push('\n');
    Ok(out)
}

/// `set <id>`: validate, then set all three roles. Prints nothing.
fn set(audio: &AudioSystem<'_>, id: &OsStr) -> Result<(), Failure> {
    let requested = EndpointId::from_wide(cli::to_wide_nul(id));
    let canonical = audio.require_active(&requested)?;
    audio.set_default_all_roles(&canonical)
}

/// `toggle <idA> <idB>`: pick the target from the current eConsole default,
/// then validate and set like `set`. Prints the target id.
fn toggle(audio: &AudioSystem<'_>, a: &OsStr, b: &OsStr) -> Result<String, Failure> {
    let a = cli::to_wide_nul(a);
    let b = cli::to_wide_nul(b);
    let current = audio.default_id(eConsole)?;
    let target = cli::toggle_target(current.as_ref().map(EndpointId::as_wide), &a, &b);

    let target = audio.require_active(&EndpointId::from_wide(target.to_vec()))?;
    audio.set_default_all_roles(&target)?;

    let mut out = target.to_utf8();
    out.push('\n');
    Ok(out)
}

/// The `<flags>` column of `list`.
fn list_flags(is_default: bool, is_communications: bool) -> &'static str {
    match (is_default, is_communications) {
        (true, true) => "*c",
        (true, false) => "*",
        (false, true) => "c",
        (false, false) => "-",
    }
}

#[cfg(test)]
mod tests {
    use super::list_flags;

    #[test]
    fn list_flags_cover_all_combinations() {
        assert_eq!(list_flags(true, true), "*c");
        assert_eq!(list_flags(true, false), "*");
        assert_eq!(list_flags(false, true), "c");
        assert_eq!(list_flags(false, false), "-");
    }
}
