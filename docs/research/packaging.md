# Packaging plan: Toggle Audio (Rust CLI + settings GUI) for Windows

Status: researched on 2026-10-04. The example `.wxs` in section 4 was **built and ICE-validated on this machine** with WiX 7.0.0 (`wix build` exit 0, `wix msi validate` exit 0). Validated copies are in:
- `research\toggle-audio.wxs` (the file to adopt)
- `research\wix-test\` (scratch build: `out\toggle-audio-0.1.0-x64.msi`, `out\decompiled.wxs`, `classic.wxs` = the alternative shortcut pattern, dummy payloads, `app.ico`, `License.rtf`)

The WiX v7 dotnet tool is now installed per-user (`%USERPROFILE%\.dotnet\tools\wix.exe` 7.0.0+b8977d6). The EULA is accepted (`wix eula accept wix7`), and `WixToolset.UI.wixext` and `WixToolset.Util.wixext` 7.0.0 are in the global extension cache. No admin rights were needed for any of this.

---

## 1. Decision

**Primary: an MSI built with WiX Toolset v7 (`wix` .NET global tool, 7.0.0, released 2026-04-06), using a hand-authored `.wxs` and called directly from a PowerShell build script and from GitHub Actions. No cargo-wix.**
**Secondary channels, all pointing at the same release artifacts: winget (the main "something better"), a Scoop bucket (portable zip, no admin), and optionally Chocolatey later.**

Why:
- The user asked for an MSI that installs properly into `C:\Program Files`. A per-machine MSI does that natively. It gives clean upgrades through MajorUpgrade, a clean uninstall, an ARP entry, and silent installs for winget and GPO/Intune.
- WiX v4+ uses one `wix build` command and one schema (`http://wixtoolset.org/schemas/v4/wxs`). The same `.wxs` is accepted by v4, v5, v6 and v7. v7 is the current supported release. v3/v4 went out of support in Feb 2025.
- WiX v6+ binaries are under the Open Source Maintenance Fee (OSMF) EULA. **v7 uses OSMF EULA v1.1, which charges nothing until you earn at least US$10,000/yr from projects that use WiX.** This hobby OSS project is therefore free. v7 needs an explicit acceptance step: `-acceptEula wix7` on the command line, or a one-time `wix eula accept wix7`. If you want to avoid any EULA at all, WiX v5.0.2 (MS-RL) builds the same `.wxs`. That is untested here, but nothing in it is v6/v7-specific. Pin the version either way.
- .NET 8+ runtime is required. It is present locally, and the dotnet SDK is preinstalled on GitHub windows runners.

### Comparison

| Option | Output | Installs to Program Files | Signing needed | Status / notes | Verdict |
|---|---|---|---|---|---|
| **WiX v3.14** (candle/light) | MSI | yes | no (SmartScreen warns) | Legacy; out of support (Feb 2025). Old schema. Still preinstalled on GitHub `windows-latest` per the cargo-wix README (Apr 2026). | No: dead end |
| **WiX v5 / v6 / v7** (`dotnet tool install -g wix`) | MSI (and Burn bundles) | yes | no (SmartScreen warns) | v7.0.0 is current. Single `wix build`. v6+ under OSMF EULA (v7: no fee below $10k/yr). | **Primary (v7)** |
| **cargo-wix** | MSI | yes | no | Now supports both "Legacy" v3.14.1 (**still the default**) and "Modern" v4+ (opt-in). It generates a template and its own conventions, an abstraction over about 100 lines of XML we would own anyway. cargo-dist's MSI path is also built on cargo-wix/WiX v3. | Skip. Call `wix` directly. |
| **Inno Setup 6** | EXE installer | yes | no (SmartScreen warns) | Excellent and simple Pascal-script installer. Not an MSI. winget supports `InstallerType: inno`. | Fallback if MSI is ever dropped |
| **MSIX** | .msix | No: installs under `WindowsApps` (container). G HUB needs a stable exe path, which would need an App Execution Alias. | **Mandatory** trusted signature (self-signed only with manual cert import) | The Store re-signs for free, but needs Partner Center plus certification. Virtualization adds subtle per-user behaviour. | No |
| **winget** (winget-pkgs) | manifest -> our MSI | yes (runs the MSI) | not required for submission | Users run `winget install <Id>`. Upgrades correlate via ProductCode/UpgradeCode. | **Yes: the "something better"** |
| **Scoop** (own bucket) | manifest -> zip | No: `~\scoop\apps`, per-user, no admin | no | Great for developers. Ship a portable zip of the exes. | Optional, cheap |
| **Chocolatey** | nupkg wrapping MSI | yes | no | Community moderation queue (days). Extra maintenance. | Later / optional |

