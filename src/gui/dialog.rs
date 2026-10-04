//! The dialog procedure and its state: combo filling, Test toggle, Save, Copy.
//!
//! [`run`] first claims the single-instance mutex (a second launch brings the open dialog to the
//! front and returns), then builds a [`State`] (the COM apartment, the device choices, the path to
//! copy), boxes it and passes its address to `DialogBoxParamW` as the init parameter. `WM_INITDIALOG` stores
//! that address in the dialog's `DWLP_USER` slot and every later message reads it back as a
//! shared `&State`. Mutation goes through `Cell`s, because the dialog procedure is re-entered while
//! it runs (setting a control's text sends `WM_COMMAND` notifications back to the dialog), so a
//! `&mut State` could alias. The box outlives the dialog: `DialogBoxParamW` returns only after the
//! window is destroyed, and the box is dropped after that.
//!
//! Nothing here may panic (release builds abort on panic and would take the process down without
//! a message): failures become status-line text, and only failures to create the dialog at all are
//! returned as errors.
//!
//! # Manual test checklist
//!
//! Run with `APPDATA` pointed at a scratch directory so Save does not touch the real configuration
//! ([`crate::config::default_path`] reads the variable first), for example in PowerShell:
//! `$env:APPDATA = "$env:TEMP\ta-test"; .\toggle-audio.exe settings`.
//!
//! 1. First run (no config): both combos list the active playback devices by friendly name (CJK
//!    names render), nothing is preselected, "Also switch the Communications device" is checked,
//!    the status line asks to choose two devices, and the path field shows the exe path, never
//!    quoted. Two devices with the same friendly name are told apart by the end of their id in
//!    brackets.
//! 2. Save with a combo empty: the status line says which device is missing and focus moves to
//!    that combo. Choose the same device twice: the status line warns immediately, and Save is
//!    refused with focus on Device 2.
//! 3. Save with two different devices: the dialog closes and `%APPDATA%\toggle-audio\config.json`
//!    holds both ids and names. Reopen: both devices and the checkbox are preselected.
//! 4. Edit `config.json` so `device2.id` names an unplugged or unknown endpoint and reopen: the
//!    entry `(not connected) <name>` is appended to both combos and selected in Device 2, the
//!    status line says it is not connected, and Save without changes keeps it. Corrupt the file
//!    (for example delete the closing brace) and reopen: the status line says the saved
//!    configuration is not valid and asks to choose the devices again.
//! 5. Copy: the status line confirms, and pasting into Notepad gives exactly the path field.
//! 6. Test toggle: the default device changes and the status line shows `Default is now: <name>`
//!    (verify in Sound settings); nothing is written to `config.json`. With the preferred device
//!    unplugged the status line names the fallback and the warning; with neither available it
//!    shows the error.
//! 7. Cancel, Esc, Alt+F4 and the title-bar close button all close without writing anything.
//! 8. Keyboard only: Tab order follows reading order; Alt+1, Alt+2, Alt+C, Alt+M, Alt+Y, Alt+T
//!    and Alt+S work (each mnemonic is unique); Enter saves.
//! 9. Display scaling 100 %, 150 % and 200 %, and dragging the dialog between monitors with
//!    different scaling: the dialog manager re-lays out and re-fonts the controls (nothing is
//!    clipped or blurry, a three-line status fits). The dialog opens centred on the monitor under
//!    the mouse cursor, in front of other windows, with the application icon in the title bar and
//!    the taskbar.
//! 10. With the dialog open, run `settings` again: no second dialog appears, the open one comes to
//!     the front, and the second process exits 0.
//! 11. Without a configuration, run `toggle-audiow.exe` from Explorer (no console): the dialog
//!     says Toggle Audio is not set up yet; Save closes it and an information box says to press
//!     the hotkey again; Cancel exits with code 3.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::ptr;

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HINSTANCE, HWND, LPARAM, POINT, RECT,
    WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Controls::{
    BST_CHECKED, BST_UNCHECKED, CB_SETMINVISIBLE, CheckDlgButton, ICC_STANDARD_CLASSES,
    INITCOMMONCONTROLSEX, InitCommonControlsEx, IsDlgButtonChecked, LIM_LARGE, LIM_SMALL,
    LoadIconMetric,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BN_CLICKED, CB_ADDSTRING, CB_GETCURSEL, CB_GETITEMDATA, CB_SETCURSEL, CB_SETITEMDATA,
    CBN_SELCHANGE, DestroyIcon, DialogBoxParamW, EndDialog, FindWindowW, GetCursorPos, GetDlgItem,
    GetWindowLongPtrW, GetWindowRect, HICON, HWND_NOTOPMOST, HWND_TOPMOST, ICON_BIG, ICON_SMALL,
    IDCANCEL, IDOK, IsIconic, SW_RESTORE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SendMessageW, SetDlgItemTextW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, WINDOW_LONG_PTR_INDEX, WM_COMMAND, WM_INITDIALOG, WM_NEXTDLGCTL, WM_SETICON,
};
use windows_core::{HSTRING, PCWSTR};

use super::ids::{
    IDC_COMMS, IDC_COPY, IDC_DEV1, IDC_DEV2, IDC_PATH, IDC_STATUS, IDC_TEST, IDD_SETTINGS, IDI_APP,
};
use super::{OpenReason, clipboard, hotkey_path};
use crate::audio::{AudioSystem, Endpoint};
use crate::config::{self, Config, DeviceRef};
use crate::error::{ConfigProblem, Error, Result};
use crate::toggle::{self, Outcome, display_name};

