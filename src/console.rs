//! Text output that works for both binaries and every way they are launched.
//!
//! - `toggle-audio.exe` (console subsystem, `consoleAllocationPolicy=detached`) has a console
//!   when started from a terminal and none when started from G HUB or Explorer. Windows versions
//!   before 11 24H2 ignore the policy and give such a launch a brand-new console; [`init`] detects
//!   a console that no other process shares (`GetConsoleProcessList`), releases it and treats the
//!   run as console-less, so errors still reach the user in a message box and a missing
//!   configuration still opens Settings.
//! - `toggle-audiow.exe` (Windows subsystem) never gets a console of its own; [`init`] tries
//!   `AttachConsole(ATTACH_PARENT_PROCESS)` so output still reaches a parent terminal.
//!
//! Encoding (DESIGN.md section 5): console handles get UTF-16 through `WriteConsoleW`, so CJK
//! device names render regardless of the console code page; redirected handles (files, pipes)
//! get UTF-8 without BOM and with `\n` line endings. When neither stdout nor stderr leads
//! anywhere, errors are shown with a topmost `MessageBoxW` so a failed hotkey never fails
//! silently, and success stays silent.
//!
//! Rust's `print!` family is deliberately not used anywhere in the crate: its handle discovery
//! happens before `AttachConsole` and it cannot tell a missing handle from a broken pipe.

use std::cell::Cell;

use windows::Win32::Foundation::{GetLastError, HANDLE, HWND, NO_ERROR, POINT};
use windows::Win32::Storage::FileSystem::{FILE_TYPE_UNKNOWN, GetFileType, WriteFile};
use windows::Win32::System::Console::{
    ATTACH_PARENT_PROCESS, AttachConsole, CONSOLE_MODE, FreeConsole, GetConsoleMode,
    GetConsoleProcessList, GetStdHandle, STD_ERROR_HANDLE, STD_HANDLE, STD_OUTPUT_HANDLE,
    WriteConsoleW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, GetCursorPos, MB_ICONERROR, MB_ICONINFORMATION, MB_OK,
    MB_SETFOREGROUND, MB_TOPMOST, MESSAGEBOX_STYLE, MessageBoxW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP,
};
use windows_core::{PCWSTR, w};

/// Largest number of UTF-16 code units passed to one `WriteConsoleW` call. Old consoles failed on
/// buffers above 64 KiB; small chunks also keep every length far below `u32::MAX`.
const CONSOLE_CHUNK_UNITS: usize = 8 * 1024;

/// Largest number of bytes passed to one `WriteFile` call (keeps the length within `u32`).
const FILE_CHUNK_BYTES: usize = 1024 * 1024;

/// Where one standard stream leads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sink {
    /// No usable handle (no console and no redirection): output is dropped.
    Nowhere,
    /// A console screen buffer: written as UTF-16 with `WriteConsoleW`.
    Console(HANDLE),
    /// A file, pipe or character device such as `NUL`: written as UTF-8 bytes with `WriteFile`.
    Redirected(HANDLE),
}

impl Sink {
    /// Classifies the standard handle `which` of the current process.
    fn probe(which: STD_HANDLE) -> Self {
        // SAFETY: GetStdHandle only reads the process parameter block; it has no preconditions.
        let Ok(handle) = (unsafe { GetStdHandle(which) }) else {
            return Self::Nowhere;
        };
        // `is_invalid` covers both NULL (no handle was inherited) and INVALID_HANDLE_VALUE.
        if handle.is_invalid() {
            return Self::Nowhere;
        }
        let mut mode = CONSOLE_MODE::default();
        // SAFETY: `handle` is a non-null handle value and `mode` is a valid, writable CONSOLE_MODE.
        // GetConsoleMode fails cleanly for handles that are not console buffers.
        if unsafe { GetConsoleMode(handle, &raw mut mode) }.is_ok() {
            return Self::Console(handle);
        }
        // SAFETY: GetFileType accepts any handle value; an invalid one yields FILE_TYPE_UNKNOWN with
        // the reason in the thread's last-error value, which GetLastError reads right after.
        let usable =
            unsafe { GetFileType(handle) != FILE_TYPE_UNKNOWN || GetLastError() == NO_ERROR };
        if usable {
            Self::Redirected(handle)
        } else {
            Self::Nowhere
        }
    }

    fn is_console(self) -> bool {
        matches!(self, Self::Console(_))
    }

    /// This sink after the process released its console: console output goes nowhere, while
    /// redirected handles (files, pipes) are unaffected.
    fn without_console(self) -> Self {
        if self.is_console() {
            Self::Nowhere
        } else {
            self
        }
    }
}

