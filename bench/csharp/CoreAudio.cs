// Core Audio COM access through raw vtable calls.
//
// NativeAOT does not support built-in COM interop ([ComImport] interfaces), so each COM
// method is called the way C does it: read the object's vtable pointer, pick the function
// pointer at the documented slot and call it with the object as the first argument.
// `delegate* unmanaged[Stdcall]` is the native calling convention; on x64 Stdcall and the
// platform default are the same thing.
//
// Slot numbers are 0-based and include IUnknown (QueryInterface=0, AddRef=1, Release=2).
// They were verified on Windows 11 26300 (see docs/research/core-audio-api.md, sections A-B).
//
// Ownership rules (identical to C): every interface pointer we receive is released exactly
// once, every string returned by IMMDevice::GetId is freed with CoTaskMemFree, and every
// PROPVARIANT filled by IPropertyStore::GetValue is cleared with PropVariantClear.

using System;
using System.Runtime.CompilerServices;

namespace ToggleAudio.Bench;

/// <summary>Constants from mmdeviceapi.h.</summary>
internal static class AudioConst
{
    public const int eRender = 0;

    public const int eConsole = 0;
    public const int eMultimedia = 1;
    public const int eCommunications = 2;

    public const uint DEVICE_STATE_ACTIVE = 0x1;
    public const uint STGM_READ = 0;
}

/// <summary>Class and interface ids. Written out as numbers so no string parsing runs at startup.</summary>
internal static class AudioGuids
{
    // {BCDE0395-E52F-467C-8E3D-C4579291692E}
    public static readonly Guid CLSID_MMDeviceEnumerator =
        new(0xBCDE0395, 0xE52F, 0x467C, 0x8E, 0x3D, 0xC4, 0x57, 0x92, 0x91, 0x69, 0x2E);

    // {A95664D2-9614-4F35-A746-DE8DB63617E6}
    public static readonly Guid IID_IMMDeviceEnumerator =
        new(0xA95664D2, 0x9614, 0x4F35, 0xA7, 0x46, 0xDE, 0x8D, 0xB6, 0x36, 0x17, 0xE6);

    // Undocumented CPolicyConfigClient {870AF99C-171D-4F9E-AF0D-E63DF40C2BC9} (AudioSes.dll).
    public static readonly Guid CLSID_CPolicyConfigClient =
        new(0x870AF99C, 0x171D, 0x4F9E, 0xAF, 0x0D, 0xE6, 0x3D, 0xF4, 0x0C, 0x2B, 0xC9);

    // Undocumented IPolicyConfig {F8679F50-850A-41CF-9C72-430F290290C8} (Windows 7 and later).
    public static readonly Guid IID_IPolicyConfig =
        new(0xF8679F50, 0x850A, 0x41CF, 0x9C, 0x72, 0x43, 0x0F, 0x29, 0x02, 0x90, 0xC8);

    // Fallback: CPolicyConfigVistaClient {294935CE-F637-4E7C-A41B-AB255460B862}.
    public static readonly Guid CLSID_CPolicyConfigVistaClient =
        new(0x294935CE, 0xF637, 0x4E7C, 0xA4, 0x1B, 0xAB, 0x25, 0x54, 0x60, 0xB8, 0x62);

    // Fallback: IPolicyConfigVista {568B9108-44BF-40B4-9006-86AFE5B5A620}.
    public static readonly Guid IID_IPolicyConfigVista =
        new(0x568B9108, 0x44BF, 0x40B4, 0x90, 0x06, 0x86, 0xAF, 0xE5, 0xB5, 0xA6, 0x20);

    // PKEY_Device_FriendlyName {A45C254E-DF1C-4EFD-8020-67D146A850E0}, pid 14: the name shown
    // by Windows Sound settings, e.g. "喇叭 (FiiO BTA30 PRO)".
    public static readonly Guid FMTID_Device =
        new(0xA45C254E, 0xDF1C, 0x4EFD, 0x80, 0x20, 0x67, 0xD1, 0x46, 0xA8, 0x50, 0xE0);
    public const uint PID_Device_FriendlyName = 14;
}

/// <summary>Reads function pointers out of a COM object's vtable.</summary>
internal static unsafe class Vtbl
{
    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    public static void* Slot(void* comObject, int index) => (*(void***)comObject)[index];

