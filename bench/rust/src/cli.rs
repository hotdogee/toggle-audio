//! Command-line parsing and the toggle decision. Pure code: no Win32, no COM,
//! fully unit-tested.

use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::OsStrExt;

/// The global flag that turns on phase timestamps on stderr. Accepted anywhere.
pub const TIMING_FLAG: &str = "--timing";

/// One usage line, printed to stderr when the arguments are missing or wrong.
pub const USAGE: &str = "usage: ta-rs [--timing] list | get | set <id> | toggle <idA> <idB>\n";

/// A command of the bench CLI contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Print every active render endpoint: `<id>\t<name>\t<flags>`.
    List,
    /// Print the default (eConsole) render endpoint: `<id>\t<name>`.
    Get,
    /// Make `id` the default for eConsole, eMultimedia and eCommunications.
    Set { id: OsString },
    /// If the current default is `a`, set `b`; otherwise set `a`.
    Toggle { a: OsString, b: OsString },
}

/// The parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// `--timing` was present somewhere in the arguments.
    pub timing: bool,
    /// `None` means a usage error (no command, unknown command, wrong arity).
    pub command: Option<Command>,
}

/// Parses the arguments that follow the program name.
///
/// `--timing` is removed wherever it appears; the remaining words must form
/// exactly one command with its exact number of operands. Command names are
/// case-sensitive, like every other implementation of the contract.
pub fn parse<I>(args: I) -> Invocation
where
    I: IntoIterator<Item = OsString>,
{
    let mut timing = false;
    let mut words: Vec<OsString> = Vec::with_capacity(3);
    for arg in args {
        if arg == TIMING_FLAG {
            timing = true;
        } else {
            words.push(arg);
        }
    }

    let command = match words.split_first() {
        Some((verb, operands)) => match (verb.to_str(), operands) {
            (Some("list"), []) => Some(Command::List),
            (Some("get"), []) => Some(Command::Get),
            (Some("set"), [id]) => Some(Command::Set { id: id.clone() }),
            (Some("toggle"), [a, b]) => Some(Command::Toggle {
                a: a.clone(),
                b: b.clone(),
            }),
            _ => None,
        },
        None => None,
    };
    Invocation { timing, command }
}

