# Settings GUI: framework decision and implementation guide

Status: researched, prototyped, compiled and measured on the target PC on 2026-10-04.
Working prototype (two variants, both build clean, no warnings):
`C:\Users\Hotdogee\AppData\Local\Temp\claude\I--Projects-switch-audio\c5969902-99b1-4723-bb94-f401b9680340\scratchpad\research\gui-skeleton\`

```
gui-skeleton/
  Cargo.toml            windows 0.62.2 + embed-resource 3.0.11, release profile tuned for size
  build.rs              compiles res/app.rc (manifest + icon + dialog + VERSIONINFO), passes Cargo version
  res/app.manifest      PerMonitorV2, Common-Controls 6.0.0.0, asInvoker, Win10/11 supportedOS
  res/app.rc            DIALOGEX settings dialog (Segoe UI 9) + icon + manifest + VERSIONINFO
  res/resource.h        control IDs shared by .rc and Rust
  res/app.ico           placeholder multi-size PNG-compressed icon (16..256)
  src/lib.rs            list_render_devices() (IMMDeviceEnumerator), copy_to_clipboard(), ids
  src/bin/dlg.rs        RECOMMENDED: resource dialog + DialogBoxParamW (full settings UI)
  src/bin/wnd.rs        FALLBACK: CreateWindowExW + WndProc + DPI scale helper + WM_DPICHANGED
  target/dlg.png, target/wnd.png   screenshots taken on this PC (96 DPI, zh-TW locale)
