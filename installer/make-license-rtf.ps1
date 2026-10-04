#Requires -Version 7.0
<#
.SYNOPSIS
    Generates installer/License.rtf (the license page of the MSI UI) from LICENSE.

.DESCRIPTION
    Hard-wrapped lines of the plain-text license are joined into paragraphs
    (paragraphs are separated by blank lines), so the text reflows to the width
    of the installer's license box. The output is plain ASCII RTF: backslashes
    and braces are escaped and any non-ASCII character is written as a \uN?
    escape. The output is deterministic, so a regenerated file only differs
    when LICENSE changed.

.PARAMETER LicensePath
    Source license text. Default: LICENSE at the repository root.

.PARAMETER OutPath
    RTF file to write. Default: installer/License.rtf.

.PARAMETER Check
    Do not write anything; exit with code 1 if OutPath is missing or differs
    from what would be generated. build-msi.ps1 runs this.

.EXAMPLE
    ./installer/make-license-rtf.ps1
.EXAMPLE
    ./installer/make-license-rtf.ps1 -Check
#>
[CmdletBinding()]
param(
    [string] $LicensePath = (Join-Path $PSScriptRoot '..' 'LICENSE'),
    [string] $OutPath = (Join-Path $PSScriptRoot 'License.rtf'),
    [switch] $Check
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function ConvertTo-RtfText([string] $text) {
    $sb = [System.Text.StringBuilder]::new()
    foreach ($ch in $text.ToCharArray()) {
        $code = [int] $ch
        if ($ch -eq '\' -or $ch -eq '{' -or $ch -eq '}') {
            [void] $sb.Append('\').Append($ch)
        } elseif ($code -lt 0x20) {
            [void] $sb.Append(' ')
        } elseif ($code -gt 0x7E) {
            # \uN takes a signed 16-bit value; '?' is the fallback for readers without Unicode.
            $signed = if ($code -gt 0x7FFF) { $code - 0x10000 } else { $code }
            [void] $sb.Append('\u').Append($signed).Append('?')
        } else {
            [void] $sb.Append($ch)
        }
    }
    $sb.ToString()
}

$raw = [System.IO.File]::ReadAllText((Resolve-Path -LiteralPath $LicensePath).Path)
$raw = $raw -replace "`r`n?", "`n"

# Paragraphs: runs of non-blank lines, joined with single spaces.
$paragraphs = @(
    ($raw -split "`n[ `t]*`n") |
        ForEach-Object { (($_ -split "`n") | ForEach-Object { $_.Trim() } | Where-Object { $_ }) -join ' ' } |
        Where-Object { $_ }
)
if ($paragraphs.Count -eq 0) { throw "License file '$LicensePath' is empty." }

$lines = [System.Collections.Generic.List[string]]::new()
$lines.Add('{\rtf1\ansi\ansicpg1252\deff0{\fonttbl{\f0\fswiss\fcharset0 Segoe UI;}}')
$lines.Add('\viewkind4\uc1\pard\sa160\f0\fs18')
for ($i = 0; $i -lt $paragraphs.Count; $i++) {
    $body = ConvertTo-RtfText $paragraphs[$i]
    # The first paragraph is the license title ("MIT License"): bold.
    if ($i -eq 0) { $lines.Add("\b $body\b0\par") } else { $lines.Add("$body\par") }
}
$lines.Add('}')
$rtf = ($lines -join "`r`n") + "`r`n"
$bytes = [System.Text.Encoding]::ASCII.GetBytes($rtf)

if ($Check) {
    if (-not (Test-Path -LiteralPath $OutPath)) {
        Write-Error "$OutPath is missing. Run installer/make-license-rtf.ps1." -ErrorAction Continue
        exit 1
    }
    $existing = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $OutPath).Path)
    if ([System.Convert]::ToBase64String($existing) -ne [System.Convert]::ToBase64String($bytes)) {
        Write-Error "$OutPath is out of date with LICENSE. Run installer/make-license-rtf.ps1 and commit the result." -ErrorAction Continue
        exit 1
    }
    Write-Host "License.rtf is up to date."
    exit 0
}

[System.IO.File]::WriteAllBytes([System.IO.Path]::GetFullPath($OutPath), $bytes)
Write-Host "Wrote $OutPath ($($paragraphs.Count) paragraphs)."