Sources:
- WiX tool on NuGet (7.0.0, 2026-04-06; requires .NET 8): https://www.nuget.org/packages/wix
- WiX v7 release: https://www.firegiant.com/blog/2026/4/6/wix-v7-heatwave-and-heatwave-build-tools-are-released/
- WiX v7 RC notes (EULA v1.1, US$10k threshold, `wix eula accept wix7`, `-acceptEula wix7`, Heat removed, `Files` relative-path change): https://www.firegiant.com/blog/2026/2/6/wix-v700-rc1-is-here/
- WiX v6 and OSMF: https://firegiant.com/blog/2025/4/7/wix-v600-available
- cargo-wix (Legacy default, Modern opt-in, runner notes): https://github.com/volks73/cargo-wix

---

## 2. Toolchain setup (no admin)

```powershell
dotnet tool install --global wix --version 7.0.0      # pin exactly; bump deliberately
wix eula accept wix7                                   # one-time per machine/runner (or pass -acceptEula wix7)
wix extension add -g WixToolset.UI.wixext/7.0.0        # extension version MUST match wix.exe major
wix extension add -g WixToolset.Util.wixext/7.0.0      # only if WixShellExec / util:* is used later
wix --version                                          # 7.0.0+b8977d6
```
In CI, add `$env:USERPROFILE\.dotnet\tools` to PATH (`"$env:USERPROFILE\.dotnet\tools" >> $env:GITHUB_PATH`).

Alternative: an SDK-style `packaging\wix\ToggleAudio.wixproj` (`<Project Sdk="WixToolset.Sdk/7.0.0">`) with `dotnet build`. FireGiant recommends it. For a single `.wxs` the CLI is simpler and more transparent, so this plan uses the CLI.

---

## 3. Authoring spec (WiX v4+ schema, built with v7)

Repo layout:
```
packaging/wix/toggle-audio.wxs     # the file in section 4
packaging/wix/License.rtf          # generated from LICENSE (see 3.9)
packaging/wix/app.ico              # copy of assets/toggle-audio.ico (multi-size 16/20/24/32/48/64/256)
scripts/build-msi.ps1              # reads version from Cargo.toml, builds, validates, hashes
```

### 3.1 Package element
- `Name="Toggle Audio"`: placeholder; the final project name lands later. The ARP display name equals this. winget `PackageName` should match it.
- `Manufacturer="Toggle Audio contributors"`, or the maintainer's name/handle. This is the ARP "Publisher" and must match winget `Publisher`.
- `Version="$(var.Version)"`, passed in as `-d Version=X.Y.Z`. **MSI ProductVersion rules:**
  - The format is `major.minor.build[.revision]`, with major/minor <= 255 and build <= 65535.
  - **MajorUpgrade ignores the 4th field.**
  - SemVer pre-release suffixes (`0.2.0-beta.1`) are invalid. The build script must strip them.
  - Never publish two different MSIs with the same 3-part version.
- `UpgradeCode="C82A4013-F2FF-448E-A4AE-63CD73760A63"`: generated for this project on 2026-10-04. **Constant forever** (see section 9).
- `Scope="perMachine"`: the v4+ default, stated explicitly. It sets `ALLUSERS=1` and requires elevation (verified: Property `ALLUSERS` is present).
- `InstallerVersion="500"` (Windows Installer 5.0, the v4+ default). Verified: summary info PageCount = 500.
- `ProductCode`: **do not author it.** WiX generates a fresh one per build, which major upgrades require. Verified: the 0.1.0 build got `{C3FC30DB-8D6C-4F20-8C33-018932D89C33}` and the 0.2.0 build got `{73F863B3-237F-41A2-A953-EAFB3DDD44D2}`.
- `Compressed="yes"` and `<MediaTemplate EmbedCab="yes" />` give a single-file MSI (about 200 KB with dummy payloads).
- `-arch x64` on the command line does three things:
  - marks the package x64 (summary `Template = x64;1033`, verified)
  - makes components 64-bit (`Bitness="always64"`, verified)
  - makes `ProgramFiles64Folder` resolve to `C:\Program Files`

  Do not set Platform attributes in the source.

Version from Cargo.toml (PowerShell):
```powershell
$ver = ((cargo metadata --no-deps --format-version 1 | ConvertFrom-Json).packages |
        Where-Object name -eq 'toggle-audio').version
$msiVer = ($ver -split '[-+]')[0]          # strip pre-release/build metadata
```

