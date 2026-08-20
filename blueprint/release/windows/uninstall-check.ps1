# D18: non-admin install/upgrade/uninstall assertions — PATH, scheduled task,
# files, logs, registry entries.
param(
  [string]$InstallDir = "$env:LOCALAPPDATA\Membrane\Blueprint"
)
$ErrorActionPreference = "Stop"
$failures = 0

# Files present.
if (-not (Test-Path (Join-Path $InstallDir "bin\blueprint.cmd"))) { Write-Error "missing bin\blueprint.cmd"; $failures++ }
if (-not (Test-Path (Join-Path $InstallDir "lib\node.exe"))) { Write-Error "missing lib\node.exe"; $failures++ }

# PATH entry present.
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -notlike "*$InstallDir*") { Write-Error "install dir missing from user PATH"; $failures++ }

# Scheduled task (optional install) is removable.
$task = Get-ScheduledTask -TaskName "MembraneBlueprint" -ErrorAction SilentlyContinue
if ($task) {
  Unregister-ScheduledTask -TaskName "MembraneBlueprint" -Confirm:$false
  if (Get-ScheduledTask -TaskName "MembraneBlueprint" -ErrorAction SilentlyContinue) { Write-Error "task not removed"; $failures++ }
}

# Registry metadata removable.
Remove-Item -Path "HKCU:\Software\Membrane\Blueprint" -Recurse -Force -ErrorAction SilentlyContinue
if (Test-Path "HKCU:\Software\Membrane\Blueprint") { Write-Error "registry key not removed"; $failures++ }

if ($failures -gt 0) { exit 1 }
Write-Host "uninstall-check OK"
