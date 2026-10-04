//! Integration tests against the real Windows audio stack.
//!
//! Hosted CI runners have no audio devices, so every test here is `#[ignore]`d. Run them on a
//! machine with at least one active playback device:
//!
//! ```text
//! cargo test -- --ignored
//! ```
//!
//! None of these tests may leave the default device changed: the only `set_default` calls target
//! the device that already is the default for the roles involved, or an id that is rejected
//! before `SetDefaultEndpoint` is reached.
//!
//! The one exception is opt-in twice: `toggle_round_trip_restores_the_default` only runs when
//! `TOGGLE_AUDIO_TEST_DEVICES` names two active playback endpoints (`<id1>;<id2>`), and a guard
//! restores every role's original default afterwards, also when an assertion fails (the test
//! profile unwinds on panic):
//!
//! ```text
//! $env:TOGGLE_AUDIO_TEST_DEVICES = '{0.0.0.00000000}.{...};{0.0.0.00000000}.{...}'
//! cargo test --test real_device -- --ignored toggle_round_trip
//! ```

#![expect(
    clippy::panic,
    clippy::expect_used,
    reason = "test helpers report failures by panicking, like the tests that call them"
)]

use std::ffi::c_void;
use std::sync::{Mutex, MutexGuard, PoisonError};

use toggle_audio::Error;
use toggle_audio::audio::{AudioSystem, Endpoint, Role, roles_for};
use toggle_audio::config::DeviceRef;
use toggle_audio::toggle;
use windows::Win32::Media::Audio::{
    DEVICE_STATE_ACTIVE, IMMDeviceEnumerator, MMDeviceEnumerator, eCapture,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};

const ALL_ROLES: [Role; 3] = [Role::Console, Role::Multimedia, Role::Communications];

/// `<id1>;<id2>`: the two active playback endpoints the round-trip test may switch between.
const DEVICES_VAR: &str = "TOGGLE_AUDIO_TEST_DEVICES";

/// A well-formed render endpoint id that no system has: `GetDevice` returns `E_NOTFOUND`.
const UNKNOWN_ID: &str = "{0.0.0.00000000}.{00000000-0000-0000-0000-000000000000}";

/// Serializes the tests in this file. The test harness runs tests on parallel threads, and the
/// round-trip test switches the default device while it runs: a read-only test that reads the
/// defaults at that moment sees a transient state (and `setting_the_current_default_changes_nothing`
/// would re-assert a stale default after the round trip restored the original one). Every test
/// holds this lock for its whole body; a panic in one test must not fail the others, so a poisoned
/// lock is taken over.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The default endpoint for every role, in [`ALL_ROLES`] order.
fn defaults(audio: &AudioSystem) -> toggle_audio::Result<Vec<Option<Endpoint>>> {
    ALL_ROLES
        .iter()
        .map(|&role| audio.default_for(role))
        .collect()
}

fn console_default(audio: &AudioSystem) -> toggle_audio::Result<Endpoint> {
    audio
        .default_for(Role::Console)?
        .ok_or(Error::NoDefaultDevice)
}

#[test]
#[ignore = "needs a Windows machine with an active playback device"]
fn enumeration_is_not_empty() -> toggle_audio::Result<()> {
    let _serial = serial();
    let audio = AudioSystem::new()?;
    let endpoints = audio.list_active()?;
    assert!(!endpoints.is_empty(), "no active playback endpoints");
    for endpoint in &endpoints {
        assert!(
            endpoint.id.starts_with("{0.0.0."),
            "not a render endpoint id: {endpoint:?}"
        );
        assert!(!endpoint.name.is_empty(), "no friendly name: {endpoint:?}");
    }
    Ok(())
}

#[test]
#[ignore = "needs a Windows machine with an active playback device"]
fn enumeration_contains_the_default_device() -> toggle_audio::Result<()> {
    let _serial = serial();
    let audio = AudioSystem::new()?;
    let default = console_default(&audio)?;
    let endpoints = audio.list_active()?;
    assert!(
        endpoints.contains(&default),
        "{default:?} missing from {endpoints:?}"
    );
    Ok(())
}

#[test]
#[ignore = "needs a Windows machine with an active playback device"]
fn every_role_has_an_active_default() -> toggle_audio::Result<()> {
    let _serial = serial();
    let audio = AudioSystem::new()?;
    for (role, default) in ALL_ROLES.iter().zip(defaults(&audio)?) {
        let default = default.unwrap_or_else(|| panic!("no default for {role:?}"));
        assert!(audio.is_active(&default.id)?, "{role:?}: {default:?}");
        // `name_of` is `None` for an endpoint without a friendly name, where `default_for` has "".
        assert_eq!(
            audio.name_of(&default.id)?.unwrap_or_default(),
            default.name
        );
    }
    Ok(())
}

