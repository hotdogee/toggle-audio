//! The undocumented `IPolicyConfig` COM interface that sets the default
//! endpoint (there is no documented API for it). Every audio switcher uses it:
//! `SoundSwitch`, `EarTrumpet`, `AudioDeviceCmdlets`, `NirCmd`.
//!
//! Layout verified against the public PDB symbols of AudioSes.dll
//! 10.0.26100.8875 (see docs/research/core-audio-api.md, B.2/B.3):
//! `SetDefaultEndpoint` is 0-based vtable slot **13** of `IPolicyConfig` on
//! `CPolicyConfigClient`, and slot **12** of `IPolicyConfigVista` on
//! `CPolicyConfigVistaClient`. The other methods are declared only to keep the
//! slots in place; they are never called, so their parameter types are opaque.
//!
//! Do not copy the Vista layout from `SoundSwitch` or `AudioDeviceCmdlets`: theirs
//! has an extra `ResetDeviceFormat` that shifts `SetDefaultEndpoint` to a stub.

// The method names mirror the C++ declarations so the slots can be checked
// against the sources line by line.
#![allow(non_snake_case)]
// The `#[interface]` expansion casts between vtable references with transmute.
#![allow(clippy::transmute_ptr_to_ptr)]

use std::ffi::c_void;

use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::ERole;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::core::{GUID, HRESULT, PCWSTR};
// `#[interface]` expands to `::windows_core::...` paths and refers to the
// parent vtable type unqualified, so `IUnknown_Vtbl` must be in scope.
use windows_core::{IUnknown, IUnknown_Vtbl, interface};

use crate::audio::ComApartment;
use crate::error::Failure;

/// `CPolicyConfigClient` (AudioSes.dll, ThreadingModel=Both).
const CLSID_POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

/// `CPolicyConfigVistaClient` (AudioSes.dll, ThreadingModel=Both).
const CLSID_POLICY_CONFIG_VISTA_CLIENT: GUID =
    GUID::from_u128(0x294935ce_f637_4e7c_a41b_ab255460b862);

/// `IPolicyConfig` (Windows 7, 8, 10 1607+, 11).
#[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
pub unsafe trait IPolicyConfig: IUnknown {
    fn GetMixFormat(&self, id: PCWSTR, format: *mut *mut c_void) -> HRESULT; // 3
    fn GetDeviceFormat(&self, id: PCWSTR, default: i32, format: *mut *mut c_void) -> HRESULT; // 4
    fn ResetDeviceFormat(&self, id: PCWSTR) -> HRESULT; // 5
    fn SetDeviceFormat(&self, id: PCWSTR, endpoint: *mut c_void, mix: *mut c_void) -> HRESULT; // 6
    fn GetProcessingPeriod(&self, id: PCWSTR, default: i32, d: *mut i64, m: *mut i64) -> HRESULT; // 7
    fn SetProcessingPeriod(&self, id: PCWSTR, period: *mut i64) -> HRESULT; // 8
    fn GetShareMode(&self, id: PCWSTR, mode: *mut c_void) -> HRESULT; // 9
    fn SetShareMode(&self, id: PCWSTR, mode: *mut c_void) -> HRESULT; // 10
    fn GetPropertyValue(
        &self,
        id: PCWSTR,
        fx: i32,
        key: *const PROPERTYKEY,
        pv: *mut c_void,
    ) -> HRESULT; // 11
    fn SetPropertyValue(
        &self,
        id: PCWSTR,
        fx: i32,
        key: *const PROPERTYKEY,
        pv: *mut c_void,
    ) -> HRESULT; // 12
    fn SetDefaultEndpoint(&self, id: PCWSTR, role: ERole) -> HRESULT; // 13
    fn SetEndpointVisibility(&self, id: PCWSTR, visible: i32) -> HRESULT; // 14
}

