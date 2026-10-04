//! Command-line parsing (hand-rolled: no clap, for startup time and binary size).
//!
//! Grammar (DESIGN.md section 5): at most one subcommand plus any number of flags, in any order.
//!
//! | Argument | Meaning |
//! | --- | --- |
//! | *(none)*, `toggle` | [`Command::Toggle`] |
//! | `list` | [`Command::List`] |
//! | `get` | [`Command::Get`] |
//! | `set <id-or-name>` | [`Command::Set`] |
//! | `settings`, `--settings`, `config`, `gui` | [`Command::Settings`] |
//! | `--help`, `-h` | [`Command::Help`] |
//! | `--version`, `-V` | [`Command::Version`] |
//! | `--timing` | [`Options::timing`] |
//! | `--comm` / `--no-comm` | [`Options::comm_override`] |
//! | `--` | everything after it is a positional argument (a device name starting with `-`) |
//!
//! Any argument that starts with `-` (other than `-` itself) is an option, wherever it appears;
//! the first positional argument is the subcommand and the second one is the `set` argument.

use std::ffi::{OsStr, OsString};

use crate::error::{Error, Result};

/// What the user asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Toggle between the configured devices (the default when no subcommand is given).
    Toggle,
    /// Print the active playback endpoints.
    List,
    /// Print the current default playback endpoint.
    Get,
    /// Make the given endpoint (id, exact name or unique name substring) the default.
    Set(String),
    /// Open the settings dialog.
    Settings,
    /// Print usage.
    Help,
    /// Print the version.
    Version,
}

/// A parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// The subcommand.
    pub command: Command,
    /// `--timing`: append per-phase timings to stderr.
    pub timing: bool,
    /// `--comm` (`Some(true)`) / `--no-comm` (`Some(false)`): override the configured
    /// `switch_communications` for this run. `None` uses the configuration.
    pub comm_override: Option<bool>,
}

/// Parses the arguments after the program name (`std::env::args_os().skip(1)`).
///
/// `--help` and `--version` win over everything else on the line, including arguments that would
/// otherwise be errors; whichever comes first is used, and the other flags are then ignored.
/// Repeating `--timing`, `--comm` or `--no-comm` is harmless.
///
/// # Errors
///
/// [`Error::Usage`] for an unknown subcommand or option, a second subcommand, `set` without an
/// argument (or with an empty or whitespace-only one), conflicting `--comm` / `--no-comm`, or an
/// argument that is not valid Unicode.
pub fn parse(args: &[OsString]) -> Result<Options> {
    if let Some(command) = help_or_version(args) {
        return Ok(Options {
            command,
            timing: false,
            comm_override: None,
        });
    }

    let mut command: Option<(Command, &str)> = None;
    let mut awaiting_set_argument = false;
    let mut timing = false;
    let mut comm_override = None;
    let mut options_done = false;

    for arg in args {
        let arg = arg.to_str().ok_or_else(|| {
            usage(&format!(
                "argument \"{}\" is not valid Unicode",
                arg.to_string_lossy()
            ))
        })?;
        let is_option = !options_done && arg.starts_with('-') && arg != "-";
        if is_option && arg != "--settings" {
            match arg {
                "--" => options_done = true,
                "--timing" => timing = true,
                "--comm" | "--no-comm" => {
                    let value = arg == "--comm";
                    if comm_override == Some(!value) {
                        return Err(usage("--comm and --no-comm cannot be used together"));
                    }
                    comm_override = Some(value);
                }
                _ => return Err(usage(&format!("unknown option \"{arg}\""))),
            }
            continue;
        }

        if awaiting_set_argument && !is_option {
            if arg.trim().is_empty() {
                return Err(usage(SET_NEEDS_ARGUMENT));
            }
            command = Some((Command::Set(arg.to_owned()), "set"));
            awaiting_set_argument = false;
            continue;
        }
        if let Some((_, word)) = &command {
            return Err(usage(&format!(
                "unexpected argument \"{arg}\" after \"{word}\""
            )));
        }
        let parsed = match arg {
            "toggle" => Command::Toggle,
            "list" => Command::List,
            "get" => Command::Get,
            "set" => {
                awaiting_set_argument = true;
                // Placeholder until the argument arrives; checked after the loop.
                Command::Set(String::new())
            }
            "settings" | "config" | "gui" => Command::Settings,
            // After `--` it is a plain word, and no command is spelled that way.
            "--settings" if is_option => Command::Settings,
            _ => return Err(usage(&format!("unknown command \"{arg}\""))),
        };
        command = Some((parsed, arg));
    }

    if awaiting_set_argument {
        return Err(usage(SET_NEEDS_ARGUMENT));
    }
    Ok(Options {
        command: command.map_or(Command::Toggle, |(command, _)| command),
        timing,
        comm_override,
    })
}

