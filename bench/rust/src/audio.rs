//! Core Audio access through the `windows` crate: COM apartment, endpoint
//! enumeration, default lookup, state validation and `SetDefaultEndpoint`.
//!
//! Resource rules:
//! * Every interface (`IMMDeviceEnumerator`, `IMMDevice`, `IMMDeviceCollection`,
//!   `IMMEndpoint`, `IPropertyStore`, `IPolicyConfig`) is a `windows` crate
//!   smart pointer whose `Drop` calls `Release`.
//! * `AudioSystem<'com>` holds a borrow of the `ComApartment`, so the borrow
//!   checker guarantees the enumerator is released before `CoUninitialize`.
//!   Every other interface is a local inside an `AudioSystem` method and is
//!   dropped before that method returns: no `IMMDevice` or `IPolicyConfig`
//!   leaves this module (callers only see the plain-data `EndpointId` and
//!   `Endpoint`), and `PolicyConfig::create` demands a `&ComApartment`.
//! * Strings returned by `IMMDevice::GetId` are wrapped in `CoTaskString`,
//!   whose `Drop` calls `CoTaskMemFree`, so they are freed on every path.
//! * Each `PROPVARIANT` is cleared with `PropVariantClear` right after use.

use std::ffi::c_void;

use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::{E_INVALIDARG, ERROR_NOT_FOUND, RPC_E_CHANGED_MODE};
use windows::Win32::Media::Audio::{
    DEVICE_STATE_ACTIVE, ERole, IMMDevice, IMMDeviceEnumerator, IMMEndpoint, MMDeviceEnumerator,
    eCommunications, eConsole, eMultimedia, eRender,
};
use windows::Win32::System::Com::StructuredStorage::PropVariantClear;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
    CoInitializeEx, CoTaskMemFree, CoUninitialize, STGM_READ,
};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::core::{HRESULT, Interface, PCWSTR, PWSTR};

use crate::cli::ids_equal;
use crate::error::{Failure, Step};
use crate::policy::PolicyConfig;

/// `HRESULT_FROM_WIN32(ERROR_NOT_FOUND)` = `0x80070490`: unknown endpoint id,
/// or no default endpoint. (`windows` 0.62 only exports an unrelated
/// `E_NOTFOUND` from its `HtmlHelp` module, so it is built here.)
const E_NOTFOUND: HRESULT = HRESULT::from_win32(ERROR_NOT_FOUND.0);

/// The roles set by `set`, in the contract's order, each with the step name
/// reported if its call fails. The three calls are not atomic, so a failure
/// in a later role names the roles that were already switched.
const ALL_ROLES: [(ERole, &str); 3] = [
    (eConsole, "SetDefaultEndpoint(eConsole)"),
    (
        eMultimedia,
        "SetDefaultEndpoint(eMultimedia) (eConsole was already switched)",
    ),
    (
        eCommunications,
        "SetDefaultEndpoint(eCommunications) (eConsole and eMultimedia were already switched)",
    ),
];

// ---------------------------------------------------------------------------
// COM apartment
// ---------------------------------------------------------------------------

/// Initializes COM on the current thread (single-threaded apartment) and
/// uninitializes it on drop.
///
/// STA is what the product's settings dialog needs; both CLSIDs used here are
/// registered `ThreadingModel=Both`, so STA and MTA measure the same.
pub struct ComApartment {
    /// False when the thread was already in the other apartment
    /// (`RPC_E_CHANGED_MODE`): that initialization is not ours to undo.
    uninitialize: bool,
}

impl ComApartment {
    /// `CoInitializeEx(COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE)`.
    pub fn init() -> Result<Self, Failure> {
        // SAFETY: no reserved pointer is passed; the call only affects this
        // thread. Every successful call (S_OK, S_FALSE) is paired with
        // CoUninitialize in Drop.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        if hr == RPC_E_CHANGED_MODE {
            return Ok(Self {
                uninitialize: false,
            });
        }
        if hr.is_err() {
            return Err(Failure::Com {
                step: "CoInitializeEx",
                hr,
            });
        }
        Ok(Self { uninitialize: true })
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.uninitialize {
            // SAFETY: paired with the successful CoInitializeEx in `init`.
            // `AudioSystem` borrows `self`, so no interface pointer obtained
            // through it can still be alive here.
            unsafe { CoUninitialize() };
        }
    }
}

// ---------------------------------------------------------------------------
// Strings
// ---------------------------------------------------------------------------

/// Owns a `CoTaskMemAlloc`'d wide string (from `IMMDevice::GetId`) and frees
/// it with `CoTaskMemFree` on drop.
struct CoTaskString(PWSTR);

impl CoTaskString {
    /// The UTF-16 units without the NUL terminator.
    fn as_wide(&self) -> &[u16] {
        if self.0.is_null() {
            return &[];
        }
        // SAFETY: the pointer is non-null and points to the NUL-terminated
        // string the callee allocated; it stays valid until `drop`.
        unsafe { self.0.as_wide() }
    }
}

