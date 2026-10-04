//! Toggle Audio: flip the Windows default playback device between two configured endpoints.
//!
//! The crate is a library with two thin binaries (the `python.exe` / `pythonw.exe` convention):
//!
//! - `toggle-audio.exe`, console subsystem with a `consoleAllocationPolicy=detached` manifest:
//!   the command-line tool, and the hotkey target on Windows 11 24H2 and later.
//! - `toggle-audiow.exe`, Windows subsystem: never creates a console window; the hotkey target on
//!   older Windows versions and the Start Menu "Toggle Audio Settings" shortcut.
//!
//! Both call [`run`]. Module map:
//!
//! - `cli`: command-line parsing.
//! - [`config`]: the JSON configuration file under `%APPDATA%\toggle-audio`.
//! - [`toggle`]: the toggle decision (pure) and its execution.
//! - [`audio`]: Core Audio enumeration and `IPolicyConfig::SetDefaultEndpoint`.
//! - `console`: console / redirected output and message-box error reporting.
//! - `timing`: `--timing` phase stamps.
//! - `gui`: the settings dialog.
//! - [`error`]: the error type and exit codes.
//!
//! See `docs/DESIGN.md` for the full specification.

pub mod audio;
mod cli;
pub mod config;
mod console;
pub mod error;
mod gui;
mod timing;
pub mod toggle;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

pub use error::{Error, Result};

use audio::{AudioSystem, Endpoint, Role, roles_for};
use cli::{Command, Options};
use console::Output;
use error::EXIT_CONFIG;
use gui::{DialogOutcome, OpenReason};
use timing::Timing;
use toggle::display_name;

/// Title of every message box the program shows.
pub const APP_TITLE: &str = "Toggle Audio";

/// Runs the program with `args` (the arguments after the program name) and returns the process
/// exit code (see [`error`] for the table).
///
/// `launched_windowed` is `true` for the Windows-subsystem binary `toggle-audiow.exe`: it has no
/// console of its own, so output attaches to the parent's console when there is one.
///
/// Errors go to stderr when a console or redirected stderr is available, and to a message box
/// otherwise, so a failed hotkey press never fails silently. Success output goes to stdout and is
/// silent when there is no console.
#[must_use]
#[expect(
    clippy::needless_pass_by_value,
    reason = "takes the collected std::env::args_os() so the binaries stay one-liners"
)]
pub fn run(args: Vec<OsString>, launched_windowed: bool) -> ExitCode {
    let output = console::init(launched_windowed);
    let options = match cli::parse(&args) {
        Ok(options) => options,
        Err(error) => return fail(&output, &error),
    };
    // The parsed `--timing` flag is the only source of truth. Starting the clock after parsing
    // loses nothing: every stamp is measured from process creation, so the first one ("start")
    // covers loader start-up, console set-up and parsing (a few microseconds).
    let mut timing = timing::start(options.timing);
    timing.mark("start");
    let result = dispatch(&options, &output, &mut timing);
    timing.mark("end");
    let code = result.unwrap_or_else(|error| fail(&output, &error));
    timing.report(&output);
    code
}

/// Reports `error` (see [`report_error`]) and returns its exit code.
fn fail(output: &Output, error: &Error) -> ExitCode {
    report_error(output, error);
    ExitCode::from(error.exit_code())
}