/// Where text output goes. Created once per process by [`init`], before anything is written.
#[derive(Debug)]
pub struct Output {
    stdout: Sink,
    stderr: Sink,
    /// Set after `AttachConsole`: the parent shell does not wait for a Windows-subsystem program and
    /// has already printed its next prompt, so the first console write starts a fresh line.
    newline_pending: Cell<bool>,
}

/// Discovers the standard handles.
///
/// - `launched_windowed` (the Windows-subsystem binary) with a missing stdout or stderr: first
///   attaches to the parent process's console if it has one.
/// - Otherwise (the console-subsystem binary), a console that this process is the only user of
///   was created just for this launch (G HUB or Explorer on Windows before 11 24H2, where the
///   detached console policy is ignored). Nobody would read it and its window closes on exit, so
///   it is released with `FreeConsole` and the run counts as console-less.
///
/// Inherited (redirected) handles are always kept. Never fails: missing handles simply make
/// [`Output::has_console_or_redirect`] return `false` and the corresponding output is dropped.
#[must_use]
pub fn init(launched_windowed: bool) -> Output {
    let mut stdout = Sink::probe(STD_OUTPUT_HANDLE);
    let mut stderr = Sink::probe(STD_ERROR_HANDLE);
    let mut attached = false;
    if !launched_windowed
        && (stdout.is_console() || stderr.is_console())
        && is_private_console(console_process_count())
    {
        // SAFETY: FreeConsole has no memory-safety preconditions. Nothing has been written to the
        // console yet and the handles referring to it are dropped from `Output` right below.
        if unsafe { FreeConsole() }.is_ok() {
            stdout = stdout.without_console();
            stderr = stderr.without_console();
        }
    }
    if launched_windowed && (stdout == Sink::Nowhere || stderr == Sink::Nowhere) {
        // SAFETY: AttachConsole has no memory-safety preconditions. It fails with
        // ERROR_ACCESS_DENIED when already attached and ERROR_INVALID_HANDLE when the parent has no
        // console; both simply mean "no console to attach to" here.
        if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) }.is_ok() {
            attached = true;
            if stdout == Sink::Nowhere {
                stdout = Sink::probe(STD_OUTPUT_HANDLE);
            }
            if stderr == Sink::Nowhere {
                stderr = Sink::probe(STD_ERROR_HANDLE);
            }
        }
    }
    let newline_pending = attached && (stdout.is_console() || stderr.is_console());
    Output {
        stdout,
        stderr,
        newline_pending: Cell::new(newline_pending),
    }
}

/// How many processes are attached to this process's console (0 if the call fails). Two slots are
/// enough to tell "only this process" from "shared".
fn console_process_count() -> u32 {
    let mut ids = [0_u32; 2];
    // SAFETY: `ids` is a live, writable buffer and the wrapper passes its exact length. When the
    // buffer is too small the call still returns the total count, which is all that is used.
    unsafe { GetConsoleProcessList(&mut ids) }
}

/// Whether a console with `process_count` attached processes belongs to this process alone, i.e.
/// was created for this launch rather than inherited from a terminal (where the shell is attached
/// too). A failed count (0) keeps the console.
fn is_private_console(process_count: u32) -> bool {
    process_count == 1
}

impl Output {
    /// Whether stderr leads to a console or a redirected file/pipe, i.e. whether an error written
    /// with [`Output::err`] can be seen. When `false`, report errors with
    /// [`Output::message_box_error`] instead.
    #[must_use]
    pub fn has_console_or_redirect(&self) -> bool {
        self.stderr != Sink::Nowhere
    }

    /// Writes `s` to stdout as is (include the trailing `\n`). Write failures are ignored: there is
    /// nobody left to report them to.
    pub fn out(&self, s: &str) {
        self.write(self.stdout, s);
    }

    /// Writes `s` to stderr as is (include the trailing `\n`). Write failures are ignored.
    pub fn err(&self, s: &str) {
        self.write(self.stderr, s);
    }

    /// Shows a modal, topmost error message box (`MB_ICONERROR | MB_TOPMOST | MB_SETFOREGROUND`)
    /// and returns when the user dismisses it.
    pub fn message_box_error(&self, title: &str, text: &str) {
        message_box(title, text, MB_ICONERROR);
    }

    /// Shows a modal, topmost information message box (`MB_ICONINFORMATION | MB_TOPMOST |
    /// MB_SETFOREGROUND`) and returns when the user dismisses it.
    pub fn message_box_info(&self, title: &str, text: &str) {
        message_box(title, text, MB_ICONINFORMATION);
    }

    fn write(&self, sink: Sink, s: &str) {
        match sink {
            Sink::Nowhere => {}
            Sink::Console(handle) => {
                if self.newline_pending.replace(false) {
                    write_console(handle, "\n");
                }
                write_console(handle, s);
            }
            Sink::Redirected(handle) => write_file(handle, s.as_bytes()),
        }
    }
}