```

Build used: `cargo +1.90.0 build --release`. I installed Rust 1.90.0 side by side with
`rustup toolchain install 1.90.0 --profile minimal` and did not change the default toolchain.
**The installed default stable 1.72 is too old**: windows 0.62 needs MSRV 1.82 and embed-resource 3.0.11 needs 1.76.
The real project needs `rustup update stable` plus a `rust-toolchain.toml`.

---

## 1. Decision

**Use raw Win32 through the `windows` crate (0.62.2). Define the settings window as a `DIALOGEX` resource in the `.rc` file you
already need for the manifest and icon, and run it with `DialogBoxParamW`. Compile it with `embed-resource` 3.0.11.**

This refines the expected "raw Win32 via windows-rs" answer. I recommend a **resource dialog** over `CreateWindowExW`
and over an in-memory `DLGTEMPLATE`, for these reasons:

| Concern | Resource dialog (`DialogBoxParamW`) | `CreateWindowExW` + WndProc | In-memory `DLGTEMPLATE` (`DialogBoxIndirectParamW`) |
|---|---|---|---|
| Segoe UI on every control | automatic: `FONT 9, "Segoe UI"` | manual: create an HFONT, then `WM_SETFONT` to every child | automatic |
| Layout | dialog units (DLU) scale with font and DPI | hand-rolled logical px × DPI helper | DLU |
| Per-Monitor-V2 `WM_DPICHANGED` | **automatic**: the dialog manager resizes, re-lays out and re-fonts ([MS Learn: PMv2 "Dialog Scaling"](https://learn.microsoft.com/en-us/windows/win32/hidpi/dpi-awareness-context), [SetDialogDpiChangeBehavior](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setdialogdpichangebehavior)) | manual: recreate the font, `SetWindowPos(suggested rect)`, re-layout | automatic |
| Tab / Shift+Tab / Alt mnemonics / Enter / Esc | automatic (modal loop) | `IsDialogMessageW` in your own loop | automatic |
| Lines of Rust (full UI) | ~230 (dlg.rs) | ~330 for a *subset* (wnd.rs) | +150 lines of unsafe WORD/DWORD-aligned template serialisation |
| Extra tooling | rc.exe (already required for the manifest and icon) | none beyond rc.exe | none beyond rc.exe |
| Edit the layout | text edit of `app.rc` (or the VS resource editor) | Rust constants | Rust byte-packing (error-prone) |

`DLGTEMPLATE` serialisation gains nothing here, because `rc.exe` already runs in the build. `wnd.rs` is kept as a documented
fallback if a dynamic layout is ever needed (for example a variable number of devices as radio buttons).

### Why not the alternatives

| Option | Verdict | Evidence |
|---|---|---|
| **native-windows-gui** 1.0.13 | Rejected | Latest release 2022-09-05; last commit 2023-02-14; README: "The development of this library is considered 'done'". It depends on the legacy **`winapi` 0.3** crate, not `windows`. Pulling in `winapi` next to `windows` doubles the bindings and it receives no DPI-v2 fixes. Sources: [crates.io](https://crates.io/crates/native-windows-gui), [GitHub](https://github.com/gabdube/native-windows-gui), [Cargo.toml](https://raw.githubusercontent.com/gabdube/native-windows-gui/master/native-windows-gui/Cargo.toml) |
| **winsafe** 0.0.29 | Viable but not chosen | Actively maintained (0.0.29, 2026-09-01; MSRV 1.87). It has its own bindings (no `windows` crate), so the CLI's core-audio code written against `windows` would sit next to a second, parallel binding set. Its `gui::dpi()` helpers use **system** DPI (`GetDeviceCaps`), and its docs say nothing about per-monitor-v2 re-layout. It is a 0.0.x API ("may still evolve"). It can load `.res` dialogs, which adds little over calling `DialogBoxParamW` directly. Sources: [crates.io](https://crates.io/crates/winsafe), [docs.rs gui](https://docs.rs/winsafe/latest/winsafe/gui/index.html) |
| **egui / iced / slint** | Rejected | None of them uses native Win32 controls. They draw their own widgets with GPU or software renderers, so they do not look like Windows 11 dialogs (Slint's "fluent" style is the closest imitation). Estimated, not measured here: 2–10 MB binaries versus ~130 KB, and 100–300+ ms cold start because of the GL/wgpu/D3D context and font loading. Each adds a large dependency tree to a crate whose main job is a ~1 ms toggle. Slint is GPLv3 or a proprietary royalty-free licence, which is an extra licensing consideration for an MIT/Apache repo ([slint.dev/pricing](https://slint.dev/pricing)). |
| **C# WinForms on .NET Framework 4.8 (separate exe)** | Rejected | Zero runtime install, and a 5.6 KB exe built with the in-box `csc.exe`. **Measured on this PC** (minimal form, no audio enumeration): median **152 ms** to `Shown` when warm and **1656 ms** on the first (cold) run, against **~60 ms** for the Rust dialog *including* COM audio enumeration. It also needs a second language, a second exe in the MSI, and hand-written COM interop for IMMDeviceEnumerator. |

---

## 2. Measured results (this PC, 96 DPI, 5 active endpoints, release build)

Timestamps are ms since process creation (`GetProcessTimes` against `GetSystemTimePreciseAsFileTime`), 10 runs each, warm.

| Phase | dlg.exe median (min–max) |
|---|---|
| `main()` entered | 7.1 (6.7–11.7) |
| after `CoInitializeEx` + `InitCommonControlsEx` | 9.5 |
| after `list_render_devices()` (5 endpoints, friendly names) | 13.1 |
| end of `WM_INITDIALOG` (dialog + controls created and filled) | 45.4 (41–59) |
| **first paint done** (WM_TIMER(0) after WM_PAINT) | **59.8 (47–66)** |
| wnd.exe (CreateWindowExW fallback) first paint | 94.7 (87–101) |
| WinForms .NET 4.8 `Shown` (no audio code) | 152 warm / 1656 cold |

Both Rust variants are well under the 200 ms target. Release binaries with `opt-level="s"`, LTO, `panic="abort"` and
`strip`: **dlg.exe 130,560 bytes**, wnd.exe 124,928 bytes (the icon included).

Visual check (`target/dlg.png`): themed Win11 combo boxes, checkbox and buttons, Segoe UI, and `喇叭 (FiiO BTA30 PRO)` rendered
correctly. That confirms comctl32 v6, the manifest and CJK fallback work. Manifest extraction (`mt.exe -inputresource:dlg.exe;#1`)
shows PerMonitorV2, Common-Controls 6.0.0.0 and asInvoker. embed-resource used the **Microsoft rc.exe** (10.0.x) from the
Windows SDK, not llvm-rc.

---

## 3. Crates and versions (verified 2026-10-04)