/// `DWLP_USER`: the dialog window slot reserved for the dialog procedure's own data. The
/// `windows` crate does not define it; it is `DWLP_MSGRESULT + sizeof(LRESULT) + sizeof(DLGPROC)`.
#[cfg(target_pointer_width = "64")]
const DWLP_USER: WINDOW_LONG_PTR_INDEX = WINDOW_LONG_PTR_INDEX(16);
#[cfg(target_pointer_width = "32")]
const DWLP_USER: WINDOW_LONG_PTR_INDEX = WINDOW_LONG_PTR_INDEX(8);

/// Control id of the Save button (the dialog's default button, so Enter also sends it).
const ID_SAVE: i32 = IDOK.0;
/// Control id of the Cancel button (Esc, Alt+F4 and the close button also send it).
const ID_CANCEL: i32 = IDCANCEL.0;

/// Rows a dropped-down device combo shows before it scrolls.
const VISIBLE_ROWS: usize = 12;

/// Prefix of a combo entry for a configured device that is not currently active.
const NOT_CONNECTED: &str = "(not connected)";

/// Named mutex held while a settings dialog is open, so there is one per logon session. Without
/// it, repeated hotkey presses before the first setup would stack up dialogs.
const SINGLE_INSTANCE_MUTEX: &str = "Local\\Hotdogee.ToggleAudio.Settings";

/// The dialog's title (`CAPTION` in `assets/app.rc`), used to find an open dialog.
const DIALOG_TITLE: &str = "Toggle Audio Settings";

/// The window class of every dialog box.
const DIALOG_CLASS: &str = "#32770";

/// Runs the modal settings dialog. See [`super::show_settings`] for the contract.
pub(super) fn run(
    config_path: &Path,
    existing: Option<Config>,
    reason: &OpenReason,
    exe_path: &Path,
) -> Result<Option<Config>> {
    let Some(_instance) = InstanceLock::acquire() else {
        activate_open_dialog();
        return Ok(None);
    };
    let audio = AudioSystem::new()?;
    let active = audio.list_active()?;
    // SAFETY: a null module name asks for this executable's own module handle, which stays valid
    // for the life of the process.
    let instance: HINSTANCE = unsafe { GetModuleHandleW(PCWSTR::null()) }
        .map_err(Error::com("GetModuleHandleW"))?
        .into();
    let state = Box::new(State::new(
        audio,
        active,
        existing,
        reason,
        config_path,
        exe_path,
        instance,
    ));

    let classes = INITCOMMONCONTROLSEX {
        dwSize: win32_size_of::<INITCOMMONCONTROLSEX>(),
        dwICC: ICC_STANDARD_CLASSES,
    };
    // SAFETY: `classes` is a fully initialized INITCOMMONCONTROLSEX. A failure leaves the system
    // control classes in place; if they are unusable, DialogBoxParamW reports it below.
    let _ = unsafe { InitCommonControlsEx(&raw const classes) };

    // SAFETY: the template id names a dialog resource linked into the executable (assets/app.rc).
    // The init parameter is the address of the boxed state, which outlives the modal loop:
    // DialogBoxParamW returns only after the dialog window is destroyed, and `state` is dropped
    // after that. The dialog procedure only ever creates shared references from it.
    let result = unsafe {
        DialogBoxParamW(
            Some(instance),
            int_resource(IDD_SETTINGS),
            None,
            Some(dialog_proc),
            LPARAM(ptr::from_ref::<State>(&state) as isize),
        )
    };
    if result == -1 {
        let error = windows_core::Error::from_thread();
        return Err(Error::Gui(format!(
            "the settings dialog cannot be created: {} (HRESULT 0x{:08X})",
            error.message(),
            error.code().0
        )));
    }
    Ok(state.saved.take())
}

/// Ownership of [`SINGLE_INSTANCE_MUTEX`] for as long as the dialog runs.
struct InstanceLock(Option<HANDLE>);

impl InstanceLock {
    /// Opens or creates the mutex. `None` when another process created it first, i.e. a settings
    /// dialog is already open. When the mutex cannot be created at all the dialog runs anyway,
    /// unguarded: a duplicate dialog is better than none.
    fn acquire() -> Option<Self> {
        let name = HSTRING::from(SINGLE_INSTANCE_MUTEX);
        // SAFETY: `name` is a NUL-terminated string that outlives the call; no security
        // attributes. The handle is closed in `Drop`. GetLastError is read immediately after the
        // call, before anything else can overwrite the thread's last-error value.
        let (handle, already_exists) = unsafe {
            let handle = CreateMutexW(None, false, &name);
            (handle, GetLastError() == ERROR_ALREADY_EXISTS)
        };
        match handle {
            Ok(handle) => {
                let lock = Self(Some(handle));
                (!already_exists).then_some(lock)
            }
            Err(_) => Some(Self(None)),
        }
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        if let Some(handle) = self.0 {
            // SAFETY: the handle came from CreateMutexW, is owned by this guard and is closed
            // exactly once.
            let _ = unsafe { CloseHandle(handle) };
        }
    }
}

/// Brings the settings dialog of another process to the front (restoring it if minimized). Does
/// nothing when it cannot be found, for example because it is still being created.
fn activate_open_dialog() {
    // SAFETY: both strings are NUL-terminated and outlive the call.
    let found = unsafe { FindWindowW(&HSTRING::from(DIALOG_CLASS), &HSTRING::from(DIALOG_TITLE)) };
    let Ok(window) = found else {
        return;
    };
    // SAFETY: no pointers involved; a stale handle only makes the calls fail.
    if unsafe { IsIconic(window) }.as_bool() {
        // SAFETY: as above. The return value is the previous visibility, not an error.
        let _ = unsafe { ShowWindow(window, SW_RESTORE) };
    }
    bring_to_front(window);
}

