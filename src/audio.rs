//! Core Audio access: enumerate playback endpoints, read and set the default device.
//!
//! This is the only module besides [`crate::gui`] that works with Core Audio (`crate::config`
//! only frees one shell string with `CoTaskMemFree`). Everything goes through [`AudioSystem`],
//! which owns the COM apartment (STA, `COINIT_DISABLE_OLE1DDE`) and the `IMMDeviceEnumerator`.
//! Setting the default uses the undocumented `IPolicyConfig` interface (CLSID
//! `{870af99c-171d-4f9e-af0d-e63df40c2bc9}`, `SetDefaultEndpoint` at vtable slot 13) with the
//! `IPolicyConfigVista` fallback described in `docs/research/core-audio-api.md` section C.2.
//!
//! Only playback (`eRender`) endpoints are ever accepted: an id that names a recording endpoint is
//! treated exactly like an unknown id, so `set` or a hand-edited configuration can never change
//! the default microphone.
//!
//! Hosted CI runners have no audio devices, so code that needs a real endpoint is exercised only
//! by the `#[ignore]`d tests in `tests/real_device.rs` and in this module.

use std::ffi::c_void;
use std::marker::PhantomData;

use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::{E_INVALIDARG, ERROR_NOT_FOUND, RPC_E_CHANGED_MODE};
use windows::Win32::Media::Audio::{
    DEVICE_STATE_ACTIVE, ERole, IMMDevice, IMMDeviceEnumerator, IMMEndpoint, MMDeviceEnumerator,
    eCommunications, eConsole, eMultimedia, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
    CoInitializeEx, CoTaskMemFree, CoUninitialize, STGM_READ,
};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows_core::{GUID, HRESULT, HSTRING, Interface as _, PCWSTR, PWSTR};

use self::policy_config::{IPolicyConfig, IPolicyConfigVista};
use crate::error::{Error, Result};

/// `E_NOTFOUND` (`HRESULT_FROM_WIN32(ERROR_NOT_FOUND)`, `0x80070490`), defined by
/// `mmdeviceapi.h` but not by the `windows` crate. Returned for unknown endpoint ids and when
/// there is no default device.
const E_NOTFOUND: HRESULT = HRESULT::from_win32(ERROR_NOT_FOUND.0);

/// `CPolicyConfigClient` (AudioSes.dll), which implements [`IPolicyConfig`] on Windows 7 and later.
const CLSID_POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870a_f99c_171d_4f9e_af0d_e63d_f40c_2bc9);

/// `CPolicyConfigVistaClient` (AudioSes.dll), which implements [`IPolicyConfigVista`].
const CLSID_POLICY_CONFIG_VISTA_CLIENT: GUID =
    GUID::from_u128(0x2949_35ce_f637_4e7c_a41b_ab25_5460_b862);

/// A Windows audio device role (`ERole`).
///
/// Windows keeps one default playback device per role. The Sound settings "Set as default" button
/// assigns [`Role::Console`] and [`Role::Multimedia`] together; "Set as default communication
/// device" assigns [`Role::Communications`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// `eConsole`: games, system sounds, most applications.
    Console,
    /// `eMultimedia`: music and video playback.
    Multimedia,
    /// `eCommunications`: voice chat and calls.
    Communications,
}

impl Role {
    /// The matching Core Audio `ERole` value.
    #[must_use]
    pub fn to_erole(self) -> ERole {
        match self {
            Self::Console => eConsole,
            Self::Multimedia => eMultimedia,
            Self::Communications => eCommunications,
        }
    }
}

impl From<Role> for ERole {
    fn from(role: Role) -> Self {
        role.to_erole()
    }
}

/// The roles a toggle or `set` assigns, in the order they are set.
///
/// Console and multimedia always; communications only when `switch_communications` is true
/// (DESIGN.md section 6).
#[must_use]
pub fn roles_for(switch_communications: bool) -> &'static [Role] {
    if switch_communications {
        &[Role::Console, Role::Multimedia, Role::Communications]
    } else {
        &[Role::Console, Role::Multimedia]
    }
}

/// A playback endpoint as Windows reports it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Endpoint {
    /// Endpoint id from `IMMDevice::GetId`, for example
    /// `{0.0.0.00000000}.{739b3554-bfed-4d61-b407-a818b317c991}`. Opaque; this is the identity.
    pub id: String,
    /// `PKEY_Device_FriendlyName`, exactly what Sound settings shows, for example
    /// `喇叭 (FiiO BTA30 PRO)`. Not unique and user-renamable; display only. Empty in the unusual
    /// case that the endpoint has no friendly name.
    pub name: String,
}

