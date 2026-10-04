//! The configuration file: which two devices to toggle between.
//!
//! Stored as JSON at `%APPDATA%\toggle-audio\config.json` ([`default_path`]) and written
//! atomically ([`save`]). Unknown fields are ignored, `switch_communications` defaults to `true`
//! and a device `name` defaults to empty; the name is informational only (error messages, and the
//! settings dialog when the device is not connected). Whitespace around a device id (a stray space
//! from hand editing) is removed when the file is loaded. Example:
//!
//! ```json
//! {
//!   "version": 1,
//!   "device1": { "id": "{0.0.0.00000000}.{739b3554-bfed-4d61-b407-a818b317c991}", "name": "PG42UQ (NVIDIA High Definition Audio)" },
//!   "device2": { "id": "{0.0.0.00000000}.{30045f40-8cfd-4441-bb89-0d13fc19b589}", "name": "喇叭 (FiiO BTA30 PRO)" },
//!   "switch_communications": true
//! }
//! ```

use std::ffi::{OsString, c_void};
use std::fs::{self, File};
use std::io::{self, Write as _};
use std::os::windows::ffi::OsStringExt as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{FOLDERID_RoamingAppData, KF_FLAG_DEFAULT, SHGetKnownFolderPath};

use crate::audio::same_endpoint_id;
use crate::error::{ConfigProblem, Error, Result};

/// The configuration format version this build reads and writes.
pub const CURRENT_VERSION: u32 = 1;

/// Directory name under `%APPDATA%`.
pub const APP_DIR_NAME: &str = "toggle-audio";

/// File name inside [`APP_DIR_NAME`].
pub const FILE_NAME: &str = "config.json";

/// Suffix appended to the configuration path for the temporary file [`save`] writes first.
const TEMP_SUFFIX: &str = ".tmp";

/// The UTF-8 byte order mark some editors (older Notepad) put in front of a file.
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// A configured playback endpoint.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceRef {
    /// Endpoint id (`IMMDevice::GetId`). This is the identity used for switching.
    pub id: String,
    /// Friendly name at the time the device was chosen. Informational only.
    #[serde(default)]
    pub name: String,
}

/// The whole configuration file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// Format version; [`CURRENT_VERSION`] when written by this build. Defaults to it when absent.
    #[serde(default = "current_version")]
    pub version: u32,
    /// Device 1: the toggle goes here from anywhere except Device 1 itself.
    pub device1: DeviceRef,
    /// Device 2: the toggle goes here from Device 1.
    pub device2: DeviceRef,
    /// Also switch the default communications device. Defaults to `true` when absent.
    #[serde(default = "default_true")]
    pub switch_communications: bool,
}

fn current_version() -> u32 {
    CURRENT_VERSION
}

fn default_true() -> bool {
    true
}

impl Config {
    /// A current-version configuration for the given devices.
    #[must_use]
    pub fn new(device1: DeviceRef, device2: DeviceRef, switch_communications: bool) -> Self {
        Self {
            version: CURRENT_VERSION,
            device1,
            device2,
            switch_communications,
        }
    }

    /// Checks that the configuration is usable: a supported version, both device ids non-empty
    /// and the two ids different (compared case-insensitively).
    ///
    /// Returns the problem rather than an [`Error`] because the caller knows the file path:
    /// [`load`] wraps it into [`Error::Config`], the settings dialog shows it in its status line.
    ///
    /// # Errors
    ///
    /// [`ConfigProblem::Invalid`] describing the first problem found.
    pub fn validate(&self) -> std::result::Result<(), ConfigProblem> {
        let invalid = |message: String| Err(ConfigProblem::Invalid(message));
        if !(1..=CURRENT_VERSION).contains(&self.version) {
            return invalid(format!(
                "version {} is not supported (this build reads version {CURRENT_VERSION})",
                self.version
            ));
        }
        for (label, device) in [("device1", &self.device1), ("device2", &self.device2)] {
            if device.id.trim().is_empty() {
                return invalid(format!("{label} has no device id"));
            }
        }
        if same_endpoint_id(self.device1.id.trim(), self.device2.id.trim()) {
            return invalid("device1 and device2 are the same device".to_owned());
        }
        Ok(())
    }
}

