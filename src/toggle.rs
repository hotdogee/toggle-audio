//! Toggle semantics: which device to switch to (DESIGN.md section 6).
//!
//! [`choose_target`] is a pure function of the current default, the two configured devices and an
//! `is_active` predicate, so the whole decision table is unit-tested without audio hardware.
//! [`perform`] wires it to an [`AudioSystem`]; the CLI `toggle` command and the settings dialog's
//! Test toggle button both use it, so the hot path exists exactly once.
//!
//! # Finding the configured devices (DESIGN.md section 6)
//!
//! A Windows or audio driver update can replace an endpoint's id, so a saved id can stop working
//! while the device itself is fine. Each configured device is looked for in this order, and each
//! step runs only when the one before it failed:
//!
//! 1. The saved `id`. This is the hot path, and when it finds an active endpoint nothing below
//!    costs anything.
//! 2. The saved `stable_id`, which Windows tries to preserve across those updates.
//! 3. The saved friendly name, only when Windows knows neither id, and only when exactly one active
//!    endpoint has that exact name and it is not the other configured device. This is the last
//!    resort, for configurations without a stable id and for the few peripherals whose stable id
//!    changes too. An unplugged device is known to Windows and never gets here, so unplugging a
//!    device cannot silently retarget the toggle to its twin.
//!
//! When a device is found under a new id, [`update_config`] records that, so the next toggle is
//! back on the hot path. It also records a stable id that is not in the configuration yet.

use std::fmt;

use crate::audio::{AudioSystem, Endpoint, Role, roles_for, same_endpoint_id};
use crate::config::{Config, DeviceRef};
use crate::error::{Error, Result};

/// A non-fatal condition reported alongside a successful toggle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Warning {
    /// The preferred device was not active, so the toggle went to the other configured device.
    PreferredUnavailable {
        /// The device the toggle would normally have switched to.
        preferred: DeviceRef,
    },
    /// Windows no longer knows the preferred device, so the toggle went to the other one.
    PreferredLost {
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
            Self::PreferredLost { preferred } => write!(
                f,
                "Windows no longer has the saved device \"{}\"; switched to the other device \
                 instead. Open Settings to choose a replacement",
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
    /// The new ids of Device 1 and Device 2, for a device found under a different id than the
    /// configured one. Pass it to [`update_config`].
    pub moved: [Option<String>; 2],
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
            lost: Vec::new(),
        });
    };
    // Skip the lookup when the fallback already is the default: switching to it would be a silent
    // no-op, which is exactly what this decision table exists to avoid.
    if is_current(fallback) {
        return Err(Error::NoDeviceAvailable {
            unavailable: vec![name(preferred)],
            already_default: Some(name(fallback)),
            lost: Vec::new(),
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
        lost: Vec::new(),
    })
}

/// Performs a toggle: reads the current console default's id, applies [`choose_target`] and
/// makes the chosen device the default for [`roles_for`]`(switch_communications)`.
///
/// The configured devices are first looked up as the module documentation describes, so the
/// decision compares ids as Windows spells them: a configured id in another case, or a stable id
/// that `GetDevice` also accepts, is still recognized as the current default. A device that cannot
/// be found keeps its configured id and counts as inactive.
///
/// [`Outcome::moved`] carries any new ids. The configuration is the caller's to update, because
/// the settings dialog's Test toggle runs on unsaved selections.
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
    let (found1, found2) = locate_both(audio, device1, device2)?;
    let target = choose_target(current.as_deref(), &found1.device, &found2.device, |id| {
        Ok(if id == found1.device.id {
            found1.active
        } else {
            found2.active
        })
    })
    .map_err(|error| separate_lost(error, [&found1, &found2]))?;
    mark("target_chosen");
    // The console default was read just above; passing it on saves a second read (~1 ms).
    audio.set_default_known(
        &target.device.id,
        roles_for(switch_communications),
        Some((Role::Console, current.as_deref())),
    )?;
    mark("set_done");
    let (chosen, configured, not_chosen, other) =
        if std::ptr::eq(target.device, std::ptr::from_ref(&found1.device)) {
            (&found1, device1, &found2, device2)
        } else {
            (&found2, device2, &found1, device1)
        };
    // Report the configured devices, not the looked-up copies: the only warning names the device
    // that was not chosen.
    let warning = target.warning.map(|_| {
        let preferred = other.clone();
        if not_chosen.lost {
            Warning::PreferredLost { preferred }
        } else {
            Warning::PreferredUnavailable { preferred }
        }
    });
    let mut device = configured.clone();
    if chosen.moved {
        device.id.clone_from(&chosen.device.id);
    }
    Ok(Outcome {
        device,
        warning,
        moved: [found1.new_id(), found2.new_id()],
    })
}

