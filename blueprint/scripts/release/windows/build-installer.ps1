# D18: build the per-user Windows installer — delegated to right-release per EC v4 D-18.
# See docs/rules/release-signing.md: signing and installer sealing live in
# tools/rightkit/packages/release, not in product-repo scripts.
param(
  [Parameter(Mandatory = $true)][string]$Staging,
  [Parameter(Mandatory = $true)][string]$OutDir
)
$ErrorActionPreference = "Stop"

# Legacy entry point retained for verify-only / local smoke; actual signing
# and sealing are performed by `right-release` from the primary checkout.
# No Azure credentials are handled here.

# Copy the staged runtime into release/windows/staged for Inno Setup.
$Staged = Join-Path $PSScriptRoot "..\..\release\windows\staged"
if (Test-Path $Staged) { Remove-Item -Recurse -Force $Staged }
Copy-Item -Recurse $Staging $Staged

# blueprint-task.xml was removed per EC v4 U15 (D-S03/D-17: OS templates deleted).
# Do not copy a missing template — Hub owns lifecycle via `blueprint service run`.
if (Test-Path (Join-Path $PSScriptRoot "..\..\service\templates\blueprint-task.xml")) {
  Copy-Item -Force (Join-Path $PSScriptRoot "..\..\service\templates\blueprint-task.xml") (Join-Path $Staged "blueprint-task.xml")
}

# Compile with ISCC if present.
$Iscc = "C:\Program Files (x86)\Inno Setup 6\ISCC.exe"
if (-not (Test-Path $Iscc)) { throw "Inno Setup not found at $Iscc" }
& $Iscc (Join-Path $PSScriptRoot "..\..\release\windows\Blueprint.iss") "/O$OutDir"
if ($LASTEXITCODE -ne 0) { throw "ISCC failed: $LASTEXITCODE" }

Write-Host "Installer built in $OutDir"