/// An OK-only, topmost message box with `icon`, brought to the foreground.
///
/// The box is owned by a [`TopmostOwner`]: a process started by a background program (G HUB
/// reacting to a hotkey, Explorer) may not take the foreground, and Windows 11 then creates an
/// unowned `MB_TOPMOST` box without `WS_EX_TOPMOST` at the bottom of the z-order, hidden behind
/// every other window. A window owned by a topmost window is topmost itself, so the box stays
/// visible even when the foreground request is refused. The owner sits at the mouse cursor, so the
/// box opens centred on the monitor the user is working on, like the settings dialog.
fn message_box(title: &str, text: &str, icon: MESSAGEBOX_STYLE) {
    let text = to_wide_nul(text);
    let title = to_wide_nul(title);
    let owner = TopmostOwner::create();
    // SAFETY: both buffers are NUL-terminated UTF-16 strings that outlive the call; the owner is
    // `None` or a window of this thread that stays alive until `owner` is dropped after the call.
    // The return value (which button closed the box) is irrelevant for an OK-only box.
    unsafe {
        MessageBoxW(
            owner.0,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | icon | MB_TOPMOST | MB_SETFOREGROUND,
        );
    }
}

/// A hidden, zero-size, topmost tool window at the mouse cursor that owns a message box (see
/// [`message_box`]). Destroyed on drop. Holds `None` when it cannot be created; the box is then
/// shown unowned, which is still better than no box.
struct TopmostOwner(Option<HWND>);

impl TopmostOwner {
    fn create() -> Self {
        let mut cursor = POINT::default();
        // SAFETY: `cursor` is a valid, writable POINT. On failure it stays at (0, 0), which is on
        // the primary monitor.
        let _ = unsafe { GetCursorPos(&raw mut cursor) };
        // SAFETY: "STATIC" is a system window class and both strings are NUL-terminated literals;
        // no parent, menu, instance or creation data. WS_POPUP without WS_VISIBLE is never shown,
        // and WS_EX_TOOLWINDOW keeps it out of the taskbar and Alt+Tab.
        let window = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                w!("STATIC"),
                w!(""),
                WS_POPUP,
                cursor.x,
                cursor.y,
                0,
                0,
                None,
                None,
                None,
                None,
            )
        };
        Self(window.ok())
    }
}

impl Drop for TopmostOwner {
    fn drop(&mut self) {
        if let Some(window) = self.0 {
            // SAFETY: `window` was created by this thread in `create` and is destroyed exactly once.
            let _ = unsafe { DestroyWindow(window) };
        }
    }
}

/// Writes `text` to the console buffer `handle` as UTF-16, retrying after partial writes.
/// Gives up silently on the first failure.
fn write_console(handle: HANDLE, text: &str) {
    let wide: Vec<u16> = text.encode_utf16().collect();
    let mut rest = wide.as_slice();
    while !rest.is_empty() {
        let chunk = &rest[..console_chunk_len(rest, CONSOLE_CHUNK_UNITS)];
        let mut written = 0_u32;
        // SAFETY: `handle` was classified as a console buffer by GetConsoleMode, `chunk` is a live
        // slice (its length fits in u32 because of CONSOLE_CHUNK_UNITS) and `written` is a valid
        // out pointer.
        let result = unsafe { WriteConsoleW(handle, chunk, Some(&raw mut written), None) };
        let written = usize::try_from(written).unwrap_or(usize::MAX);
        if result.is_err() || written == 0 {
            return;
        }
        rest = &rest[written.min(chunk.len())..];
    }
}

/// Writes `bytes` to the file or pipe `handle`, retrying after partial writes. Gives up silently
/// on the first failure (for example a pipe whose reader has gone away).
fn write_file(handle: HANDLE, bytes: &[u8]) {
    let mut rest = bytes;
    while !rest.is_empty() {
        let chunk = &rest[..rest.len().min(FILE_CHUNK_BYTES)];
        let mut written = 0_u32;
        // SAFETY: `handle` is an inherited standard handle that GetFileType accepted, `chunk` is a
        // live slice whose length fits in u32, `written` is a valid out pointer, and no OVERLAPPED
        // is passed, so the write completes before the call returns.
        let result = unsafe { WriteFile(handle, Some(chunk), Some(&raw mut written), None) };
        let written = usize::try_from(written).unwrap_or(usize::MAX);
        if result.is_err() || written == 0 {
            return;
        }
        rest = &rest[written.min(chunk.len())..];
    }
}