/// The configured devices with their current ids, for the settings dialog, which opens with the
/// right devices selected even when a saved id has stopped working. A device that cannot be found
/// is returned as configured.
///
/// # Errors
///
/// [`Error::Com`] when a lookup fails.
pub fn relocate(
    audio: &AudioSystem,
    device1: &DeviceRef,
    device2: &DeviceRef,
) -> Result<(DeviceRef, DeviceRef)> {
    let (found1, found2) = locate_both(audio, device1, device2)?;
    let current = |found: Located, configured: &DeviceRef| {
        if found.moved {
            found.device
        } else {
            configured.clone()
        }
    };
    Ok((current(found1, device1), current(found2, device2)))
}

/// Brings `config` up to date after a toggle and returns whether it changed. The caller saves it.
///
/// `moved` is [`Outcome::moved`]: a device found under a new id gets that id, its current friendly
/// name and a fresh stable id. A device without a stable id gets one, or `""` when Windows has
/// none, which keeps this from running again on the next toggle. `name_of` and `stable_id_of` are
/// [`AudioSystem::name_of`] and [`AudioSystem::stable_id_of`].
///
/// On the normal path this costs two `Option` checks and calls neither. A read that fails changes
/// nothing, so the next toggle tries again: the switch has already happened, and these reads must
/// not turn it into an error.
pub fn update_config(
    config: &mut Config,
    moved: &[Option<String>; 2],
    mut name_of: impl FnMut(&str) -> Result<Option<String>>,
    mut stable_id_of: impl FnMut(&str) -> Result<Option<String>>,
) -> bool {
    let mut changed = false;
    for (device, moved) in [&mut config.device1, &mut config.device2]
        .into_iter()
        .zip(moved)
    {
        if let Some(id) = moved {
            device.id.clone_from(id);
            device.stable_id = None;
            if let Ok(Some(name)) = name_of(id) {
                device.name = name;
            }
            changed = true;
        }
        if device.stable_id.is_none() {
            if let Ok(stable_id) = stable_id_of(&device.id) {
                device.stable_id = Some(stable_id.unwrap_or_default());
                changed = true;
            }
        }
    }
    changed
}

/// A configured device as Windows knows it right now.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Located {
    /// The configured device with its current id, as Windows spells it. The hot path has no use
    /// for the stable id here, so it is not copied.
    device: DeviceRef,
    /// Whether that endpoint is active.
    active: bool,
    /// The saved id stopped working and `device.id` is the endpoint found in its place.
    moved: bool,
    /// Windows knows neither saved id, so `device.id` still is the saved id.
    lost: bool,
}

impl Located {
    /// `device` found under `id`.
    fn new(device: &DeviceRef, id: String, active: bool) -> Self {
        Self {
            device: DeviceRef {
                id,
                name: device.name.clone(),
                stable_id: None,
            },
            active,
            moved: false,
            lost: false,
        }
    }

    /// The id to record, when the device moved.
    fn new_id(&self) -> Option<String> {
        self.moved.then(|| self.device.id.clone())
    }
}

