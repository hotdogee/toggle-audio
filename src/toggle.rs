//! Toggle semantics: which device to switch to (DESIGN.md section 6).
//!
//! [`choose_target`] is a pure function of the current default, the two configured devices and an
//! `is_active` predicate, so the whole decision table is unit-tested without audio hardware.
//! [`perform`] wires it to an [`AudioSystem`]; the CLI `toggle` command and the settings dialog's
//! Test toggle button both use it, so the hot path exists exactly once.

use std::fmt;

use crate::audio::{AudioSystem, Role, roles_for, same_endpoint_id};
use crate::config::DeviceRef;
use crate::error::{Error, Result};

/// A non-fatal condition reported alongside a successful toggle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Warning {
    /// The preferred device was not active, so the toggle went to the other configured device.
    PreferredUnavailable {
        /// The device the toggle would normally have switched to.
        preferred: DeviceRef,
    },
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PreferredUnavailable { preferred } => write!(
                f,
                "\"{}\" is not connected; switched to the other device instead",
                display_name(preferred)
            ),
        }
    }
}

/// The decision made by [`choose_target`].
///
/// Borrows the chosen device from the caller's configuration; the warning owns a copy of the
/// preferred device so it can outlive the configuration (for example in an error report).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target<'a> {
    /// The device to make the default.
    pub device: &'a DeviceRef,
    /// Set when the preferred device was unavailable and `device` is the fallback.
    pub warning: Option<Warning>,
}

/// The result of [`perform`]: the device that is now the default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The device that was made the default.
    pub device: DeviceRef,
    /// Set when the preferred device was unavailable and `device` is the fallback.
    pub warning: Option<Warning>,
}

/// Decides which configured device a toggle switches to.
///
/// - `current` is the id of the current default playback device (`None` when there is none).
/// - If `current` is Device 1, the preferred target is Device 2 with no fallback.
/// - Otherwise (Device 2, another device or none) the preferred target is Device 1 with Device 2
///   as the fallback.
/// - The preferred device wins when `is_active` says it is active. Otherwise the fallback is used,
///   with [`Warning::PreferredUnavailable`], when it is active and not already the default.
///
/// Ids are compared case-insensitively. `is_active` is called at most twice.
///
/// # Errors
///
/// [`Error::NoDeviceAvailable`] when there is nothing to switch to, naming the device that is not
/// available and, when that is the reason the other one cannot be used, the device that already
/// is the default. Any error returned by `is_active` is passed through.
pub fn choose_target<'a>(
    current: Option<&str>,
    device1: &'a DeviceRef,
    device2: &'a DeviceRef,
    mut is_active: impl FnMut(&str) -> Result<bool>,
) -> Result<Target<'a>> {
    let is_current =
        |device: &DeviceRef| current.is_some_and(|id| same_endpoint_id(id, &device.id));
    let (preferred, fallback) = if is_current(device1) {
        (device2, None)
    } else {
        (device1, Some(device2))
    };
    if is_active(&preferred.id)? {
        return Ok(Target {
            device: preferred,
            warning: None,
        });
    }
    let name = |device: &DeviceRef| display_name(device).to_owned();
    let Some(fallback) = fallback else {
        // On Device 1 and Device 2 is unavailable.
        return Err(Error::NoDeviceAvailable {
            unavailable: vec![name(preferred)],
            already_default: Some(name(device1)),
        });
    };
    // Skip the lookup when the fallback already is the default: switching to it would be a silent
    // no-op, which is exactly what this decision table exists to avoid.
    if is_current(fallback) {
        return Err(Error::NoDeviceAvailable {
            unavailable: vec![name(preferred)],
            already_default: Some(name(fallback)),
        });
    }
    if is_active(&fallback.id)? {
        return Ok(Target {
            device: fallback,
            warning: Some(Warning::PreferredUnavailable {
                preferred: preferred.clone(),
            }),
        });
    }
    Err(Error::NoDeviceAvailable {
        unavailable: vec![name(preferred), name(fallback)],
        already_default: None,
    })
}

