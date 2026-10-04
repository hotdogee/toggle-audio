//! The crate-wide error type and its mapping to process exit codes.
//!
//! Every fallible function in the crate returns [`Result`]. Each [`Error`] variant maps to one of
//! the documented exit codes (see [`Error::exit_code`] and DESIGN.md section 5):
//!
//! | Code | Constant | Meaning |
//! | --- | --- | --- |
//! | 0 | [`EXIT_SUCCESS`] | success |
//! | 1 | [`EXIT_FAILURE`] | unexpected error (COM/Win32, I/O, dialog) |
//! | 2 | [`EXIT_USAGE`] | command-line usage error |
//! | 3 | [`EXIT_CONFIG`] | no configuration or an invalid configuration |
//! | 4 | [`EXIT_DEVICE`] | device not found, ambiguous or not active |

use std::fmt;
use std::io;
use std::path::PathBuf;

use windows_core::HRESULT;

use crate::audio::Endpoint;

/// Exit code for success.
pub const EXIT_SUCCESS: u8 = 0;
/// Exit code for an unexpected failure (COM/Win32 call, I/O, settings dialog).
pub const EXIT_FAILURE: u8 = 1;
/// Exit code for a command-line usage error.
pub const EXIT_USAGE: u8 = 2;
/// Exit code for a missing or invalid configuration.
pub const EXIT_CONFIG: u8 = 3;
/// Exit code for a device that is unknown, ambiguous or not active.
pub const EXIT_DEVICE: u8 = 4;

/// Convenience alias used by every fallible function in the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can go wrong in toggle-audio.
#[derive(Debug)]
pub enum Error {
    /// The command line could not be parsed. The message is shown to the user as is.
    Usage(String),
    /// No configuration file exists at the given path (the user has not run Settings yet).
    NoConfig(PathBuf),
    /// The configuration file at `path` could not be read, parsed, validated or written.
    Config {
        /// The configuration file involved.
        path: PathBuf,
        /// What exactly is wrong with it.
        problem: ConfigProblem,
    },
    /// No playback endpoint matches the given id or name. The string is the id or name that was
    /// looked up.
    DeviceNotFound(String),
    /// The endpoint exists but is not active (unplugged, disabled or not present). The string is
    /// the friendly name when known, otherwise the endpoint id.
    DeviceInactive(String),
    /// A name query for `set` matched more than one active endpoint.
    AmbiguousDevice {
        /// The query as typed by the user.
        query: String,
        /// Every matching endpoint. The message lists their ids, since two endpoints can share a
        /// friendly name.
        matches: Vec<Endpoint>,
    },
    /// Toggle found nothing to switch to: the preferred device is not active and the fallback is
    /// either not active or already the default. The strings are display names (the friendly
    /// name, or the id when the configuration has no name).
    NoDeviceAvailable {
        /// The configured devices that are not active (unplugged, disabled or unknown): one or
        /// two, preferred device first.
        unavailable: Vec<String>,
        /// The configured device that already is the default, if that is why the other one could
        /// not be used as a fallback.
        already_default: Option<String>,
    },
    /// Windows reports no default playback device (`GetDefaultAudioEndpoint` returned `E_NOTFOUND`).
    NoDefaultDevice,
    /// A COM or Win32 call failed.
    Com {
        /// The API that failed, for example `"IMMDeviceEnumerator::GetDevice"`.
        call: &'static str,
        /// The failure code it returned.
        hr: HRESULT,
    },
    /// An I/O error outside configuration handling.
    Io(io::Error),
    /// JSON serialization failed outside configuration loading.
    Json(serde_json::Error),
    /// The settings dialog could not be created or failed while running.
    Gui(String),
}

/// The specific problem with a configuration file, carried by [`Error::Config`].
#[derive(Debug)]
pub enum ConfigProblem {
    /// The file exists but could not be read.
    Read(io::Error),
    /// The file (or its temporary sibling) could not be written or moved into place.
    Write(io::Error),
    /// The file is not valid JSON or does not have the expected shape.
    Parse(serde_json::Error),
    /// The file parsed but its content is not usable, for example both devices are the same.
    Invalid(String),
}

impl Error {
    /// Returns a closure that wraps a `windows_core::Error` into [`Error::Com`] tagged with `call`.
    ///
    /// Intended for `map_err`:
    ///
    /// ```
    /// use toggle_audio::Error;
    /// use windows_core::HRESULT;
    ///
    /// // What a failing `IMMDeviceEnumerator::GetDevice(&id)` call returns for an unknown id.
    /// let failed: windows_core::Result<()> =
    ///     Err(windows_core::Error::from_hresult(HRESULT::from_win32(1168)));
    /// let error = failed
    ///     .map_err(Error::com("IMMDeviceEnumerator::GetDevice"))
    ///     .unwrap_err();
    /// assert!(error.to_string().starts_with(
    ///     "IMMDeviceEnumerator::GetDevice failed with HRESULT 0x80070490"
    /// ));
    /// assert_eq!(error.exit_code(), toggle_audio::error::EXIT_FAILURE);
    /// ```
    pub fn com(call: &'static str) -> impl FnOnce(windows_core::Error) -> Self {
        move |error| Self::Com {
            call,
            hr: error.code(),
        }
    }