    /// <summary>IUnknown::Release (slot 2). Clears the caller's pointer so a second call is a no-op.</summary>
    public static void Release(ref void* comObject)
    {
        void* p = comObject;
        if (p != null)
        {
            comObject = null;
            ((delegate* unmanaged[Stdcall]<void*, uint>)Slot(p, 2))(p);
        }
    }
}

/// <summary>IMMDeviceEnumerator (mmdeviceapi.h).</summary>
internal static unsafe class MMDeviceEnumerator
{
    // HRESULT EnumAudioEndpoints(EDataFlow, DWORD stateMask, IMMDeviceCollection**)  slot 3
    public static int EnumAudioEndpoints(void* self, int dataFlow, uint stateMask, void** devices) =>
        ((delegate* unmanaged[Stdcall]<void*, int, uint, void**, int>)Vtbl.Slot(self, 3))(self, dataFlow, stateMask, devices);

    // HRESULT GetDefaultAudioEndpoint(EDataFlow, ERole, IMMDevice**)  slot 4
    public static int GetDefaultAudioEndpoint(void* self, int dataFlow, int role, void** device) =>
        ((delegate* unmanaged[Stdcall]<void*, int, int, void**, int>)Vtbl.Slot(self, 4))(self, dataFlow, role, device);

    // HRESULT GetDevice(LPCWSTR id, IMMDevice**)  slot 5
    public static int GetDevice(void* self, char* id, void** device) =>
        ((delegate* unmanaged[Stdcall]<void*, char*, void**, int>)Vtbl.Slot(self, 5))(self, id, device);
}

/// <summary>IMMDeviceCollection (mmdeviceapi.h).</summary>
internal static unsafe class MMDeviceCollection
{
    // HRESULT GetCount(UINT*)  slot 3
    public static int GetCount(void* self, uint* count) =>
        ((delegate* unmanaged[Stdcall]<void*, uint*, int>)Vtbl.Slot(self, 3))(self, count);

    // HRESULT Item(UINT, IMMDevice**)  slot 4
    public static int Item(void* self, uint index, void** device) =>
        ((delegate* unmanaged[Stdcall]<void*, uint, void**, int>)Vtbl.Slot(self, 4))(self, index, device);
}

/// <summary>IMMDevice (mmdeviceapi.h).</summary>
internal static unsafe class MMDevice
{
    // HRESULT OpenPropertyStore(DWORD stgmAccess, IPropertyStore**)  slot 4
    public static int OpenPropertyStore(void* self, uint stgmAccess, void** store) =>
        ((delegate* unmanaged[Stdcall]<void*, uint, void**, int>)Vtbl.Slot(self, 4))(self, stgmAccess, store);

    // HRESULT GetId(LPWSTR*)  slot 5; the string is CoTaskMemAlloc'ed
    public static int GetId(void* self, char** id) =>
        ((delegate* unmanaged[Stdcall]<void*, char**, int>)Vtbl.Slot(self, 5))(self, id);

    // HRESULT GetState(DWORD*)  slot 6
    public static int GetState(void* self, uint* state) =>
        ((delegate* unmanaged[Stdcall]<void*, uint*, int>)Vtbl.Slot(self, 6))(self, state);
}

/// <summary>IPropertyStore (propsys.h).</summary>
internal static unsafe class PropertyStore
{
    // HRESULT GetValue(REFPROPERTYKEY, PROPVARIANT*)  slot 5
    public static int GetValue(void* self, PropertyKey* key, PropVariant* value) =>
        ((delegate* unmanaged[Stdcall]<void*, PropertyKey*, PropVariant*, int>)Vtbl.Slot(self, 5))(self, key, value);
}

/// <summary>
/// Undocumented IPolicyConfig / IPolicyConfigVista. Only SetDefaultEndpoint is used:
/// slot 13 on IPolicyConfig (Windows 7+), slot 12 on IPolicyConfigVista.
/// </summary>
internal static unsafe class PolicyConfig
{
    public const int SetDefaultEndpointSlot = 13;
    public const int SetDefaultEndpointSlotVista = 12;

    // HRESULT SetDefaultEndpoint(LPCWSTR deviceId, ERole role)
    public static int SetDefaultEndpoint(void* self, int slot, char* deviceId, int role) =>
        ((delegate* unmanaged[Stdcall]<void*, char*, int, int>)Vtbl.Slot(self, slot))(self, deviceId, role);
}