impl Drop for CoTaskString {
    fn drop(&mut self) {
        // SAFETY: the pointer came from CoTaskMemAlloc (GetId contract) and is
        // freed exactly once. CoTaskMemFree(NULL) is a documented no-op.
        unsafe { CoTaskMemFree(Some(self.0.0 as *const c_void)) };
    }
}

/// An endpoint id as UTF-16, always NUL-terminated so it can be passed to COM.
///
/// Deliberately not `PartialEq`: ids compare ASCII case-insensitively, so use
/// [`EndpointId::same_as`] rather than a derived, case-sensitive `==`.
#[derive(Debug, Clone)]
pub struct EndpointId(Vec<u16>);

impl EndpointId {
    /// Wraps a UTF-16 buffer, adding the NUL terminator if it is missing.
    pub fn from_wide(mut units: Vec<u16>) -> Self {
        if units.last() != Some(&0) {
            units.push(0);
        }
        Self(units)
    }

    /// Reads the id of `device` (`IMMDevice::GetId`).
    fn of(device: &IMMDevice) -> Result<Self, Failure> {
        // SAFETY: `device` is a live interface; GetId returns a CoTaskMem
        // string that `CoTaskString` frees.
        let raw = CoTaskString(unsafe { device.GetId() }.step("IMMDevice::GetId")?);
        let mut units = Vec::with_capacity(raw.as_wide().len() + 1);
        units.extend_from_slice(raw.as_wide());
        Ok(Self::from_wide(units))
    }

    /// Pointer for COM calls; valid while `self` is alive and unmodified.
    pub fn as_pcwstr(&self) -> PCWSTR {
        PCWSTR(self.0.as_ptr())
    }

    /// The UTF-16 units including the NUL terminator.
    pub fn as_wide(&self) -> &[u16] {
        &self.0
    }

    /// UTF-8 for output. Ids are ASCII in practice; anything else is replaced
    /// with U+FFFD rather than failing.
    pub fn to_utf8(&self) -> String {
        let units = self.0.strip_suffix(&[0]).unwrap_or(&self.0);
        String::from_utf16_lossy(units)
    }

    /// True if both ids name the same endpoint (ASCII case-insensitive).
    pub fn same_as(&self, other: &Self) -> bool {
        ids_equal(&self.0, &other.0)
    }
}

/// An active render endpoint: id plus `PKEY_Device_FriendlyName`, the exact
/// string Windows Sound settings shows (e.g. `喇叭 (FiiO BTA30 PRO)`).
pub struct Endpoint {
    pub id: EndpointId,
    pub name: String,
}

// ---------------------------------------------------------------------------
// Audio system
// ---------------------------------------------------------------------------

/// The device enumerator, tied to the lifetime of the COM apartment.
pub struct AudioSystem<'com> {
    enumerator: IMMDeviceEnumerator,
    /// Proof that COM is initialized; also handed to `PolicyConfig::create`.
    apartment: &'com ComApartment,
}

