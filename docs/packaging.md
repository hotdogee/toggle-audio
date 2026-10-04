# Packaging and installation

Toggle Audio ships as a per-machine Windows Installer package (MSI) built with [WiX Toolset 7](https://wixtoolset.org/), plus a portable zip. This page covers what the MSI installs, how to build and verify it, silent installs, upgrades, the identity rules that must never be broken, and the winget plan. The background research is in [`research/packaging.md`](research/packaging.md).

Sources: [`installer/toggle-audio.wxs`](../installer/toggle-audio.wxs), [`installer/build-msi.ps1`](../installer/build-msi.ps1), [`installer/make-license-rtf.ps1`](../installer/make-license-rtf.ps1), [`installer/License.rtf`](../installer/License.rtf).

## What the MSI installs

| Item | Where | Feature |
| --- | --- | --- |
| `toggle-audio.exe` (console subsystem, for terminals and scripts) | `C:\Program Files\Toggle Audio\` | Main |
| `toggle-audiow.exe` (GUI subsystem, never opens a console) | `C:\Program Files\Toggle Audio\` | Main |
| `LICENSE.txt`, and `README.md` when it existed at build time | `C:\Program Files\Toggle Audio\` | Main |
| Start Menu shortcut **Toggle Audio Settings**, runs `toggle-audiow.exe settings` | all-users Start Menu (`%ProgramData%\Microsoft\Windows\Start Menu\Programs`) | Main |
| App Paths keys for both exes, so Win+R and Start search find `toggle-audio` | `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\toggle-audio.exe` and `...\toggle-audiow.exe` | Main |
| Install folder appended to the **system** `PATH` | `HKLM` environment | PathEnv ("Add to PATH", on by default) |
| Marker value for the PATH feature | `HKLM\SOFTWARE\Toggle Audio\PathEntry` | PathEnv |
| Installed apps entry "Toggle Audio" (publisher Han Lin, icon, links to the repository, issues and releases) | Settings > Apps > Installed apps | |

The install folder has no version in its name, so a launcher binding such as `C:\Program Files\Toggle Audio\toggle-audiow.exe` survives upgrades. The per-user configuration in `%APPDATA%\toggle-audio\config.json` is never touched by the installer: not on install, upgrade or uninstall.

The Start Menu shortcut is an *advertised* shortcut. Windows Installer checks the Main feature's files when it is launched and repairs them (with a UAC prompt) only if one was deleted. This is the pattern that passes ICE validation for a per-machine shortcut without a per-user registry key.

### Installer UI

The interactive UI is `WixUI_FeatureTree`: Welcome, License (MIT), Custom Setup, Ready to install. Custom Setup shows the "Add to PATH" feature, which can be switched off, and a Browse button to change the install folder. The feature tree was chosen over `WixUI_InstallDir` so that the PATH entry is a visible choice rather than a command-line-only option. For the same reason the Installed apps entry keeps **Modify**, which reopens Custom Setup to add or remove the PATH entry later.

## Building the MSI

Prerequisites: the Rust toolchain and Build Tools from [CONTRIBUTING.md](../CONTRIBUTING.md), a .NET 8 or later SDK, and WiX 7 as a .NET global tool:

```powershell
dotnet tool install --global wix --version 7.0.0
wix eula accept wix7                               # once per machine (OSMF EULA v1.1)
wix extension add -g WixToolset.UI.wixext/7.0.0    # extension versions must match wix.exe
wix extension add -g WixToolset.Util.wixext/7.0.0
```

Then, from the repository root (PowerShell 7):

```powershell
.\installer\build-msi.ps1              # cargo build --release, then wix build and validate
.\installer\build-msi.ps1 -SkipBuild   # package the executables that are already built
```

| Parameter | Default | Meaning |
| --- | --- | --- |
| `-Configuration` | `Release` | Cargo profile to package (`Release` or `Debug`). |
| `-SkipBuild` | off | Do not run `cargo build`; the exes must already exist. |
| `-OutDir` | `installer/out` | Output folder (relative to the repository root). |
| `-AcceptEula` | off | Pass `-acceptEula wix7` to wix for this run only, for machines where `wix eula accept wix7` has not been run. |

The script:

1. reads `version` from the `[package]` table of `Cargo.toml` and requires a plain `MAJOR.MINOR.PATCH` version (MSI versions have no pre-release field; major and minor at most 255, patch at most 65535);
2. checks that `installer/License.rtf` matches `LICENSE` (regenerate it with `.\installer\make-license-rtf.ps1` after editing `LICENSE`);
3. runs `cargo build --release --locked` from the repository root unless `-SkipBuild`, honouring `CARGO_TARGET_DIR` (cargo reads `.cargo/config.toml`, which links the C runtime statically, from the current directory), then fails if either exe imports `VCRUNTIME140.dll` or `api-ms-win-crt-*.dll`, so the package never needs the Visual C++ redistributable;
4. finds `wix.exe` on `PATH` or in `%USERPROFILE%\.dotnet\tools` and runs `wix build -arch x64` with the UI and Util extensions, passing `Version`, `BinDir` and, when `README.md` exists, `ReadmeFile`;
5. runs `wix msi validate` (the standard ICE suite; no administrator rights needed);
6. reads ProductVersion, UpgradeCode and ProductCode back from the package and fails if the first two are wrong;
7. prints the path, size and SHA256 and writes `toggle-audio-<version>-x64.msi.sha256` (sha256sum format) next to the MSI.

Output: `installer/out/toggle-audio-<version>-x64.msi` (about 0.5 MB for 0.1.0). The `.wixpdb` beside it is debug data for the build; do not ship it.

CI builds and validates the MSI on every push and pull request (`.github/workflows/ci.yml`, job `msi`). Pushing a `vX.Y.Z` tag runs `.github/workflows/release.yml`, which builds the MSI, the portable zip and `SHA256SUMS.txt` and publishes the GitHub Release.

### Inspecting a package without installing it

```powershell
wix msi validate installer\out\toggle-audio-0.1.0-x64.msi

# Administrative install: extracts the payload only, changes nothing on the system.
msiexec /a installer\out\toggle-audio-0.1.0-x64.msi /qn TARGETDIR="$env:TEMP\ta-admin"
Get-ChildItem -Recurse "$env:TEMP\ta-admin"

# Read any table (read-only).
$i  = New-Object -ComObject WindowsInstaller.Installer
$db = $i.OpenDatabase((Resolve-Path installer\out\toggle-audio-0.1.0-x64.msi).Path, 0)
$v  = $db.OpenView('SELECT `Property`, `Value` FROM `Property`'); $v.Execute()
while ($r = $v.Fetch()) { '{0} = {1}' -f $r.StringData(1), $r.StringData(2) }
```

`msiexec /a` returns 1618 while another installation is running (or waiting at a UAC prompt); finish or cancel that one and retry.

## Installing

Download `toggle-audio-<version>-x64.msi` from [Releases](https://github.com/hotdogee/toggle-audio/releases) and run it. Installation is per machine, so Windows asks for administrator approval (UAC).

The MSI is **not code-signed** yet, so:

- SmartScreen may show "Windows protected your PC". Choose **More info**, then **Run anyway**.
- The UAC prompt shows **Publisher: Unknown**.

Verify the download first (next section) if you want to be sure the file is the one the release published.

### Silent and scripted installs

Run these from an **elevated** terminal. `/qn` (no UI) cannot show a UAC prompt, so from a normal terminal it fails; `/passive` (progress bar only) does show the prompt.

```powershell
# Install everything (Main + Add to PATH), silently, with a log
msiexec /i toggle-audio-0.1.0-x64.msi /qn /l*v install.log

# Install without the PATH entry
msiexec /i toggle-audio-0.1.0-x64.msi /qn ADDLOCAL=Main

# Install to another folder
msiexec /i toggle-audio-0.1.0-x64.msi /qn INSTALLFOLDER="D:\Tools\Toggle Audio"

# Later: remove only the PATH entry, or add it back (the product stays installed)
msiexec /i toggle-audio-0.1.0-x64.msi /qn REMOVE=PathEnv
msiexec /i toggle-audio-0.1.0-x64.msi /qn ADDLOCAL=PathEnv

# Uninstall with the same package...
msiexec /x toggle-audio-0.1.0-x64.msi /qn
# ...or by ProductCode, without the file
$app = Get-ItemProperty HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\* |
       Where-Object DisplayName -eq 'Toggle Audio'
msiexec /x $app.PSChildName /qn
```

Exit code 0 means success; 3010 means success with a restart required (for example when a file was in use); 1603 is a generic failure (read the `/l*v` log); 1618 means another installation is in progress.

The PATH change reaches new terminals only. Terminals that were already open keep their old `PATH`.

### Upgrades and downgrades

- Installing a newer version removes the old one completely and then installs the new one (`MajorUpgrade`, scheduled right after `InstallInitialize`). There is always exactly one "Toggle Audio" entry in Installed apps.
- The install folder stays the same, so launcher bindings keep working. Feature choices carry over: if the PATH entry was switched off, it stays off.
- Close the settings dialog before upgrading; otherwise the installer asks to close it.
- Installing an **older** version over a newer one is blocked with "A newer version of Toggle Audio is already installed". Uninstall first to downgrade.
- Running the same MSI again opens maintenance mode (Modify, Repair, Remove).

### Uninstall

Uninstall from Settings > Apps > Installed apps, or with `msiexec /x` above. It removes the program folder and everything in it that the MSI installed, the Start Menu shortcut, both App Paths keys, `HKLM\SOFTWARE\Toggle Audio` and only the Toggle Audio segment of the system `PATH`. It keeps `%APPDATA%\toggle-audio\` (your device choices); delete that folder by hand if you want no trace. Remove or rebind any hotkey launcher that pointed at the exe.

## Verifying a download

Each release publishes `SHA256SUMS.txt` next to the MSI and the zip. Compare it with the hash of the file you downloaded:

```powershell
(Get-FileHash .\toggle-audio-0.1.0-x64.msi -Algorithm SHA256).Hash.ToLower()
Get-Content .\SHA256SUMS.txt
```

`certutil -hashfile toggle-audio-0.1.0-x64.msi SHA256` gives the same value. Releases also carry a GitHub build provenance attestation when the repository allows it:

```powershell
gh attestation verify .\toggle-audio-0.1.0-x64.msi --repo hotdogee/toggle-audio
```

### Why the MSI is unsigned

No certificate gives instant SmartScreen trust any more, the individual tier of Azure Artifact Signing is limited to the USA and Canada, and commercial certificates cost a yearly fee and need an HSM. For v0.x the MSI is unsigned and the SHA256 sums and provenance attestation are the integrity check. The planned route is [SignPath Foundation](https://signpath.org/), which signs open-source projects for free once a public release and a signing policy exist. When signing arrives, the order is: sign both exes, build the MSI, sign the MSI, then compute the hashes.

## Identity rules (do not break these)

| Identifier | Rule |
| --- | --- |
| **UpgradeCode** `{C82A4013-F2FF-448E-A4AE-63CD73760A63}` | Never changes, not for a rename, a rewrite or a major version. A new UpgradeCode turns upgrades into side-by-side installs with two Installed apps entries. `build-msi.ps1` fails if the built package has a different one. |
| **ProductCode** | Not authored. WiX generates a new one for every build, which `MajorUpgrade` requires. Each released MSI therefore has its own ProductCode, which the winget manifest records. |
| **Component GUIDs** | Not authored. WiX derives each from the component's directory and key path, so they stay stable while file names, the install folder and registry key paths stay the same. If a GUID is ever hand-written, it must change whenever its key path or directory changes, and must never be reused for something else. Never move a file to another component while keeping its GUID. |
| **ProductVersion** | `MAJOR.MINOR.PATCH` from `Cargo.toml`. Every published MSI must have a higher three-part version than the previous one. Windows Installer ignores a fourth field when deciding upgrades, and two MSIs with the same version but different ProductCodes install side by side. Pre-release versions such as `0.2.0-beta.1` are rejected by the build script. |
| **Install folder** | `C:\Program Files\Toggle Audio\`, never versioned. |
| **Name / Manufacturer** | "Toggle Audio" / "Han Lin". These are the Installed apps DisplayName and Publisher and must match the winget `PackageName` and `Publisher`. |

## winget

Planned package identifier: `Hotdogee.ToggleAudio`. Submit after the first public release, to [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) (manifest schema 1.12.0, three files: version, defaultLocale, installer).

Fields that stay the same in every release:

```yaml
PackageIdentifier: Hotdogee.ToggleAudio
Publisher: Han Lin                    # = MSI Manufacturer
PackageName: Toggle Audio             # = MSI Name
License: MIT
LicenseUrl: https://github.com/hotdogee/toggle-audio/blob/main/LICENSE
PackageUrl: https://github.com/hotdogee/toggle-audio
ShortDescription: Flip Windows audio output between two devices in milliseconds.
Tags: [audio, default-audio-device, audio-switcher, hotkey, logitech, g-hub]
InstallerType: wix
Scope: machine
InstallModes: [interactive, silent, silentWithProgress]
UpgradeBehavior: install
Commands: [toggle-audio, toggle-audiow]
Installers:
- Architecture: x64
  AppsAndFeaturesEntries:
  - DisplayName: Toggle Audio
    Publisher: Han Lin
    UpgradeCode: '{C82A4013-F2FF-448E-A4AE-63CD73760A63}'
```

Fields that change with every release:

| Field | Value |
| --- | --- |
| `PackageVersion` | the new version, for example `0.2.0` |
| `InstallerUrl` | `https://github.com/hotdogee/toggle-audio/releases/download/v<version>/toggle-audio-<version>-x64.msi` |
| `InstallerSha256` | the MSI's line in the release's `SHA256SUMS.txt` (uppercase or lowercase) |
| `ProductCode` (on the installer and in `AppsAndFeaturesEntries`) | read from the released file, see below |
| `ReleaseNotesUrl`, `ReleaseDate` | the GitHub Release page and date |

Read the ProductCode from the exact file that was uploaded; any rebuild changes both it and the hash:

```powershell
$i  = New-Object -ComObject WindowsInstaller.Installer
$db = $i.OpenDatabase((Resolve-Path .\toggle-audio-0.2.0-x64.msi).Path, 0)
$v  = $db.OpenView("SELECT ``Value`` FROM ``Property`` WHERE ``Property`` = 'ProductCode'"); $v.Execute()
$v.Fetch().StringData(1)
```

`build-msi.ps1` also prints it. winget supplies the silent switches for `wix` installers itself, so the manifest needs no `InstallerSwitches`. Users who do not want the PATH entry can run `winget install Hotdogee.ToggleAudio --custom "ADDLOCAL=Main"`.

Tooling: the first submission with `wingetcreate new <InstallerUrl>` (it reads the ProductCode and UpgradeCode and computes the hash), reviewed by hand. Later releases with `wingetcreate update Hotdogee.ToggleAudio -u <InstallerUrl> -v <version> --submit`, or the `vedantmgoyal9/winget-releaser` action in `release.yml` with a token that can push to a winget-pkgs fork.

## Testing a real install

`wix msi validate` and `msiexec /a` do not install anything. A real install, upgrade and uninstall needs administrator rights, so run it on a machine or VM you do not mind changing (Windows Sandbox works well):

```powershell
# elevated PowerShell
msiexec /i installer\out\toggle-audio-0.1.0-x64.msi /l*v install.log
Get-ChildItem 'C:\Program Files\Toggle Audio'
Get-ChildItem "$env:ProgramData\Microsoft\Windows\Start Menu\Programs" -Filter 'Toggle Audio*'
Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\toggle-audio.exe'
[Environment]::GetEnvironmentVariable('PATH', 'Machine') -split ';' | Select-String 'Toggle Audio'
Get-ItemProperty HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\* | Where-Object DisplayName -eq 'Toggle Audio'

msiexec /x installer\out\toggle-audio-0.1.0-x64.msi /l*v uninstall.log
# then check that the folder, shortcut, App Paths keys, HKLM\SOFTWARE\Toggle Audio,
# the PATH segment and the Installed apps entry are all gone
```

For an upgrade test, bump the version in `Cargo.toml` locally, build a second MSI, install the first and then the second: there must be one Installed apps entry with the new version, and running the first MSI again must show the downgrade message.