/// An initialized COM apartment plus an `IMMDeviceEnumerator`.
///
/// Create one per thread with [`AudioSystem::new`]. Dropping it releases every COM object it holds
/// and then balances the `CoInitializeEx` call (unless the thread was already initialized in a
/// different apartment, in which case COM is left alone).
///
/// The type is neither `Send` nor `Sync`: COM initialization and the objects created in a
/// single-threaded apartment belong to the thread that created them.
#[derive(Debug)]
pub struct AudioSystem {
    enumerator: IMMDeviceEnumerator,
    // Declared after every COM object: fields drop in declaration order, so all interface pointers
    // are released before `CoUninitialize` runs.
    _apartment: ComApartment,
}

impl AudioSystem {
    /// Initializes COM on the calling thread (STA) and creates the device enumerator.
    ///
    /// # Errors
    ///
    /// [`Error::Com`] when `CoInitializeEx` fails with anything other than `S_FALSE` /
    /// `RPC_E_CHANGED_MODE`, or when the `MMDeviceEnumerator` cannot be created.
    pub fn new() -> Result<Self> {
        let apartment = ComApartment::enter()?;
        // SAFETY: COM is initialized on this thread for as long as `apartment` lives, and the
        // enumerator is stored next to it and dropped first. No outer unknown is passed.
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER) }
                .map_err(Error::com("CoCreateInstance(MMDeviceEnumerator)"))?;
        Ok(Self {
            enumerator,
            _apartment: apartment,
        })
    }

    /// Lists the active playback endpoints (`EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)`)
    /// in the order Windows returns them.
    ///
    /// # Errors
    ///
    /// [`Error::Com`] when enumeration or reading an id or friendly name fails.
    pub fn list_active(&self) -> Result<Vec<Endpoint>> {
        // SAFETY: `self.enumerator` is a live interface pointer on this thread's apartment; both
        // arguments are valid enumeration constants.
        let collection = unsafe {
            self.enumerator
                .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
        }
        .map_err(Error::com("IMMDeviceEnumerator::EnumAudioEndpoints"))?;
        // SAFETY: `collection` is a live interface pointer returned by the call above.
        let count = unsafe { collection.GetCount() }
            .map_err(Error::com("IMMDeviceCollection::GetCount"))?;
        (0..count)
            .map(|index| {
                // SAFETY: `collection` is live and `index` is below the count it reported.
                let device = unsafe { collection.Item(index) }
                    .map_err(Error::com("IMMDeviceCollection::Item"))?;
                endpoint_of(&device)
            })
            .collect()
    }

    /// The default playback endpoint for `role`, or `None` when Windows has no default playback
    /// device (`GetDefaultAudioEndpoint` returned `E_NOTFOUND`).
    ///
    /// # Errors
    ///
    /// [`Error::Com`] for any other failure.
    pub fn default_for(&self, role: Role) -> Result<Option<Endpoint>> {
        self.default_device(role)?
            .map(|device| endpoint_of(&device))
            .transpose()
    }

    /// The id of the default playback endpoint for `role`, or `None` when Windows has no default
    /// playback device.
    ///
    /// Like [`AudioSystem::default_for`] without reading the friendly name, which needs the
    /// endpoint's property store: this is what the toggle hot path uses.
    ///
    /// # Errors
    ///
    /// [`Error::Com`] for any failure other than "no default device".
    pub fn default_id(&self, role: Role) -> Result<Option<String>> {
        self.default_device(role)?
            .map(|device| id_of(&device))
            .transpose()
    }

    /// Whether the playback endpoint `id` exists and is `DEVICE_STATE_ACTIVE`.
    ///
    /// `IMMDeviceEnumerator::GetDevice` succeeds for unplugged, disabled and not-present endpoints,
    /// so the state is always checked. An unknown (`E_NOTFOUND`) or malformed (`E_INVALIDARG`) id,
    /// and the id of a recording endpoint, yield `Ok(false)`.
    ///
    /// # Errors
    ///
    /// [`Error::Com`] for any other failure.
    pub fn is_active(&self, id: &str) -> Result<bool> {
        match self.device(id)? {
            Some(device) => is_device_active(&device),
            None => Ok(false),
        }
    }

    /// The friendly name of playback endpoint `id` in any state (active or not), or `None` when
    /// the id is unknown, malformed or names a recording endpoint, or the endpoint has no friendly
    /// name.
    ///
    /// # Errors
    ///
    /// [`Error::Com`] for any other failure.
    pub fn name_of(&self, id: &str) -> Result<Option<String>> {
        let Some(device) = self.device(id)? else {
            return Ok(None);
        };
        let name = friendly_name(&device)?;
        Ok((!name.is_empty()).then_some(name))
    }

    /// Makes playback endpoint `id` the default for each of `roles`, in order, and returns whether
    /// any role actually changed.
    ///
    /// Roles whose current default already is `id` are skipped, so calling this with the current
    /// default is a no-op that returns `false`. The endpoint must be an active playback endpoint;
    /// anything else is never passed to `SetDefaultEndpoint`. `IPolicyConfig` is created only when
    /// at least one role needs to change. `IPolicyConfigVista` takes over when
    /// `CPolicyConfigClient` cannot be created or its `SetDefaultEndpoint` fails, and is then used
    /// for the remaining roles too.
    ///
    /// # Errors
    ///
    /// [`Error::DeviceNotFound`] when `id` is unknown or not a playback endpoint,
    /// [`Error::DeviceInactive`] when it is not active, [`Error::Com`] when no policy-config
    /// interface can be created or `SetDefaultEndpoint` fails through both (the error of the
    /// primary interface is reported). Roles before the failing one stay switched.
    pub fn set_default(&self, id: &str, roles: &[Role]) -> Result<bool> {
        let device = self
            .device(id)?
            .ok_or_else(|| Error::DeviceNotFound(id.to_owned()))?;
        if !is_device_active(&device)? {
            // Best effort: the state is what matters, so a failing name lookup falls back to the
            // id instead of replacing the error.
            let name = friendly_name(&device)
                .ok()
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| id.to_owned());
            return Err(Error::DeviceInactive(name));
        }

        // The id as Windows spells it: `id` may differ in case, and on Windows 11 24H2+ it may be
        // a stable id, which `GetDevice` accepts but `SetDefaultEndpoint` is not known to.
        let target = id_of(&device)?;
        let target_wide = HSTRING::from(target.as_str());
        let mut policy: Option<PolicyConfig> = None;
        let mut changed = false;
        for &role in roles {
            let current = self.default_id(role)?;
            if current.is_some_and(|current| current.eq_ignore_ascii_case(&target)) {
                continue;
            }
            let policy = match &mut policy {
                Some(policy) => policy,
                None => policy.insert(PolicyConfig::create()?),
            };
            policy.set_default_endpoint_with_fallback(&target_wide, role)?;
            changed = true;
        }
        Ok(changed)
    }

    /// `GetDefaultAudioEndpoint(eRender, role)`, with `E_NOTFOUND` mapped to `None`.
    fn default_device(&self, role: Role) -> Result<Option<IMMDevice>> {
        // SAFETY: `self.enumerator` is a live interface pointer on this thread's apartment; both
        // arguments are valid enumeration constants.
        match unsafe {
            self.enumerator
                .GetDefaultAudioEndpoint(eRender, role.to_erole())
        } {
            Ok(device) => Ok(Some(device)),
            Err(error) if error.code() == E_NOTFOUND => Ok(None),
            Err(error) => Err(Error::com("IMMDeviceEnumerator::GetDefaultAudioEndpoint")(
                error,
            )),
        }
    }

    /// `GetDevice(id)` in any state, restricted to playback endpoints: unknown and malformed ids,
    /// and ids of recording (`eCapture`) endpoints, map to `None`.
    ///
    /// `GetDevice` resolves any endpoint id regardless of its data flow, and `SetDefaultEndpoint`
    /// would happily make a microphone the default recording device, so the data flow is checked
    /// here, once, for every caller. The `{0.0.0.` / `{0.0.1.` id prefix is not relied on: it is
    /// an undocumented detail.
    fn device(&self, id: &str) -> Result<Option<IMMDevice>> {
        let Some(id) = endpoint_id_param(id) else {
            return Ok(None);
        };
        // SAFETY: `self.enumerator` is a live interface pointer on this thread's apartment and
        // `id` is a NUL-terminated wide string that outlives the call.
        let device = match unsafe { self.enumerator.GetDevice(&id) } {
            Ok(device) => device,
            Err(error) if is_unknown_id(error.code()) => return Ok(None),
            Err(error) => return Err(Error::com("IMMDeviceEnumerator::GetDevice")(error)),
        };
        let endpoint: IMMEndpoint = device
            .cast()
            .map_err(Error::com("IMMDevice::QueryInterface(IMMEndpoint)"))?;
        // SAFETY: `endpoint` is a live interface pointer obtained from `device` just above.
        let flow =
            unsafe { endpoint.GetDataFlow() }.map_err(Error::com("IMMEndpoint::GetDataFlow"))?;
        Ok((flow == eRender).then_some(device))
    }
}

