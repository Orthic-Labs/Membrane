# D18: build the per-user Windows installer from a signed staging directory.
param(
  [Parameter(Mandatory = $true)][string]$Staging,
  [Parameter(Mandatory = $true)][string]$OutDir
)
$ErrorActionPreference = "Stop"

# Copy the staged runtime into release/windows/staged for Inno Setup.
$Staged = Join-Path $PSScriptRoot "..\..\release\windows\staged"
if (Test-Path $Staged) { Remove-Item -Recurse -Force $Staged }
Copy-Item -Recurse $Staging $Staged

# Copy the per-user task template.
Copy-Item -Force (Join-Path $PSScriptRoot "..\..\service\templates\cortex-task.xml") (Join-Path $Staged "cortex-task.xml")

# Compile with ISCC if present.
$Iscc = "C:\Program Files (x86)\Inno Setup 6\ISCC.exe"
if (-not (Test-Path $Iscc)) { throw "Inno Setup not found at $Iscc" }
& $Iscc (Join-Path $PSScriptRoot "..\..\release\windows\Cortex.iss") "/O$OutDir"
if ($LASTEXITCODE -ne 0) { throw "ISCC failed: $LASTEXITCODE" }

Write-Host "Installer built in $OutDir"