    /// The process exit code for this error (DESIGN.md section 5).
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) => EXIT_USAGE,
            Self::NoConfig(_)
            | Self::Config {
                problem:
                    ConfigProblem::Read(_) | ConfigProblem::Parse(_) | ConfigProblem::Invalid(_),
                ..
            } => EXIT_CONFIG,
            Self::DeviceNotFound(_)
            | Self::DeviceInactive(_)
            | Self::AmbiguousDevice { .. }
            | Self::NoDeviceAvailable { .. }
            | Self::NoDefaultDevice => EXIT_DEVICE,
            Self::Config {
                problem: ConfigProblem::Write(_),
                ..
            }
            | Self::Com { .. }
            | Self::Io(_)
            | Self::Json(_)
            | Self::Gui(_) => EXIT_FAILURE,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => f.write_str(message),
            Self::NoConfig(path) => write!(
                f,
                "no configuration found at {}; run \"toggle-audio settings\" to choose two devices",
                path.display()
            ),
            Self::Config { path, problem } => {
                write!(f, "configuration file {} {problem}", path.display())?;
                match problem {
                    ConfigProblem::Parse(_) | ConfigProblem::Invalid(_) => {
                        f.write_str("; run \"toggle-audio settings\" to fix it")
                    }
                    ConfigProblem::Read(_) | ConfigProblem::Write(_) => Ok(()),
                }
            }
            Self::DeviceNotFound(what) => write!(
                f,
                "audio device not found: \"{what}\" (run \"toggle-audio list\" to see the active \
                 devices)"
            ),
            Self::DeviceInactive(what) => {
                write!(f, "audio device is not connected or is disabled: {what}")
            }
            Self::AmbiguousDevice { query, matches } => {
                write!(
                    f,
                    "\"{query}\" matches several devices; use the full name or the id:"
                )?;
                for endpoint in matches {
                    write!(f, "\n  {}  {}", endpoint.id, endpoint.name)?;
                }
                Ok(())
            }
            Self::NoDeviceAvailable {
                unavailable,
                already_default,
            } => {
                f.write_str("nothing to switch to: ")?;
                match unavailable.as_slice() {
                    [device] => write!(f, "\"{device}\" is not connected or is disabled")?,
                    [first, second] => write!(
                        f,
                        "neither \"{first}\" nor \"{second}\" is connected and enabled"
                    )?,
                    _ => f.write_str("no configured device is connected and enabled")?,
                }
                match already_default {
                    Some(device) => write!(f, ", and \"{device}\" is already the default"),
                    None => Ok(()),
                }
            }
            Self::NoDefaultDevice => f.write_str("Windows reports no default playback device"),
            Self::Com { call, hr } => {
                // `{:X}` on an i32 prints its two's-complement bits, i.e. the usual 0x8xxxxxxx form.
                write!(f, "{call} failed with HRESULT 0x{:08X}", hr.0)?;
                let message = hr.message();
                let message = message.trim();
                if message.is_empty() {
                    Ok(())
                } else {
                    write!(f, ": {message}")
                }
            }
            Self::Io(error) => write!(f, "I/O error: {error}"),
            Self::Json(error) => write!(f, "JSON error: {error}"),
            Self::Gui(message) => write!(f, "settings dialog: {message}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Config { problem, .. } => Some(problem),
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

impl fmt::Display for ConfigProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(error) => write!(f, "cannot be read: {error}"),
            Self::Write(error) => write!(f, "cannot be written: {error}"),
            Self::Parse(error) => write!(f, "is not valid: {error}"),
            Self::Invalid(message) => write!(f, "is not valid: {message}"),
        }
    }
}

impl std::error::Error for ConfigProblem {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read(error) | Self::Write(error) => Some(error),
            Self::Parse(error) => Some(error),
            Self::Invalid(_) => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // HRESULT_FROM_WIN32(ERROR_NOT_FOUND) = 0x80070490.
    const E_NOTFOUND: HRESULT = HRESULT::from_win32(1168);

    fn json_error() -> serde_json::Error {
        serde_json::from_str::<serde_json::Value>("{").unwrap_err()
    }

    fn config_error(problem: ConfigProblem) -> Error {
        Error::Config {
            path: PathBuf::from("config.json"),
            problem,
        }
    }