| Crate | Version | Notes |
|---|---|---|
| `windows` | **0.62.2** (2025-10-06; newest on crates.io; MSRV 1.82) | API style in 0.62: optional handles are `Option<HWND>`, `SendMessageW(hwnd, msg, Option<WPARAM>, Option<LPARAM>)`, many functions return `windows::core::Result`, `BOOL` lives in `windows::core`. Full docs are not on docs.rs (the crate is too large). Use <https://microsoft.github.io/windows-docs-rs/> or grep `~/.cargo/registry/src/*/windows-0.62.2/src`. |
| `embed-resource` (build-dep) | **3.0.11** (newest on crates.io, 2026; MSRV 1.76) | Maintained. Finds rc.exe from the Windows SDK on MSVC and links the .res via `cargo:rustc-link-arg-bins` (all bins, not tests). 3.x requires `.manifest_optional()` or `.manifest_required()` on the result ([docs.rs](https://docs.rs/embed-resource/latest/embed_resource/)). |
| `winresource` 0.1.31 (2026-03) | alternative | Maintained fork of winres. It generates a .rc from `Cargo.toml` metadata (icon, version, `set_manifest`). It is convenient, but a hand-written .rc is still needed for the `DIALOGEX`, so it adds nothing here. |
| `winres` 0.1.12 (2021-09) | **do not use** | Unmaintained. |
| `embed-manifest` 1.5.1 (2026-09; MSRV 1.85) | alternative only if there were no .rc | Embeds a manifest without rc.exe, but cannot embed an icon or a dialog. |

### `windows` features needed (GUI part; the CLI adds its own)

```toml
[dependencies.windows]
version = "0.62.2"
features = [
    "Win32_Foundation",
    "Win32_Graphics_Gdi",                  # MonitorFromPoint, GetMonitorInfoW (+ HFONT etc. in the fallback)
    "Win32_Graphics_Dwm",                  # DwmSetWindowAttribute (only if dark title bar is ever added)
    "Win32_System_LibraryLoader",          # GetModuleHandleW
    "Win32_System_DataExchange",           # OpenClipboard, EmptyClipboard, SetClipboardData, CloseClipboard
    "Win32_System_Memory",                 # GlobalAlloc, GlobalLock, GlobalUnlock
    "Win32_System_Ole",                    # CF_UNICODETEXT
    "Win32_System_Com",                    # CoInitializeEx, CoCreateInstance, CoTaskMemFree, STGM_READ
    "Win32_System_Com_StructuredStorage",  # PROPVARIANT  } both needed for PROPVARIANT's Display/Drop impls
    "Win32_System_Variant",                #              } (extensions are gated on both features)
    "Win32_Media_Audio",                   # IMMDeviceEnumerator, eRender, DEVICE_STATE_ACTIVE
    "Win32_Devices_FunctionDiscovery",     # PKEY_Device_FriendlyName
    "Win32_UI_Shell_PropertiesSystem",     # IPropertyStore
    "Win32_UI_Controls",                   # InitCommonControlsEx, LoadIconMetric, CheckDlgButton, IsDlgButtonChecked, CB_SETMINVISIBLE, WC_*
    "Win32_UI_HiDpi",                      # GetDpiForWindow, SystemParametersInfoForDpi, AdjustWindowRectExForDpi
    "Win32_UI_Input_KeyboardAndMouse",     # SetFocus (fallback only)
    "Win32_UI_WindowsAndMessaging",        # DialogBoxParamW, SendMessageW, CB_*, WM_*, SetWindowPos ...
]
```

Pitfall: `CB_SETMINVISIBLE` is in `Win32::UI::Controls`, not `WindowsAndMessaging` (it is a comctl32 v6 message).
`CF_UNICODETEXT` is `CLIPBOARD_FORMAT(13u16)`, so pass `CF_UNICODETEXT.0 as u32`.

---

## 4. Resources: build.rs, .rc, manifest, resource.h

Recommended repo layout: `res/` beside `src/`, ASCII-only `.rc` (or keep `#pragma code_page(65001)` and save as UTF-8).

### build.rs (verified)

```rust
fn main() {
    println!("cargo:rerun-if-changed=res/app.rc");
    println!("cargo:rerun-if-changed=res/app.manifest");
    println!("cargo:rerun-if-changed=res/app.ico");
    println!("cargo:rerun-if-changed=res/resource.h");
    // Pass the Cargo version into the .rc so VERSIONINFO never drifts from Cargo.toml.
    let v = |k: &str| std::env::var(k).unwrap();
    let macros = [
        format!("VER_MAJOR={}", v("CARGO_PKG_VERSION_MAJOR")),
        format!("VER_MINOR={}", v("CARGO_PKG_VERSION_MINOR")),
        format!("VER_PATCH={}", v("CARGO_PKG_VERSION_PATCH")),
    ];
    // Compiles res/app.rc with rc.exe (Windows SDK) and links the .res into every bin of this crate
    // (cargo:rustc-link-arg-bins). manifest_required(): fail the build if it could not be linked,
    // because without the manifest we silently lose visual styles + DPI awareness.
    embed_resource::compile("res/app.rc", &macros)
        .manifest_required()
        .unwrap();
}
```

I verified this by bumping the Cargo version to 0.3.7: Explorer's FileVersion and ProductVersion showed 0.3.7.
If the crate ever has a bin that must *not* get the resources, use `embed_resource::compile_for("res/app.rc", &["toggle-audio"], &macros)`.
If the build machine has no Windows SDK, embed-resource fails. The VS 2022 Build Tools plus Windows SDK workload provide rc.exe,
and GitHub Actions `windows-latest` has it.

### res/app.manifest

```xml
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="ToggleAudio" version="0.1.0.0" processorArchitecture="*"/>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0"
                        processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>  <!-- Windows 10 / 11 -->
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2, PerMonitor</dpiAwareness>
    </windowsSettings>
  </application>
</assembly>
```

Do not also call `SetProcessDpiAwarenessContext` in code: the manifest is the recommended route, and the call fails if
the manifest already set awareness. The manifest also applies to the toggle (CLI) path of the same exe, which is harmless
(asInvoker matters there, so G HUB never triggers UAC). Optional: `<activeCodePage>UTF-8</activeCodePage>` is not needed,
because only `W` APIs are used.

### res/resource.h

```c
#define IDI_APP              1
#define IDD_SETTINGS         101
#define IDC_DEV1             1001
#define IDC_DEV2             1002
#define IDC_COMMS            1003
#define IDC_PATH             1004
#define IDC_COPY             1005
#define IDC_TEST             1006
#define IDC_STATUS           1007
```

Mirror these IDs in Rust (`mod ids { pub const IDC_DEV1: i32 = 1001; ... }`). Optionally add a unit test that parses
resource.h and asserts the two match.

### res/app.rc (verified; produces the screenshot)

```rc
#pragma code_page(65001)
#include <windows.h>
#include "resource.h"

#define STR_(x) #x
#define STR(x) STR_(x)
#define VER_STR STR(VER_MAJOR) "." STR(VER_MINOR) "." STR(VER_PATCH)

CREATEPROCESS_MANIFEST_RESOURCE_ID RT_MANIFEST "app.manifest"
IDI_APP ICON "app.ico"          // lowest-ID icon = Explorer / shortcut / Add-Remove-Programs icon

// Units are dialog units: 4 horizontal DLU = avg char width, 8 vertical DLU = char height of the dialog font.
IDD_SETTINGS DIALOGEX 0, 0, 300, 141
STYLE DS_SETFONT | DS_MODALFRAME | WS_POPUP | WS_CAPTION | WS_SYSMENU
EXSTYLE WS_EX_APPWINDOW                     // taskbar button even though it's a dialog
CAPTION "Toggle Audio Settings"
FONT 9, "Segoe UI", 400, 0, 0x1
BEGIN
    LTEXT           "Device &1:", -1, 7, 9, 50, 8
    COMBOBOX        IDC_DEV1, 60, 7, 233, 160, CBS_DROPDOWNLIST | WS_VSCROLL | WS_TABSTOP
    LTEXT           "Device &2:", -1, 7, 27, 50, 8
    COMBOBOX        IDC_DEV2, 60, 25, 233, 160, CBS_DROPDOWNLIST | WS_VSCROLL | WS_TABSTOP
    AUTOCHECKBOX    "Also switch the &Communications device", IDC_COMMS, 60, 43, 233, 10, WS_TABSTOP
    LTEXT           "Toggle &command (paste into Logitech G HUB):", -1, 7, 62, 286, 8
    EDITTEXT        IDC_PATH, 7, 73, 236, 14, ES_AUTOHSCROLL | ES_READONLY | WS_TABSTOP
    PUSHBUTTON      "Cop&y", IDC_COPY, 247, 73, 46, 14, WS_TABSTOP
    LTEXT           "", IDC_STATUS, 7, 97, 286, 18, SS_NOPREFIX | SS_LEFT
    PUSHBUTTON      "&Test toggle", IDC_TEST, 7, 120, 60, 14, WS_TABSTOP
    DEFPUSHBUTTON   "&Save", IDOK, 183, 120, 52, 14, WS_TABSTOP
    PUSHBUTTON      "Cancel", IDCANCEL, 241, 120, 52, 14, WS_TABSTOP
END

VS_VERSION_INFO VERSIONINFO
 FILEVERSION VER_MAJOR,VER_MINOR,VER_PATCH,0
 PRODUCTVERSION VER_MAJOR,VER_MINOR,VER_PATCH,0
 FILEFLAGSMASK 0x3fL
 FILEFLAGS 0x0L
 FILEOS VOS_NT_WINDOWS32
 FILETYPE VFT_APP
 FILESUBTYPE VFT2_UNKNOWN
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904b0"
        BEGIN
            VALUE "CompanyName", "toggle-audio contributors"
            VALUE "FileDescription", "Toggle Audio"
            VALUE "FileVersion", VER_STR
            VALUE "InternalName", "toggle-audio"
            VALUE "OriginalFilename", "toggle-audio.exe"
            VALUE "ProductName", "Toggle Audio"
            VALUE "ProductVersion", VER_STR
            VALUE "LegalCopyright", "MIT OR Apache-2.0"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
```

Layout recipe (DLU grid): 7 DLU outer margin; label column x=7, w=50; field column x=60 to 293; rows at y=7, 25, 43
(18 DLU pitch for combos, 14 DLU buttons and edit, 8 DLU text); bottom button row at y = height − 7 − 14; buttons 50–60 DLU
wide with 6 DLU gaps. These follow the [Windows layout guidance](https://learn.microsoft.com/en-us/windows/win32/uxguide/vis-layout)
(7 DLU margins, 14 DLU button height). Child z-order (= Tab order) is the order in the template, so keep it in reading order.
Each `LTEXT "Device &1:"` placed directly before its combo also gives the combo its accessible name and makes Alt+1 focus the combo.

Icon: ship a real multi-size `.ico` with 16, 20, 24, 32, 40, 48, 64 and 256 px entries (256 as PNG). The prototype's
generated icon is 2 KB.

---

## 5. Code walkthrough (recommended variant, `src/bin/dlg.rs`)

The full file is in the prototype. Key parts:

### 5.1 Entry, COM, common controls, modal dialog

```rust
#![windows_subsystem = "windows"]          // no console flash when G HUB launches the exe

fn run_settings() {
    unsafe {
        // STA: the GUI thread owns windows; core-audio objects work fine in STA.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        // Loads comctl32 v6 (selected by the manifest's activation context) and registers v6 classes.
        let icc = INITCOMMONCONTROLSEX { dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32, dwICC: ICC_STANDARD_CLASSES };
        let _ = InitCommonControlsEx(&icc);
        let hinst: HINSTANCE = GetModuleHandleW(None).unwrap().into();
        let mut app = App { /* devices, saved ids, comms flag, toggle exe path */ };
        let rc = DialogBoxParamW(Some(hinst), PCWSTR(IDD_SETTINGS as usize as *const u16) /* MAKEINTRESOURCEW */,
                                 None, Some(dlg_proc), LPARAM(&mut app as *mut App as isize));
        if rc == -1 { /* template missing => resources not linked; MessageBoxW + exit 1 */ }
    }
}
```

`app` lives on `main`'s stack for the whole modal loop. Its pointer is stashed in `GWLP_USERDATA` in `WM_INITDIALOG`
(`lparam` carries it), so no `Box`, no global and no `thread_local` are needed.

### 5.2 DialogProc

```rust
unsafe extern "system" fn dlg_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> isize {
    unsafe { match msg {
        WM_INITDIALOG => { SetWindowLongPtrW(hwnd, GWLP_USERDATA, lparam.0); on_init(hwnd, app(hwnd)); 1 } // TRUE = default focus
        WM_COMMAND    => { on_command(hwnd, (wparam.0 & 0xFFFF) as i32, ((wparam.0 >> 16) & 0xFFFF) as u32); 1 }
        _ => 0,   // incl. WM_DPICHANGED: FALSE lets the dialog manager rescale everything
    } }
}
```

Rules: return non-zero only for messages you handled, and never call `DefDlgProc` or `DefWindowProc` from a DialogProc.
Esc, Alt+F4 and the [X] button all arrive as `WM_COMMAND(IDCANCEL)`. Enter arrives as `WM_COMMAND(IDOK)` (the
`DEFPUSHBUTTON`), unless focus is on another push button, which then gets clicked. That is native behaviour.

### 5.3 ComboBox: fill, item data, selection

```rust
let combo = GetDlgItem(Some(hwnd), IDC_DEV1)?;
SendMessageW(combo, CB_RESETCONTENT, None, None);
for (i, d) in devices.iter().enumerate() {
    let text = wide(&d.name);                                  // Vec<u16> with trailing NUL; CJK is just UTF-16
    let idx = SendMessageW(combo, CB_ADDSTRING, None, Some(LPARAM(text.as_ptr() as isize))).0;
    SendMessageW(combo, CB_SETITEMDATA, Some(WPARAM(idx as usize)), Some(LPARAM(i as isize))); // index into Vec<Device>
    if Some(d.id.as_str()) == saved_id { sel = idx; }
}
SendMessageW(combo, CB_SETMINVISIBLE, Some(WPARAM(12)), None);       // comctl v6: rows shown when dropped
SendMessageW(combo, CB_SETCURSEL, Some(WPARAM(sel as usize)), None); // sel = -1 => no selection
// read back
let cur = SendMessageW(combo, CB_GETCURSEL, None, None).0;            // CB_ERR (-1) if none
let i   = SendMessageW(combo, CB_GETITEMDATA, Some(WPARAM(cur as usize)), None).0 as usize;
```

`CB_ADDSTRING` copies the string synchronously, so the temporary `Vec<u16>` only needs to outlive the call.
Persist the **endpoint ID**, never the display name or index. On `CBN_SELCHANGE` (HIWORD of wParam) show a warning
in the status line if both combos point at the same device.

**Saved device not currently active.** This matters for Bluetooth headsets that are switched off. Enumerate
`DEVICE_STATE_ACTIVE` for the list. If a saved ID is not in it, append an extra item `"<saved name> (not connected)"`.
Keep the last-known friendly name in the config for this, or fall back to `IMMDeviceEnumerator::GetDevice(id)` and read
`PKEY_Device_FriendlyName`, which works for unplugged endpoints too. Give it item data that points into a second
`Vec<Device>` (for example `ACTIVE_COUNT + k`) and select it, so that Save without changes does not silently drop the user's choice.
The BTA30 PRO dongle itself is always active, but a classic BT headset is not.

### 5.4 Checkbox, path field, status

```rust
CheckDlgButton(hwnd, IDC_COMMS, if comms { BST_CHECKED } else { BST_UNCHECKED })?;
let comms = IsDlgButtonChecked(hwnd, IDC_COMMS) == BST_CHECKED.0;
SetDlgItemTextW(hwnd, IDC_PATH, &HSTRING::from(exe_path.as_str()))?;   // EDITTEXT ES_READONLY|ES_AUTOHSCROLL
SetDlgItemTextW(hwnd, IDC_STATUS, &HSTRING::from(msg))?;               // LTEXT with SS_NOPREFIX ('&' in names!)
```

Use `std::env::current_exe()` for the path. Once installed by the MSI it is `C:\Program Files\<Product>\toggle-audio.exe`.
A read-only EDIT is selectable and copyable with Ctrl+C on its own, and is readable by screen readers.

### 5.5 Copy button: clipboard (verified)

```rust
pub fn copy_to_clipboard(owner: HWND, text: &str) -> windows::core::Result<()> {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        OpenClipboard(Some(owner))?;                       // can fail transiently if another app holds it
        let result = (|| {
            EmptyClipboard()?;                              // makes `owner` the clipboard owner
            let hmem = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2)?;
            let dst = GlobalLock(hmem) as *mut u16;
            if dst.is_null() { return Err(windows::core::Error::from_thread()); }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), dst, wide.len());
            let _ = GlobalUnlock(hmem);                     // returns FALSE+NO_ERROR at lock count 0 -> ignore
            SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(hmem.0)))?; // system owns hmem on success
            Ok(())
        })();
        let _ = CloseClipboard();
        result
    }
}
```

There is a simpler equivalent that needs no extra features: `SendDlgItemMessageW(hwnd, IDC_PATH, EM_SETSEL, WPARAM(0), LPARAM(-1))`
then `SendDlgItemMessageW(hwnd, IDC_PATH, WM_COPY, ...)`. A read-only edit supports `WM_COPY`. Either is fine; the
explicit version leaves the edit's selection unchanged. On success, set the status to
"Path copied. In G HUB: assign the key to System > Launch Application." In G HUB the path goes in the Launch Application
"Path" field, with no arguments for "toggle".

### 5.6 Test toggle and Save

- **Test toggle**: call the same library function the CLI uses, with the *currently selected, unsaved* pair and comms flag.
  Then show `"Default is now: <name>"` (or the error) in the status line. It runs synchronously on the UI thread
  (~ms), so no worker thread is needed. Do not persist anything.
- **Save (IDOK)**: validate (both selected and different), write the config atomically (write `config.toml.tmp`, then
  `MoveFileExW(.., MOVEFILE_REPLACE_EXISTING)`) to `%APPDATA%\<product>\config.toml`. Never write under Program Files.
  Then call `EndDialog(hwnd, IDOK)`. On validation failure, put the message in the status line and `SetFocus` the offending combo.
- **Cancel / Esc**: `EndDialog(hwnd, IDCANCEL)`.

### 5.7 Icons, centering, single instance

```rust
let icon = LoadIconMetric(Some(hinst), PCWSTR(IDI_APP as usize as *const u16), LIM_SMALL)?; // DPI-correct size
SendMessageW(hwnd, WM_SETICON, Some(WPARAM(ICON_SMALL as usize)), Some(LPARAM(icon.0 as isize)));
// same with LIM_LARGE / ICON_BIG
```

To center, take `GetCursorPos`, then `MonitorFromPoint(.., MONITOR_DEFAULTTONEAREST)`, `GetMonitorInfoW().rcWork` and
`GetWindowRect`, then `SetWindowPos(.., SWP_NOSIZE|SWP_NOZORDER|SWP_NOACTIVATE)` in `WM_INITDIALOG` (code in dlg.rs).
This places the dialog on the monitor the user is looking at. `DS_CENTER` instead picks the owner's monitor or a
system-chosen one. If the target monitor has a different DPI, PMv2 sends `WM_DPICHANGED` and the dialog manager rescales.

For a single instance, call `FindWindowW(w!("#32770"), w!("Toggle Audio Settings"))` before creating the dialog. If it
finds one, `SetForegroundWindow` it and exit. That prevents two settings dialogs when the user double-clicks the shortcut twice.

### 5.8 DPI

The resource dialog needs no DPI code. Under PerMonitorV2, `DialogBoxParamW` creates the dialog at the monitor's DPI.
DLUs scale with the DPI-scaled font, and on `WM_DPICHANGED` (return FALSE) the dialog manager resizes, repositions
controls and sends them a new font.
Only *custom-drawn* elements need `GetDpiForWindow(hwnd)` and `GetSystemMetricsForDpi`, and this UI has none.

---

## 6. Fallback: CreateWindowExW (`src/bin/wnd.rs`, compiled and measured)

Use this only if a dynamic layout is needed. It shows everything the dialog manager otherwise does:

- **Class**: `WNDCLASSEXW { lpfnWndProc, hIcon: LoadIconW(hinst, IDI_APP), hCursor: IDC_ARROW, hbrBackground: COLOR_BTNFACE+1 }`.
  BTNFACE because STATIC children paint that colour; with COLOR_WINDOW you must handle `WM_CTLCOLORSTATIC`.
- **Main window**: `WS_OVERLAPPED|WS_CAPTION|WS_SYSMENU|WS_MINIMIZEBOX` with `WS_EX_CONTROLPARENT`. Create it hidden, read
  `GetDpiForWindow(hwnd)`, compute the outer size with `AdjustWindowRectExForDpi(client, style, false, exstyle, dpi)`, center it, then `ShowWindow`.
- **Children**: `CreateWindowExW(0, WC_COMBOBOXW, .., WS_CHILD|WS_VISIBLE|CBS_DROPDOWNLIST|WS_VSCROLL|WS_TABSTOP, .., Some(parent), Some(HMENU(id as isize as *mut _)), ..)`.
  The control ID goes in the hMenu slot. For a drop-down list, the height you pass is field plus dropped list (pass ~200 logical px);
  the field height follows the font.
- **Scale helper**: `fn s(&self, v: i32) -> i32 { (v * self.dpi as i32 + 48) / 96 }` (MulDiv rounding). All layout
  constants are 96-DPI logical px on a fixed grid: margin 12, row height 23, label width 70, button width 88, gap 8.
- **Font**: `SystemParametersInfoForDpi(SPI_GETNONCLIENTMETRICS.0, size, Some(&mut ncm as *mut _ as _), 0, dpi)` then
  `CreateFontIndirectW(&ncm.lfMessageFont)`. Apply it with `EnumChildWindows` and `WM_SETFONT(font, TRUE)`, and delete the old HFONT.
  The fallback is a `LOGFONTW` "Segoe UI" with `lfHeight = -MulDiv(9, dpi, 72)`.
- **WM_DPICHANGED**: `dpi = LOWORD(wParam)`; recreate and apply the font; `SetWindowPos(hwnd, None, rc.left, rc.top, w, h, SWP_NOZORDER|SWP_NOACTIVATE)`
  with the suggested `*(lParam as *const RECT)`; `layout()`.
- **Loop**: `while GetMessageW(&mut msg, None, 0, 0).as_bool() { if !IsDialogMessageW(hwnd, &msg).as_bool() { TranslateMessage; DispatchMessageW } }`.
  `IsDialogMessageW` handles Tab, arrows and mnemonics. Enter sends `WM_COMMAND(IDOK)` and Esc sends `WM_COMMAND(IDCANCEL)`,
  so give Save the ID `IDOK` and Cancel the ID `IDCANCEL`. Known gap: the `BS_DEFPUSHBUTTON` border does not move with focus
  as it does in a real dialog.
- **State**: `Box<Ui>` stored in `GWLP_USERDATA`, freed in `WM_DESTROY`, followed by `PostQuitMessage(0)`.

**Font finding on this zh-TW machine:** `lfMessageFont` is **"Microsoft JhengHei UI", -12 px at 96 DPI**, not Segoe UI.
Two approaches look native:
(a) the template font `"Segoe UI"` 9 pt. Latin text renders in Segoe UI and CJK glyphs come through font linking
(`HKLM\...\FontLink\SystemLink\Segoe UI` = Tahoma, then **Microsoft JhengHei UI**, ...). This is verified in the screenshot.
(b) the locale's message font (JhengHei UI for Latin too). The spec asks for Segoe UI, so the recommended dialog uses (a).
Do **not** use `"MS Shell Dlg 2"`: on this machine it maps to Tahoma.

---

## 7. Dark mode: not in v1

`DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE /*20*/, &BOOL(1), 4)` is public, documented (Win11, and
Win10 20H1+), and works. It only darkens the **title bar**. Win32 common controls (buttons, checkboxes, combo boxes, edit,
dialog background) have **no public dark-mode API**. Full dark mode depends on undocumented uxtheme ordinals
(`#135 SetPreferredAppMode`, `#133 AllowDarkModeForWindow`) and `SetWindowTheme(L"DarkMode_Explorer")`, plus owner-draw
for checkboxes and static text. That is fragile across Windows builds
([background](https://zenn.dev/tenka/articles/win32_darkmode)). A dark title bar over light controls looks unfinished.
**Recommendation: ship v1 light-only, like most Win32 utilities (Control Panel, mmsys.cpl, nirsoft).**
`set_dark_title_bar()` is in dlg.rs, unused, in case it is wanted later.

---

## 8. Integration with the CLI crate

- Use one crate and **one exe**, `toggle-audio.exe`, with `#![windows_subsystem = "windows"]` so G HUB launches never flash a console.
  - No args: toggle (fast path; the GUI code is never touched, so toggle latency is unaffected).
  - `--settings`: `run_settings()`.
  - No args and no config yet: open settings (first run).
  - `--list` and other text output: `AttachConsole(ATTACH_PARENT_PROCESS)` when launched from a terminal.
- The MSI creates a Start-menu shortcut "Toggle Audio Settings" pointing to `toggle-audio.exe --settings`, and can
  launch it at the end of setup.
- The `IDC_PATH` field then shows exactly the exe G HUB must launch, which is `current_exe()`.
- Put GUI code in `src/settings/` (`mod.rs` with the dialog proc, `ids.rs`). Device enumeration and toggling belong to the shared
  `audio` module the CLI uses. The prototype's `list_render_devices()` (5 endpoints in ~4 ms) can be reused as is.
- If two binaries are preferred instead, embed-resource links the .res into every bin. That is fine, since both need the manifest.

## 9. Pitfalls (hit or checked during prototyping)

1. `cargo +stable` on this PC is 1.72, which fails on windows 0.62 (MSRV 1.82). Pin with `rust-toolchain.toml` (`channel = "stable"`, plus `rust-version` in Cargo.toml).
2. `PROPVARIANT`'s `Display`/`Drop` impls only exist with **both** `Win32_System_Com_StructuredStorage` and `Win32_System_Variant`.
3. `IMMDevice::GetId()` returns a CoTaskMem `PWSTR`: copy it to a `String`, then call `CoTaskMemFree(Some(p.0 as _))`.
4. `DialogBoxParamW` returns −1 if the template is missing, for example when resources were not linked. Treat that as a fatal error with a MessageBox.
5. Status and label statics showing device names need `SS_NOPREFIX`, otherwise `&` in a name becomes an underline.
6. `GlobalUnlock` reports `Err` (BOOL FALSE with NO_ERROR) at lock count 0: ignore its result.
7. `WM_INITDIALOG` must return TRUE (default focus) unless you `SetFocus` yourself, in which case return FALSE.
8. Never block the UI thread on anything slow. Toggling and enumeration are both a few ms; if a BT endpoint ever hangs
   `SetDefaultEndpoint`, move Test toggle to a worker thread and `PostMessageW(WM_APP)` back.
9. Do not register the dialog as topmost. G HUB launches it in the background; with an unowned top-level window plus
   `WS_EX_APPWINDOW` Windows may flash the taskbar instead of activating. That is acceptable, and `SetForegroundWindow` is
   allowed when the user just clicked a shortcut.

## 10. Manual test checklist

100/125/150/200% scaling, and dragging between monitors with different scales (layout and font must rescale crisply);
keyboard only (Tab order, Alt+1/2/C/Y/T/S, Enter=Save, Esc=Cancel); Narrator reads "Device 1, combo box, <name>";
High Contrast theme; names containing `&` and CJK; zero active devices (Save disabled or warning); a saved BT device
switched off (shown as "(not connected)" and selection kept); running a second `--settings` (activates the existing window).

## Sources

- windows crate versions and MSRV: https://crates.io/api/v1/crates/windows/versions ; API docs: https://microsoft.github.io/windows-docs-rs/
- embed-resource: https://docs.rs/embed-resource/latest/embed_resource/ , https://crates.io/crates/embed-resource
- winresource: https://crates.io/crates/winresource ; winres: https://crates.io/crates/winres ; embed-manifest: https://crates.io/crates/embed-manifest
- native-windows-gui: https://github.com/gabdube/native-windows-gui , https://crates.io/crates/native-windows-gui
- winsafe: https://github.com/rodrigocfd/winsafe , https://docs.rs/winsafe/latest/winsafe/gui/index.html
- PMv2 behaviours (dialog scaling, non-client scaling, comctl32): https://learn.microsoft.com/en-us/windows/win32/hidpi/dpi-awareness-context
- Dialog DPI behaviour: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setdialogdpichangebehavior
- Visual styles / comctl v6 manifest: https://learn.microsoft.com/en-us/windows/win32/controls/cookbook-overview
- DPI awareness in manifests: https://learn.microsoft.com/en-us/windows/win32/hidpi/setting-the-default-dpi-awareness-for-a-process
- Layout metrics (DLU margins and button sizes): https://learn.microsoft.com/en-us/windows/win32/uxguide/vis-layout
- Win32 dark mode (undocumented uxtheme ordinals): https://zenn.dev/tenka/articles/win32_darkmode
- Slint licensing: https://slint.dev/pricing