### 3.2 MajorUpgrade
`<MajorUpgrade Schedule="afterInstallInitialize" DowngradeErrorMessage="A newer version of [ProductName] is already installed. ..." />`
- With `afterInstallInitialize`, the old product is fully removed before the new one is installed. This is simplest and robust, and it does not depend on perfect component rules.
- Verified sequencing in the built MSI: `InstallInitialize 1500`, `RemoveExistingProducts 1501`. Note: `wix msi decompile` misreports it as `afterInstallFinalize`. Trust the table, not the decompiler.
- Leave `AllowSameVersionUpgrades` off. Every release bumps the 3-part version.
- Upgrades keep the same `INSTALLFOLDER`, so the G HUB binding to `C:\Program Files\Toggle Audio\toggle-audio.exe` survives. **Never put the version in the directory name.**

### 3.3 Directories
```xml
<StandardDirectory Id="ProgramFiles64Folder">
  <Directory Id="INSTALLFOLDER" Name="Toggle Audio" />
</StandardDirectory>
```
`ProgramMenuFolder` is referenced implicitly by the shortcut's `Directory` attribute. Do not declare it again.

### 3.4 Components (and GUID rules)
- One component per executable, with the exe as the key path:
  - `ToggleAudioExe` (`toggle-audio.exe`, the thing G HUB launches)
  - `SettingsExe` (`toggle-audio-settings.exe`)

  If the GUI is a subcommand of one binary (`toggle-audio.exe settings`), delete `SettingsExe` and move the `<Shortcut>` under `ToggleAudioExe`'s `<File>` with `Arguments="settings"`.
- **Component GUIDs are omitted.** In v4+, omitting them means "auto". WiX derives a deterministic GUID from directory + key path. Verified identical across the 0.1.0 and 0.2.0 builds:
  - `ToggleAudioExe` = `{0F65088D-968D-5CA8-B1BB-90D1122EBF25}`
  - `SettingsExe` = `{5207E431-66D4-5C0E-8A4B-61B49E82720E}`
  - `PathEntry` = `{84CB066F-799F-55D3-9913-BE0D06A0C789}`

  This is safer than hand-written GUIDs because the GUID changes exactly when the component rules say it must, that is, when the key path or directory changes.
- If someone prefers explicit GUIDs: freeze them, and **generate a new one whenever a component's key-path file name, target directory, or key-path registry key changes.** Never reuse a GUID for different content. Never move a file between components while keeping the GUID.

### 3.5 Start Menu shortcut to the settings GUI
**Chosen pattern: an advertised shortcut nested in the settings exe's `<File>`, placed directly in `ProgramMenuFolder` (no subfolder).**
```xml
<File Id="SettingsExe" Source="$(var.BinDir)\toggle-audio-settings.exe" KeyPath="yes">
  <Shortcut Id="SettingsStartMenuShortcut" Directory="ProgramMenuFolder"
            Name="Toggle Audio Settings" WorkingDirectory="INSTALLFOLDER"
            Icon="AppIcon.ico" Advertise="yes" />
</File>
```
Test results on this machine:
- **Non-advertised** shortcut in the file component: `wix msi validate` **fails** with:
  - `ICE43: Component SettingsExe has non-advertised shortcuts. It should use a registry key under HKCU as its KeyPath, not a file.`
  - `ICE57: Component 'SettingsExe' has both per-user and per-machine data with a per-machine KeyPath.`
- **Advertised** shortcut (above): validates clean. An advertised shortcut runs a quick MSI health check on launch. It triggers a repair (with UAC) only if key files were deleted.
- **Classic "shortcut-removal registry key" pattern** (`research\wix-test\classic.wxs`): a separate component in the Start menu dir with an HKCU keypath. It validates with one benign warning, ICE69 (mismatched component reference). Use it only if a non-advertised shortcut is required:
```xml
<Component Id="StartMenuShortcut" Directory="ProgramMenuFolder">   <!-- or a subfolder Directory -->
  <Shortcut Id="SettingsStartMenuShortcut" Name="Toggle Audio Settings"
            Target="[#SettingsExe]" WorkingDirectory="INSTALLFOLDER" Icon="AppIcon.ico" />
  <!-- if the shortcut lives in a subfolder of ProgramMenuFolder, also add:
       <RemoveFolder Id="RemoveAppStartMenuDir" Directory="AppStartMenuDir" On="uninstall" /> -->
  <RegistryValue Root="HKCU" Key="Software\Toggle Audio" Name="StartMenuShortcut"
                 Type="integer" Value="1" KeyPath="yes" />
</Component>
```
Its downside: the HKCU keypath is written only for the installing user.

Skip a Start-menu subfolder. One shortcut belongs in the root, which needs no RemoveFolder. Also skip a desktop shortcut.