    fn endpoint(id: &str, name: &str) -> Endpoint {
        Endpoint {
            id: id.to_owned(),
            name: name.to_owned(),
        }
    }

    #[test]
    fn exit_codes_follow_the_design() {
        let cases = [
            (Error::Usage("bad".into()), EXIT_USAGE),
            (Error::NoConfig(PathBuf::from("config.json")), EXIT_CONFIG),
            (
                config_error(ConfigProblem::Parse(json_error())),
                EXIT_CONFIG,
            ),
            (
                config_error(ConfigProblem::Invalid("same".into())),
                EXIT_CONFIG,
            ),
            (
                config_error(ConfigProblem::Read(io::Error::other("locked"))),
                EXIT_CONFIG,
            ),
            (
                config_error(ConfigProblem::Write(io::Error::other("full"))),
                EXIT_FAILURE,
            ),
            (Error::DeviceNotFound("x".into()), EXIT_DEVICE),
            (Error::DeviceInactive("x".into()), EXIT_DEVICE),
            (
                Error::AmbiguousDevice {
                    query: "pg".into(),
                    matches: vec![endpoint("{a}", "A"), endpoint("{b}", "A")],
                },
                EXIT_DEVICE,
            ),
            (
                Error::NoDeviceAvailable {
                    unavailable: vec!["a".into(), "b".into()],
                    already_default: None,
                },
                EXIT_DEVICE,
            ),
            (Error::NoDefaultDevice, EXIT_DEVICE),
            (
                Error::Com {
                    call: "CoCreateInstance",
                    hr: E_NOTFOUND,
                },
                EXIT_FAILURE,
            ),
            (Error::Io(io::Error::other("x")), EXIT_FAILURE),
            (Error::Json(json_error()), EXIT_FAILURE),
            (Error::Gui("x".into()), EXIT_FAILURE),
        ];
        for (error, code) in cases {
            assert_eq!(error.exit_code(), code, "{error:?}");
        }
    }

    #[test]
    fn com_errors_show_call_and_hresult() {
        let wrap = Error::com("IMMDeviceEnumerator::GetDevice");
        let error = wrap(windows_core::Error::from_hresult(E_NOTFOUND));
        let text = error.to_string();
        assert!(
            text.starts_with("IMMDeviceEnumerator::GetDevice failed with HRESULT 0x80070490"),
            "{text}"
        );
    }

    #[test]
    fn config_errors_expose_their_source() {
        use std::error::Error as _;
        let error = config_error(ConfigProblem::Parse(json_error()));
        assert!(error.source().is_some());
        assert!(
            error
                .to_string()
                .starts_with("configuration file config.json is not valid: ")
        );
    }

    #[test]
    fn broken_configurations_point_to_settings() {
        let hint = "; run \"toggle-audio settings\" to fix it";
        for problem in [
            ConfigProblem::Parse(json_error()),
            ConfigProblem::Invalid("device1 and device2 are the same device".into()),
        ] {
            let text = config_error(problem).to_string();
            assert!(text.ends_with(hint), "{text}");
        }
        let text = config_error(ConfigProblem::Write(io::Error::other("disk full"))).to_string();
        assert!(!text.contains("settings"), "{text}");
    }

    #[test]
    fn device_errors_are_actionable() {
        assert_eq!(
            Error::DeviceNotFound("Headphones".into()).to_string(),
            "audio device not found: \"Headphones\" (run \"toggle-audio list\" to see the active \
             devices)"
        );
        // Twins with the same name can only be told apart by id, so every id is listed.
        let error = Error::AmbiguousDevice {
            query: "speakers".into(),
            matches: vec![endpoint("{a}", "Speakers"), endpoint("{b}", "Speakers")],
        };
        assert_eq!(
            error.to_string(),
            "\"speakers\" matches several devices; use the full name or the id:\n  \
             {a}  Speakers\n  {b}  Speakers"
        );
    }

    #[test]
    fn no_device_available_names_the_precise_reason() {
        let text = |unavailable: &[&str], already_default: Option<&str>| {
            Error::NoDeviceAvailable {
                unavailable: unavailable.iter().map(|&name| name.to_owned()).collect(),
                already_default: already_default.map(str::to_owned),
            }
            .to_string()
        };
        assert_eq!(
            text(&["B"], Some("A")),
            "nothing to switch to: \"B\" is not connected or is disabled, and \"A\" is already \
             the default"
        );
        assert_eq!(
            text(&["A", "B"], None),
            "nothing to switch to: neither \"A\" nor \"B\" is connected and enabled"
        );
        assert_eq!(
            text(&[], None),
            "nothing to switch to: no configured device is connected and enabled"
        );
    }
}
