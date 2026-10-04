//! Black-box tests of both executables: argument handling, exit codes, the `toggle-audio: ` error
//! prefix and the byte format of redirected output (UTF-8, `\n` line endings, no byte order mark;
//! DESIGN.md section 5).
//!
//! Every case here is decided before the program touches COM (usage errors, `--help`,
//! `--version`, and `toggle` without a usable configuration), so these tests need no audio device
//! and run on hosted CI. `APPDATA` always points at a scratch directory, so a regression cannot
//! touch the real configuration either.
//!
//! Running `toggle-audiow.exe` with piped output also proves that the Windows-subsystem binary
//! keeps inherited (redirected) handles instead of attaching to a console. If that detection ever
//! regressed, the program would show a message box and wait; [`run`] kills it after a timeout so
//! the test fails instead of hanging.

#![expect(
    clippy::panic,
    clippy::expect_used,
    reason = "test helpers report failures by panicking, like the tests that call them"
)]

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Both binaries, which share every code path tested here.
const BINARIES: [&str; 2] = [
    env!("CARGO_BIN_EXE_toggle-audio"),
    env!("CARGO_BIN_EXE_toggle-audiow"),
];

/// The suffix of every usage error.
const HELP_HINT: &str = "(run \"toggle-audio --help\" for usage)\n";

/// How long a run may take before it is treated as hung (for example on a message box).
const TIMEOUT: Duration = Duration::from_secs(30);

/// Runs `exe` with `args`, piped stdout and stderr, no stdin, and `APPDATA` pointing at a
/// directory that does not exist.
fn run(exe: &str, args: &[&str]) -> Output {
    let app_data: PathBuf = std::env::temp_dir().join("toggle-audio-cli-test-no-such-dir");
    run_with_app_data(exe, args, &app_data)
}

/// Runs `exe` with `args`, piped stdout and stderr, no stdin and the given `APPDATA`. Kills the
/// process and panics when it does not exit within [`TIMEOUT`].
fn run_with_app_data(exe: &str, args: &[&str], app_data: &Path) -> Output {
    let mut child = Command::new(exe)
        .args(args)
        .env("APPDATA", app_data)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("cannot run {exe}: {error}"));
    // Drain both pipes on their own threads so a full pipe can never block the child.
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                pipe.read_to_end(&mut bytes).expect("read a child pipe");
            }
            bytes
        })
    };
    let stdout = drain(child.stdout.take().map(|pipe| Box::new(pipe) as _));
    let stderr = drain(child.stderr.take().map(|pipe| Box::new(pipe) as _));
    let deadline = Instant::now() + TIMEOUT;
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll the child") {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{exe} {args:?} did not exit within {TIMEOUT:?} (waiting on a message box?)");
        }
        thread::sleep(Duration::from_millis(10));
    };
    Output {
        status,
        stdout: stdout.join().expect("stdout reader"),
        stderr: stderr.join().expect("stderr reader"),
    }
}

/// A scratch `APPDATA` directory, removed on drop.
struct ScratchAppData(PathBuf);

impl ScratchAppData {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "toggle-audio-cli-test-{}-{label}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("toggle-audio")).expect("create the scratch APPDATA");
        Self(path)
    }

    fn config_path(&self) -> PathBuf {
        self.0.join("toggle-audio").join("config.json")
    }
}

impl Drop for ScratchAppData {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
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

#[test]
fn toggle_without_a_configuration_exits_3_from_a_script() {
    let app_data = std::env::temp_dir().join("toggle-audio-cli-test-no-such-dir");
    let expected_path = app_data.join("toggle-audio").join("config.json");
    for exe in BINARIES {
        for args in [&[][..], &["toggle"][..]] {
            let output = run(exe, args);
            assert_eq!(output.status.code(), Some(3), "{exe} {args:?}");
            assert!(output.stdout.is_empty(), "{exe} {args:?}");
            let stderr = text(&output.stderr, "stderr");
            assert_eq!(
                stderr,
                format!(
                    "toggle-audio: no configuration found at {}; run \"toggle-audio settings\" to \
                     choose two devices\n",
                    expected_path.display()
                ),
                "{exe} {args:?}"
            );
        }
    }
}

#[test]
fn toggle_with_a_corrupt_configuration_exits_3_from_a_script() {
    let app_data = ScratchAppData::new("corrupt");
    std::fs::write(app_data.config_path(), "{ not json").expect("write the corrupt config");
    for exe in BINARIES {
        let output = run_with_app_data(exe, &["toggle"], &app_data.0);
        assert_eq!(output.status.code(), Some(3), "{exe}");
        assert!(output.stdout.is_empty(), "{exe}");
        let stderr = text(&output.stderr, "stderr");
        let prefix = format!(
            "toggle-audio: configuration file {} is not valid: ",
            app_data.config_path().display()
        );
        assert!(stderr.starts_with(&prefix), "{exe}: {stderr:?}");
        assert!(
            stderr.ends_with("; run \"toggle-audio settings\" to fix it\n"),
            "{exe}: {stderr:?}"
        );
    }
}
