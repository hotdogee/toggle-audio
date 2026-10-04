//! The settings dialog (DESIGN.md section 9).
//!
//! A resource dialog (`IDD_SETTINGS DIALOGEX` in `assets/app.rc`) run modally with
//! `DialogBoxParamW`: two device combo boxes, the "also switch Communications" checkbox, the
//! read-only program path field with a Copy button, a status line, Test toggle, Save and Cancel.
//! The dialog manager provides fonts, keyboard navigation, Enter/Esc and per-monitor DPI
//! rescaling. Only one settings dialog runs at a time per session: a second launch brings the
//! open one to the front instead.

mod clipboard;
mod dialog;

use std::path::Path;

use crate::config::Config;
use crate::error::Result;

/// Resource identifiers. Must match `assets/resource.h` (checked by a unit test).
pub mod ids {
    /// Application icon (`IDI_APP`).
    pub const IDI_APP: u16 = 1;
    /// The settings dialog template (`IDD_SETTINGS`).
    pub const IDD_SETTINGS: u16 = 101;
    /// Device 1 combo box.
    pub const IDC_DEV1: i32 = 1001;
    /// Device 2 combo box.
    pub const IDC_DEV2: i32 = 1002;
    /// "Also switch the Communications device" checkbox.
    pub const IDC_COMMS: i32 = 1003;
    /// Read-only program path field.
    pub const IDC_PATH: i32 = 1004;
    /// Copy button.
    pub const IDC_COPY: i32 = 1005;
    /// Test toggle button.
    pub const IDC_TEST: i32 = 1006;
    /// Status line.
    pub const IDC_STATUS: i32 = 1007;
}

/// Why the settings dialog is opened; decides the first line of its status text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenReason {
    /// The user asked for it (`toggle-audio settings`, the Start Menu shortcut).
    Requested,
    /// A toggle was requested without a console (a hotkey) before anything was configured.
    FirstRun,
    /// The saved configuration cannot be used. The text says why, as a complete sentence (for
    /// example `The saved configuration is not valid: ...`); the dialog adds what to do about it.
    Repair(String),
}

/// Shows the settings dialog and blocks until it closes.
///
/// - `config_path`: where Save writes the configuration (atomically, via [`crate::config::save`]).
/// - `existing`: the current configuration, used to preselect the combos and the checkbox. A
///   configured device that is not active is listed as `(not connected) <name>` and stays
///   selectable.
/// - `reason`: why the dialog opens, which selects the initial status text.
/// - `exe_path`: the executable to show and copy for the G HUB binding (normally
///   `std::env::current_exe()`).
///
/// Returns `Ok(Some(config))` when the user saved (the file has already been written) and
/// `Ok(None)` when the dialog was cancelled, or when another settings dialog was already open and
/// has been brought to the front instead.
///
/// # Errors
///
/// [`crate::Error::Gui`] when the dialog cannot be created (for example the resources are missing),
/// [`crate::Error::Com`] when COM initialization or audio enumeration fails before the dialog can
/// be shown. Failures while the dialog is open (Save, Test toggle, Copy) are shown in its status
/// line instead, and the dialog stays open.
pub fn show_settings(
    config_path: &Path,
    existing: Option<Config>,
    reason: &OpenReason,
    exe_path: &Path,
) -> Result<Option<Config>> {
    dialog::run(config_path, existing, reason, exe_path)
}

/// The text to paste into the Path field of G HUB's "Launch Application" action: the plain path
/// of `exe_path`, never quoted.
///
/// G HUB takes the program path and its arguments in separate fields, and its own Browse button
/// fills the Path field with an unquoted path (spaces included), so an unquoted path is the form
/// it is known to accept. A quoted one could be taken literally.
#[must_use]
pub fn hotkey_path(exe_path: &Path) -> String {
    exe_path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_match_resource_header() {
        let header = include_str!("../../assets/resource.h");
        let expected = [
            ("IDI_APP", i32::from(ids::IDI_APP)),
            ("IDD_SETTINGS", i32::from(ids::IDD_SETTINGS)),
            ("IDC_DEV1", ids::IDC_DEV1),
            ("IDC_DEV2", ids::IDC_DEV2),
            ("IDC_COMMS", ids::IDC_COMMS),
            ("IDC_PATH", ids::IDC_PATH),
            ("IDC_COPY", ids::IDC_COPY),
            ("IDC_TEST", ids::IDC_TEST),
            ("IDC_STATUS", ids::IDC_STATUS),
        ];
        let defines: Vec<(&str, i32)> = header
            .lines()
            .filter_map(|line| {
                let mut parts = line.split_whitespace();
                if parts.next()? != "#define" {
                    return None;
                }
                let name = parts.next()?;
                let value = parts.next()?.parse().ok()?;
                Some((name, value))
            })
            .collect();
        assert_eq!(defines, expected);
    }

    #[test]
    fn hotkey_path_is_the_plain_path() {
        assert_eq!(
            hotkey_path(Path::new(
                r"C:\Program Files\Toggle Audio\toggle-audiow.exe"
            )),
            r"C:\Program Files\Toggle Audio\toggle-audiow.exe"
        );
        assert_eq!(
            hotkey_path(Path::new(r"C:\bin\toggle-audio.exe")),
            r"C:\bin\toggle-audio.exe"
        );
    }

    #[test]
    fn manifest_version_matches_the_package_version() {
        // The manifest cannot take build.rs macros, so this test catches a forgotten bump.
        let manifest = include_str!("../../assets/app.manifest");
        let expected = format!(
            "name=\"Hotdogee.ToggleAudio\" version=\"{}.0\"",
            env!("CARGO_PKG_VERSION")
        );
        assert!(
            manifest.contains(&expected),
            "assets/app.manifest assemblyIdentity must read {expected}"
        );
    }
}