/// Performs a toggle: reads the current console default's id, applies [`choose_target`] and
/// makes the chosen device the default for [`roles_for`]`(switch_communications)`.
///
/// The configured ids are first looked up with [`AudioSystem::lookup`], so the decision compares
/// ids as Windows spells them: a configured id in another case, or a stable id that `GetDevice`
/// also accepts, is still recognized as the current default. A configured id that Windows does
/// not know is kept as written and counts as inactive.
///
/// `mark` is called with the name of each phase as it ends (`"default_read"`,
/// `"target_chosen"`, `"set_done"`), which is how `--timing` stamps the hot path; pass `|_| {}`
/// when nothing is measured.
///
/// # Errors
///
/// Whatever [`AudioSystem::default_id`], [`AudioSystem::lookup`], [`choose_target`] or
/// [`AudioSystem::set_default`] returns.
pub fn perform(
    audio: &AudioSystem,
    device1: &DeviceRef,
    device2: &DeviceRef,
    switch_communications: bool,
    mut mark: impl FnMut(&'static str),
) -> Result<Outcome> {
    let current = audio.default_id(Role::Console)?;
    mark("default_read");
    let (windows1, active1) = as_windows_knows_it(audio, device1)?;
    let (windows2, active2) = as_windows_knows_it(audio, device2)?;
    let target = choose_target(current.as_deref(), &windows1, &windows2, |id| {
        Ok(if id == windows1.id { active1 } else { active2 })
    })?;
    mark("target_chosen");
    // The console default was read just above; passing it on saves a second read (~1 ms).
    audio.set_default_known(
        &target.device.id,
        roles_for(switch_communications),
        Some((Role::Console, current.as_deref())),
    )?;
    mark("set_done");
    let (configured, other) = if std::ptr::eq(target.device, std::ptr::from_ref(&windows1)) {
        (device1, device2)
    } else {
        (device2, device1)
    };
    // Report the configured devices, not the looked-up copies: the only warning names the device
    // that was not chosen.
    let warning = target.warning.map(|_| Warning::PreferredUnavailable {
        preferred: other.clone(),
    });
    Ok(Outcome {
        device: configured.clone(),
        warning,
    })
}

/// `device` with its id as Windows spells it, and whether it is active. An id Windows does not
/// know is kept as written and reported inactive.
fn as_windows_knows_it(audio: &AudioSystem, device: &DeviceRef) -> Result<(DeviceRef, bool)> {
    let (id, active) = audio
        .lookup(&device.id)?
        .unwrap_or_else(|| (device.id.clone(), false));
    Ok((
        DeviceRef {
            id,
            name: device.name.clone(),
        },
        active,
    ))
}

/// The name to show for a configured device: its friendly name, or its id when the name is empty.
#[must_use]
pub fn display_name(device: &DeviceRef) -> &str {
    if device.name.is_empty() {
        &device.id
    } else {
        &device.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "{0.0.0.00000000}.{739b3554-bfed-4d61-b407-a818b317c991}";
    const B: &str = "{0.0.0.00000000}.{5b124733-5d8f-428c-b83c-ee05ce6467fb}";
    const OTHER: &str = "{0.0.0.00000000}.{30045f40-8cfd-4441-bb89-0d13fc19b589}";

    fn device(id: &str, name: &str) -> DeviceRef {
        DeviceRef {
            id: id.to_owned(),
            name: name.to_owned(),
        }
    }

    fn devices() -> (DeviceRef, DeviceRef) {
        (
            device(A, "PG42UQ (NVIDIA High Definition Audio)"),
            device(B, "喇叭 (FiiO BTA30 PRO)"),
        )
    }

    /// A decision table row: current default, active ids, expected decision and the ids
    /// `is_active` must be asked about, in order.
    type Row<'a> = (Option<&'a str>, &'a [&'a str], Expect, &'a [&'a str]);

    /// What a table row expects.
    #[derive(Debug, PartialEq, Eq)]
    enum Expect {
        /// Switch to this id without a warning.
        Switch(&'static str),
        /// Switch to this id, warning that the other id was preferred.
        Fallback(&'static str, &'static str),
        /// `NoDeviceAvailable`: this device is unavailable and that one already is the default.
        Blocked(&'static str, &'static str),
        /// `NoDeviceAvailable`: neither device is available.
        Neither,
    }

    /// The test id of a display name produced by [`devices`].
    fn id_named(name: &str) -> &'static str {
        if name == "PG42UQ (NVIDIA High Definition Audio)" {
            A
        } else {
            assert_eq!(name, "喇叭 (FiiO BTA30 PRO)");
            B
        }
    }

    /// Runs `choose_target` with the given active set and returns the result plus the ids that
    /// `is_active` was asked about, in order.
    fn decide(current: Option<&str>, active: &[&str]) -> (Expect, Vec<String>) {
        let (device1, device2) = devices();
        let mut asked = Vec::new();
        let result = choose_target(current, &device1, &device2, |id| {
            asked.push(id.to_owned());
            Ok(active.iter().any(|active| active.eq_ignore_ascii_case(id)))
        });
        let expect = match result {
            Ok(Target {
                device,
                warning: None,
            }) => Expect::Switch(if device.id == A { A } else { B }),
            Ok(Target {
                device,
                warning: Some(Warning::PreferredUnavailable { preferred }),
            }) => {
                assert_ne!(device.id, preferred.id);
                if device.id == A {
                    Expect::Fallback(A, B)
                } else {
                    Expect::Fallback(B, A)
                }
            }
            Err(Error::NoDeviceAvailable {
                unavailable,
                already_default,
            }) => match (unavailable.as_slice(), already_default) {
                ([missing], Some(default)) => {
                    Expect::Blocked(id_named(missing), id_named(&default))
                }
                ([first, second], None) => {
                    assert_eq!((id_named(first), id_named(second)), (A, B));
                    Expect::Neither
                }
                other => panic!("unexpected NoDeviceAvailable {other:?}"),
            },
            Err(other) => panic!("unexpected error {other:?}"),
        };
        (expect, asked)
    }

    #[test]
    fn decision_table() {
        let a_upper = A.to_uppercase();
        let b_upper = B.to_uppercase();
        #[rustfmt::skip]
        let rows: &[Row<'_>] = &[
            // current       active        expected                     is_active calls
            // No default device: Device 1, else Device 2.
            (None,           &[A, B],      Expect::Switch(A),           &[A]),
            (None,           &[A],         Expect::Switch(A),           &[A]),
            (None,           &[B],         Expect::Fallback(B, A),      &[A, B]),
            (None,           &[],          Expect::Neither,             &[A, B]),
            // On Device 1: Device 2 only, never a fallback.
            (Some(A),        &[A, B],      Expect::Switch(B),           &[B]),
            (Some(A),        &[B],         Expect::Switch(B),           &[B]),
            (Some(A),        &[A],         Expect::Blocked(B, A),       &[B]),
            (Some(A),        &[],          Expect::Blocked(B, A),       &[B]),
            // On Device 2: Device 1; falling back to Device 2 would be a no-op.
            (Some(B),        &[A, B],      Expect::Switch(A),           &[A]),
            (Some(B),        &[A],         Expect::Switch(A),           &[A]),
            (Some(B),        &[B],         Expect::Blocked(A, B),       &[A]),
            (Some(B),        &[],          Expect::Blocked(A, B),       &[A]),
            // On some other device: Device 1, else Device 2.
            (Some(OTHER),    &[A, B, OTHER], Expect::Switch(A),         &[A]),
            (Some(OTHER),    &[B, OTHER],  Expect::Fallback(B, A),      &[A, B]),
            (Some(OTHER),    &[OTHER],     Expect::Neither,             &[A, B]),
            (Some(""),       &[B],         Expect::Fallback(B, A),      &[A, B]),
            // Ids compare case-insensitively.
            (Some(&a_upper), &[A, B],      Expect::Switch(B),           &[B]),
            (Some(&b_upper), &[B],         Expect::Blocked(A, B),       &[A]),
        ];
        for (current, active, expected, calls) in rows {
            let (actual, asked) = decide(*current, active);
            assert_eq!(&actual, expected, "current {current:?}, active {active:?}");
            assert_eq!(asked, *calls, "current {current:?}, active {active:?}");
        }
    }

    #[test]
    fn target_borrows_the_configured_device_and_warning_names_the_preferred_one() {
        let (device1, device2) = devices();
        let target = choose_target(None, &device1, &device2, |id| Ok(id == B)).unwrap();
        assert!(std::ptr::eq(target.device, std::ptr::from_ref(&device2)));
        assert_eq!(
            target.warning,
            Some(Warning::PreferredUnavailable {
                preferred: device1.clone()
            })
        );
    }

    #[test]
    fn is_active_errors_propagate() {
        let (device1, device2) = devices();
        let failure = || Error::Com {
            call: "IMMDeviceEnumerator::GetDevice",
            hr: windows_core::HRESULT::from_win32(5), // E_ACCESSDENIED
        };

        // Failing on the preferred device stops before the fallback is consulted.
        let mut calls = 0;
        let result = choose_target(None, &device1, &device2, |_| {
            calls += 1;
            Err(failure())
        });
        assert!(matches!(result, Err(Error::Com { .. })), "{result:?}");
        assert_eq!(calls, 1);

        // Failing on the fallback is reported too, not turned into NoDeviceAvailable.
        let result = choose_target(Some(OTHER), &device1, &device2, |id| {
            if id == A { Ok(false) } else { Err(failure()) }
        });
        assert!(matches!(result, Err(Error::Com { .. })), "{result:?}");
    }

    #[test]
    fn no_device_error_uses_ids_for_unnamed_devices() {
        let device1 = device(A, "");
        let device2 = device(B, "Speakers");
        let error = choose_target(None, &device1, &device2, |_| Ok(false)).unwrap_err();
        match error {
            Error::NoDeviceAvailable {
                unavailable,
                already_default: None,
            } => assert_eq!(unavailable, [A, "Speakers"]),
            other => panic!("unexpected error {other:?}"),
        }
        let error = choose_target(Some(A), &device1, &device2, |_| Ok(false)).unwrap_err();
        match error {
            Error::NoDeviceAvailable {
                unavailable,
                already_default: Some(default),
            } => {
                assert_eq!(unavailable, ["Speakers"]);
                assert_eq!(default, A);
            }
            other => panic!("unexpected error {other:?}"),
        }
    }

    #[test]
    fn warning_text_names_the_unavailable_device() {
        let warning = Warning::PreferredUnavailable {
            preferred: device(A, "喇叭 (FiiO BTA30 PRO)"),
        };
        assert_eq!(
            warning.to_string(),
            "\"喇叭 (FiiO BTA30 PRO)\" is not connected; switched to the other device instead"
        );
    }

    #[test]
    fn display_name_falls_back_to_the_id() {
        assert_eq!(display_name(&device(A, "Name")), "Name");
        assert_eq!(display_name(&device(A, "")), A);
    }
}