/// Usage message for `set` without a usable argument.
const SET_NEEDS_ARGUMENT: &str = "\"set\" needs a device id or name";

/// The first `--help` / `-h` / `--version` / `-V` before any `--`, if there is one.
fn help_or_version(args: &[OsString]) -> Option<Command> {
    args.iter()
        .map(OsString::as_os_str)
        .take_while(|arg| *arg != OsStr::new("--"))
        .find_map(|arg| {
            if arg == "--help" || arg == "-h" {
                Some(Command::Help)
            } else if arg == "--version" || arg == "-V" {
                Some(Command::Version)
            } else {
                None
            }
        })
}

/// A usage error with a pointer to `--help`.
fn usage(message: &str) -> Error {
    Error::Usage(format!("{message} (run \"toggle-audio --help\" for usage)"))
}

/// The `--help` text, ending with a newline.
#[must_use]
pub fn help_text() -> String {
    format!(
        "\
toggle-audio {version}
Flip the Windows default playback device between two configured devices.

Usage:
  toggle-audio [toggle]          Toggle between Device 1 and Device 2
                                 (chosen in Settings)
  toggle-audio list              List active playback devices:
                                 <id> TAB <name> TAB <flags>, where flags are
                                 * default, c default communications, - neither
  toggle-audio get               Print the default playback device:
                                 <id> TAB <name>
  toggle-audio set <id-or-name>  Make a device the default (id, exact name or
                                 unique part of a name)
  toggle-audio settings          Open the settings dialog
                                 (aliases: --settings, config, gui)
  toggle-audio --help | -h       Show this help
  toggle-audio --version | -V    Show the version

Options:
  --timing                       Print per-phase timings (microseconds since
                                 process start) to stderr
  --comm | --no-comm             Also / do not switch the default
                                 communications device this time
  --                             Stop reading options (for a device name
                                 that starts with -)

Configuration: %APPDATA%\\toggle-audio\\config.json

Exit codes: 0 success, 1 unexpected error, 2 usage error,
            3 no or invalid configuration, 4 device not found or not active.
",
        version = env!("CARGO_PKG_VERSION")
    )
}

