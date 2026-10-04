//! `--timing`: per-phase timestamps in microseconds since process start.
//!
//! [`start`] reads the process creation time (`GetProcessTimes`) and the precise wall clock
//! (`GetSystemTimePreciseAsFileTime`, through [`SystemTime::now`]) once, which places the start of
//! the QPC-based monotonic clock ([`Instant`], `QueryPerformanceCounter` on Windows) on the
//! process timeline. Every [`Timing::mark`] is then a single QPC read, and the first mark also
//! covers loader and runtime start-up (the `create_to_entry` figure of
//! `docs/research/benchmark-method.md` section 1.3).
//!
//! The process creation time comes from the kernel's system clock and can be coarser than QPC.
//! If it is unavailable, times are reported relative to [`start`] instead.
//!
//! When timing is disabled, [`start`] reads no clock and `mark` and `report` do nothing.

use std::fmt::Write as _;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

use crate::console::Output;

/// 100-nanosecond intervals between 1601-01-01 (the `FILETIME` epoch) and 1970-01-01.
const FILETIME_UNIX_EPOCH: u64 = 116_444_736_000_000_000;

/// Phase timestamps for one run.
#[derive(Debug)]
pub struct Timing {
    clock: Option<Clock>,
}

/// The clock state of an enabled [`Timing`].
#[derive(Debug)]
struct Clock {
    /// When [`start`] ran (QPC).
    origin: Instant,
    /// Microseconds from process creation to `origin`; 0 when the creation time is unavailable.
    origin_us: f64,
    /// Recorded phases, in order, as offsets from `origin`.
    marks: Vec<(&'static str, Duration)>,
}

/// Starts timing. Call as early as possible; with `enabled == false` nothing is measured.
#[must_use]
pub fn start(enabled: bool) -> Timing {
    let clock = enabled.then(|| {
        let origin = Instant::now();
        let now = SystemTime::now();
        let origin_us = match (process_creation_time(), filetime_of(now)) {
            (Some(created), Some(now)) => elapsed_us(created, now),
            _ => 0.0,
        };
        Clock {
            origin,
            origin_us,
            marks: Vec::with_capacity(16),
        }
    });
    Timing { clock }
}

impl Timing {
    /// Records that `phase` (for example `"com_ready"`) ended now.
    pub fn mark(&mut self, phase: &'static str) {
        if let Some(clock) = &mut self.clock {
            clock.marks.push((phase, clock.origin.elapsed()));
        }
    }

    /// Writes one line per mark, `timing<TAB><phase><TAB><microseconds since process start>`, to
    /// stderr through `out`, in a single write. Does nothing when timing is disabled.
    pub fn report(&self, out: &Output) {
        if let Some(clock) = &self.clock {
            out.err(&format_marks(clock.origin_us, &clock.marks));
        }
    }
}

/// Formats the report lines; microseconds with one decimal.
fn format_marks(origin_us: f64, marks: &[(&'static str, Duration)]) -> String {
    let mut text = String::with_capacity(marks.len() * 32);
    for (phase, offset) in marks {
        let us = origin_us + offset.as_secs_f64() * 1e6;
        // Writing to a String cannot fail.
        let _ = writeln!(text, "timing\t{phase}\t{us:.1}");
    }
    text
}

/// The creation time of the current process, in 100 ns units since 1601 (`FILETIME`).
fn process_creation_time() -> Option<u64> {
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no closing, and the four out
    // pointers refer to live, writable FILETIME values.
    unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &raw mut creation,
            &raw mut exit,
            &raw mut kernel,
            &raw mut user,
        )
    }
    .ok()?;
    Some((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
}

/// `time` in 100 ns units since 1601 (`FILETIME`), or `None` before 1970.
fn filetime_of(time: SystemTime) -> Option<u64> {
    let since_unix = time.duration_since(UNIX_EPOCH).ok()?;
    let ticks = u64::try_from(since_unix.as_nanos() / 100).ok()?;
    ticks.checked_add(FILETIME_UNIX_EPOCH)
}

/// Microseconds from `from` to `to`, both in 100 ns `FILETIME` units. Negative when the coarser
/// creation timestamp lands after `to`.
#[expect(
    clippy::cast_precision_loss,
    reason = "differences are far below 2^52 ticks (over 14 years)"
)]
fn elapsed_us(from: u64, to: u64) -> f64 {
    if to >= from {
        (to - from) as f64 / 10.0
    } else {
        -((from - to) as f64 / 10.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_timing_records_nothing() {
        let mut timing = start(false);
        timing.mark("start");
        assert!(timing.clock.is_none());
    }

    #[test]
    fn enabled_timing_keeps_marks_in_order() {
        let mut timing = start(true);
        timing.mark("start");
        timing.mark("end");
        let clock = timing.clock.as_ref().unwrap();
        let phases: Vec<_> = clock.marks.iter().map(|(phase, _)| *phase).collect();
        assert_eq!(phases, ["start", "end"]);
        assert!(clock.marks[0].1 <= clock.marks[1].1);
        // The process started before `start` ran (allowing for a coarse creation timestamp).
        assert!(clock.origin_us > -100_000.0, "{}", clock.origin_us);
    }

    #[test]
    fn marks_are_formatted_as_tab_separated_microseconds() {
        let marks = [
            ("start", Duration::ZERO),
            ("com_ready", Duration::from_nanos(1_234_560)),
        ];
        assert_eq!(
            format_marks(5_000.0, &marks),
            "timing\tstart\t5000.0\ntiming\tcom_ready\t6234.6\n"
        );
        assert_eq!(format_marks(0.0, &[]), "");
    }

    #[test]
    fn filetime_conversion_matches_the_windows_epoch() {
        assert_eq!(filetime_of(UNIX_EPOCH), Some(FILETIME_UNIX_EPOCH));
        assert_eq!(
            filetime_of(UNIX_EPOCH + Duration::from_micros(1)),
            Some(FILETIME_UNIX_EPOCH + 10)
        );
        assert_eq!(filetime_of(UNIX_EPOCH - Duration::from_secs(1)), None);
    }

    #[test]
    fn elapsed_time_can_be_negative() {
        assert!((elapsed_us(100, 150) - 5.0).abs() < f64::EPSILON);
        assert!((elapsed_us(150, 100) + 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn process_creation_time_is_in_the_past() {
        let created = process_creation_time().unwrap();
        let now = filetime_of(SystemTime::now()).unwrap();
        // Allow for the coarse kernel timestamp.
        assert!(created <= now + 200_000, "{created} > {now}");
    }
}