/// Executes the parsed command and returns the exit code for a run that did not fail.
fn dispatch(options: &Options, output: &Output, timing: &mut Timing) -> Result<ExitCode> {
    match &options.command {
        Command::Toggle => toggle(options, output, timing),
        Command::List => list(output, timing),
        Command::Get => get(output, timing),
        Command::Set(query) => set(query, options, output, timing),
        Command::Settings => settings(output),
        Command::Help => {
            output.out(&cli::help_text());
            Ok(ExitCode::SUCCESS)
        }
        Command::Version => {
            output.out(&cli::version_text());
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// `toggle`: the hot path (DESIGN.md section 7), through [`toggle::perform`], which stamps its
/// phases into `timing`.
fn toggle(options: &Options, output: &Output, timing: &mut Timing) -> Result<ExitCode> {
    let path = config::default_path()?;
    let config = match config::load(&path) {
        Ok(Some(config)) => config,
        Ok(None) => {
            let missing = Error::NoConfig(path.clone());
            return set_up(&path, output, missing);
        }
        Err(error) => return set_up(&path, output, error),
    };
    timing.mark("config_loaded");
    let switch_communications = options
        .comm_override
        .unwrap_or(config.switch_communications);

    let audio = AudioSystem::new()?;
    timing.mark("com_ready");
    let outcome = toggle::perform(
        &audio,
        &config.device1,
        &config.device2,
        switch_communications,
        |phase| timing.mark(phase),
    )?;

    if output.has_console_or_redirect() {
        // The configured name may be stale (devices can be renamed in Sound settings). The live
        // one costs a property-store read, so it is only fetched when someone can see it, and a
        // failure to read it must not turn a successful switch into an error.
        let name = audio
            .name_of(&outcome.device.id)
            .ok()
            .flatten()
            .unwrap_or_else(|| display_name(&outcome.device).to_owned());
        output.out(&format!("Switched to {}\n", single_line(&name)));
        if let Some(warning) = &outcome.warning {
            output.err(&format!("toggle-audio: warning: {warning}\n"));
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// `toggle` without a usable configuration: `problem` is [`Error::NoConfig`] or the
/// [`Error::Config`] that loading it produced.
///
/// From a terminal: fail with `problem`, whose message says to run `settings` (a script must
/// never block on a dialog). From a hotkey (no console): open Settings so the press sets the
/// program up or repairs it; after Save, confirm with a message box and exit 0 (without toggling
/// on the same press); exit [`EXIT_CONFIG`] when the dialog is cancelled, and 0 when a settings
/// dialog was already open (it is brought to the front, as with `settings`).
fn set_up(path: &Path, output: &Output, problem: Error) -> Result<ExitCode> {
    if output.has_console_or_redirect() {
        return Err(problem);
    }
    let reason = match &problem {
        Error::NoConfig(_) => OpenReason::FirstRun,
        Error::Config { .. } => OpenReason::Repair(problem_sentence(&problem)),
        _ => return Err(problem),
    };
    match gui::show_settings(path, None, &reason, &current_exe()?)? {
        DialogOutcome::Saved(config) => {
            output.message_box_info(
                APP_TITLE,
                &format!(
                    "Saved. Press your hotkey again to switch between \"{}\" and \"{}\".",
                    display_name(&config.device1),
                    display_name(&config.device2)
                ),
            );
            Ok(ExitCode::SUCCESS)
        }
        // A settings dialog was already open (an earlier press) and has been brought to the
        // front; that one decides.
        DialogOutcome::AlreadyOpen => Ok(ExitCode::SUCCESS),
        DialogOutcome::Cancelled => Ok(ExitCode::from(EXIT_CONFIG)),
    }
}

/// A configuration load failure as a sentence for the settings dialog's status line, without the
/// file path (which the user cannot change there) and without the "run settings" hint.
fn problem_sentence(error: &Error) -> String {
    match error {
        Error::Config { problem, .. } => format!("The saved configuration {problem}."),
        other => format!("The saved configuration cannot be used: {other}."),
    }
}

/// This executable's path, for the settings dialog's Copy button.
fn current_exe() -> Result<PathBuf> {
    std::env::current_exe()
        .map_err(|error| Error::Gui(format!("cannot find this program's path: {error}")))
}

/// `list`: one `<id>\t<name>\t<flags>` line per active playback endpoint.
fn list(output: &Output, timing: &mut Timing) -> Result<ExitCode> {
    let audio = AudioSystem::new()?;
    timing.mark("com_ready");
    // Only the ids are needed: `default_for` would also read each default's friendly name.
    let default = audio.default_id(Role::Console)?;
    let communications = audio.default_id(Role::Communications)?;
    timing.mark("default_read");
    let endpoints = audio.list_active()?;
    timing.mark("listed");
    output.out(&format_list(
        &endpoints,
        default.as_deref(),
        communications.as_deref(),
    ));
    Ok(ExitCode::SUCCESS)
}

/// `get`: `<id>\t<name>` of the default playback device (console role).
fn get(output: &Output, timing: &mut Timing) -> Result<ExitCode> {
    let audio = AudioSystem::new()?;
    timing.mark("com_ready");
    let endpoint = audio
        .default_for(Role::Console)?
        .ok_or(Error::NoDefaultDevice)?;
    timing.mark("default_read");
    output.out(&format!(
        "{}\t{}\n",
        endpoint.id,
        single_line(&endpoint.name)
    ));
    Ok(ExitCode::SUCCESS)
}

/// `set <id-or-name>`: make one endpoint the default for the configured roles.
///
/// Something that looks like an endpoint id (`{...}`) and is known to Windows as a playback
/// endpoint is used directly, without enumerating, so a known but inactive device is reported as
/// such. Anything else is resolved against the active endpoints with [`audio::resolve`].
fn set(query: &str, options: &Options, output: &Output, timing: &mut Timing) -> Result<ExitCode> {
    let switch_communications = options
        .comm_override
        .unwrap_or_else(|| configured_switch_communications(output));
    timing.mark("config_loaded");

    let audio = AudioSystem::new()?;
    timing.mark("com_ready");
    let query = query.trim();
    // `lookup` decides whether the id is known: `name_of` is also `None` for an endpoint without a
    // friendly name, which would turn a known but inactive device into "not found".
    let by_id = if looks_like_endpoint_id(query) && audio.lookup(query)?.is_some() {
        Some(Endpoint {
            id: query.to_owned(),
            name: audio.name_of(query)?.unwrap_or_default(),
        })
    } else {
        None
    };
    let target = match by_id {
        Some(endpoint) => endpoint,
        None => audio::resolve(&audio.list_active()?, query)?.clone(),
    };
    timing.mark("target_chosen");
    let changed = audio.set_default(&target.id, roles_for(switch_communications))?;
    timing.mark("set_done");

    let name = if target.name.is_empty() {
        target.id.clone()
    } else {
        single_line(&target.name)
    };
    if changed {
        output.out(&format!("Switched to {name}\n"));
    } else {
        output.out(&format!("{name} is already the default\n"));
    }
    Ok(ExitCode::SUCCESS)
}

/// The configured "also switch Communications" setting, which is all `set` needs from the
/// configuration: `true` when there is none.
///
/// `set` must keep working while the configuration file is broken (it is the way out of a bad
/// state), so a file that cannot be loaded only produces a warning and the default is used.
fn configured_switch_communications(output: &Output) -> bool {
    match config::default_path().and_then(|path| config::load(&path)) {
        Ok(config) => config.is_none_or(|config| config.switch_communications),
        Err(error) => {
            output.err(&format!(
                "toggle-audio: warning: {error}\n\
                 toggle-audio: warning: using the default setting: also switch the \
                 Communications device\n"
            ));
            true
        }
    }
}

/// `settings`: open the settings dialog with the current configuration.
///
/// A configuration that cannot be loaded does not block the dialog: the dialog starts empty and
/// its status line says what was wrong, so saving repairs the file. The problem is also written
/// to stderr when there is one.
fn settings(output: &Output) -> Result<ExitCode> {
    let path = config::default_path()?;
    let (existing, reason) = match config::load(&path) {
        Ok(existing) => (existing, OpenReason::Requested),
        Err(error) => {
            let problem = problem_sentence(&error);
            output.err(&format!("toggle-audio: warning: {problem}\n"));
            (None, OpenReason::Repair(problem))
        }
    };
    gui::show_settings(&path, existing, &reason, &current_exe()?)?;
    Ok(ExitCode::SUCCESS)
}

/// Shows `error` on stderr, or in a message box when nobody would see stderr.
///
/// Usage errors already end with a pointer to `--help` (see [`cli::parse`]).
fn report_error(output: &Output, error: &Error) {
    if output.has_console_or_redirect() {
        output.err(&format!("toggle-audio: {error}\n"));
    } else {
        output.message_box_error(APP_TITLE, &error.to_string());
    }
}

/// Whether a `set` argument has the shape of an endpoint id such as
/// `{0.0.0.00000000}.{739b3554-bfed-4d61-b407-a818b317c991}`.
fn looks_like_endpoint_id(query: &str) -> bool {
    query.starts_with('{') && query.ends_with('}')
}

/// The `list` output: `<id>\t<name>\t<flags>\n` per endpoint, in the given order.
///
/// Flags are `*` for the default device (console role) and `c` for the default communications
/// device, or `-` when neither applies, so every line has three non-empty fields.
fn format_list(
    endpoints: &[Endpoint],
    default: Option<&str>,
    communications: Option<&str>,
) -> String {
    let matches = |role_default: Option<&str>, id: &str| {
        role_default.is_some_and(|default_id| audio::same_endpoint_id(default_id, id))
    };
    let mut text = String::with_capacity(endpoints.len() * 96);
    for endpoint in endpoints {
        let flags = match (
            matches(default, &endpoint.id),
            matches(communications, &endpoint.id),
        ) {
            (true, true) => "*c",
            (true, false) => "*",
            (false, true) => "c",
            (false, false) => "-",
        };
        text.push_str(&endpoint.id);
        text.push('\t');
        text.push_str(&single_line(&endpoint.name));
        text.push('\t');
        text.push_str(flags);
        text.push('\n');
    }
    text
}

/// `name` with tabs and line breaks replaced by spaces, so it cannot break the tab-separated,
/// one-line-per-device output format.
fn single_line(name: &str) -> String {
    name.replace(['\t', '\r', '\n'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const PG42UQ: &str = "{0.0.0.00000000}.{739b3554-bfed-4d61-b407-a818b317c991}";
    const FIIO: &str = "{0.0.0.00000000}.{30045f40-8cfd-4441-bb89-0d13fc19b589}";

    fn endpoint(id: &str, name: &str) -> Endpoint {
        Endpoint {
            id: id.to_owned(),
            name: name.to_owned(),
        }
    }

    fn two_endpoints() -> [Endpoint; 2] {
        [
            endpoint(PG42UQ, "PG42UQ (NVIDIA High Definition Audio)"),
            endpoint(FIIO, "喇叭 (FiiO BTA30 PRO)"),
        ]
    }

    #[test]
    fn list_marks_the_default_on_both_roles() {
        assert_eq!(
            format_list(&two_endpoints(), Some(PG42UQ), Some(PG42UQ)),
            format!(
                "{PG42UQ}\tPG42UQ (NVIDIA High Definition Audio)\t*c\n\
                 {FIIO}\t喇叭 (FiiO BTA30 PRO)\t-\n"
            )
        );
    }

    #[test]
    fn list_marks_split_roles_case_insensitively() {
        let communications = FIIO.to_ascii_uppercase();
        assert_eq!(
            format_list(&two_endpoints(), Some(PG42UQ), Some(&communications)),
            format!(
                "{PG42UQ}\tPG42UQ (NVIDIA High Definition Audio)\t*\n\
                 {FIIO}\t喇叭 (FiiO BTA30 PRO)\tc\n"
            )
        );
    }

    #[test]
    fn list_without_defaults_or_endpoints() {
        assert_eq!(
            format_list(&two_endpoints()[..1], None, None),
            format!("{PG42UQ}\tPG42UQ (NVIDIA High Definition Audio)\t-\n")
        );
        assert_eq!(format_list(&[], Some(PG42UQ), None), "");
    }

    #[test]
    fn names_cannot_break_the_line_format() {
        assert_eq!(single_line("a\tb\r\nc"), "a b  c");
        assert_eq!(
            format_list(&[endpoint(FIIO, "x\ny")], None, None),
            format!("{FIIO}\tx y\t-\n")
        );
    }

    #[test]
    fn load_problems_become_dialog_sentences() {
        let error = Error::Config {
            path: PathBuf::from(r"C:\x\config.json"),
            problem: error::ConfigProblem::Invalid("device1 has no device id".to_owned()),
        };
        assert_eq!(
            problem_sentence(&error),
            "The saved configuration is not valid: device1 has no device id."
        );
    }

    #[test]
    fn endpoint_ids_are_recognized_by_shape() {
        assert!(looks_like_endpoint_id(PG42UQ));
        assert!(!looks_like_endpoint_id("PG42UQ"));
        assert!(!looks_like_endpoint_id("{unterminated"));
        assert!(!looks_like_endpoint_id(""));
    }
}