/// Resolves the `set` argument against `endpoints` (normally [`AudioSystem::list_active`]).
///
/// Leading and trailing whitespace in `query` is ignored. Tried in order, first hit wins:
///
/// 1. an exact endpoint id (ASCII case-insensitive),
/// 2. an exact friendly name,
/// 3. a case-insensitive friendly name,
/// 4. a case-insensitive substring of a friendly name.
///
/// A name rule that matches more than one endpoint is ambiguous rather than falling through to the
/// next rule.
///
/// # Errors
///
/// [`Error::DeviceNotFound`] when nothing matches (or `query` is blank),
/// [`Error::AmbiguousDevice`] when the first matching rule matches more than one endpoint.
pub fn resolve<'a>(endpoints: &'a [Endpoint], query: &str) -> Result<&'a Endpoint> {
    let query = query.trim();
    if query.is_empty() {
        return Err(Error::DeviceNotFound(query.to_owned()));
    }
    if let Some(endpoint) = endpoints
        .iter()
        .find(|endpoint| endpoint.id.eq_ignore_ascii_case(query))
    {
        return Ok(endpoint);
    }

    let folded = query.to_lowercase();
    let rules: [&dyn Fn(&Endpoint) -> bool; 3] = [
        &|endpoint| endpoint.name == query,
        &|endpoint| endpoint.name.to_lowercase() == folded,
        &|endpoint| endpoint.name.to_lowercase().contains(&folded),
    ];
    for rule in rules {
        let matches: Vec<&Endpoint> = endpoints.iter().filter(|endpoint| rule(endpoint)).collect();
        match matches.as_slice() {
            [] => {}
            [endpoint] => return Ok(endpoint),
            several => {
                return Err(Error::AmbiguousDevice {
                    query: query.to_owned(),
                    matches: several.iter().map(|&endpoint| endpoint.clone()).collect(),
                });
            }
        }
    }
    Err(Error::DeviceNotFound(query.to_owned()))
}

