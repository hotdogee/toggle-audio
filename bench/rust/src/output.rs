//! Writing text to stdout / stderr without depending on any code page.
//!
//! * Redirected handle (file, pipe, NUL): the UTF-8 bytes go out unchanged via
//!   `WriteFile`, with `\n` line endings. This is what benchmarks and scripts
//!   see, and it is byte-identical across all implementations of the contract.
//! * Real console (`GetConsoleMode` succeeds): the text is converted to UTF-16
//!   and written with `WriteConsoleW`, so CJK names such as `喇叭` display
//!   correctly even though this machine's console code page is 950.
//! * No handle at all (launched from G HUB/Explorer with the detached console
//!   policy and no redirection): the text is silently dropped.
//!
//! We deliberately bypass `std::io::stdout()`: it would do the same thing, but
//! through a buffered, locked writer with its own lazy initialisation, and the
//! contract asks for one direct write per stream.

use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::WriteFile;
use windows::Win32::System::Console::{
    CONSOLE_MODE, GetConsoleMode, GetStdHandle, STD_ERROR_HANDLE, STD_HANDLE, STD_OUTPUT_HANDLE,
    WriteConsoleW,
};

/// Writes `text` to standard output. Errors (closed pipe, no handle) are ignored:
/// there is nowhere left to report them.
pub fn stdout(text: &str) {
    write_std(STD_OUTPUT_HANDLE, text);
}

/// Writes `text` to standard error. Errors are ignored for the same reason.
pub fn stderr(text: &str) {
    write_std(STD_ERROR_HANDLE, text);
}

fn write_std(which: STD_HANDLE, text: &str) {
    if text.is_empty() {
        return;
    }
    // SAFETY: GetStdHandle has no preconditions. The returned handle is owned
    // by the process and must not be closed, which we never do.
    let Ok(handle) = (unsafe { GetStdHandle(which) }) else {
        return;
    };
    if handle.is_invalid() || handle.0.is_null() {
        return;
    }
    let mut mode = CONSOLE_MODE::default();
    // SAFETY: `handle` is a valid standard handle and `mode` is a live,
    // writable CONSOLE_MODE. The call fails (harmlessly) for non-console handles.
    if unsafe { GetConsoleMode(handle, &raw mut mode) }.is_ok() {
        write_console(handle, text);
    } else {
        write_bytes(handle, text.as_bytes());
    }
}

/// `WriteFile` loop: a pipe may accept fewer bytes than requested.
fn write_bytes(handle: HANDLE, mut bytes: &[u8]) {
    while !bytes.is_empty() {
        // WriteFile takes a u32 length; cap each chunk accordingly.
        let chunk = &bytes[..bytes.len().min(u32::MAX as usize)];
        let mut written = 0u32;
        // SAFETY: `handle` is a valid, synchronous standard handle (no
        // OVERLAPPED needed), `chunk` is a live byte slice and `written` is a
        // live, writable u32.
        let result = unsafe { WriteFile(handle, Some(chunk), Some(&raw mut written), None) };
        if result.is_err() || written == 0 {
            return;
        }
        bytes = &bytes[(written as usize).min(bytes.len())..];
    }
}

/// UTF-16 units per `WriteConsoleW` call, well below the console's limits.
const CONSOLE_CHUNK: usize = 8192;

/// Length of the next console chunk: at most `max` units, shortened by one if
/// it would end on a high surrogate, so a surrogate pair (a non-BMP character
/// such as an emoji) is never split across two `WriteConsoleW` calls.
fn console_chunk_len(rest: &[u16], max: usize) -> usize {
    let len = rest.len().min(max);
    if len > 1 && len < rest.len() && (0xD800..=0xDBFF).contains(&rest[len - 1]) {
        len - 1
    } else {
        len
    }
}

/// `WriteConsoleW` loop in chunks that stay well below the console's limits.
fn write_console(handle: HANDLE, text: &str) {
    let wide: Vec<u16> = text.encode_utf16().collect();
    let mut rest = wide.as_slice();
    while !rest.is_empty() {
        let chunk = &rest[..console_chunk_len(rest, CONSOLE_CHUNK)];
        let mut written = 0u32;
        // SAFETY: `handle` is a console output handle (GetConsoleMode
        // succeeded), `chunk` is a live UTF-16 slice and `written` is a live,
        // writable u32. The reserved parameter must be null.
        let result = unsafe { WriteConsoleW(handle, chunk, Some(&raw mut written), None) };
        if result.is_err() || written == 0 {
            return;
        }
        rest = &rest[(written as usize).min(rest.len())..];
    }
}

#[cfg(test)]
mod tests {
    use super::console_chunk_len;

    #[test]
    fn console_chunks_never_split_a_surrogate_pair() {
        // "ab" + U+1F3A7 (headphone emoji) = a, b, 0xD83C, 0xDFA7.
        let text: Vec<u16> = "ab\u{1F3A7}".encode_utf16().collect();
        assert_eq!(console_chunk_len(&text, 3), 2); // would end on 0xD83C
        assert_eq!(console_chunk_len(&text, 4), 4);
        assert_eq!(console_chunk_len(&text, 2), 2);
        assert_eq!(console_chunk_len(&text, 100), 4);
    }
}