/// Raises `window` above other windows and asks for the foreground.
///
/// A process launched by a background program (G HUB reacting to a hotkey) may not take the
/// foreground; the brief topmost round trip still puts the window on top of the z-order, so it is
/// visible instead of hidden behind the active application.
fn bring_to_front(window: HWND) {
    let flags = SWP_NOMOVE | SWP_NOSIZE;
    // SAFETY: no pointers involved; failures leave the z-order as it was.
    unsafe {
        let _ = SetWindowPos(window, Some(HWND_TOPMOST), 0, 0, 0, 0, flags);
        let _ = SetWindowPos(window, Some(HWND_NOTOPMOST), 0, 0, 0, 0, flags);
        let _ = SetForegroundWindow(window);
    }
}

/// One entry of both device combo boxes. Each combo item's data is an index into
/// [`State::choices`].
#[derive(Debug, Clone, PartialEq, Eq)]
struct Choice {
    /// The device as it would be written to the configuration.
    device: DeviceRef,
    /// `false` for a configured device that is not currently active.
    connected: bool,
}

impl Choice {
    /// The combo box text: the friendly name, prefixed with "(not connected)" when inactive.
    fn label(&self) -> String {
        if self.connected {
            display_name(&self.device).to_owned()
        } else {
            format!("{NOT_CONNECTED} {}", display_name(&self.device))
        }
    }
}

/// The combo box text of every choice, in order. Choices whose [`Choice::label`] is shared with
/// another one (two identical monitors, two "Speakers (USB Audio Device)") get the end of their
/// endpoint id appended in brackets, so they can be told apart; unlike a position, the id suffix
/// is stable across launches.
fn labels(choices: &[Choice]) -> Vec<String> {
    let plain: Vec<String> = choices.iter().map(Choice::label).collect();
    plain
        .iter()
        .zip(choices)
        .map(|(label, choice)| {
            if plain.iter().filter(|other| *other == label).count() > 1 {
                format!("{label} [{}]", id_suffix(&choice.device.id))
            } else {
                label.clone()
            }
        })
        .collect()
}

/// The last eight characters of an endpoint id without its closing brace: `ce6467fb` for
/// `{0.0.0.00000000}.{5b124733-5d8f-428c-b83c-ee05ce6467fb}`. Shorter ids are returned whole.
fn id_suffix(id: &str) -> &str {
    let id = id.trim_end_matches('}');
    let start = id.char_indices().rev().nth(7).map_or(0, |(index, _)| index);
    &id[start..]
}

/// Everything the dialog procedure needs, shared through `DWLP_USER` (see the module docs).
struct State {
    /// The COM apartment and device enumerator, used by Test toggle.
    audio: AudioSystem,
    /// The combo entries: active endpoints first, then configured devices that are not active.
    choices: Vec<Choice>,
    /// Index into `choices` to preselect in each combo (Device 1, Device 2).
    preselected: [Option<usize>; 2],
    /// Initial state of the "also switch Communications" checkbox.
    switch_communications: bool,
    /// Initial status line text.
    initial_status: String,
    /// Where Save writes the configuration.
    config_path: PathBuf,
    /// The program path shown in the path field and put on the clipboard by Copy.
    path: String,
    /// This executable's module, which holds the dialog and icon resources.
    instance: HINSTANCE,
    /// The saved configuration, set by Save just before the dialog closes.
    saved: Cell<Option<Config>>,
    /// Title bar and taskbar icons, destroyed when the state drops (after the window is gone).
    icons: [Cell<Option<OwnedIcon>>; 2],
}

impl State {
    fn new(
        audio: AudioSystem,
        active: Vec<Endpoint>,
        existing: Option<Config>,
        reason: &OpenReason,
        config_path: &Path,
        exe_path: &Path,
        instance: HINSTANCE,
    ) -> Self {
        let unconfigured = existing.is_none();
        let (configured, switch_communications) = match existing {
            Some(config) => (
                vec![config.device1, config.device2],
                config.switch_communications,
            ),
            None => (Vec::new(), true),
        };
        let choices = build_choices(active, &configured, |id| audio.name_of(id).ok().flatten());
        let preselected = [
            configured.first().and_then(|d| find(&choices, &d.id)),
            configured.get(1).and_then(|d| find(&choices, &d.id)),
        ];
        let initial_status = initial_status(&choices, &preselected, reason, unconfigured);
        Self {
            audio,
            choices,
            preselected,
            switch_communications,
            initial_status,
            config_path: config_path.to_owned(),
            path: hotkey_path(exe_path),
            instance,
            saved: Cell::new(None),
            icons: [Cell::new(None), Cell::new(None)],
        }
    }
}

/// An icon from `LoadIconMetric`, which the caller must destroy.
struct OwnedIcon(HICON);

impl Drop for OwnedIcon {
    fn drop(&mut self) {
        // SAFETY: the icon came from LoadIconMetric (not shared) and is destroyed exactly once.
        // The window that displayed it has been destroyed before the state drops.
        let _ = unsafe { DestroyIcon(self.0) };
    }
}

