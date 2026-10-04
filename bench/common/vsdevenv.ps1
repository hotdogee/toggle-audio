# vsdevenv.ps1 - import the MSVC x64 developer environment into the current PowerShell session.
#
# Dot-source this file, then call Import-VsDevEnv. It is a no-op when an x64-targeting MSVC
# environment is already active (a "Developer PowerShell" / "x64 Native Tools" prompt started with
# -Arch amd64, or a previous call). Otherwise it locates Visual Studio with vswhere (or, without
# vswhere, probes the default VS 2022 install folders), runs vcvars64.bat once in cmd.exe and copies
# the resulting environment variables (PATH, INCLUDE, LIB, LIBPATH, ...) into this process, so
# cl.exe, link.exe, dumpbin.exe and mt.exe can be called directly afterwards.
#
# Note: the stock "Developer PowerShell for VS" and "Developer Command Prompt" target x86 by
# default. Their environment is deliberately NOT reused, because a 32-bit (WOW64) build would have
# completely different process-startup costs and invalidate every benchmark number.
#
# Used by bench\c\build.ps1 and bench\baseline\build.ps1.

function Import-VsDevEnv {
    [CmdletBinding()]
    param()

    if ($env:VSCMD_ARG_TGT_ARCH -eq 'x64' -and $env:VSCMD_ARG_HOST_ARCH -eq 'x64' -and
        $env:INCLUDE -and $env:LIB -and (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
        return
    }

    $installerDir = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer'
    $vswhere = Join-Path $installerDir 'vswhere.exe'
    $vcvars = $null
    if (Test-Path $vswhere) {
        $vsPath = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
            -property installationPath | Select-Object -First 1
        if ($vsPath) { $vcvars = Join-Path $vsPath 'VC\Auxiliary\Build\vcvars64.bat' }
    }
    if (-not $vcvars -or -not (Test-Path $vcvars)) {
        # Fallback for machines without vswhere: probe the default VS 2022 locations
        # (Build Tools installs under Program Files (x86), the IDE editions under Program Files).
        $candidates = foreach ($root in @($env:ProgramFiles, ${env:ProgramFiles(x86)})) {
            foreach ($edition in 'BuildTools', 'Community', 'Professional', 'Enterprise') {
                Join-Path $root "Microsoft Visual Studio\2022\$edition\VC\Auxiliary\Build\vcvars64.bat"
            }
        }
        $vcvars = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
    }
    if (-not $vcvars) {
        throw "vcvars64.bat not found. Install Visual Studio 2022 (or its Build Tools) with the 'Desktop development with C++' workload."
    }

    # vcvarsall.bat calls vswhere.exe unqualified; put its folder on PATH to avoid noisy errors.
    if (Test-Path $installerDir) { $env:PATH = "$installerDir;$env:PATH" }

    $envDump = & cmd.exe /d /c "call `"$vcvars`" >nul 2>&1 && set"
    if ($LASTEXITCODE -ne 0) { throw "vcvars64.bat failed with exit code $LASTEXITCODE" }
    foreach ($line in $envDump) {
        $eq = $line.IndexOf('=')
        if ($eq -gt 0) {
            [Environment]::SetEnvironmentVariable($line.Substring(0, $eq), $line.Substring($eq + 1), 'Process')
        }
    }
    if (-not (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
        throw 'cl.exe is still not on PATH after running vcvars64.bat'
    }
    if ($env:VSCMD_ARG_TGT_ARCH -ne 'x64') {
        throw "vcvars64.bat did not produce an x64 environment (VSCMD_ARG_TGT_ARCH='$env:VSCMD_ARG_TGT_ARCH')"
    }
}

# Throws unless the image named $Name is an x64 (AMD64) executable. $Headers is the output of
# 'dumpbin /headers' for that image.
function Assert-X64Image {
    param([Parameter(Mandatory)][string]$Name, [Parameter(Mandatory)][object[]]$Headers)
    if (-not ($Headers | Select-String -Pattern '8664 machine \(x64\)' -Quiet)) {
        throw "$Name is not an x64 image (dumpbin /headers lacks '8664 machine (x64)')"
    }
}