/// Converts an argument to the NUL-terminated UTF-16 string that COM expects.
/// `OsStr` on Windows holds the original UTF-16 losslessly (as WTF-8), so this
/// round-trips exactly what the user typed.
pub fn to_wide_nul(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// Compares two endpoint ids. Ids look like `{0.0.0.00000000}.{guid}`; Windows
/// returns them in lower case, but a user may paste upper-case GUIDs, so the
/// comparison ignores ASCII case. A trailing NUL terminator is ignored too.
pub fn ids_equal(x: &[u16], y: &[u16]) -> bool {
    let x = trim_nul(x);
    let y = trim_nul(y);
    x.len() == y.len()
        && x.iter()
            .zip(y)
            .all(|(&p, &q)| ascii_lower(p) == ascii_lower(q))
}

/// The toggle decision of the contract: if the current default is `a`, the
/// target is `b`; in every other case (current is `b`, some third device, or
/// there is no default at all) the target is `a`.
pub fn toggle_target<'a>(current: Option<&[u16]>, a: &'a [u16], b: &'a [u16]) -> &'a [u16] {
    match current {
        Some(cur) if ids_equal(cur, a) => b,
        _ => a,
    }
}

/// Returns the part of `s` before the first NUL (all of `s` if there is none).
fn trim_nul(s: &[u16]) -> &[u16] {
    match s.iter().position(|&c| c == 0) {
        Some(end) => &s[..end],
        None => s,
    }
}

/// Lower-cases ASCII `A`..=`Z`; every other UTF-16 unit is returned unchanged.
fn ascii_lower(c: u16) -> u16 {
    if (u16::from(b'A')..=u16::from(b'Z')).contains(&c) {
        c + 32
    } else {
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    const PG42UQ: &str = "{0.0.0.00000000}.{739b3554-bfed-4d61-b407-a818b317c991}";
    const BTA30: &str = "{0.0.0.00000000}.{30045f40-8cfd-4441-bb89-0d13fc19b589}";
    const OTHER: &str = "{0.0.0.00000000}.{5b124733-0000-0000-0000-000000000000}";

    fn set(id: &str) -> Command {
        Command::Set { id: id.into() }
    }

    fn toggle(a: &str, b: &str) -> Command {
        Command::Toggle {
            a: a.into(),
            b: b.into(),
        }
    }

    #[test]
    fn no_args_is_usage() {
        let inv = parse(args(&[]));
        assert_eq!(
            inv,
            Invocation {
                timing: false,
                command: None
            }
        );
    }

    #[test]
    fn timing_alone_is_usage_with_timing() {
        let inv = parse(args(&["--timing"]));
        assert_eq!(
            inv,
            Invocation {
                timing: true,
                command: None
            }
        );
    }

    #[test]
    fn list_and_get() {
        assert_eq!(parse(args(&["list"])).command, Some(Command::List));
        assert_eq!(parse(args(&["get"])).command, Some(Command::Get));
        assert!(!parse(args(&["list"])).timing);
    }

    #[test]
    fn set_takes_exactly_one_id() {
        assert_eq!(parse(args(&["set", PG42UQ])).command, Some(set(PG42UQ)));
        assert_eq!(parse(args(&["set"])).command, None);
        assert_eq!(parse(args(&["set", PG42UQ, BTA30])).command, None);
    }

    #[test]
    fn toggle_takes_exactly_two_ids() {
        assert_eq!(
            parse(args(&["toggle", PG42UQ, BTA30])).command,
            Some(toggle(PG42UQ, BTA30))
        );
        assert_eq!(parse(args(&["toggle"])).command, None);
        assert_eq!(parse(args(&["toggle", PG42UQ])).command, None);
        assert_eq!(parse(args(&["toggle", PG42UQ, BTA30, OTHER])).command, None);
    }

    #[test]
    fn timing_is_accepted_anywhere() {
        for line in [
            &["--timing", "set", PG42UQ][..],
            &["set", "--timing", PG42UQ][..],
            &["set", PG42UQ, "--timing"][..],
        ] {
            let inv = parse(args(line));
            assert!(inv.timing, "{line:?}");
            assert_eq!(inv.command, Some(set(PG42UQ)), "{line:?}");
        }
        let inv = parse(args(&["toggle", PG42UQ, "--timing", BTA30]));
        assert!(inv.timing);
        assert_eq!(inv.command, Some(toggle(PG42UQ, BTA30)));
    }

    #[test]
    fn repeated_timing_flag_is_harmless() {
        let inv = parse(args(&["--timing", "list", "--timing"]));
        assert_eq!(
            inv,
            Invocation {
                timing: true,
                command: Some(Command::List)
            }
        );
    }

    #[test]
    fn unknown_or_extra_words_are_usage() {
        assert_eq!(parse(args(&["frobnicate"])).command, None);
        assert_eq!(parse(args(&["list", "extra"])).command, None);
        assert_eq!(parse(args(&["get", "extra"])).command, None);
        assert_eq!(parse(args(&["LIST"])).command, None);
        assert_eq!(parse(args(&["--help"])).command, None);
        assert_eq!(parse(args(&["--TIMING", "list"])).command, None);
    }

    #[test]
    fn wide_conversion_is_nul_terminated_and_keeps_cjk() {
        assert_eq!(to_wide_nul(OsStr::new("喇叭")), vec![0x5587, 0x53ED, 0]);
        assert_eq!(to_wide_nul(OsStr::new("")), vec![0]);
    }

    #[test]
    fn ids_compare_ascii_case_insensitively() {
        assert!(ids_equal(&w(PG42UQ), &w(&PG42UQ.to_uppercase())));
        assert!(ids_equal(&w(PG42UQ), &to_wide_nul(OsStr::new(PG42UQ))));
        assert!(!ids_equal(&w(PG42UQ), &w(BTA30)));
        assert!(!ids_equal(&w(PG42UQ), &w(&PG42UQ[..PG42UQ.len() - 1])));
        assert!(ids_equal(&[], &[0]));
        // Only ASCII is folded: '@' (0x40) and '`' (0x60) must stay different.
        assert!(!ids_equal(&w("@"), &w("`")));
    }

    #[test]
    fn toggle_on_a_goes_to_b() {
        let (a, b) = (w(PG42UQ), w(BTA30));
        assert_eq!(toggle_target(Some(&a), &a, &b), b.as_slice());
    }

    #[test]
    fn toggle_on_b_goes_to_a() {
        let (a, b) = (w(PG42UQ), w(BTA30));
        assert_eq!(toggle_target(Some(&b), &a, &b), a.as_slice());
    }

    #[test]
    fn toggle_on_third_device_goes_to_a() {
        let (a, b, other) = (w(PG42UQ), w(BTA30), w(OTHER));
        assert_eq!(toggle_target(Some(&other), &a, &b), a.as_slice());
    }

    #[test]
    fn toggle_without_default_goes_to_a() {
        let (a, b) = (w(PG42UQ), w(BTA30));
        assert_eq!(toggle_target(None, &a, &b), a.as_slice());
    }

    #[test]
    fn toggle_matches_current_regardless_of_case_and_nul() {
        let a = to_wide_nul(OsStr::new(&PG42UQ.to_uppercase()));
        let b = to_wide_nul(OsStr::new(BTA30));
        let current = w(PG42UQ);
        assert_eq!(toggle_target(Some(&current), &a, &b), b.as_slice());
    }
}
