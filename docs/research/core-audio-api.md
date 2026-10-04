# Windows Core Audio API reference for toggle-audio

This is an implementer's reference for a tiny Windows tool that lists render endpoints and toggles the default playback device between two stored endpoints. It covers C (MSVC) and Rust (`windows` crate). It was researched and **verified on this PC** on 2026-10-04.

- **OS:** Windows 11 Pro for Workstations 10.0.26300.9550 (DisplayVersion "26H2").
- **Audio DLL:** `AudioSes.dll` 10.0.26100.8875.
- **Toolchains:** MSVC 14.44, Windows SDK 10.0.26100, Rust 1.90.0 and stable 1.99.0, `windows` 0.62.2.

Every claim marked **[verified]** was reproduced by a probe program under `research/probe`, `research/rsprobe` or `research/cuiprobe` (see section 10). No probe changed the default device. The only `SetDefaultEndpoint` calls made re-asserted each role's *current* default, and the follow-up check confirmed nothing changed.

---

## 0. TL;DR decisions

| Topic | Decision |
|---|---|
| Enumerate | `IMMDeviceEnumerator::EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)`. For each endpoint: `GetId`, then `OpenPropertyStore(STGM_READ)`, then read **`PKEY_Device_FriendlyName` {a45c254e-df1c-4efd-8020-67d146a850e0},14**. This is exactly the Sound-settings string, e.g. `喇叭 (FiiO BTA30 PRO)` [verified]. |
| Identity to store | Store the **endpoint ID string** (`{0.0.0.00000000}.{guid}`). Optionally also store the 24H2+ **StableId** (see A.7). Never key on the friendly name: on this PC three endpoints are named `PG42UQ (NVIDIA High Definition Audio)` [verified]. |
| Set default | Undocumented **`IPolicyConfig`**: IID `{f8679f50-850a-41cf-9c72-430f290290c8}` on CLSID **`CPolicyConfigClient` `{870af99c-171d-4f9e-af0d-e63df40c2bc9}`**. **`SetDefaultEndpoint` is vtable slot 13** (0-based, IUnknown = 0..2). This was verified against Microsoft public PDB symbols of AudioSes.dll on this build. |
| Fallback | `CPolicyConfigVistaClient` `{294935ce-f637-4e7c-a41b-ab255460b862}` → `IPolicyConfigVista` `{568b9108-44bf-40b4-9006-86afe5b5a620}`, `SetDefaultEndpoint` at **slot 12** [verified]. It is never needed on Windows 7 through 11, but the fallback costs about 10 lines. |
| Roles | Call `SetDefaultEndpoint` for **eConsole(0), eMultimedia(1), eCommunications(2)**, in that order (SoundSwitch's order). Skip any role whose current default is already the target. Make "also switch communications" a config option that defaults to on, because that mimics `Set-AudioDevice`. |
| Cost | In-process cost (measured): CoInit + enumerator about 3–4 ms; `GetDefaultAudioEndpoint` about 1.3 ms per role; **`SetDefaultEndpoint` about 5–10 ms per role**. Total toggle work is about 25–45 ms. A bare process start costs about 4–5 ms. Language choice is not the bottleneck. |
| Apartment | STA or MTA both work. The CLSIDs are registered `ThreadingModel=Both`, so there is no marshaling and latency is identical [verified]. Use `COINIT_APARTMENTTHREADED` (needed for the GUI anyway). |
| Rust crates | Pin **`windows = "=0.62.2"`** and **`windows-core = "=0.62.2"`** (MSRV 1.82). **Do not** let `windows-core` float to 0.100.0, released 2026-09-03 without a matching `windows` crate. |
| Console | **Preferred (24H2+):** build as **console subsystem** and embed a manifest with `<consoleAllocationPolicy>detached</consoleAllocationPolicy>`. A G HUB or Explorer launch then gets *no* console window [verified], and shells wait and redirect normally. **Fallback:** GUI subsystem plus `AttachConsole(ATTACH_PARENT_PROCESS)`. That has known quirks: PowerShell does not capture `>` or `$x = &` output from GUI-subsystem exes [verified]. |
| CRT | Use `/MT` (C) and `-C target-feature=+crt-static` (Rust) so no VC++ redist is needed. Going CRT-less saves only about 0.6–1 ms [verified] and is not worth the effort. |

---

## A. Enumerating render endpoints

### A.1 Call sequence (all HRESULTs must be checked)

```
CoInitializeEx(NULL, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE)
    -> S_OK, or S_FALSE (already initialised on this thread; still pair with CoUninitialize)
    -> RPC_E_CHANGED_MODE (thread already in the other apartment; do NOT call CoUninitialize, just proceed)
CoCreateInstance(CLSID_MMDeviceEnumerator, NULL, CLSCTX_ALL, IID_IMMDeviceEnumerator, &en)
en->EnumAudioEndpoints(eRender /*0*/, DEVICE_STATE_ACTIVE /*1*/, &col)
col->GetCount(&n)
for i in 0..n:
    col->Item(i, &dev)
    dev->GetId(&pwszId)                      // LPWSTR allocated with CoTaskMemAlloc -> CoTaskMemFree(pwszId)
    dev->GetState(&state)                    // optional; ACTIVE here by construction
    dev->OpenPropertyStore(STGM_READ /*0*/, &ps)
    PropVariantInit(&pv); ps->GetValue(PKEY_Device_FriendlyName, &pv)
    if pv.vt == VT_LPWSTR (31): name = pv.pwszVal
    PropVariantClear(&pv)                    // ole32; frees pwszVal
    ps->Release(); dev->Release()
col->Release(); en->Release(); CoUninitialize()
```

### A.2 GUIDs and constants

| Name | Value |
|---|---|
| `CLSID_MMDeviceEnumerator` | `{BCDE0395-E52F-467C-8E3D-C4579291692E}` (MMDevApi.dll, ThreadingModel=both) |
| `IID_IMMDeviceEnumerator` | `{A95664D2-9614-4F35-A746-DE8DB63617E6}` |
| `IID_IMMDevice` | `{D666063F-1587-4E43-81F1-B948E807363F}` |
| `EDataFlow` | `eRender=0, eCapture=1, eAll=2` |
| `ERole` | `eConsole=0, eMultimedia=1, eCommunications=2, ERole_enum_count=3` |
| `DEVICE_STATE_*` | `ACTIVE=0x1, DISABLED=0x2, NOTPRESENT=0x4, UNPLUGGED=0x8, DEVICE_STATEMASK_ALL=0xF` |
| `STGM_READ` | `0` |
| `E_NOTFOUND` | `HRESULT_FROM_WIN32(ERROR_NOT_FOUND)` = **`0x80070490`** |
| `E_INVALIDARG` | `0x80070057`, returned by `GetDevice("garbage")` [verified] |
| `E_NOINTERFACE` | `0x80004002` |

**C pitfall [verified]:** in C (not C++), `mmdeviceapi.h` only *declares* `CLSID_MMDeviceEnumerator` and `IID_IMMDeviceEnumerator` (`EXTERN_C const IID ...`). **No SDK .lib defines them**, so the link fails with `LNK2019 unresolved external symbol IID_IMMDeviceEnumerator`. Define them yourself after `#include <initguid.h>` (see D.2). C++ code should use `__uuidof(MMDeviceEnumerator)` / `__uuidof(IMMDeviceEnumerator)` instead.

### A.3 Which name key matches Windows Sound settings [verified on this PC]

| Key | fmtid, pid | Value for BTA30 | Value for PG42UQ |
|---|---|---|---|
| **`PKEY_Device_FriendlyName`** | `{a45c254e-df1c-4efd-8020-67d146a850e0}`, **14** | **`喇叭 (FiiO BTA30 PRO)`** | `PG42UQ (NVIDIA High Definition Audio)` |
| `PKEY_Device_DeviceDesc` | `{a45c254e-df1c-4efd-8020-67d146a850e0}`, 2 | `喇叭` | `PG42UQ` |
| `PKEY_DeviceInterface_FriendlyName` | `{026e516e-b814-414b-83cd-856d6fef4822}`, 2 | `FiiO BTA30 PRO` | `NVIDIA High Definition Audio` |

- `PKEY_Device_FriendlyName` = `DeviceDesc + " (" + InterfaceFriendlyName + ")"`. This is the string the Sound settings page, the volume flyout and AudioDeviceCmdlets' `.Name` show.
- The **DeviceDesc** part is user-renamable (Sound settings → device properties → Rename) and **localized** (`喇叭` = "Speakers" in zh-TW). Display FriendlyName in the GUI. Never use it as identity.
- AudioDeviceCmdlets' `PKEY.cs` mislabels `{a45c254e…},14` as `PKEY_DeviceInterface_FriendlyName`. The SDK header `functiondiscoverykeys_devpkey.h` (10.0.26100, lines 53/62/210) is authoritative and matches the table above.
- Duplicates are real. This PC has **3** render endpoints named `PG42UQ (NVIDIA High Definition Audio)` (one ACTIVE, two NOTPRESENT) and 7 named `NVIDIA Output (…)` [verified, `EnumAudioEndpoints(eRender, DEVICE_STATEMASK_ALL)` returns 32 endpoints; 5 are ACTIVE]. Store IDs.

### A.4 Current defaults

`IMMDeviceEnumerator::GetDefaultAudioEndpoint(eRender, role, &dev)` returns the default for that role. It returns `E_NOTFOUND` if there is no render device at all. On this PC all three roles currently return `{0.0.0.00000000}.{739b3554-bfed-4d61-b407-a818b317c991}` (PG42UQ). It costs about **1.3 ms per call** [verified], so query only what you need. For the toggle decision, use **eMultimedia**, because that is what AudioDeviceCmdlets' `Get-AudioDevice -Playback` reads and what the POC compared against. Alternatively, use eConsole; Windows Sound settings sets both together.

### A.5 Resolving a stored ID: `IMMDeviceEnumerator::GetDevice(id)` [verified]

| Situation | Result |
|---|---|
| ID of a present, enabled endpoint | `S_OK`, `GetState()` = `DEVICE_STATE_ACTIVE` (1) |
| ID of a known endpoint whose hardware is gone (`NOTPRESENT`), and likewise `DISABLED`/`UNPLUGGED` | **`S_OK`**, with `GetState()` = 0x4, 0x2 or 0x8. **GetDevice does not fail.** Check the state yourself. |
| Well-formed ID that the system has never seen, or that was removed by driver uninstall | **`E_NOTFOUND` 0x80070490** |
| Malformed string | `E_INVALIDARG` 0x80070057 |
| 24H2+: a `PKEY_AudioEndpoint_StableId` string | `S_OK`, same endpoint as the ordinary ID (A.7) |

`GetDevice` is cheap (about 0.04 ms). The toggle must call `GetState()` and require `DEVICE_STATE_ACTIVE` before calling `SetDefaultEndpoint`. When the BTA30 is unplugged or powered off, its USB endpoint becomes NOTPRESENT or UNPLUGGED. `SetDefaultEndpoint` on an unknown ID returns `0x80070490` [verified]. **Never** call it on a non-ACTIVE endpoint; that behaviour is untested and risks a "no audio" default.

### A.6 Default endpoint ID format

`{0.0.0.00000000}.{<endpoint guid>}` for render endpoints (`{0.0.1.00000000}.{…}` for capture). The docs call it opaque; treat it as such. Microsoft notes that driver or OS updates can change it (see A.7). The BTA30 is a **USB Audio Class 2** device to Windows (`USB\VID_2972&PID_0047`, driver `usbaudio2.inf`) [verified]. The Bluetooth link is inside the dongle, so Windows sees no A2DP/HFP split and no Bluetooth reconnection churn. Its endpoint ID is stable across replugs on the same USB port path.

### A.7 Windows 11 24H2+: `PKEY_AudioEndpoint_StableId` (new, optional)

- Learn (updated 2026-08-27): <https://learn.microsoft.com/en-us/windows/win32/coreaudio/pkey-audioendpoint-stableid>. It says "The ordinary endpoint ID that is returned by IMMDevice::GetId is not stable. An operating system update or an audio driver update can cause the same physical peripheral to be assigned a different endpoint ID." Since 24H2 (build 26100), **`GetDevice` also accepts a StableId string**.
- The value is `VT_LPWSTR` or `VT_EMPTY`. Treat it as opaque and case-sensitive.
- **The SDK 10.0.26100 header and `windows` 0.62.2 do not define this key yet.** Dumping all property keys on this PC showed one string property per endpoint at **`{1da5d803-d492-4edd-8c23-e0c0ffee7f0e}`, pid 12**, with a value like `{0.0.0.00000000}.{001.{19DD8A43-B8DD-4C72-B721-7B506462F9FA}}`. Passing that string to `GetDevice` resolved to the matching endpoint for all 5 active endpoints [verified]. **Inference (not yet confirmed by a header): PKEY_AudioEndpoint_StableId = {1da5d803-…},12** (same fmtid as all other `PKEY_AudioEndpoint_*`, next free pid after `Min_VolumeInDb`=11).
- Recommendation: in config, store `id` (required) and `stable_id` (optional). At toggle time try `GetDevice(id)`. On `E_NOTFOUND`, try `GetDevice(stable_id)` and, if that succeeds, re-save the new `id`. Always pass the **ordinary ID (from `GetId`)** to `SetDefaultEndpoint`; nobody has verified that it accepts StableIds.

---

## B. Setting the default device: the IPolicyConfig family (undocumented)

There is **no documented Win32/WinRT API** to set the system default endpoint. Every switcher (SoundSwitch, EarTrumpet, AudioDeviceCmdlets, NirCmd, Sunshine) uses `IPolicyConfig` from `AudioSes.dll`.

### B.1 All known GUIDs

| Name | GUID | Status on 10.0.26300 [verified QI matrix] |
|---|---|---|
| CLSID `CPolicyConfigClient` | `{870af99c-171d-4f9e-af0d-e63df40c2bc9}` | Creates OK (AudioSes.dll, ThreadingModel=Both, about 0.9 ms) |
| CLSID `CPolicyConfigVistaClient` | `{294935ce-f637-4e7c-a41b-ab255460b862}` | Creates OK (AudioSes.dll, Both, about 0.25 ms) |
| IID `IPolicyConfig` (Win7, Win8, Win10 RS1+, Win11) | `{f8679f50-850a-41cf-9c72-430f290290c8}` | **S_OK on CPolicyConfigClient**; E_NOINTERFACE on the Vista client |
| IID `IPolicyConfigVista` | `{568b9108-44bf-40b4-9006-86afe5b5a620}` | **S_OK on CPolicyConfigVistaClient**; E_NOINTERFACE on CPolicyConfigClient |
| IID "IPolicyConfig10" / W10 TH1 (1507) | `{ca286fc3-91fd-42c3-8e9b-caafa66242e3}` | E_NOINTERFACE on both |
| IID W10 TH2 (1511) | `{6be54be8-a068-4875-a49d-0c2966473b11}` | (per EarTrumpet; not present since RS1) |
| IID "IPolicyConfigX" (SoundSwitch) | `{8f9fb2aa-1c0b-4d54-b6bb-b2f2a10ce03c}` | E_NOINTERFACE on both |

Corrections to the task brief:

1. `{294935CE-F637-4E7C-A41B-AB255460B862}` is **`CPolicyConfigVistaClient`**, not an "IPolicyConfig10" CLSID. Sources: DefSound/AudioEndPointLibrary `PolicyConfig.h`, and the probe, where QI for IPolicyConfigVista succeeds only on it.
2. `{ca286fc3-…}` was the IID used only by Windows 10 1507 (TH1). EarTrumpet's `IPolicyConfig.cs` comments: `W10_TH1: CA286FC3-…`, `W10_TH2: 6BE54BE8-…`, `Win7-Win8, W10_RS1-Present: F8679F50-…`. Since 1607 the original `IPolicyConfig` IID is back, and on Windows 11 26300 `ca286fc3` is not implemented.
3. AudioDeviceCmdlets' "IPolicyConfig10" class is declared with **`IID_IUnknown` `{00000000-0000-0000-C000-000000000046}`**. That is a hack: it QIs for IUnknown, which always succeeds, then calls slot 13. It is only reached if both IPolicyConfig and IPolicyConfigVista QIs fail.

### B.2 Exact vtable layouts

These were **verified by resolving every vtable slot to a public PDB symbol** with `dbghelp`/`symsrv` against `msdl.microsoft.com` (`probe/vtdump.c` → `probe/vtdump-out.txt`).

**`IPolicyConfig` on `CPolicyConfigClient`** (AudioSes.dll 10.0.26100.8875):

| Slot | Method (PDB symbol `CPolicyConfigClient::…`) | Signature used by implementers |
|---|---|---|
| 0 | QueryInterface | `HRESULT (REFIID, void**)` |
| 1 | AddRef | `ULONG ()` |
| 2 | Release | `ULONG ()` |
| 3 | GetMixFormat | `HRESULT (PCWSTR id, WAVEFORMATEX **ppFormat)` |
| 4 | GetDeviceFormat | `HRESULT (PCWSTR id, BOOL bDefault, WAVEFORMATEX **ppFormat)` |
| 5 | ResetDeviceFormat | `HRESULT (PCWSTR id)` |
| 6 | SetDeviceFormat | `HRESULT (PCWSTR id, WAVEFORMATEX *pEndpointFormat, WAVEFORMATEX *pMixFormat)` |
| 7 | GetProcessingPeriod | `HRESULT (PCWSTR id, BOOL bDefault, INT64 *pDefault, INT64 *pMin)` |
| 8 | SetProcessingPeriod | `HRESULT (PCWSTR id, INT64 *pPeriod)` |
| 9 | GetShareMode | `HRESULT (PCWSTR id, struct DeviceShareMode *pMode)` |
| 10 | SetShareMode | `HRESULT (PCWSTR id, struct DeviceShareMode *pMode)` |
| 11 | GetPropertyValue | `HRESULT (PCWSTR id, BOOL bFxStore, const PROPERTYKEY *key, PROPVARIANT *pv)` † |
| 12 | SetPropertyValue | `HRESULT (PCWSTR id, BOOL bFxStore, const PROPERTYKEY *key, PROPVARIANT *pv)` † |
| **13** | **SetDefaultEndpoint** | **`HRESULT (PCWSTR wszDeviceId, ERole eRole)`** |
| 14 | SetEndpointVisibility | `HRESULT (PCWSTR id, BOOL bVisible)` |

Slots 15–18 hold a second interface on the same object (QI/AddRef/Release thunks plus `CPolicyConfigClient::AccessChanged`). They are not part of IPolicyConfig and must not be declared.

† Sources disagree on the GetPropertyValue/SetPropertyValue parameters. DefSound and Sunshine declare 3 parameters (no `bFxStore`); SoundSwitch, AudioDeviceCmdlets and EarTrumpet declare 4. **This does not matter**: we never call them, and the parameter count does not change the slot index. Declare them as opaque placeholders.

**`IPolicyConfigVista` on `CPolicyConfigVistaClient`**:

| Slot | Method |
|---|---|
| 0–2 | IUnknown |
| 3 | GetMixFormat |
| 4 | GetDeviceFormat |
| 5 | SetDeviceFormat |
| 6 | GetProcessingPeriod (stub on Win7+) |
| 7 | SetProcessingPeriod (stub) |
| 8 | GetShareMode (stub) |
| 9 | SetShareMode (stub) |
| 10 | GetPropertyValue |
| 11 | SetPropertyValue |
| **12** | **SetDefaultEndpoint** `HRESULT (PCWSTR, ERole)` |
| 13 | SetEndpointVisibility (stub) |

Slots 6–9 and 13 all point to one shared not-implemented stub. **There is no `ResetDeviceFormat` in the Vista interface.** DefSound's `IPolicyConfigVista` declaration is correct (SetDefaultEndpoint = 12). **AudioDeviceCmdlets' and SoundSwitch's `IPolicyConfigVista` declarations wrongly include ResetDeviceFormat**, which puts their SetDefaultEndpoint at slot 13, the stub. They never hit this because the IPolicyConfig QI succeeds first.

### B.3 Cross-source agreement for `IPolicyConfig` slot 13

| Source | Declaration | SetDefaultEndpoint slot |
|---|---|---|
| Microsoft PDB symbols (this PC) | `CPolicyConfigClient::SetDefaultEndpoint` at vtbl[13] | **13** |
| DefSound/AudioEndPointLibrary `DefSound/PolicyConfig.h` (EreTIk) | 3–14 as above | 13 |
| LizardByte/Sunshine `src/platform/windows/PolicyConfig.h` (same EreTIk origin) | same | 13 |
| Belphemur/SoundSwitch `SoundSwitch.Audio.Manager/Interop/Interface/Policy/IPolicyConfig.cs` | GetMixFormat…SetEndpointVisibility | 13 |
| frgnca/AudioDeviceCmdlets `SOURCE/IPolicyConfig.cs` | same | 13 |
| File-New-Project/EarTrumpet `Interop/MMDeviceAPI/IPolicyConfig.cs` | `Unused1..Unused8`, GetPropertyValue, SetPropertyValue, SetDefaultEndpoint | 3+8+2 = 13 |

### B.4 What the real tools do (fetched 2026-10-04)

- **SoundSwitch** (`dev` branch, `Interop/Client/PolicyClient.cs`):
  1. Creates `CPolicyConfigClient` once and casts it to `IPolicyConfigX`, then `IPolicyConfig`, then `IPolicyConfigVista`. It uses the first that is non-null. On Win11 that is `IPolicyConfig`, because IPolicyConfigX QI fails [verified].
  2. `AudioSwitcher.SwitchTo(id, ERole_enum_count)` calls `SwitchTo` for **eConsole, then eMultimedia, then eCommunications**. Each call is skipped when the device is already default for that role (`IsDefault` check). All calls run on a dedicated COM thread (`ComThread.Invoke`).
  3. It maps `0x80070490` to `DeviceNotFoundException`.
  4. Separately, it optionally re-routes the *foreground app* with the per-app `IAudioPolicyConfigFactory` (WinRT, `Windows.Media.Internal.AudioPolicyConfig`), which is not needed here.
- **AudioDeviceCmdlets** `Set-AudioDevice -ID x` (what the POC calls): `client.SetDefaultEndpoint(id, eCommunications)` then `client.SetDefaultEndpoint(id, eMultimedia)`. It **never sets eConsole explicitly**. `-DefaultOnly` skips communications; `-CommunicationOnly` skips multimedia. PolicyConfigClient order: IPolicyConfig → IPolicyConfigVista (buggy layout, never reached) → "IPolicyConfig10" (IUnknown hack).
- **EarTrumpet**: casts `CPolicyConfigClient` to `IPolicyConfigWin7` (= IPolicyConfig IID), calls `SetDefaultEndpoint(id, role)` per role the user picks, and swallows exceptions ("Racing with the system, the device may not be valid anymore").

### B.5 Roles: what to set and in what order

- The three roles are independent slots: "At any time, each role in the table is assigned to one (and only one) rendering device" ([Device Roles](https://learn.microsoft.com/en-us/windows/win32/coreaudio/device-roles)). The Sound settings "Set as default" assigns console and multimedia together; "Set as default communication device" assigns communications.
- **Recommendation:** set eConsole, eMultimedia and eCommunications, in that order. Skip roles that already equal the target, which saves about 5–10 ms each and avoids redundant `OnDefaultDeviceChanged` notifications. Expose `switch_communications = true|false`; the user might want to keep a headset or Yeti as the communications device.
- **Order does not matter functionally.** Each call is independent and synchronous; when it returns S_OK the role is switched. Doing console/multimedia first makes the audible switch happen a few ms earlier.
- Whether setting only eMultimedia also moves eConsole (AudioDeviceCmdlets behaviour) was **not tested**, because testing it would have changed the device. Setting all three explicitly avoids the question.

### B.6 Latency [verified, no-op re-assert of current default, 3 runs, STA and MTA]

| Call | Time |
|---|---|
| `CoInitializeEx` | 1.1–2.4 ms |
| `CoCreateInstance(MMDeviceEnumerator)` | 2.0–2.7 ms |
| `EnumAudioEndpoints(ACTIVE)` + 5 friendly names | 1.0–1.8 ms |
| `GetDefaultAudioEndpoint` | 1.25–3.7 ms each |
| `GetDevice(id)` | 0.04 ms |
| `CoCreateInstance(CPolicyConfigClient)` | 0.9–1.1 ms |
| **`SetDefaultEndpoint`** | **4.9–12 ms per role**, even when it is a no-op. Expect similar or slightly more for a real change; the audio engine reroutes asynchronously after it returns. |
| `CoUninitialize` | 0.4–0.8 ms |
| Whole Rust probe incl. 3× SetDefaultEndpoint (in-process) | 35.7 ms |
| Process start/exit of a trivial exe | ~4–5 ms |

STA versus MTA made no difference (both CLSIDs are `ThreadingModel=Both`, i.e. in-proc, no proxy). Expect a full toggle exe at roughly **30–50 ms wall** versus about 900 ms for the ps2exe POC. To shave time: skip `EnumAudioEndpoints` in toggle mode (use `GetDevice` on the two stored IDs), read only one default role to decide, and skip roles already set.

### B.7 Toggle algorithm (reference)

```
load config {dev1:{id, stable_id?, name}, dev2:{...}, switch_communications:bool}
CoInitializeEx(STA)
en = CoCreateInstance(MMDeviceEnumerator)
cur = en.GetDefaultAudioEndpoint(eRender, eMultimedia).GetId()     // E_NOTFOUND => cur = ""
target = (cur == dev1.id) ? dev2 : dev1                            // "neither" -> dev1 (POC behaviour)
d = resolve(target)   // GetDevice(id) || GetDevice(stable_id) ; require GetState()==ACTIVE
if !d: try the other device if it is ACTIVE and not current; else report error (exit code 2, optional toast/beep)
pc = CoCreateInstance(CPolicyConfigClient, IID_IPolicyConfig) || CoCreateInstance(CPolicyConfigVistaClient, IID_IPolicyConfigVista)
for role in [eConsole, eMultimedia] + ([eCommunications] if switch_communications):
    if en.GetDefaultAudioEndpoint(eRender, role).GetId() != d.id: hr = pc.SetDefaultEndpoint(d.id, role); check hr
release; CoUninitialize; exit 0
```

---

## C. Rust (`windows` crate)

### C.1 Versions (checked 2026-10-04 against the crates.io index and API)

| Crate | Latest | Published | Notes |
|---|---|---|---|
| `windows` | **0.62.2** | 2025-10-06 | MSRV 1.82. Still the latest umbrella crate. |
| `windows-core` | 0.100.0 / **0.62.2** | 2026-09-03 / 2025-10-06 | 0.100.0 is release 74 ("Rust for Windows 0.100.0", [issue #4867](https://github.com/microsoft/windows-rs/issues/4867)). It has breaking changes and new lower-case header-based modules, and says *"We may initially publish the release without `windows` and `windows-sys`."* No `windows` 0.100 exists yet. |
| `windows-interface` / `windows-implement` | 0.59.3 / 0.60.2 (for 0.62) | | pulled in by windows-core 0.62.2 |

**Pin exactly:**

```toml
[dependencies]
windows-core = "=0.62.2"          # MUST be a direct dependency: #[interface] expands to ::windows_core::... paths
[dependencies.windows]
version  = "=0.62.2"
features = [
  "Win32_Foundation",                    # PROPERTYKEY, HANDLE, HWND
  "Win32_Media_Audio",                   # IMMDeviceEnumerator, MMDeviceEnumerator, eRender, ERole, DEVICE_STATE_*
  "Win32_System_Com",                    # CoInitializeEx, CoCreateInstance, CoTaskMemFree, STGM_READ, CLSCTX_ALL
  "Win32_System_Com_StructuredStorage",  # PROPVARIANT, PropVariantClear, PropVariantToStringAlloc
  "Win32_System_Variant",                # VT_LPWSTR; ALSO required for PROPVARIANT/IPropertyStore::GetValue to exist
  "Win32_UI_Shell_PropertiesSystem",     # IPropertyStore (OpenPropertyStore is cfg-gated on it)
  "Win32_Devices_FunctionDiscovery",     # PKEY_Device_FriendlyName (typed as Foundation::PROPERTYKEY)
  "Win32_System_Console",                # AttachConsole, GetStdHandle, WriteConsoleW, GetConsoleMode (only if needed)
  "Win32_UI_WindowsAndMessaging",        # MessageBoxW (error reporting from GUI launch)
  # "Win32_Storage_FileSystem", "Win32_System_IO"   # only if you call WriteFile yourself (WriteFile is gated on System_IO)
]
```

If `windows-core` resolves to 0.100 while `windows` is 0.62, the `#[interface]` expansion uses `::windows_core` 0.100 types and won't match `windows::core::IUnknown` from 0.62, which causes type errors. Hence `=` pins. Also commit `Cargo.lock`.

Release profile used for the probe: `opt-level=3, lto=true, codegen-units=1, panic="abort", strip=true`. That gives a 146 KB exe with enumeration, IPolicyConfig and console code. For no VC++ redist dependency, add `.cargo/config.toml`:

```toml
[target.x86_64-pc-windows-msvc]
rustflags = ["-C", "target-feature=+crt-static"]
```

(Rust's MSVC target links vcruntime dynamically by default.)

### C.2 Verified, compiling code (`research/rsprobe/src/main.rs`, built with windows 0.62.2 on Rust 1.90)

```rust
#![windows_subsystem = "windows"]          // omit if using the console-subsystem + detached-manifest approach (E.2)
#![allow(non_snake_case, non_camel_case_types)]

use std::ffi::c_void;
use windows::core::{GUID, HRESULT, PCWSTR, PWSTR, HSTRING};
use windows_core::{interface, IUnknown, IUnknown_Vtbl};   // parent vtable type MUST be in scope unqualified
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::{
    eCommunications, eConsole, eMultimedia, eRender, ERole, IMMDevice, IMMDeviceEnumerator,
    MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToStringAlloc};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, STGM_READ,
};

pub const CLSID_POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

// Undocumented. Slots verified against AudioSes.dll PDB (10.0.26100.8875). Unused methods are
// declared with opaque pointer params purely to keep the vtable layout; never call them.
#[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
pub unsafe trait IPolicyConfig: IUnknown {
    fn GetMixFormat(&self, id: PCWSTR, fmt: *mut *mut c_void) -> HRESULT;                          // 3
    fn GetDeviceFormat(&self, id: PCWSTR, default: i32, fmt: *mut *mut c_void) -> HRESULT;         // 4
    fn ResetDeviceFormat(&self, id: PCWSTR) -> HRESULT;                                             // 5
    fn SetDeviceFormat(&self, id: PCWSTR, ep: *mut c_void, mix: *mut c_void) -> HRESULT;           // 6
    fn GetProcessingPeriod(&self, id: PCWSTR, default: i32, d: *mut i64, m: *mut i64) -> HRESULT;  // 7
    fn SetProcessingPeriod(&self, id: PCWSTR, p: *mut i64) -> HRESULT;                             // 8
    fn GetShareMode(&self, id: PCWSTR, mode: *mut c_void) -> HRESULT;                              // 9
    fn SetShareMode(&self, id: PCWSTR, mode: *mut c_void) -> HRESULT;                              // 10
    fn GetPropertyValue(&self, id: PCWSTR, fx: i32, key: *const PROPERTYKEY, pv: *mut c_void) -> HRESULT; // 11
    fn SetPropertyValue(&self, id: PCWSTR, fx: i32, key: *const PROPERTYKEY, pv: *mut c_void) -> HRESULT; // 12
    fn SetDefaultEndpoint(&self, id: PCWSTR, role: ERole) -> HRESULT;                              // 13
    fn SetEndpointVisibility(&self, id: PCWSTR, visible: i32) -> HRESULT;                          // 14
}
// Optional fallback (CLSID 294935ce-f637-4e7c-a41b-ab255460b862):
// #[interface("568b9108-44bf-40b4-9006-86afe5b5a620")] unsafe trait IPolicyConfigVista: IUnknown {
//   GetMixFormat, GetDeviceFormat, SetDeviceFormat, GetProcessingPeriod, SetProcessingPeriod,
//   GetShareMode, SetShareMode, GetPropertyValue, SetPropertyValue, SetDefaultEndpoint /*12*/, SetEndpointVisibility }

unsafe fn device_id(dev: &IMMDevice) -> windows::core::Result<String> {
    let p: PWSTR = dev.GetId()?;                       // CoTaskMemAlloc'd
    let s = p.to_string().unwrap_or_default();        // PWSTR::to_string -> Result<String, FromUtf16Error>
    CoTaskMemFree(Some(p.0 as *const c_void));         // free it!
    Ok(s)
}

unsafe fn friendly_name(dev: &IMMDevice) -> windows::core::Result<String> {
    let store = dev.OpenPropertyStore(STGM_READ)?;
    let mut pv = store.GetValue(&PKEY_Device_FriendlyName)?;   // raw PROPVARIANT: no Drop in 0.62 -> must clear
    // Option A (propsys.dll): converts any string-convertible VT.
    let r = PropVariantToStringAlloc(&pv).map(|p| {
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as *const c_void));
        s
    });
    let _ = PropVariantClear(&mut pv);
    r
}

unsafe fn friendly_name_direct(dev: &IMMDevice) -> windows::core::Result<String> {   // Option B: no propsys
    use windows::Win32::System::Variant::VT_LPWSTR;
    let store = dev.OpenPropertyStore(STGM_READ)?;
    let mut pv = store.GetValue(&PKEY_Device_FriendlyName)?;
    let s = if pv.Anonymous.Anonymous.vt == VT_LPWSTR {
        pv.Anonymous.Anonymous.Anonymous.pwszVal.to_string().unwrap_or_default()
    } else { String::new() };
    PropVariantClear(&mut pv)?;
    Ok(s)
}

fn main() -> windows::core::Result<()> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE); // returns HRESULT (S_FALSE ok)
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let col = en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
        for i in 0..col.GetCount()? {
            let d = col.Item(i)?;
            let _ = (device_id(&d)?, friendly_name(&d)?);
        }
        // stored-id lookup: &HSTRING implements Param<PCWSTR>
        let dev = en.GetDevice(&HSTRING::from("{0.0.0.00000000}.{30045f40-8cfd-4441-bb89-0d13fc19b589}"))?; // Err(0x80070490) if unknown
        if dev.GetState()? == DEVICE_STATE_ACTIVE {
            let id = HSTRING::from(device_id(&dev)?.as_str());
            let pc: IPolicyConfig = CoCreateInstance(&CLSID_POLICY_CONFIG_CLIENT, None, CLSCTX_ALL)?;
            for role in [eConsole, eMultimedia, eCommunications] {
                pc.SetDefaultEndpoint(PCWSTR(id.as_ptr()), role).ok()?;     // HRESULT -> Result
            }
        }
        // drop COM pointers BEFORE CoUninitialize
    }
    unsafe { CoUninitialize() };
    Ok(())
}
```

Notes verified against the 0.62.2 sources in `~/.cargo/registry`:

- `CoInitializeEx` returns `HRESULT` (not `Result`). `CoCreateInstance<P1, T: Interface>(rclsid, punkouter, dwclscontext) -> Result<T>` QIs straight to `T::IID`, so the `IPolicyConfig` type parameter selects the IID.
- `IMMDevice::GetId() -> Result<PWSTR>` (caller frees). `OpenPropertyStore(STGM) -> Result<IPropertyStore>` is cfg-gated on `Win32_System_Com` + `Win32_UI_Shell_PropertiesSystem`.
- `IPropertyStore::GetValue(*const PROPERTYKEY) -> Result<PROPVARIANT>`. `PROPVARIANT` lives in `Win32::System::Com::StructuredStorage`, needs `Win32_System_Variant`, and **has no Drop**. Call `PropVariantClear`.
- `PKEY_Device_FriendlyName: Foundation::PROPERTYKEY` is in `Win32::Devices::FunctionDiscovery`.
- `PropVariantToStringAlloc(*const PROPVARIANT) -> Result<PWSTR>` links propsys.dll. Option B avoids propsys entirely.
- The `#[interface]` macro (windows-interface 0.59.3) generates `::windows_core::Interface` impls and a vtable struct `IPolicyConfig_Vtbl { base__: IUnknown_Vtbl, … }`. The parent's `IUnknown_Vtbl` is referenced **unqualified**, so import it. Methods returning `HRESULT` are generated as `unsafe fn(&self, …) -> HRESULT`. Call `.ok()` to get `Result<()>`.
- Methods in the trait may use any FFI-safe types. Placeholder methods only need the right *count* of slots; parameter types are irrelevant because they are never called.
- `windows::core::w!("…")` gives a `PCWSTR` literal. `HSTRING::as_ptr()` gives a NUL-terminated `*const u16` valid while the HSTRING lives.

---

## D. C / C++ (MSVC)

### D.1 Headers and libs

| Need | Header | Lib |
|---|---|---|
| COM init, CoCreateInstance, CoTaskMemFree, PropVariantClear | `<objbase.h>` (via windows.h), `<propidl.h>` | `ole32.lib` |
| IMMDeviceEnumerator, IMMDevice, ERole, DEVICE_STATE_* | `<mmdeviceapi.h>` | none (define GUIDs yourself in C) |
| PKEY_Device_FriendlyName etc. | `<functiondiscoverykeys_devpkey.h>` (include `<initguid.h>` **before** it in exactly one TU, or define the key yourself) | none |
| WAVEFORMATEX (only for full IPolicyConfig prototypes) | `<mmreg.h>` | none |
| PropVariantToStringAlloc (optional) | `<propvarutil.h>` | `propsys.lib` (not needed if you read `pv.pwszVal` when `pv.vt == VT_LPWSTR`) |
| MessageBoxW | windows.h | `user32.lib` |
| AttachConsole / WriteConsoleW | windows.h | `kernel32.lib` (default) |

### D.2 Plain C declaration (compiled and run by the probes)

```c
#define COBJMACROS
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <initguid.h>                     // makes DEFINE_GUID/DEFINE_PROPERTYKEY *define* in this TU
#include <mmdeviceapi.h>
#include <functiondiscoverykeys_devpkey.h>

DEFINE_GUID(CLSID_MMDeviceEnumerator,  0xbcde0395,0xe52f,0x467c,0x8e,0x3d,0xc4,0x57,0x92,0x91,0x69,0x2e);
DEFINE_GUID(IID_IMMDeviceEnumerator,   0xa95664d2,0x9614,0x4f35,0xa7,0x46,0xde,0x8d,0xb6,0x36,0x17,0xe6);
DEFINE_GUID(CLSID_CPolicyConfigClient, 0x870af99c,0x171d,0x4f9e,0xaf,0x0d,0xe6,0x3d,0xf4,0x0c,0x2b,0xc9);
DEFINE_GUID(IID_IPolicyConfig,         0xf8679f50,0x850a,0x41cf,0x9c,0x72,0x43,0x0f,0x29,0x02,0x90,0xc8);
DEFINE_GUID(CLSID_CPolicyConfigVistaClient, 0x294935ce,0xf637,0x4e7c,0xa4,0x1b,0xab,0x25,0x54,0x60,0xb8,0x62);
DEFINE_GUID(IID_IPolicyConfigVista,    0x568b9108,0x44bf,0x40b4,0x90,0x06,0x86,0xaf,0xe5,0xb5,0xa6,0x20);

typedef struct IPolicyConfig IPolicyConfig;
typedef struct IPolicyConfigVtbl {
    HRESULT (STDMETHODCALLTYPE *QueryInterface)(IPolicyConfig*, REFIID, void**);   /* 0 */
    ULONG   (STDMETHODCALLTYPE *AddRef)(IPolicyConfig*);                            /* 1 */
    ULONG   (STDMETHODCALLTYPE *Release)(IPolicyConfig*);                           /* 2 */
    void *GetMixFormat, *GetDeviceFormat, *ResetDeviceFormat, *SetDeviceFormat,     /* 3..6  */
         *GetProcessingPeriod, *SetProcessingPeriod, *GetShareMode, *SetShareMode,  /* 7..10 */
         *GetPropertyValue, *SetPropertyValue;                                      /* 11..12 */
    HRESULT (STDMETHODCALLTYPE *SetDefaultEndpoint)(IPolicyConfig*, LPCWSTR, ERole); /* 13 */
    void *SetEndpointVisibility;                                                    /* 14 */
} IPolicyConfigVtbl;
struct IPolicyConfig { const IPolicyConfigVtbl *lpVtbl; };

typedef struct IPolicyConfigVista IPolicyConfigVista;
typedef struct IPolicyConfigVistaVtbl {
    HRESULT (STDMETHODCALLTYPE *QueryInterface)(IPolicyConfigVista*, REFIID, void**);
    ULONG   (STDMETHODCALLTYPE *AddRef)(IPolicyConfigVista*);
    ULONG   (STDMETHODCALLTYPE *Release)(IPolicyConfigVista*);
    void *GetMixFormat, *GetDeviceFormat, *SetDeviceFormat, *GetProcessingPeriod,   /* 3..6  */
         *SetProcessingPeriod, *GetShareMode, *SetShareMode,                        /* 7..9  */
         *GetPropertyValue, *SetPropertyValue;                                      /* 10..11 */
    HRESULT (STDMETHODCALLTYPE *SetDefaultEndpoint)(IPolicyConfigVista*, LPCWSTR, ERole); /* 12 */
    void *SetEndpointVisibility;                                                    /* 13 */
} IPolicyConfigVistaVtbl;
struct IPolicyConfigVista { const IPolicyConfigVistaVtbl *lpVtbl; };

/* usage */
IPolicyConfig *pc = NULL;
HRESULT hr = CoCreateInstance(&CLSID_CPolicyConfigClient, NULL, CLSCTX_ALL, &IID_IPolicyConfig, (void**)&pc);
if (SUCCEEDED(hr)) { hr = pc->lpVtbl->SetDefaultEndpoint(pc, id, eConsole); /* ... */ pc->lpVtbl->Release(pc); }
```

The `void *` placeholders work because every vtable slot is pointer-sized. Always use `STDMETHODCALLTYPE` (`__stdcall`). It is a no-op on x64 but required for x86 and ARM64EC correctness. DefSound's header omits it on `GetMixFormat`; harmless on x64, wrong on x86 if ever called.

### D.3 C++ declaration (compiled and run: `probe/pc.cpp`)

```cpp
#include <windows.h>
#include <mmdeviceapi.h>
#include <functiondiscoverykeys_devpkey.h>
#include <mmreg.h>
interface DECLSPEC_UUID("f8679f50-850a-41cf-9c72-430f290290c8") DECLSPEC_NOVTABLE IPolicyConfig : public IUnknown {
    STDMETHOD(GetMixFormat)(PCWSTR, WAVEFORMATEX**) = 0;
    STDMETHOD(GetDeviceFormat)(PCWSTR, BOOL, WAVEFORMATEX**) = 0;
    STDMETHOD(ResetDeviceFormat)(PCWSTR) = 0;
    STDMETHOD(SetDeviceFormat)(PCWSTR, WAVEFORMATEX*, WAVEFORMATEX*) = 0;
    STDMETHOD(GetProcessingPeriod)(PCWSTR, BOOL, PINT64, PINT64) = 0;
    STDMETHOD(SetProcessingPeriod)(PCWSTR, PINT64) = 0;
    STDMETHOD(GetShareMode)(PCWSTR, void*) = 0;
    STDMETHOD(SetShareMode)(PCWSTR, void*) = 0;
    STDMETHOD(GetPropertyValue)(PCWSTR, BOOL, const PROPERTYKEY&, PROPVARIANT*) = 0;
    STDMETHOD(SetPropertyValue)(PCWSTR, BOOL, const PROPERTYKEY&, PROPVARIANT*) = 0;
    STDMETHOD(SetDefaultEndpoint)(PCWSTR wszDeviceId, ERole eRole) = 0;
    STDMETHOD(SetEndpointVisibility)(PCWSTR, BOOL) = 0;
};
class DECLSPEC_UUID("870af99c-171d-4f9e-af0d-e63df40c2bc9") CPolicyConfigClient;
// CoCreateInstance(__uuidof(CPolicyConfigClient), nullptr, CLSCTX_ALL, IID_PPV_ARGS(&pc));
```

In C++, `__uuidof(MMDeviceEnumerator)` / `__uuidof(IMMDeviceEnumerator)` work without any extra GUID definitions.

### D.4 Compiler and linker flags for a small, fast exe

```
cl /nologo /W4 /O1 /GS /guard:cf- /MT /utf-8 /DUNICODE /D_UNICODE /DWIN32_LEAN_AND_MEAN toggle.c ^
   /link /SUBSYSTEM:CONSOLE /OPT:REF /OPT:ICF /DYNAMICBASE /NXCOMPAT /HIGHENTROPYVA /CETCOMPAT ^
         /MANIFEST:EMBED /MANIFESTINPUT:toggle-audio.manifest ^
         ole32.lib user32.lib
```

- **/O1 versus /O2:** the code is I/O and COM bound, so /O1 (size) is fine. Neither is measurable here.
- **/MT versus /MD:** /MT statically links the UCRT and vcruntime, so there is no VC++ redistributable dependency. It adds about 90 KB (crt.exe = 102,912 B; /MD = 9,728 B). Startup median over 40 launches [verified]: CRT-less 3.8 ms, **/MT 4.4–4.9 ms**, /MD 4.7–5.4 ms. **CRT-less (`/NODEFAULTLIB /ENTRY:` + `ExitProcess`) saves under 1 ms. Not worth it**, since you would lose `swprintf` and `_wfopen` and need `/GS-`.
- **GUI-subsystem variant (fallback, E.3):** `/SUBSYSTEM:WINDOWS /ENTRY:wmainCRTStartup` lets you keep `int wmain(int, wchar_t**)`. Alternatively use `wWinMain` + `CommandLineToArgvW` (shell32.lib).
- **/DYNAMICBASE /NXCOMPAT /HIGHENTROPYVA** are the defaults in modern link.exe. Keep them for security; they cost nothing.
- `/CETCOMPAT` (shadow stack) is fine.
- `/utf-8` is needed if source literals contain CJK.

---

## E. Console output: "toggle-audio list"

### E.1 Problem

G HUB launches the exe. A **console-subsystem** exe launched from a GUI process normally gets a **new console window**, which would flash on every key press. A **GUI-subsystem** exe gets no console, which is good for G HUB but bad for `toggle-audio list` in a terminal.

### E.2 Recommended (Windows 11 24H2+, this machine): console subsystem + `consoleAllocationPolicy=detached`

Learn: <https://learn.microsoft.com/en-us/windows/console/console-allocation-policy>. Spec: microsoft/terminal `doc/specs/#7335 - Console Allocation Policy.md`.

```xml
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <application>
    <windowsSettings>
      <consoleAllocationPolicy xmlns="http://schemas.microsoft.com/SMI/2024/WindowsSettings">detached</consoleAllocationPolicy>
    </windowsSettings>
  </application>
</assembly>
```

(Merge with your other manifest content: `supportedOS`, `dpiAwareness` PerMonitorV2 for the settings GUI, Common Controls v6 dependency, `asInvoker`, `activeCodePage` UTF-8 if desired.)

- Semantics per the spec: "The new process is not attached to a console session (similar to DETACHED_PROCESS) **unless one was inherited**." Also: "All console inheritance will proceed as normal." Shells still **wait** for CUI apps, and redirection and pipes behave normally.
- **Verified on 26300** with a Rust CUI exe and an embedded manifest (`research/cuiprobe`):
  - `Start-Process` (ShellExecute, the same path Explorer and G HUB-style launchers use): `GetConsoleWindow()` = **NULL**, so no console window. The control build without the manifest got a console window `0x2d4130e`.
  - Redirect (`> file`), capture (`$x = & exe`) and pipes all receive the UTF-8 `喇叭 (FiiO BTA30 PRO)` correctly.
- Before 24H2 the manifest element is ignored, so a console would flash. The project targets this user's 26300, so this is acceptable; document "Windows 11 24H2+ required (or use the GUI build)".
- **Residual risk:** if G HUB calls `CreateProcess` with an explicit `CREATE_NEW_CONSOLE`, a window could still appear (the policy only supplies *defaults*). **Verify once with the real G1 binding.** If a window flashes, use E.3.
- With this approach, the settings GUI (`toggle-audio config` or no-args double-click) simply creates its window. No console exists when launched from Explorer or G HUB. When launched from a terminal, the shell waits until the GUI closes, which is fine.
- Rust: in `build.rs`, emit `cargo:rustc-link-arg-bins=/MANIFEST:EMBED` and `cargo:rustc-link-arg-bins=/MANIFESTINPUT:<abs path>` (verified working). Alternatively use the `embed-manifest` crate or a `.rc` via `embed-resource`. Do **not** set `#![windows_subsystem = "windows"]`. Plain `println!` then works.

### E.3 Fallback: GUI subsystem + `AttachConsole(ATTACH_PARENT_PROCESS)`

The pattern, in this order:

1. `h = GetStdHandle(STD_OUTPUT_HANDLE)`. If it is non-NULL and valid, the parent redirected stdout (file or pipe) and passed the handle. Use it and write **UTF-8 bytes** (`WriteFile`).
2. Otherwise call `AttachConsole(ATTACH_PARENT_PROCESS)` (`(DWORD)-1`). On success, call `GetStdHandle` again. The console attach sets the std handles to the console when they were not inherited.
3. If `GetConsoleMode(h, &m)` succeeds, `h` is a console. Write with **`WriteConsoleW`** (UTF-16), which renders CJK correctly regardless of the console code page.
4. If both fail, there is no console (launched from G HUB or Explorer). Stay silent or show a `MessageBoxW` for errors only.
5. In Rust, call `AttachConsole` **before the first `println!`**. Rust std caches the stdout write mode and handle on the first successful write (`library/std/src/sys/stdio/windows.rs`, `write_mode` cache). It already uses `WriteConsoleW` for consoles and passes UTF-8 through for pipes and files. With no handle, writes fail with `ERROR_INVALID_HANDLE`, which `println!` silently ignores (`is_ebadf`).

Known quirks:

- **Prompt overlap:** cmd and PowerShell do not wait for GUI-subsystem programs, so the prompt is printed first and the output lands after it. The user must press Enter to get a clean prompt. Mitigation: a leading `\r\n`. True fixes are E.2, `start /wait`, or a separate console twin (`toggle-audio-cli.exe` or a `.com` twin, as `devenv.com` does).
- **PowerShell redirection/capture of GUI-subsystem exes does not work [verified, pwsh 7.6]:**
  - `exe > out.txt` returned in 9 ms with an empty, still-locked file.
  - `$x = & exe` captured 0 lines.
  - `exe | Select-Object …` did work.
  - `cmd /c "exe > out.txt"` worked.
  - Users scripting `toggle-audio list` from PowerShell would hit this. That is a strong reason to prefer E.2.
- `AttachConsole` fails with `ERROR_ACCESS_DENIED` if already attached and with `ERROR_INVALID_HANDLE` if the parent has no console. Both are fine to ignore.
- Ctrl+C in the parent console is delivered to the attached process too. Irrelevant for a run-and-exit tool.
- Never call `FreeConsole` + `AllocConsole`, which would pop a window.
- 24H2+ also offers `AllocConsoleWithOptions(&{ALLOC_CONSOLE_MODE_DEFAULT}, &result)`. It returns `ALLOC_CONSOLE_RESULT_EXISTING_CONSOLE` (2) when it attaches to an inherited console, and `…_NO_CONSOLE` (0) when the parent asked for none. With E.2 you don't need it.

### E.4 Output encoding rules (either approach)

- Console: UTF-16 via `WriteConsoleW` (C), or Rust `println!` (does this automatically).
- File or pipe: UTF-8 without BOM. Use `\r\n` line endings for Windows tools. `list` output should be machine-friendly: one device per line, `*` marker for default, `name<TAB>id` (as in the Rust probe), or a `--json` flag.
- C with the CRT: avoid `wprintf` to a console (it is code-page-converted and mangles CJK unless `_setmode(_fileno(stdout), _O_U16TEXT)` is set). Write the bytes or `WriteConsoleW` yourself.

---

## F. Windows 11 24H2 / 25H2 / 26H2 behaviour notes

1. **IPolicyConfig still works on 10.0.26300 (26H2).** CPolicyConfigClient/IPolicyConfig QI succeeds, slot 13 = `CPolicyConfigClient::SetDefaultEndpoint` per PDB, and a same-ID call returns S_OK [verified]. No public reports of breakage in 24H2–26H2 were found. Searches of SoundSwitch FAQ and issues, EarTrumpet and the web turned up only the "app locked to a device" issue below. It remains undocumented: keep the Vista fallback and surface HRESULTs.
2. **New in 24H2 (build 26100):** `PKEY_AudioEndpoint_StableId` and `GetDevice(stableId)` (A.7); console allocation policy and `AllocConsoleWithOptions` (E.2).
3. **Per-app device overrides beat the default.** Settings → System → Sound → Volume mixer lets users pin an app to a specific output. Those apps **will not follow** `SetDefaultEndpoint`. SoundSwitch's FAQ fix is "Volume mixer → Reset sound devices and volumes for all apps" (<https://sn.aaflalo.me/faq/app-not-switching-after-update>). Mention this in the README troubleshooting section. The per-app API (`IAudioPolicyConfigFactory`, WinRT activatable class `Windows.Media.Internal.AudioPolicyConfig`, IIDs differ pre and post 21H2, see EarTrumpet `AudioPolicyConfigFactoryImplFor21H2.cs`) is out of scope.
4. **Who follows a default change:** apps that render to the *default* device move automatically. That covers WASAPI activated via `DEVINTERFACE_AUDIO_RENDER` (automatic stream routing since Windows 10 1607), and higher-level APIs since Windows 7 ([Automatic Stream Routing](https://learn.microsoft.com/en-us/windows/win32/coreaudio/automatic-stream-routing)). Apps that opened a *specific* endpoint (many DAWs, some games, exclusive-mode players) stay put until restarted or reconfigured. This is not a bug in the tool.
5. **Notifications:** each successful role change fires `IMMNotificationClient::OnDefaultDeviceChanged(flow, role, id)` in every process that registered (Explorer volume flyout, Sound settings, SoundSwitch, EarTrumpet, games). Three roles produce three callbacks. Windows shows no toast. The tray/flyout updates the icon and name. The tool may optionally show its own lightweight feedback (e.g. a balloon, or a short system sound on the new device), which costs extra time; keep it optional.
6. **Communications role and Bluetooth:** with a Windows-paired classic BT headset, Windows exposes an A2DP "Headphones" endpoint and a Hands-Free endpoint. Making the HFP endpoint the default communications device can drop music quality when a call app opens the mic. LE Audio (25H2, "Use LE Audio when available", stereo-while-mic) changes this. Not relevant to the BTA30 PRO: it is a USB UAC2 device with a single `喇叭 (FiiO BTA30 PRO)` endpoint. It is relevant if users pick a Windows-paired BT headset, which is why `switch_communications` should be configurable. The "Communications activity → ducking" setting (mmsys.cpl Communications tab) is unaffected by default switching.
7. **Spatial sound** (Windows Sonic / Dolby Atmos / DTS) and enhancements are **per-endpoint properties**. Switching defaults does not copy them; each endpoint keeps its own. No action needed.
8. **25H2 "Shared audio" (LE Audio broadcast to two devices):** it is a separate Quick Settings feature. Not affected by and not affecting `SetDefaultEndpoint` as far as public docs say. Untested.
9. **Endpoint IDs can change** after driver or OS updates (Microsoft's statement in A.7). The settings GUI must therefore tolerate a stored device that no longer resolves: mark it "(missing)" and let the user re-pick. The toggle should exit with a clear error, and try the StableId if one was saved.

---

## Pitfalls checklist

- [ ] **Free every `GetId()` string with `CoTaskMemFree`**, and every `PropVariantToStringAlloc` result too.
- [ ] **`PropVariantInit` before and `PropVariantClear` after** every `IPropertyStore::GetValue`. windows 0.62's `PROPVARIANT` implements `Drop` (it calls `PropVariantClear`). An explicit `PropVariantClear` is still fine and makes the release point explicit. With windows-sys, or in C, you must clear it yourself.
- [ ] Check `pv.vt == VT_LPWSTR` (31) before reading `pwszVal`. A missing name is `VT_EMPTY`.
- [ ] C: **define `CLSID_MMDeviceEnumerator` / `IID_IMMDeviceEnumerator` yourself** (`initguid.h` + `DEFINE_GUID`), or LNK2019 follows. Include `initguid.h` in only one TU, or use `DECLSPEC_SELECTANY`.
- [ ] Use **`PKEY_Device_FriendlyName` (pid 14)** for display. Never use names as identity, because duplicate names exist (3× "PG42UQ …" on this PC).
- [ ] `GetDevice(id)` **succeeds for NOTPRESENT/DISABLED/UNPLUGGED** endpoints. Always check `GetState() == DEVICE_STATE_ACTIVE` before switching.
- [ ] Handle `E_NOTFOUND` = `0x80070490` (unknown ID) from `GetDevice`, `GetDefaultAudioEndpoint` and `SetDefaultEndpoint`.
- [ ] Use **`IPolicyConfig` slot 13 on CLSID `870af99c…`**. If you implement the Vista fallback, it is **slot 12 on CLSID `294935ce…`**. Do not copy AudioDeviceCmdlets' or SoundSwitch's Vista layout (it has an extra ResetDeviceFormat).
- [ ] Do not use `{ca286fc3…}` or `{6be54be8…}` (Windows 10 1507/1511 only) or `{8f9fb2aa…}` (E_NOINTERFACE on Win11).
- [ ] Set **eConsole, eMultimedia, and optionally eCommunications**. Skip roles already set. Decide the toggle from **eMultimedia**.
- [ ] Release COM pointers **before** `CoUninitialize`. In Rust, drop or scope them; don't let them outlive the uninit call.
- [ ] `CoInitializeEx` may return `S_FALSE` (still pair with Uninitialize) or `RPC_E_CHANGED_MODE` (do not Uninitialize).
- [ ] Rust: **pin `windows` and `windows-core` to `=0.62.2`**. `windows-core` must be a direct dependency for `#[interface]`, and `IUnknown_Vtbl` must be imported. Enable `Win32_System_Variant`, or `PROPVARIANT`/`GetValue` silently disappear behind cfg. `WriteFile` additionally needs `Win32_System_IO`.
- [ ] Rust: link the CRT statically (`+crt-static`) for redist-free MSI installs. C: `/MT`.
- [ ] Console: prefer **CUI + `consoleAllocationPolicy=detached` manifest**, and verify once that G HUB launches with no flash. If using the GUI subsystem: call `AttachConsole` *before* the first write, use `WriteConsoleW` for consoles and UTF-8 for redirected handles, and expect PowerShell `>`/capture not to work.
- [ ] Never print CJK through code-page-dependent `printf`/`wprintf` to a console.
- [ ] Apps pinned in Volume mixer and apps bound to a specific endpoint won't follow the default. Document this, don't "fix" it.
- [ ] Endpoint IDs may change after driver or OS updates. Store `stable_id` (24H2+, inferred key `{1da5d803-d492-4edd-8c23-e0c0ffee7f0e},12`) as a secondary key and re-save the fresh ID when it resolves.
- [ ] Never call `SetDefaultEndpoint` on a non-ACTIVE endpoint, and surface every failing HRESULT. This is undocumented API and could change in a future Windows release.
- [ ] Do not hold a long-lived `IMMDeviceEnumerator` across device changes in the GUI without registering `IMMNotificationClient`. Re-enumerate when the settings dialog opens or refreshes.

---

## 10. Probe artifacts (all under `…\scratchpad\research\`)

| File | What it proves |
|---|---|
| `probe/probe.c`, `probe/probe-out.txt` | Enumeration of all 32 render endpoints and their three name keys; default per role; GetDevice error codes; QI matrix of both CLSIDs × 4 IIDs; timings; `reassert` mode = no-op SetDefaultEndpoint latency (STA/MTA). |
| `probe/vtdump.c`, `probe/vtdump-out.txt` | Vtable slots of IPolicyConfig/IPolicyConfigVista resolved to Microsoft public PDB symbols. Uses the SDK Debuggers `dbghelp.dll` + `symsrv.dll`; cache in `probe/symcache`. |
| `probe/props.c`, `probe/props-out.txt` | All property keys of active endpoints; StableId candidate `{1da5d803…},12` resolves via GetDevice. |
| `probe/notpresent.c` | GetDevice on a NOTPRESENT endpoint returns S_OK with state 0x4. |
| `probe/pc.cpp` | C++ MIDL-style IPolicyConfig declaration compiles and CoCreates. |
| `probe/crt.c`, `probe/nocrt.c` | Startup cost /MT vs /MD vs CRT-less. |
| `rsprobe/` | Rust windows 0.62.2: Cargo features, `#[interface]` IPolicyConfig, enumeration, both PROPVARIANT methods, GetDevice errors, no-op SetDefaultEndpoint (4.9/6.2/5.7 ms), GUI-subsystem console output behaviour. |
| `cuiprobe/` | CUI + detached manifest: no console on ShellExecute launch (control build without the manifest gets one); redirect and capture work. |
| `src/PolicyConfig_AEL.h`, `src/sunshine_PolicyConfig.h` | Fetched copies of the C/C++ headers used for cross-checking. |

## Sources

- Microsoft Learn: [Device Roles](https://learn.microsoft.com/en-us/windows/win32/coreaudio/device-roles) · [IMMDeviceEnumerator::GetDevice](https://learn.microsoft.com/en-us/windows/win32/api/mmdeviceapi/nf-mmdeviceapi-immdeviceenumerator-getdevice) · [PKEY_AudioEndpoint_StableId](https://learn.microsoft.com/en-us/windows/win32/coreaudio/pkey-audioendpoint-stableid) · [Automatic Stream Routing](https://learn.microsoft.com/en-us/windows/win32/coreaudio/automatic-stream-routing) · [Console Allocation Policy](https://learn.microsoft.com/en-us/windows/console/console-allocation-policy) · [AllocConsoleWithOptions](https://learn.microsoft.com/en-us/windows/console/allocconsolewithoptions)
- Windows SDK 10.0.26100 headers: `um/mmdeviceapi.h` (E_NOTFOUND, DEVICE_STATE_*, PKEY_AudioEndpoint_* pids 0–11), `um/functiondiscoverykeys_devpkey.h` (lines 53, 62, 210), `shared/devpkey.h`
- SoundSwitch: <https://github.com/Belphemur/SoundSwitch> (`SoundSwitch.Audio.Manager/Interop/Client/PolicyClient.cs`, `Interop/Interface/ComGuid.cs`, `Interop/Interface/Policy/IPolicyConfig.cs`, `IPolicyConfigVista.cs`, `IPolicyConfigX.cs`, `AudioSwitcher.cs`); FAQ <https://sn.aaflalo.me/faq/app-not-switching-after-update>
- AudioEndPointLibrary/DefSound: <https://github.com/Belphemur/AudioEndPointLibrary/blob/master/DefSound/PolicyConfig.h>
- AudioDeviceCmdlets: <https://github.com/frgnca/AudioDeviceCmdlets> (`SOURCE/PolicyConfigClient.cs`, `IPolicyConfig.cs`, `IPolicyConfigVista.cs`, `IPolicyConfig10.cs`, `PKEY.cs`, `AudioDeviceCmdlets.cs` lines 940–1000)
- EarTrumpet: <https://github.com/File-New-Project/EarTrumpet> (`EarTrumpet/Interop/MMDeviceAPI/IPolicyConfig.cs`, `PolicyConfigClient.cs`, `DataModel/WindowsAudio/Internal/AudioDeviceManager.cs`)
- Sunshine: <https://github.com/LizardByte/Sunshine/blob/master/src/platform/windows/PolicyConfig.h>
- windows-rs: <https://github.com/microsoft/windows-rs/releases> (releases 70–74), [issue #4867](https://github.com/microsoft/windows-rs/issues/4867); crate sources `windows-0.62.2`, `windows-core-0.62.2`, `windows-interface-0.59.3` (`src/lib.rs` macro expansion); docs <https://docs.rs/windows/0.62.2/windows/Win32/Media/Audio/struct.IMMDeviceEnumerator.html>
- Rust std console handling: <https://github.com/rust-lang/rust/blob/master/library/std/src/sys/stdio/windows.rs>
- microsoft/terminal spec: `doc/specs/#7335 - Console Allocation Policy.md`