/// Balances one `CoInitializeEx` call on the current thread.
#[derive(Debug)]
struct ComApartment {
    /// False when the thread already was in a multithreaded apartment (`RPC_E_CHANGED_MODE`):
    /// that initialization belongs to someone else and must not be undone here.
    uninitialize: bool,
    /// COM initialization is per thread, so the guard must stay on the thread that created it.
    _not_send: PhantomData<*const ()>,
}

impl ComApartment {
    /// `CoInitializeEx(COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE)`.
    ///
    /// `S_OK` and `S_FALSE` (already initialized as an STA) are paired with `CoUninitialize` on
    /// drop. `RPC_E_CHANGED_MODE` (already an MTA) is accepted without pairing: the Core Audio and
    /// policy-config classes are registered `ThreadingModel=Both`, so they work in either.
    fn enter() -> Result<Self> {
        // SAFETY: the reserved parameter must be null (`None`), and the flags are a valid COINIT
        // combination. Every successful call is balanced in `Drop`.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        let uninitialize = if hr == RPC_E_CHANGED_MODE {
            false
        } else if hr.is_ok() {
            true
        } else {
            return Err(Error::Com {
                call: "CoInitializeEx",
                hr,
            });
        };
        Ok(Self {
            uninitialize,
            _not_send: PhantomData,
        })
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.uninitialize {
            // SAFETY: balances the successful `CoInitializeEx` in `enter` on the same thread (the
            // guard is `!Send`). `AudioSystem` declares this guard last, so every interface
            // pointer it owned has been released already.
            unsafe { CoUninitialize() };
        }
    }
}

/// Whichever policy-config interface could be created.
enum PolicyConfig {
    /// `IPolicyConfig` on `CPolicyConfigClient`: Windows 7 and later, including Windows 11.
    Current(IPolicyConfig),
    /// `IPolicyConfigVista` on `CPolicyConfigVistaClient`: fallback, never needed so far.
    Vista(IPolicyConfigVista),
}

impl PolicyConfig {
    /// Creates `IPolicyConfig`, falling back to `IPolicyConfigVista`.
    ///
    /// If both fail, the error of the primary interface is reported, since that is the one
    /// expected to work.
    ///
    /// Only called while an `AudioSystem` (and so the COM apartment) is alive on this thread; the
    /// object is released before it.
    fn create() -> Result<Self> {
        match Self::create_current() {
            Ok(policy) => Ok(policy),
            Err(primary) => Self::create_vista().map_err(|_| primary),
        }
    }

    /// `CoCreateInstance(CPolicyConfigClient)` for `IPolicyConfig`.
    fn create_current() -> Result<Self> {
        // SAFETY: COM is initialized on this thread (see `create`); no outer unknown.
        unsafe {
            CoCreateInstance::<_, IPolicyConfig>(
                &CLSID_POLICY_CONFIG_CLIENT,
                None,
                CLSCTX_INPROC_SERVER,
            )
        }
        .map(Self::Current)
        .map_err(Error::com("CoCreateInstance(CPolicyConfigClient)"))
    }

    /// `CoCreateInstance(CPolicyConfigVistaClient)` for `IPolicyConfigVista`.
    fn create_vista() -> Result<Self> {
        // SAFETY: COM is initialized on this thread (see `create`); no outer unknown.
        unsafe {
            CoCreateInstance::<_, IPolicyConfigVista>(
                &CLSID_POLICY_CONFIG_VISTA_CLIENT,
                None,
                CLSCTX_INPROC_SERVER,
            )
        }
        .map(Self::Vista)
        .map_err(Error::com("CoCreateInstance(CPolicyConfigVistaClient)"))
    }