/// <summary>Small helpers built on the raw calls. Each returns an HRESULT and owns its cleanup.</summary>
internal static unsafe class Audio
{
    /// <summary>IMMDevice::GetId copied into a managed string; the COM string is always freed.</summary>
    public static int GetId(void* device, out string id)
    {
        char* raw = null;
        int hr = MMDevice.GetId(device, &raw);
        id = hr >= 0 && raw != null ? new string(raw) : string.Empty;
        Native.CoTaskMemFree(raw); // CoTaskMemFree(NULL) is a no-op
        return hr;
    }

    /// <summary>
    /// PKEY_Device_FriendlyName of a device. A device without a name (VT_EMPTY) yields "".
    /// Tab, CR and LF are replaced with a space so a renamed endpoint cannot break the
    /// tab-separated, one-line-per-device output (same rule as ta.c).
    /// </summary>
    public static int GetFriendlyName(void* device, out string name)
    {
        name = string.Empty;
        void* store = null;
        int hr = MMDevice.OpenPropertyStore(device, AudioConst.STGM_READ, &store);
        if (hr < 0)
        {
            return hr;
        }

        try
        {
            PropertyKey key = new(AudioGuids.FMTID_Device, AudioGuids.PID_Device_FriendlyName);
            PropVariant value = default; // all zero bytes == PropVariantInit
            hr = PropertyStore.GetValue(store, &key, &value);
            if (hr >= 0 && value.vt == PropVariant.VT_LPWSTR && value.pwszVal != null)
            {
                name = SanitizeField(new string(value.pwszVal));
            }

            Native.PropVariantClear(&value); // frees pwszVal; harmless on VT_EMPTY
            return hr;
        }
        finally
        {
            Vtbl.Release(ref store);
        }
    }

    /// <summary>Returns <paramref name="text"/> with '\t', '\r' and '\n' replaced by ' '.</summary>
    private static string SanitizeField(string text)
    {
        if (text.AsSpan().IndexOfAny('\t', '\r', '\n') < 0)
        {
            return text; // the common case: no copy
        }

        return string.Create(text.Length, text, static (span, source) =>
        {
            for (int i = 0; i < span.Length; i++)
            {
                char c = source[i];
                span[i] = c is '\t' or '\r' or '\n' ? ' ' : c;
            }
        });
    }

    /// <summary>
    /// Id of the default render endpoint for <paramref name="role"/>, or null when there is no
    /// default device (E_NOTFOUND is reported as success with a null id).
    /// </summary>
    public static int TryGetDefaultId(void* enumerator, int role, out string? id)
    {
        id = null;
        void* device = null;
        int hr = MMDeviceEnumerator.GetDefaultAudioEndpoint(enumerator, AudioConst.eRender, role, &device);
        if (hr == Native.E_NOTFOUND)
        {
            return Native.S_OK;
        }

        if (hr < 0)
        {
            return hr;
        }

        try
        {
            hr = GetId(device, out string value);
            if (hr >= 0)
            {
                id = value;
            }

            return hr;
        }
        finally
        {
            Vtbl.Release(ref device);
        }
    }

    /// <summary>
    /// Creates the policy-config object used to change the default endpoint. Tries
    /// CPolicyConfigClient/IPolicyConfig first and falls back to the Vista variant.
    /// </summary>
    /// <param name="policyConfig">The created object; the caller releases it.</param>
    /// <param name="setDefaultSlot">Vtable slot of SetDefaultEndpoint on the returned object.</param>
    public static int CreatePolicyConfig(out void* policyConfig, out int setDefaultSlot)
    {
        void* p = null;
        Guid clsid = AudioGuids.CLSID_CPolicyConfigClient;
        Guid iid = AudioGuids.IID_IPolicyConfig;
        int hr = Native.CoCreateInstance(&clsid, null, Native.CLSCTX_INPROC_SERVER, &iid, &p);
        if (hr >= 0)
        {
            policyConfig = p;
            setDefaultSlot = PolicyConfig.SetDefaultEndpointSlot;
            return hr;
        }

        clsid = AudioGuids.CLSID_CPolicyConfigVistaClient;
        iid = AudioGuids.IID_IPolicyConfigVista;
        p = null;
        int hrVista = Native.CoCreateInstance(&clsid, null, Native.CLSCTX_INPROC_SERVER, &iid, &p);
        if (hrVista >= 0)
        {
            policyConfig = p;
            setDefaultSlot = PolicyConfig.SetDefaultEndpointSlotVista;
            return hrVista;
        }

        policyConfig = null;
        setDefaultSlot = 0;
        return hr; // report the primary failure, it is the interesting one
    }
}