/// How many leading UTF-16 units of `units` to write in one call: at most `max`, and never ending
/// between the two halves of a surrogate pair (unless `max` is 1, where progress wins).
fn console_chunk_len(units: &[u16], max: usize) -> usize {
    if units.len() <= max {
        return units.len();
    }
    let ends_in_high_surrogate = (0xD800..=0xDBFF).contains(&units[max - 1]);
    if ends_in_high_surrogate && max > 1 {
        max - 1
    } else {
        max
    }
}

/// `s` as a NUL-terminated UTF-16 string for Win32. Interior NULs become spaces so the text is not
/// silently cut short.
fn to_wide_nul(s: &str) -> Vec<u16> {
    s.encode_utf16()
        .map(|unit| if unit == 0 { u16::from(b' ') } else { unit })
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};
    use std::os::windows::io::AsRawHandle as _;

    use super::*;

    #[test]
    fn wide_strings_are_nul_terminated_utf16() {
        assert_eq!(to_wide_nul("喇叭 A"), vec![0x5587, 0x53ED, 0x20, 0x41, 0]);
        assert_eq!(to_wide_nul(""), vec![0]);
    }

    #[test]
    fn interior_nuls_do_not_truncate_wide_strings() {
        assert_eq!(to_wide_nul("a\0b"), vec![0x61, 0x20, 0x62, 0]);
    }

    #[test]
    fn redirected_output_is_utf8_without_bom_or_crlf() {
        // A file handle classified the way `Sink::probe` does it, written through `Output`.
        let path = std::env::temp_dir().join(format!(
            "toggle-audio-console-test-{}.txt",
            std::process::id()
        ));
        let file = File::create(&path).unwrap();
        let handle = HANDLE(file.as_raw_handle());
        let output = Output {
            stdout: Sink::Redirected(handle),
            stderr: Sink::Nowhere,
            newline_pending: Cell::new(true),
        };
        output.out("{id}\t喇叭 (FiiO BTA30 PRO)\t*\n");
        output.out("second\n");
        output.err("dropped\n");
        drop(file);
        let bytes = fs::read(&path).unwrap();
        let _ = fs::remove_file(&path);
        // UTF-8, no BOM, `\n` untouched and no newline inserted for a redirected handle.
        assert_eq!(bytes, "{id}\t喇叭 (FiiO BTA30 PRO)\t*\nsecond\n".as_bytes());
        assert!(!output.has_console_or_redirect());
    }

    #[test]
    fn only_a_console_nobody_else_uses_is_released() {
        assert!(is_private_console(1));
        assert!(!is_private_console(0), "a failed count keeps the console");
        assert!(
            !is_private_console(2),
            "a terminal shares its console with the shell"
        );
        assert!(!is_private_console(5));
    }

    #[test]
    fn releasing_the_console_keeps_redirected_handles() {
        let console = Sink::Console(HANDLE(std::ptr::without_provenance_mut(4)));
        let file = Sink::Redirected(HANDLE(std::ptr::without_provenance_mut(8)));
        assert_eq!(console.without_console(), Sink::Nowhere);
        assert_eq!(file.without_console(), file);
        assert_eq!(Sink::Nowhere.without_console(), Sink::Nowhere);
    }

    #[test]
    fn short_text_is_written_in_one_chunk() {
        let units: Vec<u16> = "喇叭".encode_utf16().collect();
        assert_eq!(console_chunk_len(&units, 8), 2);
        assert_eq!(console_chunk_len(&[], 8), 0);
    }

    #[test]
    fn chunks_never_split_surrogate_pairs() {
        // U+1F50A SPEAKER WITH THREE SOUND WAVES is a surrogate pair in UTF-16.
        let units: Vec<u16> = "ab\u{1F50A}cd".encode_utf16().collect();
        assert_eq!(units.len(), 6);
        assert_eq!(console_chunk_len(&units, 2), 2);
        assert_eq!(
            console_chunk_len(&units, 3),
            2,
            "would end on a high surrogate"
        );
        assert_eq!(console_chunk_len(&units, 4), 4);
        assert_eq!(
            console_chunk_len(&units[2..], 1),
            1,
            "max 1 still makes progress"
        );
    }

    #[test]
    fn chunking_covers_long_text_exactly() {
        // The leading "a" shifts the pattern so a chunk boundary falls inside a surrogate pair.
        let text = format!("a{}", "喇叭\u{1F50A}".repeat(5_000));
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut rest = units.as_slice();
        let mut rebuilt = String::new();
        while !rest.is_empty() {
            let len = console_chunk_len(rest, CONSOLE_CHUNK_UNITS);
            assert!(len > 0 && len <= CONSOLE_CHUNK_UNITS);
            // Every chunk is valid UTF-16 on its own, i.e. no surrogate pair was split.
            rebuilt.push_str(&String::from_utf16(&rest[..len]).unwrap());
            rest = &rest[len..];
        }
        assert_eq!(rebuilt, text);
    }
}