    /// `SetDefaultEndpoint(id, role)`; when that fails on `IPolicyConfig`, retries once through
    /// `IPolicyConfigVista` and, if the retry succeeds, keeps using it (`self` is replaced).
    ///
    /// This covers a future Windows that keeps `CPolicyConfigClient` registered but stubs or moves
    /// the method. When the retry fails too, the primary error is reported.
    fn set_default_endpoint_with_fallback(&mut self, id: &HSTRING, role: Role) -> Result<()> {
        let Err(primary) = self.set_default_endpoint(id, role) else {
            return Ok(());
        };
        if matches!(self, Self::Vista(_)) {
            return Err(primary);
        }
        let Ok(vista) = Self::create_vista() else {
            return Err(primary);
        };
        if vista.set_default_endpoint(id, role).is_err() {
            return Err(primary);
        }
        *self = vista;
        Ok(())
    }

    /// `SetDefaultEndpoint(id, role)` on whichever interface was created.
    fn set_default_endpoint(&self, id: &HSTRING, role: Role) -> Result<()> {
        let id = PCWSTR(id.as_ptr());
        let (call, hr) = match self {
            // SAFETY: `policy` is a live interface whose vtable layout matches the declaration in
            // `policy_config` (slot 13, verified against AudioSes.dll symbols), and `id` points to
            // a NUL-terminated string owned by the caller for the duration of the call.
            Self::Current(policy) => ("IPolicyConfig::SetDefaultEndpoint", unsafe {
                policy.SetDefaultEndpoint(id, role.to_erole())
            }),
            // SAFETY: as above, with `IPolicyConfigVista` (slot 12).
            Self::Vista(policy) => ("IPolicyConfigVista::SetDefaultEndpoint", unsafe {
                policy.SetDefaultEndpoint(id, role.to_erole())
            }),
        };
        hr.ok().map_err(Error::com(call))
    }
}

/// Declarations of the undocumented policy-config interfaces.
///
/// Only `SetDefaultEndpoint` is ever called. Every other method is declared with opaque pointer
/// parameters purely to keep the vtable layout; the slot numbers were resolved against the public
/// PDB symbols of AudioSes.dll 10.0.26100 (`docs/research/core-audio-api.md` section B.2).
mod policy_config {
    #![allow(non_snake_case, reason = "method names mirror the native COM vtable")]
    #![allow(
        clippy::transmute_ptr_to_ptr,
        reason = "emitted by the windows_core::interface macro expansion"
    )]

    use std::ffi::c_void;

    use windows::Win32::Media::Audio::ERole;
    use windows_core::{HRESULT, IUnknown, IUnknown_Vtbl, PCWSTR, interface};

    /// `IPolicyConfig` (Windows 7 through 11), implemented by `CPolicyConfigClient`.
    #[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
    pub(super) unsafe trait IPolicyConfig: IUnknown {
        fn GetMixFormat(&self, id: PCWSTR, format: *mut *mut c_void) -> HRESULT; // 3
        fn GetDeviceFormat(&self, id: PCWSTR, default: i32, format: *mut *mut c_void) -> HRESULT; // 4
        fn ResetDeviceFormat(&self, id: PCWSTR) -> HRESULT; // 5
        fn SetDeviceFormat(&self, id: PCWSTR, endpoint: *mut c_void, mix: *mut c_void) -> HRESULT; // 6
        fn GetProcessingPeriod(
            &self,
            id: PCWSTR,
            default: i32,
            default_period: *mut i64,
            minimum_period: *mut i64,
        ) -> HRESULT; // 7
        fn SetProcessingPeriod(&self, id: PCWSTR, period: *mut i64) -> HRESULT; // 8
        fn GetShareMode(&self, id: PCWSTR, mode: *mut c_void) -> HRESULT; // 9
        fn SetShareMode(&self, id: PCWSTR, mode: *mut c_void) -> HRESULT; // 10
        fn GetPropertyValue(
            &self,
            id: PCWSTR,
            fx_store: i32,
            key: *const c_void,
            value: *mut c_void,
        ) -> HRESULT; // 11
        fn SetPropertyValue(
            &self,
            id: PCWSTR,
            fx_store: i32,
            key: *const c_void,
            value: *mut c_void,
        ) -> HRESULT; // 12
        pub(super) fn SetDefaultEndpoint(&self, id: PCWSTR, role: ERole) -> HRESULT; // 13
        fn SetEndpointVisibility(&self, id: PCWSTR, visible: i32) -> HRESULT; // 14
    }

    /// `IPolicyConfigVista`, implemented by `CPolicyConfigVistaClient`.
    ///
    /// Unlike `IPolicyConfig` it has no `ResetDeviceFormat`, so `SetDefaultEndpoint` is slot 12.
    /// Some open-source switchers declare `ResetDeviceFormat` here too, which shifts their
    /// `SetDefaultEndpoint` onto the not-implemented stub at slot 13; do not copy those layouts.
    #[interface("568b9108-44bf-40b4-9006-86afe5b5a620")]
    pub(super) unsafe trait IPolicyConfigVista: IUnknown {
        fn GetMixFormat(&self, id: PCWSTR, format: *mut *mut c_void) -> HRESULT; // 3
        fn GetDeviceFormat(&self, id: PCWSTR, default: i32, format: *mut *mut c_void) -> HRESULT; // 4
        fn SetDeviceFormat(&self, id: PCWSTR, endpoint: *mut c_void, mix: *mut c_void) -> HRESULT; // 5
        fn GetProcessingPeriod(
            &self,
            id: PCWSTR,
            default: i32,
            default_period: *mut i64,
            minimum_period: *mut i64,
        ) -> HRESULT; // 6
        fn SetProcessingPeriod(&self, id: PCWSTR, period: *mut i64) -> HRESULT; // 7
        fn GetShareMode(&self, id: PCWSTR, mode: *mut c_void) -> HRESULT; // 8
        fn SetShareMode(&self, id: PCWSTR, mode: *mut c_void) -> HRESULT; // 9
        fn GetPropertyValue(
            &self,
            id: PCWSTR,
            fx_store: i32,
            key: *const c_void,
            value: *mut c_void,
        ) -> HRESULT; // 10
        fn SetPropertyValue(
            &self,
            id: PCWSTR,
            fx_store: i32,
            key: *const c_void,
            value: *mut c_void,
        ) -> HRESULT; // 11
        pub(super) fn SetDefaultEndpoint(&self, id: PCWSTR, role: ERole) -> HRESULT; // 12
        fn SetEndpointVisibility(&self, id: PCWSTR, visible: i32) -> HRESULT; // 13
    }
}