### 3.6 App Paths (Win+R / `start toggle-audio`)
In the `ToggleAudioExe` component (the file stays the key path):
```xml
<RegistryKey Root="HKLM" Key="SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\toggle-audio.exe">
  <RegistryValue Type="string" Value="[#ToggleAudioExe]" />          <!-- (Default) = full exe path -->
  <RegistryValue Type="string" Name="Path" Value="[INSTALLFOLDER]" />
</RegistryKey>
```
App Paths works for ShellExecute (Win+R, Start search, `Start-Process`), but not for cmd/pwsh command lookup. That is what PATH is for. The component is 64-bit, so the key is written to the 64-bit hive.

### 3.7 PATH (optional feature)
```xml
<Feature Id="EnvironmentPath" Title="Add to PATH" Level="1" AllowAbsent="yes">
  <Component Id="PathEntry" Directory="INSTALLFOLDER">
    <Environment Id="PathEntry" Name="PATH" Value="[INSTALLFOLDER]" Part="last" Action="set" System="yes" Permanent="no" />
    <RegistryValue Root="HKLM" Key="SOFTWARE\Toggle Audio" Name="PathEntry" Type="integer" Value="1" KeyPath="yes" />
  </Component>
</Feature>
```
- This is a separate feature, so it can be deselected. It is **on by default.**
- WixUI_Minimal shows no feature tree, so users opt out from the command line: `msiexec /i toggle-audio.msi ADDLOCAL=Main`. winget users can pass `--custom "ADDLOCAL=Main"`.
- To expose a checkbox, switch the UI to `WixUI_FeatureTree` (more dialogs). Recommendation: keep Minimal. The app is mostly launched by G HUB, and App Paths covers Win+R.
- `Permanent="no"` removes only our segment on uninstall. MSI broadcasts WM_SETTINGCHANGE, so new consoles see the change. Already-open shells do not.

### 3.8 ARP properties and icon
```xml
<Icon Id="AppIcon.ico" SourceFile="$(var.AssetsDir)\app.ico" />      <!-- Id must end in .ico when used by shortcuts -->
<Property Id="ARPPRODUCTICON"   Value="AppIcon.ico" />
<Property Id="ARPURLINFOABOUT"  Value="https://github.com/OWNER/toggle-audio" />
<Property Id="ARPHELPLINK"      Value="https://github.com/OWNER/toggle-audio/issues" />
<Property Id="ARPURLUPDATEINFO" Value="https://github.com/OWNER/toggle-audio/releases" />
```
- **Gotcha (verified): `WixUI_Minimal` already defines `ARPNOMODIFY=1`.** Authoring it again fails the build with `WIX0091: Duplicate Property with identifier 'ARPNOMODIFY'`. Add it yourself only with `WixUI_InstallDir`/`WixUI_FeatureTree` or no UI. Keep Repair enabled.
- Embed the same `.ico` into both exes as a Win32 resource (`embed-resource` or `winresource` crate in `build.rs`), so taskbar, Explorer and the G HUB picker show it. Also give the exes a VERSIONINFO resource with the same version.

### 3.9 UI and license
- v4+ syntax: declare `xmlns:ui="http://wixtoolset.org/schemas/v4/wxs/ui"` on `<Wix>`, use `<ui:WixUI Id="WixUI_Minimal" />`, and build with `-ext WixToolset.UI.wixext`. The extension must be in the global cache (`wix extension add -g`) or referenced by path.
- Folder-picker variant: `<ui:WixUI Id="WixUI_InstallDir" InstallDirectory="INSTALLFOLDER" />`.
- License: `<WixVariable Id="WixUILicenseRtf" Value="$(var.AssetsDir)\License.rtf" />`. The RTF must be plain ANSI RTF with non-ASCII escaped (`\'hh` or `\uN?`). Generate it from `LICENSE` in the build script:
  - start with `{\rtf1\ansi\deff0{\fonttbl{\f0 Segoe UI;}}\f0\fs18 `
  - add each line with `\`, `{` and `}` escaped, joined by `\par `
  - end with `}`
- Recommendation: **WixUI_Minimal** (license, Install, done). For a G HUB utility, the folder picker adds nothing and only risks breaking the documented exe path.
- Optional "Launch settings now" exit-dialog checkbox needs `WixToolset.Util.wixext` `WixShellExec`. Recommendation: skip it. Have `toggle-audio.exe` open the settings GUI itself when no config exists. That also covers winget/silent installs.
- Optional branding bitmaps: `WixUIBannerBmp` (493x58) and `WixUIDialogBmp` (493x312).

### 3.10 Build command (exact)
```powershell
wix build -arch x64 -acceptEula wix7 -ext WixToolset.UI.wixext `
  -d Version=$msiVer -d BinDir=target\release -d AssetsDir=packaging\wix `
  -o dist\toggle-audio-$ver-x64.msi packaging\wix\toggle-audio.wxs