#[test]
#[ignore = "needs a Windows machine with an active playback device"]
fn ids_are_matched_case_insensitively() -> toggle_audio::Result<()> {
    let _serial = serial();
    let audio = AudioSystem::new()?;
    let default = console_default(&audio)?;
    assert!(audio.is_active(&default.id.to_uppercase())?);
    Ok(())
}

#[test]
#[ignore = "needs a Windows machine with an active playback device"]
fn unknown_and_malformed_ids_are_reported_as_missing() -> toggle_audio::Result<()> {
    let _serial = serial();
    let audio = AudioSystem::new()?;
    for id in [UNKNOWN_ID, "not an endpoint id", ""] {
        assert!(!audio.is_active(id)?, "{id:?}");
        assert_eq!(audio.name_of(id)?, None, "{id:?}");
    }
    match audio.set_default(UNKNOWN_ID, &ALL_ROLES) {
        Err(Error::DeviceNotFound(id)) => assert_eq!(id, UNKNOWN_ID),
        other => panic!("expected DeviceNotFound, got {other:?}"),
    }
    Ok(())
}

#[test]
#[ignore = "needs a Windows machine with an active playback device"]
fn setting_the_current_default_changes_nothing() -> toggle_audio::Result<()> {
    let _serial = serial();
    let audio = AudioSystem::new()?;
    let before = defaults(&audio)?;

    // Each role re-set to its own current default: always a no-op, reported as "unchanged".
    for (&role, default) in ALL_ROLES.iter().zip(&before) {
        let default = default.as_ref().expect("a default for every role");
        assert!(!audio.set_default(&default.id, &[role])?, "{role:?}");
    }
    // All roles at once, but only when they share one device (otherwise this would switch).
    let console = before[0].as_ref().expect("a console default");
    if before
        .iter()
        .all(|default| default.as_ref() == Some(console))
    {
        assert!(!audio.set_default(&console.id, roles_for(true))?);
        assert!(!audio.set_default(&console.id.to_uppercase(), roles_for(true))?);
    }

    assert_eq!(defaults(&audio)?, before);
    Ok(())
}

#[test]
#[ignore = "needs a Windows machine with an active playback device"]
fn default_id_matches_default_for() -> toggle_audio::Result<()> {
    let _serial = serial();
    let audio = AudioSystem::new()?;
    for role in ALL_ROLES {
        assert_eq!(
            audio.default_id(role)?,
            audio.default_for(role)?.map(|endpoint| endpoint.id),
            "{role:?}"
        );
    }
    Ok(())
}

/// The id of the first active recording endpoint, or `None` when there is none. `_audio` keeps
/// COM initialized on this thread for the calls below.
fn first_active_capture_id(_audio: &AudioSystem) -> toggle_audio::Result<Option<String>> {
    // SAFETY: COM is initialized on this thread by `_audio`, which outlives every object created
    // here; no outer unknown.
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER) }
            .map_err(Error::com("CoCreateInstance(MMDeviceEnumerator)"))?;
    // SAFETY: `enumerator` is a live interface pointer; both arguments are valid constants.
    let collection = unsafe { enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE) }
        .map_err(Error::com("IMMDeviceEnumerator::EnumAudioEndpoints"))?;
    // SAFETY: `collection` is a live interface pointer.
    let count =
        unsafe { collection.GetCount() }.map_err(Error::com("IMMDeviceCollection::GetCount"))?;
    if count == 0 {
        return Ok(None);
    }
    // SAFETY: `collection` is live and index 0 is below its count.
    let device = unsafe { collection.Item(0) }.map_err(Error::com("IMMDeviceCollection::Item"))?;
    // SAFETY: `device` is a live interface pointer; the returned string is freed below.
    let raw = unsafe { device.GetId() }.map_err(Error::com("IMMDevice::GetId"))?;
    // SAFETY: `raw` is the NUL-terminated string GetId returned, still allocated.
    let id = unsafe { raw.to_string() };
    // SAFETY: `raw` was allocated with CoTaskMemAlloc by GetId and is freed exactly once.
    unsafe { CoTaskMemFree(Some(raw.0.cast::<c_void>().cast_const())) };
    Ok(Some(id.expect("endpoint ids are valid UTF-16")))
}