/// The configuration file path: `%APPDATA%\toggle-audio\config.json`.
///
/// Uses the `APPDATA` environment variable when it holds an absolute path, and otherwise asks the
/// shell (`SHGetKnownFolderPath(FOLDERID_RoamingAppData)`). In a normal session both name the same
/// folder. The variable comes first because reading it is free, while the shell call costs about
/// 2 ms on the toggle hot path, and because it lets tests and the manual test checklist point the
/// program at a scratch directory. Does not create the directory.
///
/// # Errors
///
/// [`Error::Com`] when `APPDATA` is not usable and the known folder cannot be resolved either.
pub fn default_path() -> Result<PathBuf> {
    let app_data = match std::env::var_os("APPDATA").map(PathBuf::from) {
        Some(path) if path.is_absolute() => path,
        _ => roaming_app_data()?,
    };
    Ok(app_data.join(APP_DIR_NAME).join(FILE_NAME))
}

/// The roaming application data folder from the shell, usually `C:\Users\<user>\AppData\Roaming`.
fn roaming_app_data() -> Result<PathBuf> {
    // SAFETY: `FOLDERID_RoamingAppData` is a valid static GUID, `KF_FLAG_DEFAULT` is a documented
    // flag value and `None` selects the current user. On success the shell hands us ownership of
    // a NUL-terminated UTF-16 string allocated with `CoTaskMemAlloc`.
    //
    // On failure the documentation asks the caller to free the out pointer as well; the windows
    // crate wrapper discards it. In practice it is null on failure, and this fallback runs at most
    // once per process, so nothing worth a raw FFI call can leak.
    let raw = unsafe { SHGetKnownFolderPath(&FOLDERID_RoamingAppData, KF_FLAG_DEFAULT, None) }
        .map_err(Error::com("SHGetKnownFolderPath"))?;
    // SAFETY: `raw` is the valid NUL-terminated string returned above and is still allocated; the
    // borrowed slice is copied into an owned `OsString` before the memory is freed below.
    let path = PathBuf::from(OsString::from_wide(unsafe { raw.as_wide() }));
    // SAFETY: `raw` was allocated by the shell with `CoTaskMemAlloc` and ownership passed to us;
    // nothing refers to it after this call.
    unsafe { CoTaskMemFree(Some(raw.0.cast::<c_void>().cast_const())) };
    Ok(path)
}

/// Reads and validates the configuration at `path`.
///
/// Returns `Ok(None)` when the file does not exist, which is the normal first-run state. A leading
/// UTF-8 byte order mark (written by some editors) is accepted, and whitespace around the device
/// ids is removed before validation, so a hand-edited `" {id} "` still finds its device.
///
/// # Errors
///
/// [`Error::Config`] with [`ConfigProblem::Read`] when the file exists but cannot be read,
/// [`ConfigProblem::Parse`] when it is not valid JSON of the expected shape, and
/// [`ConfigProblem::Invalid`] when [`Config::validate`] rejects it.
pub fn load(path: &Path) -> Result<Option<Config>> {
    let config_error = |problem| Error::Config {
        path: path.to_owned(),
        problem,
    };
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(config_error(ConfigProblem::Read(error))),
    };
    let json = bytes.strip_prefix(UTF8_BOM).unwrap_or(&bytes);
    let mut config: Config =
        serde_json::from_slice(json).map_err(|error| config_error(ConfigProblem::Parse(error)))?;
    for device in [&mut config.device1, &mut config.device2] {
        trim_in_place(&mut device.id);
    }
    config.validate().map_err(config_error)?;
    Ok(Some(config))
}

/// Removes leading and trailing whitespace from `text` without reallocating when there is none.
fn trim_in_place(text: &mut String) {
    let trimmed = text.trim();
    if trimmed.len() != text.len() {
        *text = trimmed.to_owned();
    }
}

/// Writes `config` to `path` atomically, creating the parent directory if needed.
///
/// The JSON (pretty-printed, UTF-8 without a byte order mark, trailing newline) goes to
/// `config.json.tmp` next to `path` and is flushed to disk; that file then replaces `path` in one
/// rename (`std::fs::rename`, which replaces an existing destination on Windows just like
/// `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING`), so a crash never leaves a truncated file
/// behind.
///
/// `config` is written as given; callers validate it first with [`Config::validate`].
///
/// # Errors
///
/// [`Error::Config`] with [`ConfigProblem::Write`] when the directory, the temporary file or the
/// rename fails; [`Error::Json`] if serialization fails.
pub fn save(path: &Path, config: &Config) -> Result<()> {
    let mut json = serde_json::to_string_pretty(config)?;
    json.push('\n');
    write_atomically(path, json.as_bytes()).map_err(|error| Error::Config {
        path: path.to_owned(),
        problem: ConfigProblem::Write(error),
    })
}

