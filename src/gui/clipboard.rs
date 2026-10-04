//! Putting text on the clipboard as `CF_UNICODETEXT`.
//!
//! The explicit `OpenClipboard` / `SetClipboardData` route is used instead of selecting the
//! command field and sending it `WM_COPY`: it leaves the field's selection and focus alone, and it
//! reports a clipboard held by another application as an error the dialog can show.

use std::ffi::c_void;
use std::ptr;

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::CF_UNICODETEXT;

use crate::error::{Error, Result};

/// Replaces the clipboard content with `text` (UTF-16, `CF_UNICODETEXT`), owned by `owner`.
///
/// # Errors
///
/// [`Error::Com`] when the clipboard cannot be opened (another application may hold it) or the
/// data cannot be allocated or set.
pub(super) fn copy_text(owner: HWND, text: &str) -> Result<()> {
    let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();

    // SAFETY: `owner` is a window of this thread (the dialog). The clipboard opened here is closed
    // below on every path.
    unsafe { OpenClipboard(Some(owner)) }.map_err(Error::com("OpenClipboard"))?;
    let result = replace_content(&wide);
    // SAFETY: balances the successful `OpenClipboard` above. A failure to close changes nothing
    // for the caller: the data is either set or `result` already carries the error.
    let _ = unsafe { CloseClipboard() };
    result
}

/// Empties the (already open) clipboard and sets `wide` (NUL-terminated UTF-16) as its text.
fn replace_content(wide: &[u16]) -> Result<()> {
    // SAFETY: the caller has the clipboard open; emptying it makes the owner window its owner.
    unsafe { EmptyClipboard() }.map_err(Error::com("EmptyClipboard"))?;

    // SAFETY: plain allocation; the handle is either handed to the system or freed below.
    let memory = unsafe { GlobalAlloc(GMEM_MOVEABLE, size_of_val(wide)) }
        .map_err(Error::com("GlobalAlloc"))?;
    if let Err(error) = fill(memory, wide) {
        free(memory);
        return Err(error);
    }
    // SAFETY: `memory` is an unlocked movable block holding NUL-terminated UTF-16 text, which is
    // what `CF_UNICODETEXT` requires. On success the system owns the block.
    match unsafe { SetClipboardData(u32::from(CF_UNICODETEXT.0), Some(HANDLE(memory.0))) } {
        Ok(_) => Ok(()),
        Err(error) => {
            free(memory);
            Err(Error::com("SetClipboardData")(error))
        }
    }
}

/// Copies `wide` into the movable global block `memory`, which is at least that large.
fn fill(memory: HGLOBAL, wide: &[u16]) -> Result<()> {
    // SAFETY: `memory` is a live block from `GlobalAlloc`.
    let target: *mut c_void = unsafe { GlobalLock(memory) };
    if target.is_null() {
        return Err(Error::com("GlobalLock")(windows_core::Error::from_thread()));
    }
    // SAFETY: the block holds `size_of_val(wide)` bytes and is locked; `GlobalAlloc` memory is
    // 8-byte aligned, so it is a valid `u16` destination that cannot overlap `wide`.
    unsafe { ptr::copy_nonoverlapping(wide.as_ptr(), target.cast::<u16>(), wide.len()) };
    // SAFETY: balances the `GlobalLock` above. It reports "failure" with NO_ERROR when the lock
    // count reaches zero, which is the expected outcome, so the result is ignored.
    let _ = unsafe { GlobalUnlock(memory) };
    Ok(())
}

/// Frees a block that was not handed to the clipboard.
fn free(memory: HGLOBAL) {
    // SAFETY: `memory` came from `GlobalAlloc`, is unlocked and is not owned by the system.
    let _ = unsafe { GlobalFree(Some(memory)) };
}