/// Builds the combo entries: every active endpoint, then each configured device that is not
/// among them, marked as not connected so the selection survives an unplugged device.
///
/// The name of a not-connected device comes from `name_of` (Windows remembers names of unplugged
/// endpoints), then from the configuration, then the id.
fn build_choices(
    active: Vec<Endpoint>,
    configured: &[DeviceRef],
    mut name_of: impl FnMut(&str) -> Option<String>,
) -> Vec<Choice> {
    let mut choices: Vec<Choice> = active
        .into_iter()
        .map(|endpoint| Choice {
            device: DeviceRef {
                id: endpoint.id,
                name: endpoint.name,
            },
            connected: true,
        })
        .collect();
    for device in configured {
        if device.id.is_empty() || find(&choices, &device.id).is_some() {
            continue;
        }
        let name = name_of(&device.id)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| device.name.clone());
        choices.push(Choice {
            device: DeviceRef {
                id: device.id.clone(),
                name,
            },
            connected: false,
        });
    }
    choices
}

/// The index of the choice with endpoint id `id` (compared case-insensitively).
fn find(choices: &[Choice], id: &str) -> Option<usize> {
    if id.is_empty() {
        return None;
    }
    choices
        .iter()
        .position(|choice| choice.device.id.eq_ignore_ascii_case(id))
}

/// The status line text when the dialog opens. `unconfigured` means there is no usable saved
/// configuration to preselect from.
fn initial_status(
    choices: &[Choice],
    preselected: &[Option<usize>; 2],
    reason: &OpenReason,
    unconfigured: bool,
) -> String {
    if !choices.iter().any(|choice| choice.connected) {
        return "No active playback device was found. Connect one, then open Settings again."
            .to_owned();
    }
    match reason {
        OpenReason::FirstRun => {
            return "Toggle Audio is not set up yet. Choose the two playback devices, press Save, \
                    then press your hotkey again."
                .to_owned();
        }
        OpenReason::Repair(problem) => {
            return format!("{problem} Choose the two playback devices again and press Save.");
        }
        OpenReason::Requested if unconfigured => {
            return "Choose the two playback devices to toggle between, then press Save."
                .to_owned();
        }
        OpenReason::Requested => {}
    }
    let disconnected: Vec<&str> = preselected
        .iter()
        .filter_map(|index| choices.get((*index)?))
        .filter(|choice| !choice.connected)
        .map(|choice| display_name(&choice.device))
        .collect();
    match disconnected.as_slice() {
        [] => String::new(),
        [name] => format!("\"{name}\" is not connected. It stays selected so the setting is kept."),
        [first, second, ..] => format!(
            "\"{first}\" and \"{second}\" are not connected. They stay selected so the setting is kept."
        ),
    }
}

/// Why the selected pair cannot be tested or saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PairProblem {
    /// Nothing is selected in Device 1.
    MissingDevice1,
    /// Nothing is selected in Device 2.
    MissingDevice2,
    /// Both combos name the same endpoint.
    Same,
}

impl PairProblem {
    /// Status line text.
    fn message(self) -> &'static str {
        match self {
            Self::MissingDevice1 => "Choose Device 1.",
            Self::MissingDevice2 => "Choose Device 2.",
            Self::Same => "Device 1 and Device 2 must be different devices.",
        }
    }

    /// The control to focus so the user can fix it.
    fn control(self) -> i32 {
        match self {
            Self::MissingDevice1 => IDC_DEV1,
            Self::MissingDevice2 | Self::Same => IDC_DEV2,
        }
    }
}

/// The devices selected in the two combos, checked to be present and different.
fn selected_pair(
    choices: &[Choice],
    first: Option<usize>,
    second: Option<usize>,
) -> std::result::Result<(&DeviceRef, &DeviceRef), PairProblem> {
    let device1 = first
        .and_then(|index| choices.get(index))
        .ok_or(PairProblem::MissingDevice1)?;
    let device2 = second
        .and_then(|index| choices.get(index))
        .ok_or(PairProblem::MissingDevice2)?;
    if device1.device.id.eq_ignore_ascii_case(&device2.device.id) {
        return Err(PairProblem::Same);
    }
    Ok((&device1.device, &device2.device))
}

/// Status line text after a failed Save: the reason first and the file last, so a long path
/// cannot push the reason out of the visible lines.
fn save_failure(error: &Error) -> String {
    match error {
        Error::Config {
            path,
            problem: ConfigProblem::Write(reason),
        } => format!("Save failed: {reason} ({})", path.display()),
        other => format!("Save failed: {other}"),
    }
}

/// Status line text after Test toggle.
fn test_status(result: &Result<Outcome>) -> String {
    match result {
        Ok(outcome) => match &outcome.warning {
            None => format!("Default is now: {}", display_name(&outcome.device)),
            Some(warning) => format!(
                "Default is now: {}. {warning}.",
                display_name(&outcome.device)
            ),
        },
        Err(error) => format!("Test toggle failed: {error}"),
    }
}