/// Writes `contents` to a temporary sibling of `path`, flushes it and renames it over `path`.
fn write_atomically(path: &Path, contents: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        fs::create_dir_all(dir)?;
    }
    let temp = temp_path(path);
    let result = write_and_flush(&temp, contents).and_then(|()| fs::rename(&temp, path));
    if result.is_err() {
        // Best effort: do not leave a stale temporary file behind. The original error matters more.
        let _ = fs::remove_file(&temp);
    }
    result
}

/// Creates (or truncates) `path`, writes `contents` and flushes it to disk.
fn write_and_flush(path: &Path, contents: &[u8]) -> io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(contents)?;
    // Make the data durable before the rename publishes it, so a power loss cannot leave a
    // renamed but empty file.
    file.sync_all()
}

/// `path` with [`TEMP_SUFFIX`] appended: `config.json` becomes `config.json.tmp`.
fn temp_path(path: &Path) -> PathBuf {
    let mut temp = path.as_os_str().to_owned();
    temp.push(TEMP_SUFFIX);
    PathBuf::from(temp)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::error::{EXIT_CONFIG, EXIT_FAILURE};

    const PG42UQ_ID: &str = "{0.0.0.00000000}.{739b3554-bfed-4d61-b407-a818b317c991}";
    const FIIO_ID: &str = "{0.0.0.00000000}.{30045f40-8cfd-4441-bb89-0d13fc19b589}";

    /// A fresh directory under the system temp directory, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "toggle-audio-test-{}-{label}-{unique}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn file(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn device(id: &str, name: &str) -> DeviceRef {
        DeviceRef {
            id: id.to_owned(),
            name: name.to_owned(),
        }
    }

    fn sample() -> Config {
        Config::new(
            device(PG42UQ_ID, "PG42UQ (NVIDIA High Definition Audio)"),
            device(FIIO_ID, "喇叭 (FiiO BTA30 PRO)"),
            true,
        )
    }

    fn invalid_message(config: &Config) -> String {
        match config.validate() {
            Err(ConfigProblem::Invalid(message)) => message,
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    fn load_problem(path: &Path) -> ConfigProblem {
        match load(path) {
            Err(Error::Config {
                path: reported,
                problem,
            }) => {
                assert_eq!(reported, path);
                problem
            }
            other => panic!("expected Error::Config, got {other:?}"),
        }
    }

    #[test]
    fn missing_optional_fields_take_their_defaults() {
        let config: Config =
            serde_json::from_str(r#"{ "device1": { "id": "a" }, "device2": { "id": "b" } }"#)
                .unwrap();
        assert_eq!(config.version, CURRENT_VERSION);
        assert!(config.switch_communications);
        assert_eq!(config.device1.name, "");
    }

    #[test]
    fn explicit_false_switch_communications_is_kept() {
        let config: Config = serde_json::from_str(
            r#"{ "device1": { "id": "a" }, "device2": { "id": "b" }, "switch_communications": false }"#,
        )
        .unwrap();
        assert!(!config.switch_communications);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let config: Config = serde_json::from_str(
            r#"{
                "version": 1,
                "theme": "dark",
                "device1": { "id": "a", "name": "A", "color": 3 },
                "device2": { "id": "b", "extra": [1, 2] },
                "switch_communications": false,
                "future": { "nested": true }
            }"#,
        )
        .unwrap();
        assert_eq!(
            config,
            Config::new(device("a", "A"), device("b", ""), false)
        );
    }

    #[test]
    fn saved_file_is_pretty_utf8_with_a_trailing_newline() {
        let dir = TempDir::new("format");
        let path = dir.file(FILE_NAME);
        save(&path, &sample()).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        let expected = format!(
            "{{\n  \"version\": 1,\n  \"device1\": {{\n    \"id\": \"{PG42UQ_ID}\",\n    \
             \"name\": \"PG42UQ (NVIDIA High Definition Audio)\"\n  }},\n  \"device2\": {{\n    \
             \"id\": \"{FIIO_ID}\",\n    \"name\": \"喇叭 (FiiO BTA30 PRO)\"\n  }},\n  \
             \"switch_communications\": true\n}}\n"
        );
        assert_eq!(text, expected);
    }

    #[test]
    fn round_trip_preserves_cjk_names() {
        let dir = TempDir::new("roundtrip");
        let path = dir.file(FILE_NAME);
        let config = Config::new(
            device(FIIO_ID, "喇叭 (FiiO BTA30 PRO)"),
            device(PG42UQ_ID, "耳機 (ヘッドホン) 🎧"),
            false,
        );
        save(&path, &config).unwrap();
        assert_eq!(load(&path).unwrap(), Some(config));
    }

    #[test]
    fn save_creates_missing_directories() {
        let dir = TempDir::new("mkdir");
        let path = dir.file("nested").join(APP_DIR_NAME).join(FILE_NAME);
        save(&path, &sample()).unwrap();
        assert_eq!(load(&path).unwrap(), Some(sample()));
    }

    #[test]
    fn save_replaces_an_existing_file_and_leaves_no_temp_file() {
        let dir = TempDir::new("replace");
        let path = dir.file(FILE_NAME);
        // A longer previous file proves the new content does not overwrite the old in place.
        fs::write(&path, " ".repeat(4096)).unwrap();
        save(&path, &sample()).unwrap();
        let mut swapped = sample();
        std::mem::swap(&mut swapped.device1, &mut swapped.device2);
        swapped.switch_communications = false;
        save(&path, &swapped).unwrap();

        assert_eq!(load(&path).unwrap(), Some(swapped));
        assert!(!temp_path(&path).exists());
        let entries: Vec<_> = fs::read_dir(&dir.0).unwrap().collect();
        assert_eq!(entries.len(), 1, "{entries:?}");
    }

    #[test]
    fn save_overwrites_a_stale_temp_file() {
        let dir = TempDir::new("stale");
        let path = dir.file(FILE_NAME);
        fs::write(temp_path(&path), "left over from a crash").unwrap();
        save(&path, &sample()).unwrap();
        assert_eq!(load(&path).unwrap(), Some(sample()));
        assert!(!temp_path(&path).exists());
    }

    #[test]
    fn save_reports_write_failures_with_the_path() {
        let dir = TempDir::new("writefail");
        // The would-be parent directory is a regular file, so the write cannot succeed.
        let blocker = dir.file("blocker");
        fs::write(&blocker, "not a directory").unwrap();
        let path = blocker.join(FILE_NAME);
        let error = save(&path, &sample()).unwrap_err();
        assert!(
            matches!(
                &error,
                Error::Config { path: reported, problem: ConfigProblem::Write(_) } if *reported == path
            ),
            "{error:?}"
        );
        assert_eq!(error.exit_code(), EXIT_FAILURE);
        assert!(error.to_string().contains("cannot be written"), "{error}");
    }

    #[test]
    fn temp_path_appends_the_suffix() {
        assert_eq!(
            temp_path(Path::new(r"C:\x\config.json")),
            PathBuf::from(r"C:\x\config.json.tmp")
        );
    }

    #[test]
    fn load_of_a_missing_file_is_none() {
        let dir = TempDir::new("missing");
        assert_eq!(load(&dir.file(FILE_NAME)).unwrap(), None);
        assert_eq!(
            load(&dir.file("no-such-dir").join(FILE_NAME)).unwrap(),
            None
        );
    }

    #[test]
    fn load_accepts_a_byte_order_mark() {
        let dir = TempDir::new("bom");
        let path = dir.file(FILE_NAME);
        let mut bytes = UTF8_BOM.to_vec();
        bytes.extend_from_slice(br#"{ "device1": { "id": "a" }, "device2": { "id": "b" } }"#);
        fs::write(&path, bytes).unwrap();
        assert_eq!(
            load(&path).unwrap(),
            Some(Config::new(device("a", ""), device("b", ""), true))
        );
    }

    #[test]
    fn load_reports_corrupt_json_as_a_parse_problem() {
        let dir = TempDir::new("corrupt");
        let path = dir.file(FILE_NAME);
        for content in [
            "",
            "{",
            "not json",
            "[1, 2]",
            r#"{ "device1": { "id": "a" } }"#,
            r#"{ "device1": "a", "device2": "b" }"#,
            r#"{ "device1": { "name": "A" }, "device2": { "id": "b" } }"#,
            r#"{ "device1": { "id": "a" }, "device2": { "id": "b" }, "version": -1 }"#,
            r#"{ "device1": { "id": "a" }, "device2": { "id": "b" }, "switch_communications": "yes" }"#,
        ] {
            fs::write(&path, content).unwrap();
            let problem = load_problem(&path);
            assert!(
                matches!(problem, ConfigProblem::Parse(_)),
                "{content}: {problem:?}"
            );
        }
    }

    #[test]
    fn corrupt_json_error_names_the_file_and_exits_with_the_config_code() {
        let dir = TempDir::new("message");
        let path = dir.file(FILE_NAME);
        fs::write(&path, "{ oops").unwrap();
        let error = load(&path).unwrap_err();
        assert_eq!(error.exit_code(), EXIT_CONFIG);
        let text = error.to_string();
        assert!(text.contains(&path.display().to_string()), "{text}");
        assert!(text.contains("is not valid"), "{text}");
    }

    #[test]
    fn load_trims_whitespace_around_ids() {
        let dir = TempDir::new("trim");
        let path = dir.file(FILE_NAME);
        fs::write(
            &path,
            format!(
                r#"{{ "device1": {{ "id": " {PG42UQ_ID} ", "name": " A " }}, "device2": {{ "id": "\t{FIIO_ID}\n" }} }}"#
            ),
        )
        .unwrap();
        let config = load(&path).unwrap().unwrap();
        assert_eq!(config.device1, device(PG42UQ_ID, " A "));
        assert_eq!(config.device2.id, FIIO_ID);
    }

    #[test]
    fn load_rejects_an_invalid_config() {
        let dir = TempDir::new("invalid");
        let path = dir.file(FILE_NAME);
        fs::write(
            &path,
            r#"{ "device1": { "id": "{ab}" }, "device2": { "id": "{AB}" } }"#,
        )
        .unwrap();
        let problem = load_problem(&path);
        assert!(matches!(problem, ConfigProblem::Invalid(_)), "{problem:?}");
    }

    #[test]
    fn load_reports_an_unreadable_path_as_a_read_problem() {
        let dir = TempDir::new("read");
        // Reading a directory fails with an error other than NotFound.
        let path = dir.file(FILE_NAME);
        fs::create_dir(&path).unwrap();
        let problem = load_problem(&path);
        assert!(matches!(problem, ConfigProblem::Read(_)), "{problem:?}");
    }

    #[test]
    fn validate_accepts_a_good_config() {
        assert!(sample().validate().is_ok());
    }

    #[test]
    fn validate_rejects_empty_ids() {
        let mut config = sample();
        config.device1.id = String::new();
        assert_eq!(invalid_message(&config), "device1 has no device id");

        let mut config = sample();
        config.device2.id = "  ".to_owned();
        assert_eq!(invalid_message(&config), "device2 has no device id");
    }

    #[test]
    fn validate_rejects_the_same_device_twice() {
        let mut config = sample();
        config.device2.id = PG42UQ_ID.to_uppercase();
        assert_eq!(
            invalid_message(&config),
            "device1 and device2 are the same device"
        );
    }

    #[test]
    fn validate_rejects_unsupported_versions() {
        for version in [0, CURRENT_VERSION + 1, u32::MAX] {
            let mut config = sample();
            config.version = version;
            let message = invalid_message(&config);
            assert!(
                message.starts_with(&format!("version {version} ")),
                "{message}"
            );
        }
    }

    #[test]
    fn default_path_is_under_app_data() {
        let path = default_path().unwrap();
        assert!(path.is_absolute(), "{}", path.display());
        assert!(
            path.ends_with(Path::new(APP_DIR_NAME).join(FILE_NAME)),
            "{}",
            path.display()
        );
        // APPDATA wins whenever it is set (it may point at a scratch directory during tests).
        if let Some(app_data) = std::env::var_os("APPDATA").map(PathBuf::from) {
            if app_data.is_absolute() {
                assert_eq!(path, app_data.join(APP_DIR_NAME).join(FILE_NAME));
            }
        }
    }

    #[test]
    fn the_shell_knows_the_roaming_folder() {
        let folder = roaming_app_data().unwrap();
        assert!(folder.is_absolute(), "{}", folder.display());
    }

    #[test]
    fn trim_in_place_only_touches_padded_text() {
        let mut text = String::from(" {id}\t");
        trim_in_place(&mut text);
        assert_eq!(text, "{id}");
        let mut text = String::from("{id}");
        trim_in_place(&mut text);
        assert_eq!(text, "{id}");
    }
}
