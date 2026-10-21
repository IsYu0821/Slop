# Local mirror of tools/check-source.sh for environments without a POSIX shell.
# It parses the embedded Python table and verifies every required token.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$script = Get-Content 'tools/check-source.sh' -Raw
$lines = $script -split "`n"
$required = @{}
foreach ($line in $lines) {
    if ($line -match "^\s*'([^']+)':\s*\[(.*)\],\s*$") {
        $file = $Matches[1]
        $tokens = $Matches[2] -split "," | ForEach-Object { $_.Trim().Trim("'") } | Where-Object { $_ -ne "" }
        $required[$file] = $tokens
    }
}
$failed = 0
foreach ($file in $required.Keys) {
    if (-not (Test-Path $file)) { Write-Output "MISSING FILE: $file"; $failed++; continue }
    $text = Get-Content $file -Raw
    foreach ($token in $required[$file]) {
        if (-not $text.Contains($token)) { Write-Output "MISSING TOKEN: $file -> $token"; $failed++ }
    }
}
$cargo = Get-Content 'Cargo.toml' -Raw
if (($cargo | Select-String -Pattern 'version = "0.29.0"' -AllMatches).Matches.Count -ne 1) {
    Write-Output "Cargo.toml version marker mismatch"; $failed++
}
if ($failed -gt 0) { Write-Output "source consistency checks FAILED ($failed)"; exit 1 }
Write-Output "source consistency checks passed"
$python = Get-Command python3 -ErrorAction SilentlyContinue
if (-not $python) { $python = Get-Command python -ErrorAction SilentlyContinue }
if ($python) {
    & $python.Source 'tools/check-abi.py'
    if ($LASTEXITCODE -ne 0) { exit 1 }
} else {
    Write-Output "no python interpreter found: skipping the ABI layout gate"
}