/// A wide string allocated by COM with `CoTaskMemAlloc`, freed on drop.
struct CoTaskMemString(PWSTR);

impl CoTaskMemString {
    fn to_string_lossy(&self) -> String {
        // SAFETY: the pointer came from a successful COM call that returns a NUL-terminated
        // string (or null), and it stays allocated until `self` drops.
        unsafe { wide_to_string(self.0.0) }
    }
}

impl Drop for CoTaskMemString {
    fn drop(&mut self) {
        // SAFETY: the string was allocated with `CoTaskMemAlloc` by the callee and ownership was
        // transferred to us; it is freed exactly once. `CoTaskMemFree` accepts null.
        unsafe { CoTaskMemFree(Some(self.0.0.cast::<c_void>().cast_const())) };
    }
}

/// Copies a NUL-terminated UTF-16 string, replacing invalid surrogates; null gives `""`.
///
/// # Safety
///
/// `text` must be null or point to a NUL-terminated UTF-16 string that stays valid for the call.
unsafe fn wide_to_string(text: *const u16) -> String {
    if text.is_null() {
        return String::new();
    }
    let text = PCWSTR(text);
    // SAFETY: non-null and NUL-terminated per this function's contract.
    String::from_utf16_lossy(unsafe { text.as_wide() })
}

/// The id and friendly name of `device`.
fn endpoint_of(device: &IMMDevice) -> Result<Endpoint> {
    Ok(Endpoint {
        id: id_of(device)?,
        name: friendly_name(device)?,
    })
}

/// `IMMDevice::GetId`, freeing the returned string.
fn id_of(device: &IMMDevice) -> Result<String> {
    // SAFETY: `device` is a live interface pointer; the returned string is owned by the guard.
    let id = unsafe { device.GetId() }.map_err(Error::com("IMMDevice::GetId"))?;
    Ok(CoTaskMemString(id).to_string_lossy())
}

/// `PKEY_Device_FriendlyName` of `device`, or `""` when the property is missing or not a string.
fn friendly_name(device: &IMMDevice) -> Result<String> {
    // SAFETY: `device` is a live interface pointer and `STGM_READ` is a valid access mode.
    let store = unsafe { device.OpenPropertyStore(STGM_READ) }
        .map_err(Error::com("IMMDevice::OpenPropertyStore"))?;
    // The windows crate's `Drop for PROPVARIANT` calls `PropVariantClear`, so `value` (and the
    // string it owns) is freed on every path out of this function.
    // SAFETY: `store` is a live interface pointer and the key is a static PROPERTYKEY.
    let value = unsafe { store.GetValue(&PKEY_Device_FriendlyName) }
        .map_err(Error::com("IPropertyStore::GetValue"))?;
    if value.vt() != VT_LPWSTR {
        return Ok(String::new());
    }
    // SAFETY: `vt == VT_LPWSTR`, so `pwszVal` is the initialized union member. It is null or a
    // NUL-terminated string owned by `value`, which outlives the copy below.
    Ok(unsafe { wide_to_string(value.Anonymous.Anonymous.Anonymous.pwszVal.0) })
}

/// Whether `device` is `DEVICE_STATE_ACTIVE` (not disabled, unplugged or not present).
fn is_device_active(device: &IMMDevice) -> Result<bool> {
    // SAFETY: `device` is a live interface pointer.
    let state = unsafe { device.GetState() }.map_err(Error::com("IMMDevice::GetState"))?;
    Ok(state == DEVICE_STATE_ACTIVE)
}