/// `IPolicyConfigVista` (Vista layout; still implemented on Windows 11).
#[interface("568b9108-44bf-40b4-9006-86afe5b5a620")]
pub unsafe trait IPolicyConfigVista: IUnknown {
    fn GetMixFormat(&self, id: PCWSTR, format: *mut *mut c_void) -> HRESULT; // 3
    fn GetDeviceFormat(&self, id: PCWSTR, default: i32, format: *mut *mut c_void) -> HRESULT; // 4
    fn SetDeviceFormat(&self, id: PCWSTR, endpoint: *mut c_void, mix: *mut c_void) -> HRESULT; // 5
    fn GetProcessingPeriod(&self, id: PCWSTR, default: i32, d: *mut i64, m: *mut i64) -> HRESULT; // 6
    fn SetProcessingPeriod(&self, id: PCWSTR, period: *mut i64) -> HRESULT; // 7
    fn GetShareMode(&self, id: PCWSTR, mode: *mut c_void) -> HRESULT; // 8
    fn SetShareMode(&self, id: PCWSTR, mode: *mut c_void) -> HRESULT; // 9
    fn GetPropertyValue(&self, id: PCWSTR, key: *const PROPERTYKEY, pv: *mut c_void) -> HRESULT; // 10
    fn SetPropertyValue(&self, id: PCWSTR, key: *const PROPERTYKEY, pv: *mut c_void) -> HRESULT; // 11
    fn SetDefaultEndpoint(&self, id: PCWSTR, role: ERole) -> HRESULT; // 12
    fn SetEndpointVisibility(&self, id: PCWSTR, visible: i32) -> HRESULT; // 13
}

/// Whichever policy-config interface could be created. Dropping it releases
/// the COM object.
pub enum PolicyConfig {
    Current(IPolicyConfig),
    Vista(IPolicyConfigVista),
}

impl PolicyConfig {
    /// Creates `CPolicyConfigClient` as `IPolicyConfig`; if that fails, falls
    /// back to `CPolicyConfigVistaClient` as `IPolicyConfigVista`. When both
    /// fail, the HRESULT of the primary attempt is reported.
    ///
    /// The `ComApartment` borrow proves COM is initialised on this thread
    /// and ties the call to an apartment that outlives it.
    pub fn create(_apartment: &ComApartment) -> Result<Self, Failure> {
        // SAFETY: COM is initialised on this thread; CoCreateInstance QIs the
        // new object for `IPolicyConfig::IID`, so the returned pointer really
        // implements the declared vtable.
        let primary: windows::core::Result<IPolicyConfig> =
            unsafe { CoCreateInstance(&CLSID_POLICY_CONFIG_CLIENT, None, CLSCTX_INPROC_SERVER) };
        match primary {
            Ok(pc) => Ok(Self::Current(pc)),
            Err(primary_error) => {
                // SAFETY: as above, for `IPolicyConfigVista::IID`.
                let vista: windows::core::Result<IPolicyConfigVista> = unsafe {
                    CoCreateInstance(
                        &CLSID_POLICY_CONFIG_VISTA_CLIENT,
                        None,
                        CLSCTX_INPROC_SERVER,
                    )
                };
                vista.map(Self::Vista).map_err(|_| Failure::Com {
                    step: "CoCreateInstance(PolicyConfigClient)",
                    hr: primary_error.code(),
                })
            }
        }
    }

    /// Makes `id` the default render endpoint for `role`.
    ///
    /// # Safety
    /// `id` must point to a valid NUL-terminated UTF-16 string that stays
    /// alive for the duration of the call.
    pub unsafe fn set_default_endpoint(&self, id: PCWSTR, role: ERole) -> HRESULT {
        // SAFETY: the interface pointer is live (owned by `self`), the vtable
        // slot is verified (module docs), and the caller guarantees `id`.
        unsafe {
            match self {
                Self::Current(pc) => pc.SetDefaultEndpoint(id, role),
                Self::Vista(pc) => pc.SetDefaultEndpoint(id, role),
            }
        }
    }
}
