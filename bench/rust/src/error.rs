//! Failures and their mapping to the exit codes of the bench CLI contract.

use windows::core::HRESULT;

/// Exit code: success.
pub const EXIT_OK: u8 = 0;
/// Exit code: usage error (no command, unknown command, wrong number of ids).
pub const EXIT_USAGE: u8 = 1;
/// Exit code: a COM / Win32 call failed.
pub const EXIT_COM: u8 = 2;
/// Exit code: the requested endpoint does not exist, is not a render
/// endpoint, or is not active.
pub const EXIT_DEVICE: u8 = 3;
/// Exit code: there is no default render endpoint.
pub const EXIT_NO_DEFAULT: u8 = 4;

/// Everything that can go wrong after the arguments were accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// A COM call returned a failure HRESULT. `step` names the call.
    Com { step: &'static str, hr: HRESULT },
    /// `GetDevice(id)` does not know the id (or the id is malformed).
    NotFound { id: String },
    /// The id names a capture (recording) endpoint, not a render endpoint.
    NotRender { id: String },
    /// The endpoint exists but its state is not `DEVICE_STATE_ACTIVE`
    /// (disabled, not present or unplugged).
    NotActive { id: String, state: u32 },
    /// `GetDefaultAudioEndpoint` reported that no render endpoint exists.
    NoDefault,
}

impl Failure {
    /// The process exit code for this failure.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Com { .. } => EXIT_COM,
            Self::NotFound { .. } | Self::NotRender { .. } | Self::NotActive { .. } => EXIT_DEVICE,
            Self::NoDefault => EXIT_NO_DEFAULT,
        }
    }

    /// One line for stderr, `\n`-terminated.
    pub fn message(&self) -> String {
        match self {
            // `{:08X}` on the i32 prints its two's-complement bits: 0x80070490.
            Self::Com { step, hr } => format!("error: {step} hr=0x{:08X}\n", hr.0),
            Self::NotFound { id } => format!("error: device not found: {id}\n"),
            Self::NotRender { id } => format!("error: not a render device: {id}\n"),
            Self::NotActive { id, state } => {
                format!("error: device not active: {id} state=0x{state:08X}\n")
            }
            Self::NoDefault => "error: no default render device\n".to_owned(),
        }
    }
}

/// Extension for `windows::core::Result`: attach the name of the failing call.
pub trait Step<T> {
    /// Converts a COM error into [`Failure::Com`] labeled with `step`.
    fn step(self, step: &'static str) -> Result<T, Failure>;
}

impl<T> Step<T> for windows::core::Result<T> {
    fn step(self, step: &'static str) -> Result<T, Failure> {
        self.map_err(|e| Failure::Com { step, hr: e.code() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::{E_FAIL, ERROR_NOT_FOUND};

    #[test]
    fn exit_codes_follow_the_contract() {
        let com = Failure::Com {
            step: "x",
            hr: E_FAIL,
        };
        assert_eq!(com.exit_code(), 2);
        assert_eq!(Failure::NotFound { id: String::new() }.exit_code(), 3);
        assert_eq!(Failure::NotRender { id: String::new() }.exit_code(), 3);
        assert_eq!(
            Failure::NotActive {
                id: String::new(),
                state: 4
            }
            .exit_code(),
            3
        );
        assert_eq!(Failure::NoDefault.exit_code(), 4);
    }

    #[test]
    fn com_message_prints_hresult_as_eight_hex_digits() {
        let f = Failure::Com {
            step: "GetDevice",
            hr: HRESULT::from_win32(ERROR_NOT_FOUND.0),
        };
        assert_eq!(f.message(), "error: GetDevice hr=0x80070490\n");
        let f = Failure::NotActive {
            id: "x".into(),
            state: 4,
        };
        assert_eq!(
            f.message(),
            "error: device not active: x state=0x00000004\n"
        );
    }
}