wix msi validate dist\toggle-audio-$ver-x64.msi
```
- The build takes about 0.8 s locally. It produces the `.msi` plus a `.wixpdb`; do not ship the `.wixpdb`.
- Pitfall: XML comments cannot contain `--`. Writing `cargo build --release` inside a comment fails with WIX0104.

---

## 4. Complete example .wxs (validated with WiX 7.0.0; adapt verbatim)

Identical to `research\toggle-audio.wxs`:

```xml
<?xml version="1.0" encoding="utf-8"?>
<!--
  Toggle Audio - MSI authoring (WiX Toolset v4/v5/v6/v7 schema; built and validated with WiX 7.0.0).

  Build (from repo root, after `cargo build -r`):
    wix build -arch x64 -acceptEula wix7 -ext WixToolset.UI.wixext ^
      -d Version=0.1.0 -d BinDir=target\release -d AssetsDir=packaging\wix ^
      -o dist\toggle-audio-0.1.0-x64.msi packaging\wix\toggle-audio.wxs

  GUID RULES (read before editing):
    * UpgradeCode below is the product family identity. NEVER change it, ever.
    * ProductCode is NOT authored: WiX generates a new one per build (ProductCode="*" default),
      which is what MajorUpgrade needs. Do not hard-code it.
    * Component GUIDs are omitted on purpose: WiX derives them deterministically from the
      component's directory + key path, so they are stable across builds as long as the
      file name / install directory / registry key path stays the same. If you hard-code a
      GUID instead, it must change whenever the key path or target directory changes.
-->
<?ifndef Version?>
  <?error Pass -d Version=X.Y.Z (from Cargo.toml) ?>
<?endif?>
<?ifndef BinDir?>
  <?define BinDir = "." ?>
<?endif?>
<?ifndef AssetsDir?>
  <?define AssetsDir = "." ?>
<?endif?>
<?define ProductName = "Toggle Audio" ?>
<?define Manufacturer = "Toggle Audio contributors" ?>
<?define RepoUrl = "https://github.com/OWNER/toggle-audio" ?>
<?define UpgradeCode = "C82A4013-F2FF-448E-A4AE-63CD73760A63" ?>