/// The `--version` text (`toggle-audio <version>`), ending with a newline.
#[must_use]
pub fn version_text() -> String {
    format!("toggle-audio {}\n", env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use std::os::windows::ffi::OsStringExt as _;

    use super::*;
    use crate::error::EXIT_USAGE;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    fn parse_ok(list: &[&str]) -> Options {
        parse(&args(list)).unwrap_or_else(|error| panic!("{list:?}: {error}"))
    }

    fn command(list: &[&str]) -> Command {
        parse_ok(list).command
    }

    /// Parses `list`, expects a usage error and returns its message.
    fn usage_error(list: &[&str]) -> String {
        usage_error_os(&args(list))
    }

    fn usage_error_os(list: &[OsString]) -> String {
        match parse(list) {
            Err(error @ Error::Usage(_)) => {
                assert_eq!(error.exit_code(), EXIT_USAGE);
                let message = error.to_string();
                assert!(message.ends_with("(run \"toggle-audio --help\" for usage)"));
                message
            }
            other => panic!("{list:?}: expected a usage error, got {other:?}"),
        }
    }

    fn set(arg: &str) -> Command {
        Command::Set(arg.to_owned())
    }

    fn options(command: Command, timing: bool, comm_override: Option<bool>) -> Options {
        Options {
            command,
            timing,
            comm_override,
        }
    }

    #[test]
    fn no_arguments_means_toggle() {
        assert_eq!(parse_ok(&[]), options(Command::Toggle, false, None));
    }

    #[test]
    fn subcommands() {
        let cases = [
            (&["toggle"][..], Command::Toggle),
            (&["list"], Command::List),
            (&["get"], Command::Get),
            (&["set", "PG42UQ"], set("PG42UQ")),
            (&["settings"], Command::Settings),
            (&["--settings"], Command::Settings),
            (&["config"], Command::Settings),
            (&["gui"], Command::Settings),
            (&["--help"], Command::Help),
            (&["-h"], Command::Help),
            (&["--version"], Command::Version),
            (&["-V"], Command::Version),
        ];
        for (list, expected) in cases {
            assert_eq!(parse_ok(list), options(expected, false, None), "{list:?}");
        }
    }

    #[test]
    fn set_keeps_its_argument_verbatim() {
        let id = "{0.0.0.00000000}.{739b3554-bfed-4d61-b407-a818b317c991}";
        assert_eq!(command(&["set", id]), set(id));
        assert_eq!(
            command(&["set", "喇叭 (FiiO BTA30 PRO)"]),
            set("喇叭 (FiiO BTA30 PRO)")
        );
        assert_eq!(command(&["set", " spaced "]), set(" spaced "));
        // Command words are ordinary values in the argument position.
        assert_eq!(command(&["set", "list"]), set("list"));
        assert_eq!(command(&["set", "set"]), set("set"));
        assert_eq!(command(&["set", "-"]), set("-"));
    }

    #[test]
    fn double_dash_allows_a_set_argument_that_looks_like_an_option() {
        assert_eq!(command(&["set", "--", "-weird"]), set("-weird"));
        assert_eq!(command(&["--", "set", "--help"]), set("--help"));
        assert_eq!(command(&["set", "--", "--"]), set("--"));
        assert_eq!(command(&["--"]), Command::Toggle);
        assert_eq!(command(&["--", "list"]), Command::List);
    }

    #[test]
    fn flags_combine_with_any_command_in_any_position() {
        let expected = options(set("FiiO"), true, Some(false));
        for list in [
            &["--timing", "--no-comm", "set", "FiiO"][..],
            &["set", "--timing", "FiiO", "--no-comm"],
            &["set", "FiiO", "--no-comm", "--timing"],
            &["--no-comm", "set", "--timing", "FiiO"],
        ] {
            assert_eq!(parse_ok(list), expected, "{list:?}");
        }

        let cases = [
            (&["--comm"][..], options(Command::Toggle, false, Some(true))),
            (&["--no-comm"], options(Command::Toggle, false, Some(false))),
            (
                &["toggle", "--timing"],
                options(Command::Toggle, true, None),
            ),
            (
                &["list", "--timing", "--comm"],
                options(Command::List, true, Some(true)),
            ),
            (&["--timing", "get"], options(Command::Get, true, None)),
            (
                &["--timing", "--settings"],
                options(Command::Settings, true, None),
            ),
            (
                &["gui", "--no-comm"],
                options(Command::Settings, false, Some(false)),
            ),
        ];
        for (list, expected) in cases {
            assert_eq!(parse_ok(list), expected, "{list:?}");
        }
    }

    #[test]
    fn repeated_flags_are_harmless() {
        assert_eq!(
            parse_ok(&["--timing", "--timing", "--comm", "--comm"]),
            options(Command::Toggle, true, Some(true))
        );
        assert_eq!(
            parse_ok(&["--no-comm", "--no-comm"]),
            options(Command::Toggle, false, Some(false))
        );
    }

    #[test]
    fn help_and_version_win() {
        assert_eq!(command(&["list", "--help"]), Command::Help);
        assert_eq!(command(&["set", "-h"]), Command::Help);
        assert_eq!(command(&["bogus", "--frobnicate", "-h"]), Command::Help);
        assert_eq!(command(&["--comm", "--no-comm", "-V"]), Command::Version);
        assert_eq!(command(&["get", "list", "--version"]), Command::Version);
        assert_eq!(command(&["set", "--version"]), Command::Version);
        // The first one wins.
        assert_eq!(command(&["--version", "--help"]), Command::Version);
        assert_eq!(command(&["-h", "-V"]), Command::Help);
        // Even over an argument that is not valid Unicode.
        let list = [OsString::from_wide(&[0xD800]), OsString::from("--help")];
        assert_eq!(parse(&list).unwrap().command, Command::Help);
        // Flags are not reported alongside help or version.
        assert_eq!(
            parse_ok(&["--timing", "--comm", "--help"]),
            options(Command::Help, false, None)
        );
    }

    #[test]
    fn unknown_commands_and_options_are_usage_errors() {
        let cases = [
            (&["bogus"][..], "unknown command \"bogus\""),
            (&["List"], "unknown command \"List\""),
            (&["help"], "unknown command \"help\""),
            (&["version"], "unknown command \"version\""),
            (&[""], "unknown command \"\""),
            (&["-"], "unknown command \"-\""),
            (&["--", "--settings"], "unknown command \"--settings\""),
            (&["--bogus"], "unknown option \"--bogus\""),
            (&["-x"], "unknown option \"-x\""),
            (&["-hV"], "unknown option \"-hV\""),
            (&["-settings"], "unknown option \"-settings\""),
            (&["--timing=1"], "unknown option \"--timing=1\""),
            (&["list", "--HELP"], "unknown option \"--HELP\""),
            (&["set", "--bogus", "x"], "unknown option \"--bogus\""),
        ];
        for (list, prefix) in cases {
            let message = usage_error(list);
            assert!(message.starts_with(prefix), "{list:?}: {message}");
        }
    }

    #[test]
    fn a_second_command_or_extra_argument_is_a_usage_error() {
        let cases = [
            (
                &["list", "get"][..],
                "unexpected argument \"get\" after \"list\"",
            ),
            (
                &["toggle", "toggle"],
                "unexpected argument \"toggle\" after \"toggle\"",
            ),
            (
                &["gui", "--settings"],
                "unexpected argument \"--settings\" after \"gui\"",
            ),
            (
                &["--settings", "settings"],
                "unexpected argument \"settings\" after \"--settings\"",
            ),
            (
                &["set", "a", "b"],
                "unexpected argument \"b\" after \"set\"",
            ),
            (
                &["set", "a", "list"],
                "unexpected argument \"list\" after \"set\"",
            ),
            (
                &["get", "--", "x"],
                "unexpected argument \"x\" after \"get\"",
            ),
        ];
        for (list, prefix) in cases {
            let message = usage_error(list);
            assert!(message.starts_with(prefix), "{list:?}: {message}");
        }
    }

    #[test]
    fn set_without_an_argument_is_a_usage_error() {
        for list in [
            &["set"][..],
            &["set", "--timing"],
            &["--comm", "set"],
            &["set", "--"],
            &["set", ""],
            &["set", "   "],
            &["set", "\t"],
        ] {
            let message = usage_error(list);
            assert!(
                message.starts_with("\"set\" needs a device id or name"),
                "{list:?}: {message}"
            );
        }
    }

    #[test]
    fn conflicting_comm_flags_are_a_usage_error() {
        for list in [
            &["--comm", "--no-comm"][..],
            &["--no-comm", "toggle", "--comm"],
            &["--comm", "--comm", "--no-comm"],
        ] {
            let message = usage_error(list);
            assert!(
                message.starts_with("--comm and --no-comm cannot be used together"),
                "{list:?}: {message}"
            );
        }
    }

    #[test]
    fn non_unicode_arguments_are_a_usage_error() {
        // An unpaired surrogate cannot be converted to UTF-8.
        let bad = OsString::from_wide(&[u16::from(b'a'), 0xDC00]);
        let message = usage_error_os(&[OsString::from("set"), bad.clone()]);
        assert!(
            message.starts_with("argument \"a\u{FFFD}\" is not valid Unicode"),
            "{message}"
        );
        let message = usage_error_os(&[bad]);
        assert!(message.contains("is not valid Unicode"), "{message}");
    }

    #[test]
    fn help_text_documents_every_form_and_exit_code() {
        let help = help_text();
        assert!(help.starts_with(&format!("toggle-audio {}\n", env!("CARGO_PKG_VERSION"))));
        assert!(help.ends_with('\n'));
        for needle in [
            "toggle-audio [toggle]",
            "toggle-audio list",
            "toggle-audio get",
            "toggle-audio set <id-or-name>",
            "toggle-audio settings",
            "--settings, config, gui",
            "--help | -h",
            "--version | -V",
            "--timing",
            "--comm | --no-comm",
            "\n  --  ",
            "0 success",
            "1 unexpected error",
            "2 usage error",
            "3 no or invalid configuration",
            "4 device not found or not active",
        ] {
            assert!(help.contains(needle), "help text is missing {needle:?}");
        }
    }

    #[test]
    fn help_text_fits_an_80_column_terminal() {
        for line in help_text().lines() {
            assert!(line.chars().count() <= 80, "too wide: {line:?}");
        }
    }

    #[test]
    fn version_text_is_the_package_version() {
        assert_eq!(
            version_text(),
            format!("toggle-audio {}\n", env!("CARGO_PKG_VERSION"))
        );
    }
}