/// Finds both configured devices (steps 1 to 3 of the module documentation).
fn locate_both(
    audio: &AudioSystem,
    device1: &DeviceRef,
    device2: &DeviceRef,
) -> Result<(Located, Located)> {
    let mut found1 = locate(device1, |id| audio.lookup(id))?;
    let mut found2 = locate(device2, |id| audio.lookup(id))?;
    if found1.lost || found2.lost {
        // Best effort: without the list the device simply stays lost, and the other configured
        // device can still be switched to.
        let endpoints = audio.list_active().unwrap_or_default();
        find_by_name(&mut found1, &found2.device.id, &endpoints);
        find_by_name(&mut found2, &found1.device.id, &endpoints);
    }
    Ok((found1, found2))
}

/// Steps 1 and 2 of the module documentation. `lookup` is [`AudioSystem::lookup`].
///
/// An active endpoint at the saved id returns after that one lookup: this is the hot path. The
/// stable id is consulted for an inactive endpoint too, because an update can leave the old
/// endpoint behind as "not present" next to its replacement. Unplugged or disabled devices pay
/// for that with one lookup.
fn locate(
    device: &DeviceRef,
    mut lookup: impl FnMut(&str) -> Result<Option<(String, bool)>>,
) -> Result<Located> {
    let saved = match lookup(&device.id)? {
        Some((id, true)) => return Ok(Located::new(device, id, true)),
        saved => saved,
    };
    let by_stable_id = match device.stable_id.as_deref() {
        Some(stable_id) if !stable_id.is_empty() => lookup(stable_id)?,
        _ => None,
    };
    let moved = |id, active| Located {
        moved: true,
        ..Located::new(device, id, active)
    };
    Ok(match (saved, by_stable_id) {
        (Some((old, false)), Some((new, true))) if !same_endpoint_id(&old, &new) => {
            moved(new, true)
        }
        (Some((id, active)), _) => Located::new(device, id, active),
        (None, Some((id, active))) => moved(id, active),
        (None, None) => Located {
            lost: true,
            ..Located::new(device, device.id.clone(), false)
        },
    })
}

/// Step 3 of the module documentation: a lost device takes over the one active endpoint that has
/// its saved name, unless that is `other_id`, the other configured device. Names are compared
/// exactly and no match or several matches leave it lost, because a wrong guess would switch audio
/// to the wrong device and save that.
fn find_by_name(found: &mut Located, other_id: &str, endpoints: &[Endpoint]) {
    if !found.lost || found.device.name.is_empty() {
        return;
    }
    let mut matches = endpoints.iter().filter(|endpoint| {
        endpoint.name == found.device.name && !same_endpoint_id(&endpoint.id, other_id)
    });
    if let (Some(endpoint), None) = (matches.next(), matches.next()) {
        found.device.id.clone_from(&endpoint.id);
        found.active = true;
        found.moved = true;
        found.lost = false;
    }
}

