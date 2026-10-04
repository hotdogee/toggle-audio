#Requires -Version 7.0

<#
.SYNOPSIS
    Writes THIRD-PARTY-NOTICES.txt: the license notices of every crate linked into
    toggle-audio.exe and toggle-audiow.exe.

.DESCRIPTION
    Walks the normal dependency graph (no build or dev dependencies) that
    "cargo metadata --locked" reports for x86_64-pc-windows-msvc, starting at toggle-audio.
    Procedural-macro crates and their dependencies are skipped: they run inside the compiler and
    are not linked into the executables.

    Every linked crate is available under the MIT License (some also under Apache-2.0 or the
    Unlicense, at the licensee's choice), and Toggle Audio uses each of them under MIT. The script
    copies each crate's MIT license file from the local cargo registry; identical texts are written
    once, headed by the crates they cover. A crate without an MIT option stops the script.

    Run it after every change to Cargo.lock and commit the result. CI runs it with -Check; the MSI
    and the release zip ship the file.

.PARAMETER Check
    Write nothing; exit 1 when THIRD-PARTY-NOTICES.txt is missing or differs from what the script
    would write.

.EXAMPLE
    pwsh -NoProfile -File scripts\third-party-notices.ps1

.EXAMPLE
    pwsh -NoProfile -File scripts\third-party-notices.ps1 -Check
#>
[CmdletBinding()]
param([switch]$Check)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 3.0

$repo = Split-Path -Parent $PSScriptRoot
$outFile = Join-Path $repo 'THIRD-PARTY-NOTICES.txt'

$json = cargo metadata --format-version 1 --locked --filter-platform x86_64-pc-windows-msvc `
    --manifest-path (Join-Path $repo 'Cargo.toml')
if ($LASTEXITCODE) { throw "cargo metadata failed (exit $LASTEXITCODE)" }
$meta = $json | ConvertFrom-Json -Depth 64

$packages = @{}
foreach ($p in $meta.packages) { $packages[$p.id] = $p }
$nodes = @{}
foreach ($n in $meta.resolve.nodes) { $nodes[$n.id] = $n }
$product = @($meta.packages | Where-Object name -EQ 'toggle-audio')[0]

# Linked crates: follow normal dependency edges, never into a proc-macro crate.
$linked = [Collections.Generic.HashSet[string]]::new()
$stack = [Collections.Generic.Stack[string]]::new()
$stack.Push($product.id)
while ($stack.Count) {
    foreach ($dep in $nodes[$stack.Pop()].deps) {
        if (-not @($dep.dep_kinds | Where-Object { $null -eq $_.kind })) { continue }
        $pkg = $packages[$dep.pkg]
        if (@($pkg.targets | Where-Object { $_.kind -contains 'proc-macro' })) { continue }
        if ($linked.Add($dep.pkg)) { $stack.Push($dep.pkg) }
    }
}
$crates = @($linked | ForEach-Object { $packages[$_] } | Sort-Object name, version)

# Group the crates by the text of their MIT license file.
$groups = [ordered]@{}
foreach ($crate in $crates) {
    $label = "$($crate.name) $($crate.version)"
    if ($crate.license -notmatch '(^|[\s(])MIT($|[\s)])') {
        throw "$label is not available under MIT ($($crate.license)); extend scripts\third-party-notices.ps1"
    }
    $dir = Split-Path -Parent $crate.manifest_path
    $files = @(Get-ChildItem -LiteralPath $dir -File)
    $file = @($files | Where-Object Name -Match '^licen[cs]e[-_.]mit(\.(txt|md))?$')
    if (-not $file) { $file = @($files | Where-Object Name -Match '^licen[cs]e(\.(txt|md))?$') }
    if (-not $file) { throw "$label has no MIT license file in $dir" }
    $text = ((Get-Content -Raw -LiteralPath $file[0].FullName) -replace "`r`n", "`n").Trim()
    if (-not $groups.Contains($text)) { $groups[$text] = [Collections.Generic.List[object]]::new() }
    $groups[$text].Add($crate)
}

$rule = '-' * 78
$lines = [Collections.Generic.List[string]]::new()
$lines.AddRange([string[]]@(
    "Third-party notices for Toggle Audio $($product.version)"
    ''
    'toggle-audio.exe and toggle-audiow.exe are licensed under the MIT License (see LICENSE).'
    'They are statically linked with the Rust crates listed below. Each crate is available under'
    'the MIT License (some also under Apache-2.0 or the Unlicense, at the licensee''s choice), and'
    'Toggle Audio uses each of them under the MIT License, reproduced below with its copyright'
    'notices. The Rust standard library is linked too; it is available under the same terms'
    '(https://github.com/rust-lang/rust).'
    ''
    'Generated from Cargo.lock by scripts/third-party-notices.ps1. Do not edit by hand.'
    ''
    'Crates:'
    ''
))
foreach ($crate in $crates) {
    $source = if ($crate.repository) { $crate.repository } else { "https://crates.io/crates/$($crate.name)" }
    $lines.Add(('  {0,-22} {1,-9} {2,-26} {3}' -f $crate.name, $crate.version, $crate.license, $source).TrimEnd())
}
foreach ($text in $groups.Keys) {
    $lines.Add('')
    $lines.Add($rule)
    $lines.Add(($groups[$text] | ForEach-Object { "$($_.name) $($_.version)" }) -join ', ')
    # Some license files carry no copyright line; the crates' authors are the copyright holders.
    $authors = @($groups[$text] | ForEach-Object { $_.authors } | ForEach-Object { ($_ -replace '\s*<[^>]*>', '').Trim() } |
        Where-Object { $_ } | Select-Object -Unique)
    if ($authors) { $lines.Add("Authors: $($authors -join ', ')") }
    $lines.Add($rule)
    $lines.Add('')
    $lines.AddRange([string[]]($text -split "`n" | ForEach-Object { $_.TrimEnd() }))
}
$content = ($lines -join "`n") + "`n"

if ($Check) {
    $current = if (Test-Path -LiteralPath $outFile) {
        (Get-Content -Raw -LiteralPath $outFile) -replace "`r`n", "`n"
    } else { '' }
    if ($current -ne $content) {
        Write-Host 'THIRD-PARTY-NOTICES.txt is missing or stale; run scripts\third-party-notices.ps1 and commit the result.'
        exit 1
    }
    Write-Host "THIRD-PARTY-NOTICES.txt is up to date ($($crates.Count) crates)."
    exit 0
}

[IO.File]::WriteAllText($outFile, $content, [Text.UTF8Encoding]::new($false))
Write-Host "Wrote $outFile ($($crates.Count) crates, $($groups.Count) license texts)."