/// The dialog procedure. Returns non-zero for handled messages and zero otherwise, which lets the
/// dialog manager do its default processing (including `WM_DPICHANGED` rescaling).
unsafe extern "system" fn dialog_proc(
    dialog: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> isize {
    match message {
        WM_INITDIALOG => {
            // SAFETY: `lparam` is the boxed state's address passed by `run` to DialogBoxParamW;
            // storing it in this dialog's DWLP_USER slot is what `state` reads back.
            unsafe { SetWindowLongPtrW(dialog, DWLP_USER, lparam.0) };
            // SAFETY: DWLP_USER now holds the pointer `run` passed, which outlives the dialog.
            if let Some(state) = unsafe { state(dialog) } {
                on_init(dialog, state);
            }
            // Non-zero: let the dialog manager focus the first tab stop (Device 1).
            1
        }
        WM_COMMAND => {
            // SAFETY: DWLP_USER holds 0 or the pointer `run` passed, which outlives the dialog.
            match unsafe { state(dialog) } {
                Some(state) => isize::from(on_command(
                    dialog,
                    state,
                    i32::from(low_word(wparam.0)),
                    u32::from(high_word(wparam.0)),
                )),
                None => 0,
            }
        }
        _ => 0,
    }
}

/// The state stored in the dialog's `DWLP_USER` slot, or `None` before `WM_INITDIALOG`.
///
/// # Safety
///
/// `dialog` must be the settings dialog created by [`run`], whose `DWLP_USER` slot holds either
/// zero or the address of the boxed [`State`] that `run` keeps alive until the dialog is gone.
unsafe fn state<'a>(dialog: HWND) -> Option<&'a State> {
    // SAFETY: reading a window slot has no memory-safety requirements.
    let address = unsafe { GetWindowLongPtrW(dialog, DWLP_USER) };
    // SAFETY: per the contract the address is null or points at a live `State`, which is only
    // ever accessed through shared references while the dialog exists.
    unsafe { (address as *const State).as_ref() }
}

/// `WM_INITDIALOG`: icons, combos, checkbox, command field, status line and position.
fn on_init(dialog: HWND, state: &State) {
    set_icons(dialog, state);
    fill_combo(dialog, IDC_DEV1, &state.choices, state.preselected[0]);
    fill_combo(dialog, IDC_DEV2, &state.choices, state.preselected[1]);
    set_checked(dialog, IDC_COMMS, state.switch_communications);
    set_text(dialog, IDC_PATH, &state.path);
    set_text(dialog, IDC_STATUS, &state.initial_status);
    center_on_cursor_monitor(dialog);
    bring_to_front(dialog);
}

/// `WM_COMMAND`. Returns whether the command was handled.
fn on_command(dialog: HWND, state: &State, id: i32, code: u32) -> bool {
    match id {
        ID_SAVE => on_save(dialog, state),
        ID_CANCEL => end_dialog(dialog),
        IDC_COPY if code == BN_CLICKED => on_copy(dialog, state),
        IDC_TEST if code == BN_CLICKED => on_test(dialog, state),
        IDC_DEV1 | IDC_DEV2 if code == CBN_SELCHANGE => on_selection_change(dialog, state),
        _ => return false,
    }
    true
}

/// Save: validate, write the configuration, close the dialog with the saved configuration.
///
/// The dialog closes at once, so there is no "Saved" status to read; on the first-run path the
/// caller confirms with a message box instead (see `crate::run`).
fn on_save(dialog: HWND, state: &State) {
    let (device1, device2) = match current_pair(dialog, state) {
        Ok(pair) => pair,
        Err(problem) => {
            report_pair_problem(dialog, problem);
            return;
        }
    };
    let config = Config::new(
        device1.clone(),
        device2.clone(),
        is_checked(dialog, IDC_COMMS),
    );
    if let Err(problem) = config.validate() {
        set_text(
            dialog,
            IDC_STATUS,
            &format!("Cannot save: the configuration {problem}"),
        );
        return;
    }
    match config::save(&state.config_path, &config) {
        Ok(()) => {
            state.saved.set(Some(config));
            end_dialog(dialog);
        }
        Err(error) => set_text(dialog, IDC_STATUS, &save_failure(&error)),
    }
}

/// Test toggle: run the real toggle with the current, unsaved selections.
fn on_test(dialog: HWND, state: &State) {
    let (device1, device2) = match current_pair(dialog, state) {
        Ok(pair) => pair,
        Err(problem) => {
            report_pair_problem(dialog, problem);
            return;
        }
    };
    let result = toggle::perform(
        &state.audio,
        device1,
        device2,
        is_checked(dialog, IDC_COMMS),
        |_| {},
    );
    set_text(dialog, IDC_STATUS, &test_status(&result));
}

/// Copy: put the program path on the clipboard.
fn on_copy(dialog: HWND, state: &State) {
    let status = match clipboard::copy_text(dialog, &state.path) {
        Ok(()) => "Copied. In G HUB, assign a key to System > Launch Application and paste this \
                   into its Path field."
            .to_owned(),
        Err(error) => format!("Copy failed: {error}"),
    };
    set_text(dialog, IDC_STATUS, &status);
}

/// A combo selection changed: warn at once when both name the same device.
fn on_selection_change(dialog: HWND, state: &State) {
    let status = match current_pair(dialog, state) {
        Err(PairProblem::Same) => "Device 1 and Device 2 are the same device.",
        Ok(_) | Err(PairProblem::MissingDevice1 | PairProblem::MissingDevice2) => "",
    };
    set_text(dialog, IDC_STATUS, status);
}

/// The devices currently selected in the two combos.
fn current_pair(
    dialog: HWND,
    state: &State,
) -> std::result::Result<(&DeviceRef, &DeviceRef), PairProblem> {
    selected_pair(
        &state.choices,
        combo_selection(dialog, IDC_DEV1),
        combo_selection(dialog, IDC_DEV2),
    )
}

/// Shows `problem` in the status line and focuses the control that needs fixing.
fn report_pair_problem(dialog: HWND, problem: PairProblem) {
    set_text(dialog, IDC_STATUS, problem.message());
    focus_control(dialog, problem.control());
}

/// Closes the dialog. The result is in [`State::saved`], so the return code carries nothing.
fn end_dialog(dialog: HWND) {
    // SAFETY: `dialog` is the modal dialog created by DialogBoxParamW; EndDialog only flags it to
    // close when the procedure returns.
    let _ = unsafe { EndDialog(dialog, 0) };
}

/// Fills a device combo with every choice (item data = index into `choices`) and selects
/// `selected`, or nothing.
fn fill_combo(dialog: HWND, id: i32, choices: &[Choice], selected: Option<usize>) {
    let Some(combo) = control(dialog, id) else {
        return;
    };
    let mut selected_row = None;
    for (index, label) in labels(choices).into_iter().enumerate() {
        let label = HSTRING::from(label);
        // SAFETY: `label` is a NUL-terminated UTF-16 string that outlives the synchronous call;
        // CB_ADDSTRING copies it.
        let row = unsafe {
            SendMessageW(
                combo,
                CB_ADDSTRING,
                None,
                Some(LPARAM(label.as_ptr() as isize)),
            )
        };
        // Negative results are CB_ERR / CB_ERRSPACE: the item was not added.
        let (Ok(row), Ok(data)) = (usize::try_from(row.0), isize::try_from(index)) else {
            continue;
        };
        // SAFETY: integer arguments only.
        unsafe { SendMessageW(combo, CB_SETITEMDATA, Some(WPARAM(row)), Some(LPARAM(data))) };
        if selected == Some(index) {
            selected_row = Some(row);
        }
    }
    // SAFETY: integer arguments only. CB_SETCURSEL with -1 (usize::MAX) clears the selection.
    unsafe {
        SendMessageW(combo, CB_SETMINVISIBLE, Some(WPARAM(VISIBLE_ROWS)), None);
        SendMessageW(
            combo,
            CB_SETCURSEL,
            Some(WPARAM(selected_row.unwrap_or(usize::MAX))),
            None,
        );
    }
}

/// The index into [`State::choices`] selected in combo `id`, if any.
fn combo_selection(dialog: HWND, id: i32) -> Option<usize> {
    let combo = control(dialog, id)?;
    // SAFETY: integer arguments only.
    let row = unsafe { SendMessageW(combo, CB_GETCURSEL, None, None) };
    let row = usize::try_from(row.0).ok()?;
    // SAFETY: integer arguments only.
    let data = unsafe { SendMessageW(combo, CB_GETITEMDATA, Some(WPARAM(row)), None) };
    usize::try_from(data.0).ok()
}

/// The child control `id` of `dialog`.
fn control(dialog: HWND, id: i32) -> Option<HWND> {
    // SAFETY: no pointers involved; an invalid handle or id only makes the call fail.
    unsafe { GetDlgItem(Some(dialog), id) }.ok()
}

/// Moves the keyboard focus to control `id` the way the dialog manager does (updating the
/// default button), unlike `SetFocus`.
fn focus_control(dialog: HWND, id: i32) {
    if let Some(target) = control(dialog, id) {
        // SAFETY: WM_NEXTDLGCTL with lParam = TRUE takes a window handle in wParam.
        unsafe {
            SendMessageW(
                dialog,
                WM_NEXTDLGCTL,
                Some(WPARAM(target.0 as usize)),
                Some(LPARAM(1)),
            )
        };
    }
}

/// Sets the text of control `id`. Failure (an invalid id) leaves the control unchanged.
fn set_text(dialog: HWND, id: i32, text: &str) {
    // SAFETY: the HSTRING is NUL-terminated and outlives the synchronous call.
    let _ = unsafe { SetDlgItemTextW(dialog, id, &HSTRING::from(text)) };
}

/// Checks or clears checkbox `id`.
fn set_checked(dialog: HWND, id: i32, checked: bool) {
    let state = if checked { BST_CHECKED } else { BST_UNCHECKED };
    // SAFETY: no pointers involved.
    let _ = unsafe { CheckDlgButton(dialog, id, state) };
}

/// Whether checkbox `id` is checked.
fn is_checked(dialog: HWND, id: i32) -> bool {
    // SAFETY: no pointers involved.
    unsafe { IsDlgButtonChecked(dialog, id) == BST_CHECKED.0 }
}

/// Sets the title bar (small) and Alt+Tab / taskbar (large) icons at the sizes for the current
/// DPI. The dialog keeps working without them if loading fails.
fn set_icons(dialog: HWND, state: &State) {
    let sizes = [(LIM_SMALL, ICON_SMALL), (LIM_LARGE, ICON_BIG)];
    for ((metric, kind), slot) in sizes.into_iter().zip(&state.icons) {
        // SAFETY: the resource id is an integer resource (MAKEINTRESOURCE) in this module.
        let Ok(icon) =
            (unsafe { LoadIconMetric(Some(state.instance), int_resource(IDI_APP), metric) })
        else {
            continue;
        };
        // SAFETY: WM_SETICON takes the icon kind in wParam and an icon handle in lParam. The
        // icon stays alive in `state.icons` until after the window is destroyed.
        unsafe {
            SendMessageW(
                dialog,
                WM_SETICON,
                Some(WPARAM(kind as usize)),
                Some(LPARAM(icon.0 as isize)),
            )
        };
        slot.set(Some(OwnedIcon(icon)));
    }
}

/// Centres the dialog on the work area of the monitor under the mouse cursor. If that monitor has
/// a different DPI, Per-Monitor V2 sends `WM_DPICHANGED` and the dialog manager rescales.
fn center_on_cursor_monitor(dialog: HWND) {
    let mut cursor = POINT::default();
    let mut window = RECT::default();
    let mut monitor = MONITORINFO {
        cbSize: win32_size_of::<MONITORINFO>(),
        ..MONITORINFO::default()
    };
    // SAFETY: every out-pointer refers to a live, writable local of the right type, and
    // `monitor.cbSize` is set as GetMonitorInfoW requires.
    let found = unsafe {
        GetCursorPos(&raw mut cursor).is_ok()
            && GetMonitorInfoW(
                MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST),
                &raw mut monitor,
            )
            .as_bool()
            && GetWindowRect(dialog, &raw mut window).is_ok()
    };
    if !found {
        return;
    }
    let work = monitor.rcWork;
    let width = window.right - window.left;
    let height = window.bottom - window.top;
    // Keep the title bar on screen even if the dialog is larger than the work area.
    let x = work.left + ((work.right - work.left - width) / 2).max(0);
    let y = work.top + ((work.bottom - work.top - height) / 2).max(0);
    // SAFETY: no pointers involved.
    let _ = unsafe {
        SetWindowPos(
            dialog,
            None,
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };
}

/// `MAKEINTRESOURCEW(id)`: an integer resource id in a pointer-typed parameter.
fn int_resource(id: u16) -> PCWSTR {
    PCWSTR(ptr::without_provenance(usize::from(id)))
}

/// `sizeof(T)` for the `cbSize` / `dwSize` field of a Win32 structure.
#[allow(
    clippy::cast_possible_truncation,
    reason = "Win32 structures are a few hundred bytes at most"
)]
const fn win32_size_of<T>() -> u32 {
    size_of::<T>() as u32
}

