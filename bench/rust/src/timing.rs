//! `--timing` support: phase timestamps in microseconds since process creation.
//!
//! Two clocks are combined:
//! * At entry we read `QueryPerformanceCounter` (sub-microsecond, monotonic) and
//!   `GetSystemTimePreciseAsFileTime` (wall clock, 100 ns units).
//! * When the report is produced we ask `GetProcessTimes` for the process
//!   creation time (same FILETIME clock). `entry_wall - creation` is the
//!   `create_to_entry` span: loader + CRT + Rust std startup.
//!
//! Every later phase is `create_to_entry + (qpc_now - qpc_entry)`. Recording a
//! mark is two integer stores; formatting happens once, after all the work, so
//! the I/O does not distort the phases. Stamps are always recorded (they cost
//! nanoseconds); they are only printed when `--timing` was given.

use std::fmt::Write as _;

use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::SystemInformation::GetSystemTimePreciseAsFileTime;
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

/// Upper bound on recorded phases (`entry`, `com_init`, `enumerator`, `work_done`, `exit`).
const MAX_MARKS: usize = 8;

/// Phase recorder. Create it as the very first statement of `main`.
pub struct Timing {
    /// QPC ticks per second.
    frequency: i64,
    /// Wall-clock time at entry, 100 ns units since 1601 (FILETIME).
    entry_wall: u64,
    /// Fixed-size storage: no heap allocation on the measured path.
    marks: [(&'static str, i64); MAX_MARKS],
    len: usize,
}

impl Timing {
    /// Records the `entry` phase.
    pub fn start() -> Self {
        let counter = qpc_now();
        // SAFETY: GetSystemTimePreciseAsFileTime has no preconditions and only
        // returns a value.
        let entry_wall = filetime_to_u64(unsafe { GetSystemTimePreciseAsFileTime() });
        let mut frequency = 0i64;
        // SAFETY: the out-pointer refers to a live, writable i64. The call
        // cannot fail on Windows XP and later; if it ever did, `frequency`
        // stays 0 and `report` falls back to printing zeros.
        let _ = unsafe { QueryPerformanceFrequency(&raw mut frequency) };
        let mut timing = Self {
            frequency,
            entry_wall,
            marks: [("", 0); MAX_MARKS],
            len: 0,
        };
        timing.push("entry", counter);
        timing
    }

    /// Records `phase` at the current QPC time. Extra marks beyond the fixed
    /// capacity are ignored (never happens with the phases this program uses).
    pub fn mark(&mut self, phase: &'static str) {
        self.push(phase, qpc_now());
    }

    fn push(&mut self, phase: &'static str, counter: i64) {
        if let Some(slot) = self.marks.get_mut(self.len) {
            *slot = (phase, counter);
            self.len += 1;
        }
    }

    /// Formats one `phase\t<name>\t<microseconds since process creation>` line
    /// per recorded phase, with one decimal.
    // Tick deltas and the QPC frequency are far below 2^52, so f64 is exact enough.
    #[allow(clippy::cast_precision_loss)]
    pub fn report(&self) -> String {
        let create_to_entry_us = self.create_to_entry_us();
        let entry_counter = self.marks[0].1;
        let mut out = String::with_capacity(48 * self.len);
        for &(phase, counter) in &self.marks[..self.len] {
            let since_entry_us = if self.frequency > 0 {
                (counter - entry_counter) as f64 * 1_000_000.0 / self.frequency as f64
            } else {
                0.0
            };
            // Writing into a String cannot fail.
            let _ = writeln!(
                out,
                "phase\t{phase}\t{:.1}",
                create_to_entry_us + since_entry_us
            );
        }
        out
    }

    /// Microseconds from process creation to the `entry` stamp. The creation
    /// time comes from `GetProcessTimes`; if that call fails, 0 is used and the
    /// numbers become "since entry".
    // FILETIME values stay below 2^63 until the year 30828, and the delta is tiny.
    #[allow(clippy::cast_possible_wrap, clippy::cast_precision_loss)]
    fn create_to_entry_us(&self) -> f64 {
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // SAFETY: GetCurrentProcess returns a pseudo-handle that is always
        // valid and needs no CloseHandle; all four out-pointers refer to live,
        // writable FILETIME values.
        let ok = unsafe {
            GetProcessTimes(
                GetCurrentProcess(),
                &raw mut creation,
                &raw mut exit,
                &raw mut kernel,
                &raw mut user,
            )
        };
        if ok.is_err() {
            return 0.0;
        }
        // Signed difference: the creation stamp can be coarse, so a slightly
        // negative value is possible and must be reported as such.
        let delta_100ns = self.entry_wall as i64 - filetime_to_u64(creation) as i64;
        delta_100ns as f64 / 10.0
    }
}

/// Current QPC value (0 in the impossible case that the call fails).
fn qpc_now() -> i64 {
    let mut counter = 0i64;
    // SAFETY: the out-pointer refers to a live, writable i64.
    let _ = unsafe { QueryPerformanceCounter(&raw mut counter) };
    counter
}

fn filetime_to_u64(ft: FILETIME) -> u64 {
    (u64::from(ft.dwHighDateTime) << 32) | u64::from(ft.dwLowDateTime)
}