/// The endpoint id as a wide string for `GetDevice`, or `None` when it cannot be a valid id.
///
/// An empty id is rejected up front, and so is one with an embedded NUL, which Windows would
/// silently truncate (turning `"<valid id>\0junk"` into a match).
fn endpoint_id_param(id: &str) -> Option<HSTRING> {
    (!id.is_empty() && !id.contains('\0')).then(|| HSTRING::from(id))
}

/// Whether `GetDevice` failed because the id is unknown (`E_NOTFOUND`) or malformed
/// (`E_INVALIDARG`), as opposed to a real failure.
fn is_unknown_id(hr: HRESULT) -> bool {
    hr == E_NOTFOUND || hr == E_INVALIDARG
}

#[cfg(test)]
mod tests {
    use std::mem::{offset_of, size_of};

    use windows::Win32::Foundation::E_FAIL;
    use windows_core::Interface;

    use super::policy_config::{IPolicyConfig_Vtbl, IPolicyConfigVista_Vtbl};
    use super::*;
    use crate::error::EXIT_DEVICE;

    fn endpoint(id: &str, name: &str) -> Endpoint {
        Endpoint {
            id: id.to_owned(),
            name: name.to_owned(),
        }
    }

    fn sample() -> Vec<Endpoint> {
        vec![
            endpoint(
                "{0.0.0.00000000}.{739b3554-bfed-4d61-b407-a818b317c991}",
                "PG42UQ (NVIDIA High Definition Audio)",
            ),
            endpoint(
                "{0.0.0.00000000}.{5b124733-5d8f-428c-b83c-ee05ce6467fb}",
                "PHL BDM4065 (NVIDIA High Definition Audio)",
            ),
            endpoint(
                "{0.0.0.00000000}.{30045f40-8cfd-4441-bb89-0d13fc19b589}",
                "喇叭 (FiiO BTA30 PRO)",
            ),
            endpoint(
                "{0.0.0.00000000}.{00000000-0000-0000-0000-000000000001}",
                "Speakers",
            ),
            endpoint(
                "{0.0.0.00000000}.{00000000-0000-0000-0000-000000000002}",
                "speakers",
            ),
            endpoint(
                "{0.0.0.00000000}.{00000000-0000-0000-0000-000000000003}",
                "Speakers (2)",
            ),
        ]
    }

    fn resolved_name(query: &str) -> String {
        let endpoints = sample();
        resolve(&endpoints, query).unwrap().name.clone()
    }

    #[test]
    fn roles_follow_the_design_order() {
        assert_eq!(
            roles_for(true),
            &[Role::Console, Role::Multimedia, Role::Communications]
        );
        assert_eq!(roles_for(false), &[Role::Console, Role::Multimedia]);
    }

    #[test]
    fn roles_map_to_erole() {
        assert_eq!(ERole::from(Role::Console), eConsole);
        assert_eq!(Role::Multimedia.to_erole(), eMultimedia);
        assert_eq!(Role::Communications.to_erole(), eCommunications);
        assert_eq!((eConsole.0, eMultimedia.0, eCommunications.0), (0, 1, 2));
    }

    #[test]
    fn com_identifiers_match_the_research() {
        let parse = |text: &str| GUID::try_from(text).unwrap();
        assert_eq!(
            CLSID_POLICY_CONFIG_CLIENT,
            parse("870af99c-171d-4f9e-af0d-e63df40c2bc9")
        );
        assert_eq!(
            CLSID_POLICY_CONFIG_VISTA_CLIENT,
            parse("294935CE-F637-4E7C-A41B-AB255460B862")
        );
        assert_eq!(
            IPolicyConfig::IID,
            parse("f8679f50-850a-41cf-9c72-430f290290c8")
        );
        assert_eq!(
            IPolicyConfigVista::IID,
            parse("568b9108-44bf-40b4-9006-86afe5b5a620")
        );
        // `{:X}` on the i32 prints its two's-complement bits.
        assert_eq!(format!("{:08X}", E_NOTFOUND.0), "80070490");
    }

    #[test]
    fn set_default_endpoint_sits_in_the_verified_vtable_slot() {
        let slot = |offset: usize| offset / size_of::<usize>();
        assert_eq!(slot(offset_of!(IPolicyConfig_Vtbl, SetDefaultEndpoint)), 13);
        assert_eq!(
            slot(offset_of!(IPolicyConfigVista_Vtbl, SetDefaultEndpoint)),
            12
        );
        assert_eq!(size_of::<IPolicyConfig_Vtbl>() / size_of::<usize>(), 15);
        assert_eq!(
            size_of::<IPolicyConfigVista_Vtbl>() / size_of::<usize>(),
            14
        );
    }

    #[test]
    fn unknown_and_malformed_ids_are_not_failures() {
        assert!(is_unknown_id(E_NOTFOUND));
        assert!(is_unknown_id(E_INVALIDARG));
        assert!(!is_unknown_id(E_FAIL));
        assert!(!is_unknown_id(RPC_E_CHANGED_MODE));
    }