<Wix xmlns="http://wixtoolset.org/schemas/v4/wxs"
     xmlns:ui="http://wixtoolset.org/schemas/v4/wxs/ui">

  <Package Name="$(var.ProductName)"
           Manufacturer="$(var.Manufacturer)"
           Version="$(var.Version)"
           UpgradeCode="$(var.UpgradeCode)"
           Scope="perMachine"
           InstallerVersion="500"
           Language="1033"
           Compressed="yes">

    <SummaryInformation Description="$(var.ProductName) $(var.Version) installer" />

    <MajorUpgrade Schedule="afterInstallInitialize"
                  DowngradeErrorMessage="A newer version of [ProductName] is already installed. Uninstall it first if you really want to downgrade." />

    <MediaTemplate EmbedCab="yes" CompressionLevel="high" />

    <!-- Icon: Id must end in .ico (same extension as the source) because shortcuts reference it. -->
    <Icon Id="AppIcon.ico" SourceFile="$(var.AssetsDir)\app.ico" />

    <!-- Add/Remove Programs (Settings > Apps) properties -->
    <Property Id="ARPPRODUCTICON" Value="AppIcon.ico" />
    <Property Id="ARPURLINFOABOUT" Value="$(var.RepoUrl)" />
    <Property Id="ARPHELPLINK" Value="$(var.RepoUrl)/issues" />
    <Property Id="ARPURLUPDATEINFO" Value="$(var.RepoUrl)/releases" />
    <!-- ARPNOMODIFY=1 is already set by WixUI_Minimal (defining it again = WIX0091 duplicate).
         Add <Property Id="ARPNOMODIFY" Value="1" /> only when using WixUI_InstallDir / no UI. -->

    <!-- UI: license page + install. Swap for WixUI_InstallDir to let the user pick the folder:
         <ui:WixUI Id="WixUI_InstallDir" InstallDirectory="INSTALLFOLDER" /> -->
    <ui:WixUI Id="WixUI_Minimal" />
    <WixVariable Id="WixUILicenseRtf" Value="$(var.AssetsDir)\License.rtf" />
    <!-- Optional branding (493x58 and 493x312 BMP):
    <WixVariable Id="WixUIBannerBmp" Value="$(var.AssetsDir)\banner.bmp" />
    <WixVariable Id="WixUIDialogBmp" Value="$(var.AssetsDir)\dialog.bmp" /> -->

    <!-- C:\Program Files\Toggle Audio -->
    <StandardDirectory Id="ProgramFiles64Folder">
      <Directory Id="INSTALLFOLDER" Name="$(var.ProductName)" />
    </StandardDirectory>

    <!-- ===== Main feature (always installed) ===== -->
    <Feature Id="Main" Title="$(var.ProductName)" Level="1" AllowAbsent="no" Display="expand">

      <!-- The toggle CLI (what G HUB launches). Keypath = the exe. -->
      <Component Id="ToggleAudioExe" Directory="INSTALLFOLDER">
        <File Id="ToggleAudioExe" Source="$(var.BinDir)\toggle-audio.exe" KeyPath="yes" />
        <!-- App Paths: makes Win+R / Start "toggle-audio" work without PATH. -->
        <RegistryKey Root="HKLM" Key="SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\toggle-audio.exe">
          <RegistryValue Type="string" Value="[#ToggleAudioExe]" />
          <RegistryValue Type="string" Name="Path" Value="[INSTALLFOLDER]" />
        </RegistryKey>
      </Component>

      <!-- Settings GUI (separate exe). If the GUI is the same binary (`toggle-audio.exe settings`),
           delete this component and point the shortcut at [#ToggleAudioExe] with Arguments="settings". -->
      <Component Id="SettingsExe" Directory="INSTALLFOLDER">
        <File Id="SettingsExe" Source="$(var.BinDir)\toggle-audio-settings.exe" KeyPath="yes">
          <!-- ADVERTISED shortcut as a child of the File (passes ICE43/ICE57; a non-advertised one here fails validation). Lives in the all-users Start menu root,
               is removed with the component, and needs no HKCU keypath / RemoveFolder. -->
          <Shortcut Id="SettingsStartMenuShortcut"
                    Directory="ProgramMenuFolder"
                    Name="$(var.ProductName) Settings"
                    Description="Choose the two playback devices to toggle between"
                    WorkingDirectory="INSTALLFOLDER"
                    Icon="AppIcon.ico"
                    Advertise="yes" />
        </File>
      </Component>
    </Feature>

    <!-- ===== Optional: add install dir to the system PATH (for cmd/pwsh use). =====
         Default ON. Deselect from the command line with:
           msiexec /i toggle-audio.msi ADDLOCAL=Main
         (WixUI_Minimal shows no feature tree; use WixUI_FeatureTree if a checkbox is wanted.) -->
    <Feature Id="EnvironmentPath" Title="Add to PATH" Level="1" AllowAbsent="yes">
      <Component Id="PathEntry" Directory="INSTALLFOLDER">
        <Environment Id="PathEntry" Name="PATH" Value="[INSTALLFOLDER]"
                     Part="last" Action="set" System="yes" Permanent="no" />
        <!-- A component needs a keypath; use an HKLM value under our own key. -->
        <RegistryValue Root="HKLM" Key="SOFTWARE\$(var.ProductName)" Name="PathEntry"
                       Type="integer" Value="1" KeyPath="yes" />
      </Component>
    </Feature>
  </Package>
</Wix>
```

To adapt it:
- Replace `OWNER`, `ProductName` and `Manufacturer` once the rename is final.
- **Keep `UpgradeCode`.**
- Put the real `app.ico` and `License.rtf` in `AssetsDir`.
- Keep the exe file names in sync with the Cargo `[[bin]]` names.

---

## 5. Validating without admin rights

1. **ICE validation:** `wix msi validate x.msi` runs the standard ICE suite (darice.cub, bundled with WiX) and works unelevated. Exit 0 = clean (verified). Run it in CI after every build.
2. **Table inspection:** `wix msi decompile x.msi -o x.decompiled.wxs`, then grep for ProductCode, UpgradeCode, component GUIDs, Shortcut, Environment and App Paths.
   - Caveat: the decompiler prints the MajorUpgrade `Schedule` wrongly. Check `InstallExecuteSequence` for `RemoveExistingProducts` = InstallInitialize + 1.
   - Read-only COM works unelevated: `$i = New-Object -ComObject WindowsInstaller.Installer; $db = $i.OpenDatabase($path, 0); $v = $db.OpenView('SELECT ...'); $v.Execute(); $r = $v.Fetch(); $r.StringData(1)`.
3. **Payload extraction:** `msiexec /a x.msi /qn TARGETDIR=C:\tmp\ta-admin` runs an administrative install. It is file extraction only and normally needs no elevation.
   - *Here it returned 1618 ("another installation is in progress") because a system msiexec session was busy. Retry later.*
   - Alternatives: `lessmsi x x.msi out\` (`winget install lessmsi` or `scoop install lessmsi`), or Orca from the Windows SDK (`Windows Kits\10\bin\<ver>\x86\Orca-x86_en-us.msi`; installing Orca itself needs admin).
4. **CI asserts:**
   - the ProductVersion read back equals the Cargo.toml version
   - the UpgradeCode equals the constant
   - the MSI size is sane
   - `Get-AuthenticodeSignature` is Valid (once signing exists)

## 6. Testing a real install (requires UAC; manual, Windows Sandbox, or a CI runner)
```powershell
# install (UAC prompt; with Windows sudo enabled: sudo msiexec ...)
msiexec /i dist\toggle-audio-0.1.0-x64.msi /l*v install.log
msiexec /i dist\toggle-audio-0.1.0-x64.msi /qn /l*v install.log              # silent, as winget does (elevated shell)
msiexec /i dist\toggle-audio-0.1.0-x64.msi ADDLOCAL=Main /qn /l*v nopath.log # without the PATH feature
# verify
Test-Path 'C:\Program Files\Toggle Audio\toggle-audio.exe'
Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\toggle-audio.exe'
[Environment]::GetEnvironmentVariable('PATH','Machine') -split ';' | Select-String 'Toggle Audio'
Get-ChildItem 'C:\ProgramData\Microsoft\Windows\Start Menu\Programs' -Filter 'Toggle Audio*'
Get-ItemProperty HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\* | ? DisplayName -eq 'Toggle Audio'
# upgrade: install 0.1.0 then 0.2.0 -> exactly ONE ARP entry (0.2.0); re-run 0.1.0 -> DowngradeErrorMessage
# uninstall
msiexec /x dist\toggle-audio-0.2.0-x64.msi /l*v uninstall.log               # or: msiexec /x {ProductCode}
```
- After uninstall, check that the folder, App Paths key, PATH segment and shortcut are gone. User config in `%APPDATA%` intentionally stays.
- Windows Sandbox or a VM is the safest place for this. GitHub `windows-latest` runners are admin, so CI can run the silent install, upgrade and uninstall cycle as a smoke test.
- **User migration note:** G HUB currently points at `C:\bin\Switch-Audio.exe`. After the first install, the user must re-bind G1 once to `C:\Program Files\Toggle Audio\toggle-audio.exe`.

---

## 7. Code signing and SmartScreen

**Unsigned MSI.** What the user sees:
- A browser download gets Mark-of-the-Web.
- Running it shows the SmartScreen "Windows protected your PC" dialog; the user clicks *More info*, then *Run anyway*.
- The UAC prompt then shows **"Publisher: Unknown"**.

It works, but looks untrustworthy, and some enterprise policies block it. winget-pkgs accepts unsigned installers.

**Since 2024, no certificate gives instant SmartScreen trust.** EV lost its bypass. Every signature builds reputation over time, across consecutive releases signed with the same identity. Signing mainly buys a named publisher in UAC, accumulating reputation, and AV friendliness. Source: https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options (updated 2026-08-29).

**Options for an individual OSS maintainer:**
- **Azure Artifact Signing** (formerly Trusted Signing):
  - about $9.99/month Basic (5,000 signatures/month)
  - GitHub Actions integration, no HSM token
  - **individuals: USA and Canada only**; organizations: US/CA/EU/UK
  - The maintainer appears to be in Taiwan (zh-TW locale), which would make them **ineligible as an individual**. Confirm before planning on it.
- **SignPath Foundation** (free for OSS). It requires:
  - an OSI license with no proprietary components and no commercial dual-licensing
  - an actively maintained project that is already released
  - MFA for all team members
  - defined author/reviewer/approver roles
  - a code-signing policy published on the project page
  - verifiable CI builds from source (GitHub Actions + SignPath connector)

  The certificate subject is "SignPath Foundation", not the maintainer. **This is the realistic free route.** Apply after the first public (unsigned) release exists. Terms: https://signpath.org/terms
- **OV certificate** from a commercial CA (Sectigo/DigiCert/GlobalSign, about $150–300/yr). Certum also sells a cheaper "Open Source Code Signing" certificate to individuals worldwide; verify the current price. Since June 2023 the key must sit on an HSM/token or in the CA's cloud signing service, which complicates CI.
- **Self-signed:** useless for the public. It shows as untrusted unless the cert is manually imported. Only for local testing.

**Signing order (once available):**
1. Sign `toggle-audio.exe` and `toggle-audio-settings.exe` (`signtool sign /fd SHA256 /tr <RFC3161 TSA> /td SHA256 ...`).
2. Run `wix build`.
3. Sign the `.msi`.
4. Compute hashes. Signing changes the bytes, so hashing comes last.

**Recommendation:**
- Ship v0.x unsigned.
- Add a README note on the SmartScreen click-through and SHA256 verification.
- Make winget the main install path.
- Apply to SignPath Foundation once the repo has a release and a signing policy.

---

## 8. Hashes, provenance, winget

### SHA256
```powershell
$h = (Get-FileHash dist\toggle-audio-$ver-x64.msi -Algorithm SHA256).Hash.ToLower()
"$h  toggle-audio-$ver-x64.msi" | Out-File -Encoding ascii -Append dist\SHA256SUMS.txt   # sha256sum-compatible
```
- Upload `SHA256SUMS.txt` next to the MSI (and the portable zip) in the GitHub Release.
- Users verify with `Get-FileHash x.msi -Algorithm SHA256` or `certutil -hashfile x.msi SHA256`.
- Optional free extra: `actions/attest-build-provenance` for build provenance, verified with `gh attestation verify`.

### winget (winget-pkgs, ManifestVersion 1.12.0)
MSI-dependent fields in `installer.yaml`:
```yaml
PackageIdentifier: Owner.ToggleAudio
PackageVersion: 0.1.0
InstallerType: wix            # WiX-built MSI ("msi" also valid); winget supplies /quiet and /passive itself
Scope: machine
InstallModes: [interactive, silent, silentWithProgress]
UpgradeBehavior: install      # MajorUpgrade removes the old version
Commands: [toggle-audio]
Installers:
- Architecture: x64
  InstallerUrl: https://github.com/OWNER/toggle-audio/releases/download/v0.1.0/toggle-audio-0.1.0-x64.msi
  InstallerSha256: <hex from Get-FileHash>
  ProductCode: '{C3FC30DB-8D6C-4F20-8C33-018932D89C33}'   # NEW every build -> read it from the released MSI
  AppsAndFeaturesEntries:
  - DisplayName: Toggle Audio
    Publisher: Toggle Audio contributors
    ProductCode: '{...same as above...}'
    UpgradeCode: '{C82A4013-F2FF-448E-A4AE-63CD73760A63}'  # constant -> winget correlates all versions
ManifestType: installer
ManifestVersion: 1.12.0
```
- **`InstallerSwitches` is not needed.** winget knows the MSI/WiX silent switches.
  - Add `InstallerSwitches: { Custom: "..." }` only if a property must always be passed.
  - Do not use it for the PATH opt-out; users can pass `--custom "ADDLOCAL=Main"`.
- `defaultLocale.yaml`:
  - `Publisher` and `PackageName` must match the ARP `Manufacturer` and `Name` exactly. This is what makes `winget upgrade` and `export` work.
  - Also fill in License (MIT), LicenseUrl, ShortDescription, PackageUrl, and Tags (audio, bluetooth, default-audio-device, logitech, g-hub).
- Read the ProductCode from the exact released file (`wix msi decompile`, or COM `SELECT Value FROM Property WHERE Property='ProductCode'`). Hash the exact uploaded bytes. Any rebuild or re-sign changes both.
- Tooling:
  - First submission: `wingetcreate new <InstallerUrl>` reads ProductCode/UpgradeCode and computes the hash automatically.
  - Later releases: `wingetcreate update Owner.ToggleAudio -u <url> -v X.Y.Z --submit` (needs a GitHub PAT), run in the release workflow.
  - Alternative to wingetcreate: the `vedantmgoyal9/winget-releaser` action.
- Docs: https://learn.microsoft.com/en-us/windows/package-manager/package/manifest

### Scoop (optional)
- Publish `toggle-audio-X.Y.Z-x64.zip` with the exes, LICENSE and README.
- Keep a bucket repo `OWNER/scoop-bucket` with `toggle-audio.json` containing `version`, `url`, `hash`, `bin`, `shortcuts`, `checkver: github` and `autoupdate`.

This is the no-admin, per-user alternative to the MSI.

---

## 9. GUID / identity stability rules (summary)
1. **UpgradeCode `{C82A4013-F2FF-448E-A4AE-63CD73760A63}` never changes**, across renames, rewrites and major versions.
   - Changing it breaks upgrades: you get two ARP entries and the old version is not removed.
   - If the product is renamed after a public release, keep the UpgradeCode. Only `Name` and `INSTALLFOLDER` change, and a folder change means G HUB must be re-bound.
   - The rename to the final project name should happen **before** v0.1.0 is published, so this is moot if done in order.
2. **ProductCode is never authored.** It is regenerated per build and recorded per release in winget.
3. **Component GUIDs are auto-generated** (omitted). They are stable while each component's directory and key path are unchanged. If hand-authored: freeze them, and replace one only when its key path or directory changes. Never reuse one for something else.
4. **Version strictly increases in the first three fields** for every published MSI. Pre-release tags are stripped. The 4th field is ignored by upgrades.
5. **The install path is unversioned** (`C:\Program Files\Toggle Audio\`) so the G HUB binding survives upgrades.
6. **Use the same signing identity on every release** (once signing exists) so SmartScreen reputation accumulates.