/// `LOWORD`: the control id of a `WM_COMMAND` `wParam`.
#[allow(clippy::cast_possible_truncation, reason = "masked to 16 bits")]
fn low_word(value: usize) -> u16 {
    (value & 0xFFFF) as u16
}

/// `HIWORD` (of the low 32 bits): the notification code of a `WM_COMMAND` `wParam`.
#[allow(clippy::cast_possible_truncation, reason = "masked to 16 bits")]
fn high_word(value: usize) -> u16 {
    ((value >> 16) & 0xFFFF) as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toggle::Warning;

    fn endpoint(id: &str, name: &str) -> Endpoint {
        Endpoint {
            id: id.to_owned(),
            name: name.to_owned(),
        }
    }

    fn device(id: &str, name: &str) -> DeviceRef {
        DeviceRef {
            id: id.to_owned(),
            name: name.to_owned(),
        }
    }

    fn active() -> Vec<Endpoint> {
        vec![
            endpoint(
                "{0.0.0.00000000}.{aaaa}",
                "PG42UQ (NVIDIA High Definition Audio)",
            ),
            endpoint("{0.0.0.00000000}.{bbbb}", "喇叭 (FiiO BTA30 PRO)"),
        ]
    }

    #[test]
    fn active_endpoints_become_connected_choices_in_order() {
        let choices = build_choices(active(), &[], |_| None);
        assert_eq!(choices.len(), 2);
        assert!(choices.iter().all(|choice| choice.connected));
        assert_eq!(choices[1].label(), "喇叭 (FiiO BTA30 PRO)");
    }

    #[test]
    fn configured_inactive_devices_are_appended_once() {
        let configured = [
            device("{0.0.0.00000000}.{AAAA}", "PG42UQ"),
            device("{0.0.0.00000000}.{cccc}", "Headset"),
        ];
        let mut lookups = Vec::new();
        let choices = build_choices(active(), &configured, |id| {
            lookups.push(id.to_owned());
            None
        });
        // The active device matches case-insensitively and is not duplicated.
        assert_eq!(choices.len(), 3);
        assert_eq!(lookups, ["{0.0.0.00000000}.{cccc}"]);
        assert!(!choices[2].connected);
        assert_eq!(choices[2].label(), "(not connected) Headset");
        assert_eq!(choices[2].device, configured[1]);
    }

    #[test]
    fn not_connected_name_prefers_windows_then_config_then_id() {
        let configured = [device("x", "Old name"), device("y", "")];
        let choices = build_choices(Vec::new(), &configured, |id| {
            (id == "x").then(|| "Current name".to_owned())
        });
        assert_eq!(choices[0].label(), "(not connected) Current name");
        assert_eq!(choices[1].label(), "(not connected) y");

        let choices = build_choices(Vec::new(), &configured[..1], |_| Some(String::new()));
        assert_eq!(choices[0].label(), "(not connected) Old name");
    }

    #[test]
    fn duplicate_and_empty_configured_ids_are_ignored() {
        let configured = [device("same", "A"), device("SAME", "B"), device("", "C")];
        let choices = build_choices(Vec::new(), &configured, |_| None);
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].device.name, "A");
    }

    #[test]
    fn find_is_case_insensitive_and_ignores_empty_ids() {
        let choices = build_choices(active(), &[], |_| None);
        assert_eq!(find(&choices, "{0.0.0.00000000}.{BBBB}"), Some(1));
        assert_eq!(find(&choices, ""), None);
        assert_eq!(find(&choices, "missing"), None);
    }

    #[test]
    fn selected_pair_requires_two_different_devices() {
        let choices = build_choices(active(), &[device("{0.0.0.00000000}.{AAAA}", "")], |_| None);
        assert_eq!(
            selected_pair(&choices, None, Some(1)),
            Err(PairProblem::MissingDevice1)
        );
        assert_eq!(
            selected_pair(&choices, Some(0), None),
            Err(PairProblem::MissingDevice2)
        );
        assert_eq!(
            selected_pair(&choices, Some(0), Some(7)),
            Err(PairProblem::MissingDevice2)
        );
        assert_eq!(
            selected_pair(&choices, Some(1), Some(1)),
            Err(PairProblem::Same)
        );
        let (first, second) = selected_pair(&choices, Some(1), Some(0)).unwrap();
        assert_eq!(first.name, "喇叭 (FiiO BTA30 PRO)");
        assert_eq!(second.id, "{0.0.0.00000000}.{aaaa}");
    }

    #[test]
    fn pair_problems_focus_the_combo_to_fix() {
        assert_eq!(PairProblem::MissingDevice1.control(), IDC_DEV1);
        assert_eq!(PairProblem::MissingDevice2.control(), IDC_DEV2);
        assert_eq!(PairProblem::Same.control(), IDC_DEV2);
    }

    #[test]
    fn initial_status_explains_the_situation() {
        let requested = OpenReason::Requested;
        let choices = build_choices(active(), &[device("gone", "Headset")], |_| None);
        assert!(
            initial_status(&choices, &[None, None], &requested, true).starts_with("Choose the two")
        );
        assert_eq!(
            initial_status(&choices, &[Some(0), Some(1)], &requested, false),
            ""
        );
        assert_eq!(
            initial_status(&choices, &[Some(0), Some(2)], &requested, false),
            "\"Headset\" is not connected. It stays selected so the setting is kept."
        );
        assert!(
            initial_status(&choices, &[None, None], &OpenReason::FirstRun, true)
                .starts_with("Toggle Audio is not set up yet.")
        );
        let repair = OpenReason::Repair("The saved configuration is not valid: oops.".to_owned());
        assert_eq!(
            initial_status(&choices, &[None, None], &repair, true),
            "The saved configuration is not valid: oops. Choose the two playback devices again \
             and press Save."
        );
        let only_inactive = build_choices(Vec::new(), &[device("gone", "Headset")], |_| None);
        assert!(
            initial_status(
                &only_inactive,
                &[Some(0), None],
                &OpenReason::FirstRun,
                false
            )
            .starts_with("No active playback device")
        );
    }

    #[test]
    fn duplicate_names_are_told_apart_by_id() {
        let twins = vec![
            endpoint(
                "{0.0.0.00000000}.{5b124733-5d8f-428c-b83c-ee05ce6467fb}",
                "Speakers (USB Audio Device)",
            ),
            endpoint("{0.0.0.00000000}.{aaaa}", "PG42UQ"),
            endpoint(
                "{0.0.0.00000000}.{30045f40-8cfd-4441-bb89-0d13fc19b589}",
                "Speakers (USB Audio Device)",
            ),
        ];
        let choices = build_choices(twins, &[device("{x}", "PG42UQ")], |_| None);
        assert_eq!(
            labels(&choices),
            [
                "Speakers (USB Audio Device) [ce6467fb]",
                "PG42UQ",
                "Speakers (USB Audio Device) [fc19b589]",
                // "(not connected) PG42UQ" differs from "PG42UQ", so neither gets a suffix.
                "(not connected) PG42UQ",
            ]
        );
    }

    #[test]
    fn id_suffix_takes_the_end_of_the_id() {
        assert_eq!(
            id_suffix("{0.0.0.00000000}.{5b124733-5d8f-428c-b83c-ee05ce6467fb}"),
            "ce6467fb"
        );
        assert_eq!(id_suffix("{ab}"), "{ab");
        assert_eq!(id_suffix(""), "");
    }

    #[test]
    fn save_failures_put_the_reason_before_the_path() {
        let error = Error::Config {
            path: PathBuf::from(r"C:\Users\someone\AppData\Roaming\toggle-audio\config.json"),
            problem: ConfigProblem::Write(std::io::Error::other("disk full")),
        };
        assert_eq!(
            save_failure(&error),
            r"Save failed: disk full (C:\Users\someone\AppData\Roaming\toggle-audio\config.json)"
        );
        assert_eq!(
            save_failure(&Error::Gui("x".to_owned())),
            "Save failed: settings dialog: x"
        );
    }

    #[test]
    fn test_status_reports_outcome_warning_or_error() {
        let now = Outcome {
            device: device("b", "喇叭 (FiiO BTA30 PRO)"),
            warning: None,
        };
        assert_eq!(
            test_status(&Ok(now)),
            "Default is now: 喇叭 (FiiO BTA30 PRO)"
        );

        let fallback = Outcome {
            device: device("b", "Speakers"),
            warning: Some(Warning::PreferredUnavailable {
                preferred: device("a", "Headset"),
            }),
        };
        assert_eq!(
            test_status(&Ok(fallback)),
            "Default is now: Speakers. \"Headset\" is not connected; switched to the other \
             device instead."
        );

        let failed = Err(Error::DeviceNotFound("x".to_owned()));
        assert!(test_status(&failed).starts_with("Test toggle failed: "));
    }

    #[test]
    fn command_words_are_split() {
        let wparam = 0x0001_03EA; // CBN_SELCHANGE (1) from IDC_DEV2 (1002)
        assert_eq!(i32::from(low_word(wparam)), IDC_DEV2);
        assert_eq!(u32::from(high_word(wparam)), CBN_SELCHANGE);
    }

    #[test]
    fn int_resource_is_the_id_as_a_pointer() {
        assert_eq!(int_resource(IDD_SETTINGS).0 as usize, 101);
    }
}