    #[test]
    fn suspicious_ids_never_reach_get_device() {
        assert!(endpoint_id_param("").is_none());
        assert!(endpoint_id_param("{0.0.0.00000000}.{739b3554}\0junk").is_none());
        assert_eq!(
            endpoint_id_param("{0.0.0.00000000}.{739b3554}").unwrap(),
            HSTRING::from("{0.0.0.00000000}.{739b3554}")
        );
    }

    #[test]
    fn wide_strings_convert_losslessly_and_tolerate_null() {
        let text: Vec<u16> = "喇叭 (FiiO BTA30 PRO)\0".encode_utf16().collect();
        // SAFETY: `text` is NUL-terminated and alive for the call.
        let converted = unsafe { wide_to_string(text.as_ptr()) };
        assert_eq!(converted, "喇叭 (FiiO BTA30 PRO)");
        // SAFETY: null is explicitly allowed.
        let empty = unsafe { wide_to_string(std::ptr::null()) };
        assert_eq!(empty, "");
    }

    #[test]
    fn resolve_prefers_an_exact_id_in_any_case() {
        assert_eq!(
            resolved_name("{0.0.0.00000000}.{5B124733-5D8F-428C-B83C-EE05CE6467FB}"),
            "PHL BDM4065 (NVIDIA High Definition Audio)"
        );
    }

    #[test]
    fn resolve_prefers_an_exact_name_over_a_folded_or_partial_one() {
        // "Speakers" also equals "speakers" case-insensitively and is a substring of "Speakers (2)".
        assert_eq!(resolved_name("Speakers"), "Speakers");
        assert_eq!(resolved_name("speakers"), "speakers");
        assert_eq!(resolved_name(" Speakers (2) "), "Speakers (2)");
    }

    #[test]
    fn resolve_accepts_a_unique_case_insensitive_substring() {
        assert_eq!(resolved_name("fiio"), "喇叭 (FiiO BTA30 PRO)");
        assert_eq!(resolved_name("喇叭"), "喇叭 (FiiO BTA30 PRO)");
        assert_eq!(
            resolved_name("pg42"),
            "PG42UQ (NVIDIA High Definition Audio)"
        );
        assert_eq!(
            resolved_name("pg42uq (nvidia high definition audio)"),
            "PG42UQ (NVIDIA High Definition Audio)"
        );
    }

    #[test]
    fn resolve_reports_ambiguous_names() {
        let endpoints = sample();
        match resolve(&endpoints, "NVIDIA") {
            Err(Error::AmbiguousDevice { query, matches }) => {
                assert_eq!(query, "NVIDIA");
                assert_eq!(matches, endpoints[..2]);
            }
            other => panic!("expected AmbiguousDevice, got {other:?}"),
        }
        // Two endpoints that share an exact name are ambiguous too.
        let twins = [endpoint("{a}", "Twin"), endpoint("{b}", "Twin")];
        let error = resolve(&twins, "Twin").unwrap_err();
        assert!(matches!(error, Error::AmbiguousDevice { .. }), "{error:?}");
        assert_eq!(error.exit_code(), EXIT_DEVICE);
    }

    #[test]
    fn resolve_reports_unknown_and_blank_queries() {
        let endpoints = sample();
        for query in ["Headphones", "", "   "] {
            let error = resolve(&endpoints, query).unwrap_err();
            assert!(
                matches!(error, Error::DeviceNotFound(_)),
                "{query:?}: {error:?}"
            );
            assert_eq!(error.exit_code(), EXIT_DEVICE);
        }
        assert!(matches!(
            resolve(&[], "anything"),
            Err(Error::DeviceNotFound(query)) if query == "anything"
        ));
    }

    /// Re-asserts each role's current default through both policy-config interfaces. This calls
    /// the real `SetDefaultEndpoint` (which `set_default` skips for the current default) without
    /// changing anything, and checks afterwards that nothing changed.
    #[test]
    #[ignore = "needs a Windows machine with an active playback device"]
    fn policy_config_reasserts_the_current_defaults() -> Result<()> {
        let audio = AudioSystem::new()?;
        let roles = [Role::Console, Role::Multimedia, Role::Communications];
        let before = roles
            .iter()
            .map(|&role| audio.default_for(role))
            .collect::<Result<Vec<_>>>()?;

        let current = PolicyConfig::create()?;
        assert!(matches!(current, PolicyConfig::Current(_)));
        let vista = PolicyConfig::create_vista()?;
        assert!(matches!(vista, PolicyConfig::Vista(_)));
        for (&role, endpoint) in roles.iter().zip(&before) {
            let endpoint = endpoint.as_ref().expect("a default device for every role");
            let id = HSTRING::from(endpoint.id.as_str());
            current.set_default_endpoint(&id, role)?;
            vista.set_default_endpoint(&id, role)?;
        }

        for (&role, endpoint) in roles.iter().zip(&before) {
            assert_eq!(&audio.default_for(role)?, endpoint, "{role:?} changed");
        }
        Ok(())
    }
}
