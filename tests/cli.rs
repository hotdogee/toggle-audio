//! Black-box tests of both executables: argument handling, exit codes, the `toggle-audio: ` error
//! prefix and the byte format of redirected output (UTF-8, `\n` line endings, no byte order mark;
//! DESIGN.md section 5).
//!
//! Every case here is decided before the program touches COM or the configuration file (usage
//! errors, `--help`, `--version`), so these tests need no audio device and run on hosted CI.
//! `APPDATA` is pointed at a directory that does not exist anyway, so a regression could not
//! touch the real configuration either.
//!
//! Running `toggle-audiow.exe` with piped output also proves that the Windows-subsystem binary
//! keeps inherited (redirected) handles instead of attaching to a console.

#![allow(
    clippy::panic,
    clippy::expect_used,
    reason = "test helpers report failures by panicking, like the tests that call them"
)]

use std::path::PathBuf;
use std::process::{Command, Output};

/// Both binaries, which share every code path tested here.
const BINARIES: [&str; 2] = [
    env!("CARGO_BIN_EXE_toggle-audio"),
    env!("CARGO_BIN_EXE_toggle-audiow"),
];

/// The suffix of every usage error.
const HELP_HINT: &str = "(run \"toggle-audio --help\" for usage)\n";

/// Runs `exe` with `args`, piped stdout and stderr, and no stdin.
fn run(exe: &str, args: &[&str]) -> Output {
    let app_data: PathBuf = std::env::temp_dir().join("toggle-audio-cli-test-no-such-dir");
    Command::new(exe)
        .args(args)
        .env("APPDATA", app_data)
        .output()
        .unwrap_or_else(|error| panic!("cannot run {exe}: {error}"))
}

/// The output as UTF-8, after checking the bytes follow the redirected-output rules.
fn text(bytes: &[u8], what: &str) -> String {
    assert!(
        !bytes.starts_with(b"\xEF\xBB\xBF"),
        "{what} starts with a byte order mark"
    );
    assert!(!bytes.contains(&b'\r'), "{what} contains a carriage return");
    String::from_utf8(bytes.to_vec()).unwrap_or_else(|error| panic!("{what} is not UTF-8: {error}"))
}

#[test]
fn version_prints_the_package_version() {
    for exe in BINARIES {
        for flag in ["--version", "-V"] {
            let output = run(exe, &[flag]);
            assert_eq!(output.status.code(), Some(0), "{exe} {flag}");
            assert_eq!(
                output.stdout,
                format!("toggle-audio {}\n", env!("CARGO_PKG_VERSION")).as_bytes(),
                "{exe} {flag}"
            );
            assert!(output.stderr.is_empty(), "{exe} {flag}");
        }
    }
}

#[test]
fn help_is_plain_utf8_and_fits_80_columns() {
    for exe in BINARIES {
        let output = run(exe, &["--help"]);
        assert_eq!(output.status.code(), Some(0), "{exe}");
        let help = text(&output.stdout, "--help output");
        assert!(help.starts_with("toggle-audio "), "{help}");
        assert!(help.contains("\nUsage:\n"), "{help}");
        assert!(help.ends_with('\n'), "{help}");
        for line in help.lines() {
            assert!(line.chars().count() <= 80, "too wide: {line:?}");
        }
        assert!(output.stderr.is_empty(), "{exe}");
    }
}

#[test]
fn help_wins_and_ignores_timing() {
    for exe in BINARIES {
        let output = run(exe, &["bogus", "--timing", "-h"]);
        assert_eq!(output.status.code(), Some(0), "{exe}");
        assert!(output.stdout.starts_with(b"toggle-audio "), "{exe}");
        assert!(
            output.stderr.is_empty(),
            "no timing lines with --help: {exe}"
        );
    }
}

#[test]
fn usage_errors_exit_2_with_a_prefixed_message_and_a_help_hint() {
    let cases: [(&[&str], &str); 7] = [
        (&["bogus"], "unknown command \"bogus\""),
        (&["--frobnicate"], "unknown option \"--frobnicate\""),
        (&["set"], "\"set\" needs a device id or name"),
        (&["set", "   "], "\"set\" needs a device id or name"),
        (
            &["--comm", "--no-comm", "get"],
            "--comm and --no-comm cannot be used together",
        ),
        (
            &["list", "extra"],
            "unexpected argument \"extra\" after \"list\"",
        ),
        (
            &["get", "list"],
            "unexpected argument \"list\" after \"get\"",
        ),
    ];
    for exe in BINARIES {
        for (args, message) in cases {
            let output = run(exe, args);
            assert_eq!(output.status.code(), Some(2), "{exe} {args:?}");
            assert!(output.stdout.is_empty(), "{exe} {args:?}");
            let stderr = text(&output.stderr, "stderr");
            assert_eq!(
                stderr,
                format!("toggle-audio: {message} {HELP_HINT}"),
                "{exe} {args:?}"
            );
        }
    }
}
