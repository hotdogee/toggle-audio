# Toolchain report (2026-10-04)

All verified non-interactively, non-elevated. Scratch tests under `...\scratchpad\hello-{rs,c,cs,go,zig,wix}`.

| Tool | Version | Absolute path | Status |
|---|---|---|---|
| rustc / cargo | 1.99.0 / 1.99.0 (stable-x86_64-pc-windows-msvc), rustup 1.29.1 | C:\Users\Hotdogee\.cargo\bin\{rustc,cargo}.exe | OK; rustfmt + clippy installed; hello-rs release build + run OK (MSVC link.exe auto-detected) |
| MSVC cl / link | 14.44.35207 (VS 2022 Community 17.14) | C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64\cl.exe | OK via vcvars64.bat |
| .NET SDK (NativeAOT) | 9.0.318 (ILCompiler 9.0.20) | dotnet on PATH | OK after vswhere PATH fix (see problems); hello-cs.exe 1.28 MB, runs |
| WiX Toolset | 7.0.0+b8977d6 (major v7, dotnet global tool) | C:\Users\Hotdogee\.dotnet\tools\wix.exe | OK; global extensions WixToolset.UI.wixext 7.0.0, WixToolset.Util.wixext 7.0.0; test perMachine MSI built |
| hyperfine | 1.20.0 | C:\Users\Hotdogee\AppData\Local\Microsoft\WinGet\Packages\sharkdp.hyperfine_Microsoft.Winget.Source_8wekyb3d8bbwe\hyperfine-v1.20.0-x86_64-pc-windows-msvc\hyperfine.exe (alias in %LOCALAPPDATA%\Microsoft\WinGet\Links) | OK (winget) |
| Go | go1.27.0 windows/amd64 | C:\Users\Hotdogee\AppData\Local\Programs\go\bin\go.exe | OK (portable zip; winget MSI needs UAC) ; hello-go build OK |
| Zig | 0.17.0 | C:\Users\Hotdogee\AppData\Local\Microsoft\WinGet\Packages\zig.zig_Microsoft.Winget.Source_8wekyb3d8bbwe\zig-x86_64-windows-0.17.0\zig.exe (alias in WinGet\Links) | OK; `zig cc` and `zig build-exe` OK |
| GitHub CLI | 2.102.0 | C:\Users\Hotdogee\AppData\Local\Programs\gh\bin\gh.exe | OK (portable zip; not authenticated) |

## MSVC invocation pattern (copy this)

PowerShell (prepend vswhere dir to PATH to avoid noisy error from vcvars, required for NativeAOT):

```powershell
$env:PATH = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer;$env:PATH"
cmd /c "call `"C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat`" >nul && cl /nologo /O2 hello.c /Fe:hello.exe"
```

From Git Bash: write a `.cmd` file and run it with `cmd //c "C:\full\path\build.cmd"` (relative names are not resolved), or call `cmd.exe //c` with the same string.

Cargo does NOT need vcvars (it locates link.exe itself).

## Problems / notes

1. **UAC**: `winget install GoLang.Go` (per-machine MSI) triggered a UAC prompt (consent.exe) that cannot be approved from this session; winget was killed but **a consent.exe/msiexec pair from 18:35 may still be pending on the user's desktop** (declining it is fine; accepting installs Go 1.27 to C:\Program Files\Go). While pending it holds the MSI mutex, so `winget install GitHub.cli` failed with 1618 (another install in progress). Both Go and gh were installed from official portable zips instead. Testing the final MSI install (msiexec into C:\Program Files) will likewise need elevation by the user.
2. **NativeAOT link failure**: ILCompiler's link step failed with MSB3073 because `vcvarsall.bat` (via BuildTools instance chosen by vswhere) calls `vswhere.exe` unqualified and its error text polluted the linker path. Fix: put `C:\Program Files (x86)\Microsoft Visual Studio\Installer` on PATH before `dotnet publish -p:PublishAot=true` (consider adding it to user PATH permanently).
3. Two VS instances exist: Community (C:\Program Files\...\2022\Community) and Build Tools (C:\Program Files (x86)\...\2022\BuildTools); both MSVC 14.44.35207.
4. `rustup` self-update failed: "could not create link ... cargo-miri.exe: file exists (os error 183)". Toolchain update succeeded; rustup itself is 1.29.1 and works. Harmless.
5. WiX v7 has an OSMF EULA (`wix eula accept wix7` / `--acceptEula`); builds already succeed on this machine, so no action needed now, but CI needs `--acceptEula wix7` or `wix eula accept wix7`. Note wix.exe returns 255 on `--help`.
6. New tools are not on PATH of the current shell; use absolute paths above (Go/gh zip dirs are not on PATH at all).