impl<'com> AudioSystem<'com> {
    /// `CoCreateInstance(MMDeviceEnumerator)`.
    pub fn new(apartment: &'com ComApartment) -> Result<Self, Failure> {
        // SAFETY: COM is initialized on this thread (proven by the borrowed
        // apartment); the result is QI'd to IMMDeviceEnumerator.
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER) }
                .step("CoCreateInstance(MMDeviceEnumerator)")?;
        Ok(Self {
            enumerator,
            apartment,
        })
    }

    /// The default render device for `role`, or `None` if there is none.
    fn default_device(&self, role: ERole) -> Result<Option<IMMDevice>, Failure> {
        // SAFETY: the enumerator is live; the returned device is owned.
        match unsafe { self.enumerator.GetDefaultAudioEndpoint(eRender, role) } {
            Ok(device) => Ok(Some(device)),
            Err(e) if e.code() == E_NOTFOUND => Ok(None),
            Err(e) => Err(Failure::Com {
                step: "GetDefaultAudioEndpoint",
                hr: e.code(),
            }),
        }
    }

    /// The id of the default render endpoint for `role`, if any.
    pub fn default_id(&self, role: ERole) -> Result<Option<EndpointId>, Failure> {
        self.default_device(role)?
            .map(|device| EndpointId::of(&device))
            .transpose()
    }

    /// The default render endpoint (id and name) for `role`, if any.
    pub fn default_endpoint(&self, role: ERole) -> Result<Option<Endpoint>, Failure> {
        let Some(device) = self.default_device(role)? else {
            return Ok(None);
        };
        Ok(Some(Endpoint {
            id: EndpointId::of(&device)?,
            name: friendly_name(&device)?,
        }))
    }

    /// Every ACTIVE render endpoint, in enumeration order.
    pub fn active_endpoints(&self) -> Result<Vec<Endpoint>, Failure> {
        // SAFETY: the enumerator is live; the collection is owned.
        let collection = unsafe {
            self.enumerator
                .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
        }
        .step("EnumAudioEndpoints")?;
        // SAFETY: the collection is live.
        let count = unsafe { collection.GetCount() }.step("IMMDeviceCollection::GetCount")?;
        let mut endpoints = Vec::with_capacity(count as usize);
        for index in 0..count {
            // SAFETY: `index < count`; the device is owned and released at
            // the end of the iteration.
            let device = unsafe { collection.Item(index) }.step("IMMDeviceCollection::Item")?;
            endpoints.push(Endpoint {
                id: EndpointId::of(&device)?,
                name: friendly_name(&device)?,
            });
        }
        Ok(endpoints)
    }

    /// Resolves `id` with `GetDevice` and requires a RENDER endpoint in
    /// `DEVICE_STATE_ACTIVE`.
    ///
    /// `GetDevice` succeeds for disabled, unplugged and not-present endpoints,
    /// so the state check is mandatory: `SetDefaultEndpoint` must never be
    /// called on a non-active endpoint. `GetDevice` also resolves capture
    /// (microphone) ids, and `SetDefaultEndpoint` would switch the default
    /// recording device for them, so the data flow is checked as well (exit
    /// 3). Returns the canonical id from `GetId`, which is what gets passed
    /// to `SetDefaultEndpoint`.
    pub fn require_active(&self, id: &EndpointId) -> Result<EndpointId, Failure> {
        // SAFETY: the enumerator is live and `id` is NUL-terminated and alive
        // for the duration of the call.
        let device = match unsafe { self.enumerator.GetDevice(id.as_pcwstr()) } {
            Ok(device) => device,
            Err(e) if e.code() == E_NOTFOUND || e.code() == E_INVALIDARG => {
                return Err(Failure::NotFound { id: id.to_utf8() });
            }
            Err(e) => {
                return Err(Failure::Com {
                    step: "GetDevice",
                    hr: e.code(),
                });
            }
        };
        // Every MMDevice endpoint object implements IMMEndpoint.
        let endpoint: IMMEndpoint = device.cast().step("QueryInterface(IMMEndpoint)")?;
        // SAFETY: the endpoint interface is live.
        let flow = unsafe { endpoint.GetDataFlow() }.step("IMMEndpoint::GetDataFlow")?;
        if flow != eRender {
            return Err(Failure::NotRender { id: id.to_utf8() });
        }
        // SAFETY: the device is live.
        let state = unsafe { device.GetState() }.step("IMMDevice::GetState")?;
        if state != DEVICE_STATE_ACTIVE {
            return Err(Failure::NotActive {
                id: id.to_utf8(),
                state: state.0,
            });
        }
        EndpointId::of(&device)
    }

    /// Calls `IPolicyConfig::SetDefaultEndpoint` for eConsole, eMultimedia and
    /// eCommunications, in that order, even if `id` already is the default
    /// (the bench's "set-noop" scenario measures exactly these three calls).
    /// The caller must have validated `id` with [`Self::require_active`].
    ///
    /// The three calls are not atomic: if a later role fails, the earlier
    /// roles have already been switched, and the error message says so.
    pub fn set_default_all_roles(&self, id: &EndpointId) -> Result<(), Failure> {
        let policy = PolicyConfig::create(self.apartment)?;
        for (role, step) in ALL_ROLES {
            // SAFETY: `id` is NUL-terminated and outlives the call.
            unsafe { policy.set_default_endpoint(id.as_pcwstr(), role) }
                .ok()
                .step(step)?;
        }
        Ok(())
    }
}

/// Reads `PKEY_Device_FriendlyName` (`{a45c254e-...},14`). A missing or
/// non-string value yields an empty name rather than an error, because the id
/// is what identifies the endpoint.
fn friendly_name(device: &IMMDevice) -> Result<String, Failure> {
    // SAFETY: the device is live; the store is owned and released on return.
    let store =
        unsafe { device.OpenPropertyStore(STGM_READ) }.step("IMMDevice::OpenPropertyStore")?;
    // SAFETY: the store is live and the key is a valid static PROPERTYKEY.
    let mut value =
        unsafe { store.GetValue(&PKEY_Device_FriendlyName) }.step("IPropertyStore::GetValue")?;

    // SAFETY: reading the `vt` tag of an initialized PROPVARIANT; the union
    // member `pwszVal` is only read when the tag says it is VT_LPWSTR, and the
    // string is copied before the PROPVARIANT is cleared.
    let name = unsafe {
        let inner = &value.Anonymous.Anonymous;
        if inner.vt == VT_LPWSTR && !inner.Anonymous.pwszVal.is_null() {
            String::from_utf16_lossy(inner.Anonymous.pwszVal.as_wide())
        } else {
            String::new()
        }
    };

    // SAFETY: `value` is a valid PROPVARIANT owned by us; clearing frees the
    // string and resets it to VT_EMPTY. (windows 0.62's PROPVARIANT also
    // implements Drop with PropVariantClear; clearing an already-empty
    // PROPVARIANT is a no-op, so this is not a double free. The explicit call
    // keeps the release point visible and checks its HRESULT.)
    unsafe { PropVariantClear(&raw mut value) }.step("PropVariantClear")?;
    Ok(name)
}