/// `error` with the devices Windows no longer knows moved from `unavailable` to `lost`, so the
/// message does not claim that they are disconnected.
fn separate_lost(error: Error, found: [&Located; 2]) -> Error {
    match error {
        Error::NoDeviceAvailable {
            unavailable,
            already_default,
            lost: _,
        } => {
            let (lost, unavailable) = unavailable.into_iter().partition(|name| {
                found
                    .iter()
                    .any(|found| found.lost && display_name(&found.device) == name)
            });
            Error::NoDeviceAvailable {
                unavailable,
                already_default,
                lost,
            }
        }
        other => other,
    }
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
            stable_id: None,
        }
    }

    fn stable(id: &str, name: &str, stable_id: &str) -> DeviceRef {
        DeviceRef {
            stable_id: Some(stable_id.to_owned()),
            ..device(id, name)
        }
    }

    /// `locate` against a fake Windows: `known` lists `(accepted id, current id, active)`.
    /// Returns the result and the ids that were looked up, in order.
    fn locate_in(known: &[(&str, &str, bool)], device: &DeviceRef) -> (Located, Vec<String>) {
        let mut asked = Vec::new();
        let located = locate(device, |id| {
            asked.push(id.to_owned());
            Ok(known
                .iter()
                .find(|(accepted, _, _)| *accepted == id)
                .map(|&(_, current, active)| (current.to_owned(), active)))
        })
        .unwrap();
        (located, asked)
    }

    fn state(located: &Located) -> (&str, bool, bool, bool) {
        (
            &located.device.id,
            located.active,
            located.moved,
            located.lost,
        )
    }

    const NEW: &str = "{0.0.0.00000000}.{c9f7cc37-c71e-486e-beee-5010ed8616b7}";
    const STABLE: &str = "{0.0.0.00000000}.{001.{31E3A048-D22C-4C75-853B-3DC39707E1A7}}";

    #[test]
    fn an_active_saved_id_costs_exactly_one_lookup() {
        // The hot path: the stable id is never consulted.
        let (found, asked) = locate_in(&[(A, A, true)], &stable(A, "n", STABLE));
        assert_eq!(state(&found), (A, true, false, false));
        assert_eq!(asked, [A]);
    }

    #[test]
    fn the_id_is_reported_as_windows_spells_it_without_counting_as_moved() {
        let upper = A.to_uppercase();
        let (found, _) = locate_in(&[(&upper, A, true)], &device(&upper, "n"));
        assert_eq!(state(&found), (A, true, false, false));
        assert_eq!(found.new_id(), None);
    }

    #[test]
    fn a_replaced_id_is_found_through_the_stable_id() {
        let saved = stable(A, "n", STABLE);
        let (found, asked) = locate_in(&[(STABLE, NEW, true)], &saved);
        assert_eq!(state(&found), (NEW, true, true, false));
        assert_eq!(found.new_id().as_deref(), Some(NEW));
        assert_eq!(asked, [A, STABLE]);

        // The device is off right now: the new id is still the right one to remember, and a
        // message about it is true.
        let (found, _) = locate_in(&[(STABLE, NEW, false)], &saved);
        assert_eq!(state(&found), (NEW, false, true, false));
    }

    #[test]
    fn an_old_endpoint_left_behind_gives_way_to_its_replacement() {
        let saved = stable(A, "n", STABLE);
        let (found, _) = locate_in(&[(A, A, false), (STABLE, NEW, true)], &saved);
        assert_eq!(state(&found), (NEW, true, true, false));
    }

    #[test]
    fn an_unplugged_device_stays_where_it_is() {
        // The stable id leads to the same endpoint.
        let saved = stable(A, "n", STABLE);
        let (found, asked) = locate_in(&[(A, A, false), (STABLE, A, false)], &saved);
        assert_eq!(state(&found), (A, false, false, false));
        assert_eq!(asked, [A, STABLE]);

        // Case differences are not a move.
        let upper = A.to_uppercase();
        let (found, _) = locate_in(&[(A, A, false), (STABLE, &upper, true)], &saved);
        assert_eq!(state(&found), (A, false, false, false));

        // A second inactive endpoint is no better than the saved one.
        let (found, _) = locate_in(&[(A, A, false), (STABLE, NEW, false)], &saved);
        assert_eq!(state(&found), (A, false, false, false));

        // Without a stable id, known but inactive is not lost: the name is never tried.
        let (found, _) = locate_in(&[(A, A, false)], &device(A, "n"));
        assert_eq!(state(&found), (A, false, false, false));
    }

    #[test]
    fn a_device_windows_does_not_know_is_lost() {
        for saved in [device(A, "n"), stable(A, "n", ""), stable(A, "n", STABLE)] {
            let (found, _) = locate_in(&[], &saved);
            assert_eq!(state(&found), (A, false, false, true), "{saved:?}");
        }
        // "" means Windows had none: it must not be passed to Windows.
        let (_, asked) = locate_in(&[], &stable(A, "n", ""));
        assert_eq!(asked, [A]);
    }

    #[test]
    fn lookup_errors_propagate() {
        let failure = |_: &str| {
            Err(Error::Com {
                call: "IMMDeviceEnumerator::GetDevice",
                hr: windows_core::HRESULT::from_win32(5),
            })
        };
        let result = locate(&stable(A, "n", STABLE), failure);
        assert!(matches!(result, Err(Error::Com { .. })), "{result:?}");
    }

    fn endpoint(id: &str, name: &str) -> Endpoint {
        Endpoint {
            id: id.to_owned(),
            name: name.to_owned(),
        }
    }

    fn lost(id: &str, name: &str) -> Located {
        Located {
            lost: true,
            ..Located::new(&device(id, name), id.to_owned(), false)
        }
    }

    #[test]
    fn a_lost_device_takes_over_the_one_endpoint_with_its_exact_name() {
        let mut found = lost(A, "PG42UQ");
        let endpoints = [endpoint(OTHER, "FiiO"), endpoint(NEW, "PG42UQ")];
        find_by_name(&mut found, B, &endpoints);
        assert_eq!(state(&found), (NEW, true, true, false));
    }

    #[test]
    fn the_name_never_guesses() {
        let unchanged = |name: &str, other: &str, endpoints: &[Endpoint]| {
            let mut found = lost(A, name);
            find_by_name(&mut found, other, endpoints);
            assert_eq!(state(&found), (A, false, false, true), "{name:?}");
        };
        // Nothing has that name, or only in another case.
        unchanged("PG42UQ", B, &[endpoint(NEW, "FiiO")]);
        unchanged("PG42UQ", B, &[endpoint(NEW, "pg42uq")]);
        // Two endpoints have it.
        unchanged(
            "Speakers",
            B,
            &[endpoint(NEW, "Speakers"), endpoint(OTHER, "Speakers")],
        );
        // The only match is the other configured device.
        unchanged("Speakers", NEW, &[endpoint(NEW, "Speakers")]);
        // No saved name: an endpoint without a name must not match it.
        unchanged("", B, &[endpoint(NEW, "")]);
    }

    #[test]
    fn the_other_device_does_not_make_a_name_ambiguous() {
        let mut found = lost(A, "Speakers");
        let endpoints = [endpoint(B, "Speakers"), endpoint(NEW, "Speakers")];
        find_by_name(&mut found, B, &endpoints);
        assert_eq!(state(&found), (NEW, true, true, false));
    }

    #[test]
    fn only_lost_devices_are_matched_by_name() {
        let saved = device(A, "PG42UQ");
        let mut found = Located::new(&saved, A.to_owned(), false);
        find_by_name(&mut found, B, &[endpoint(NEW, "PG42UQ")]);
        assert_eq!(state(&found), (A, false, false, false));
    }

    #[test]
    fn lost_devices_are_separated_from_unavailable_ones() {
        let unplugged = Located::new(&device(A, "Unplugged"), A.to_owned(), false);
        let gone = lost(B, "Gone");
        let error = Error::NoDeviceAvailable {
            unavailable: vec!["Unplugged".to_owned(), "Gone".to_owned()],
            already_default: None,
            lost: Vec::new(),
        };
        match separate_lost(error, [&unplugged, &gone]) {
            Error::NoDeviceAvailable {
                unavailable, lost, ..
            } => {
                assert_eq!(unavailable, ["Unplugged"]);
                assert_eq!(lost, ["Gone"]);
            }
            other => panic!("unexpected error {other:?}"),
        }
        let other = Error::NoDefaultDevice;
        assert!(matches!(
            separate_lost(other, [&unplugged, &gone]),
            Error::NoDefaultDevice
        ));
    }

    /// `update_config`'s readers for a test that must not read anything.
    fn no_read(id: &str) -> Result<Option<String>> {
        panic!("unexpected read of {id}");
    }

    fn config_with(device1: DeviceRef, device2: DeviceRef) -> Config {
        Config::new(device1, device2, true)
    }

    #[test]
    fn a_complete_configuration_is_left_alone_without_any_read() {
        // The normal path: this is what every toggle after the first one runs.
        let mut config = config_with(stable(A, "a", STABLE), stable(B, "b", ""));
        let before = config.clone();
        assert!(!update_config(&mut config, &[None, None], no_read, no_read));
        assert_eq!(config, before);
    }

    #[test]
    fn a_moved_device_gets_its_new_id_name_and_stable_id() {
        let mut config = config_with(stable(A, "old name", "old"), stable(B, "b", "kept"));
        let changed = update_config(
            &mut config,
            &[Some(NEW.to_owned()), None],
            |id| Ok(Some(format!("name of {id}"))),
            |id| Ok(Some(format!("stable of {id}"))),
        );
        assert!(changed);
        assert_eq!(config.device1.id, NEW);
        assert_eq!(config.device1.name, format!("name of {NEW}"));
        assert_eq!(config.device1.stable_id, Some(format!("stable of {NEW}")));
        assert_eq!(config.device2, stable(B, "b", "kept"));
    }

    #[test]
    fn a_missing_stable_id_is_recorded_once() {
        let mut config = config_with(device(A, "a"), device(B, "b"));
        let changed = update_config(&mut config, &[None, None], no_read, |id| {
            Ok((id == A).then(|| STABLE.to_owned()))
        });
        assert!(changed);
        assert_eq!(config.device1.stable_id.as_deref(), Some(STABLE));
        // Windows has none for Device 2: remembered as "", so the next toggle reads nothing.
        assert_eq!(config.device2.stable_id.as_deref(), Some(""));
        assert!(!update_config(&mut config, &[None, None], no_read, no_read));
    }

    #[test]
    fn failed_reads_change_nothing_and_are_tried_again() {
        let failure = |_: &str| {
            Err(Error::Com {
                call: "IPropertyStore::GetValue",
                hr: windows_core::HRESULT::from_win32(5),
            })
        };
        let mut config = config_with(device(A, "a"), stable(B, "b", ""));
        assert!(!update_config(&mut config, &[None, None], no_read, failure));
        assert_eq!(config.device1, device(A, "a"));

        // A moved device keeps its saved name, and the stable id stays unknown.
        let mut config = config_with(stable(A, "a", "old"), stable(B, "b", ""));
        let moved = [Some(NEW.to_owned()), None];
        assert!(update_config(&mut config, &moved, failure, failure));
        assert_eq!(config.device1, device(NEW, "a"));
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
            // `choose_target` only knows "not active"; `perform` tells lost devices apart.
            Ok(Target {
                warning: Some(warning),
                ..
            }) => panic!("unexpected warning {warning:?}"),
            Err(Error::NoDeviceAvailable {
                unavailable,
                already_default,
                ..
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
                ..
            } => assert_eq!(unavailable, [A, "Speakers"]),
            other => panic!("unexpected error {other:?}"),
        }
        let error = choose_target(Some(A), &device1, &device2, |_| Ok(false)).unwrap_err();
        match error {
            Error::NoDeviceAvailable {
                unavailable,
                already_default: Some(default),
                ..
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
        let warning = Warning::PreferredLost {
            preferred: device(A, "喇叭 (FiiO BTA30 PRO)"),
        };
        assert_eq!(
            warning.to_string(),
            "Windows no longer has the saved device \"喇叭 (FiiO BTA30 PRO)\"; switched to the \
             other device instead. Open Settings to choose a replacement"
        );
    }

    #[test]
    fn display_name_falls_back_to_the_id() {
        assert_eq!(display_name(&device(A, "Name")), "Name");
        assert_eq!(display_name(&device(A, "")), A);
    }
}