/// A recording endpoint must never be treated as a playback device: `GetDevice` resolves it, and
/// `SetDefaultEndpoint` would change the default microphone. Read-only: `set_default` is rejected
/// by the data-flow check before any policy-config call.
#[test]
#[ignore = "needs a Windows machine with an active playback device"]
fn recording_endpoints_are_not_playback_devices() -> toggle_audio::Result<()> {
    let _serial = serial();
    let audio = AudioSystem::new()?;
    let Some(capture_id) = first_active_capture_id(&audio)? else {
        eprintln!("no active recording endpoint; nothing to check");
        return Ok(());
    };
    assert!(!audio.is_active(&capture_id)?, "{capture_id}");
    assert_eq!(audio.name_of(&capture_id)?, None, "{capture_id}");
    match audio.set_default(&capture_id, &ALL_ROLES) {
        Err(Error::DeviceNotFound(id)) => assert_eq!(id, capture_id),
        other => panic!("expected DeviceNotFound for {capture_id}, got {other:?}"),
    }
    assert!(
        audio
            .list_active()?
            .iter()
            .all(|endpoint| !endpoint.id.eq_ignore_ascii_case(&capture_id))
    );
    Ok(())
}

/// Puts every role's original default back when dropped, including during a panic.
struct RestoreDefaults<'a> {
    audio: &'a AudioSystem,
    original: Vec<(Role, String)>,
}

impl Drop for RestoreDefaults<'_> {
    fn drop(&mut self) {
        for (role, id) in &self.original {
            if let Err(error) = self.audio.set_default(id, &[*role]) {
                eprintln!("could not restore the {role:?} default {id}: {error}");
            }
        }
    }
}

/// The two devices named by [`DEVICES_VAR`], if it is set.
fn test_devices(audio: &AudioSystem) -> toggle_audio::Result<Option<(DeviceRef, DeviceRef)>> {
    let Ok(value) = std::env::var(DEVICES_VAR) else {
        return Ok(None);
    };
    let (first, second) = value
        .split_once(';')
        .unwrap_or_else(|| panic!("{DEVICES_VAR} must be <id1>;<id2>, got {value:?}"));
    let device = |id: &str| -> toggle_audio::Result<DeviceRef> {
        let id = id.trim().to_owned();
        assert!(
            audio.is_active(&id)?,
            "{id} is not an active playback device"
        );
        let name = audio.name_of(&id)?.unwrap_or_default();
        Ok(DeviceRef { id, name })
    };
    Ok(Some((device(first)?, device(second)?)))
}

#[test]
#[ignore = "changes the default playback device; also needs TOGGLE_AUDIO_TEST_DEVICES"]
fn toggle_round_trip_restores_the_default() -> toggle_audio::Result<()> {
    let _serial = serial();
    let audio = AudioSystem::new()?;
    let Some((device1, device2)) = test_devices(&audio)? else {
        eprintln!("{DEVICES_VAR} is not set; the default device was not touched");
        return Ok(());
    };
    let original: Vec<(Role, String)> = ALL_ROLES
        .iter()
        .map(|&role| {
            let id = audio.default_id(role)?.expect("a default for every role");
            Ok((role, id))
        })
        .collect::<toggle_audio::Result<_>>()?;
    let guard = RestoreDefaults {
        audio: &audio,
        original: original.clone(),
    };

    let mut reached = Vec::new();
    for _ in 0..2 {
        let outcome = toggle::perform(&audio, &device1, &device2, true, |_| {})?;
        assert_eq!(outcome.warning, None, "both devices are active");
        for role in ALL_ROLES {
            let now = audio.default_id(role)?.expect("a default after toggling");
            assert!(
                now.eq_ignore_ascii_case(&outcome.device.id),
                "{role:?} is {now}, expected {}",
                outcome.device.id
            );
        }
        reached.push(outcome.device.id);
    }
    assert!(
        !reached[0].eq_ignore_ascii_case(&reached[1]),
        "two toggles must visit both devices: {reached:?}"
    );

    drop(guard);
    for (role, id) in &original {
        let now = audio.default_id(*role)?.expect("a default after restoring");
        assert!(now.eq_ignore_ascii_case(id), "{role:?} not restored");
    }
    Ok(())
}

#[test]
#[ignore = "needs a Windows machine with an active playback device"]
fn com_can_be_initialized_repeatedly_on_one_thread() -> toggle_audio::Result<()> {
    let _serial = serial();
    let outer = AudioSystem::new()?;
    {
        // S_FALSE path: nested initialization is balanced by the inner drop.
        let inner = AudioSystem::new()?;
        assert_eq!(
            inner.default_for(Role::Console)?,
            outer.default_for(Role::Console)?
        );
    }
    assert!(
        !outer.list_active()?.is_empty(),
        "the outer system still works after the inner one uninitialized COM"
    );
    Ok(())
}
