<#
.SYNOPSIS
Installs signed portable Membrane build & activates supported harnesses.

.DESCRIPTION
Downloads or accepts membrane-windows-x64.zip, verifies checksum plus
Authenticode signatures, swaps staged payload with rollback, activates
Membrane, then verifies exact resident generation.
#>
[CmdletBinding()]
param(
  [string]$Version = "latest",
  [string]$InstallRoot = (Join-Path $env:LOCALAPPDATA "Membrane Hub"),
  [string]$ArchivePath,
  [string]$ChecksumPath,
  [string]$Repository = "Orthic-Labs/Membrane"
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
$AssetName = "membrane-windows-x64.zip"
$InstallRoot = [IO.Path]::GetFullPath($InstallRoot)

function Write-Info($Message) { Write-Host "  > $Message" -ForegroundColor Cyan }
function Write-Success($Message) { Write-Host "  [OK] $Message" -ForegroundColor Green }

function Stop-MembraneProcesses {
  param([string]$Root)
  $ResolvedRoot = [IO.Path]::GetFullPath($Root).TrimEnd('\') + '\'
  $Names = @("membrane-hub.exe", "membrane-tray.exe", "membrane-daemon.exe", "membrane.exe", "cortex.exe")
  $Deadline = [DateTime]::UtcNow.AddSeconds(15)
  do {
    $Processes = @(
      Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
        Where-Object {
          $_.ExecutablePath -and
          $Names -contains $_.Name.ToLowerInvariant() -and
          ([IO.Path]::GetFullPath($_.ExecutablePath).StartsWith($ResolvedRoot, [StringComparison]::OrdinalIgnoreCase))
        }
    )
    foreach ($Process in $Processes) {
      Stop-Process -Id $Process.ProcessId -Force -ErrorAction SilentlyContinue
    }
    if ($Processes.Count -eq 0) { return }
    Start-Sleep -Milliseconds 250
  } while ([DateTime]::UtcNow -lt $Deadline)
  throw "Membrane processes remain active under $Root"
}

function Test-Payload {
  param([string]$Root)
  $Required = @("membrane.exe", "cortex.exe", "membrane-tray.exe", "membrane-daemon.exe", "membrane-hub.exe", "release.json")
  foreach ($Name in $Required) {
    if (-not (Test-Path (Join-Path $Root $Name) -PathType Leaf)) {
      throw "portable payload missing $Name"
    }
  }
  $Manifest = Get-Content (Join-Path $Root "release.json") -Raw -Encoding UTF8 | ConvertFrom-Json
  if ($Manifest.schemaVersion -ne 1 -or $Manifest.product -ne "membrane" -or $Manifest.os -ne "windows" -or $Manifest.arch -ne "x64") {
    throw "portable release manifest identity invalid"
  }
  foreach ($Property in $Manifest.files.PSObject.Properties) {
    $Path = Join-Path $Root ($Property.Name.Replace('/', '\'))
    if (-not (Test-Path $Path -PathType Leaf)) { throw "manifest file missing: $($Property.Name)" }
    $Actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
    if ($Actual -ne $Property.Value.ToString().ToLowerInvariant()) {
      throw "manifest hash mismatch: $($Property.Name)"
    }
  }
  foreach ($Name in $Required | Where-Object { $_.EndsWith(".exe") }) {
    $Path = Join-Path $Root $Name
    $Signature = Get-AuthenticodeSignature -LiteralPath $Path
    if ($Signature.Status -ne "Valid") { throw "invalid Authenticode signature: $Name ($($Signature.Status))" }
  }
  return $Manifest
}

function Get-HealthUri {
  $Profile = $env:USERPROFILE
  if (-not $Profile) { throw "USERPROFILE is unavailable" }
  $WorkspaceConfig = Join-Path $Profile ".config\membrane\workspace.json"
  $Workspace = Get-Content $WorkspaceConfig -Raw -Encoding UTF8 | ConvertFrom-Json
  if ($Workspace.schemaVersion -ne 3 -or -not $Workspace.workspaceRoot) { throw "Membrane workspace config invalid" }
  $RuntimePath = Join-Path $Workspace.workspaceRoot "tools\lib\memory\runtime.json"
  $Runtime = Get-Content $RuntimePath -Raw -Encoding UTF8 | ConvertFrom-Json
  if ($Runtime.schemaVersion -ne 1 -or $Runtime.serviceId -ne "membrane-local-v1" -or $Runtime.host -ne "127.0.0.1") {
    throw "Membrane runtime config invalid"
  }
  return "http://127.0.0.1:$($Runtime.port)/health"
}

function Wait-MembraneHealth {
  param([string]$ExpectedGeneration, [int]$TimeoutSeconds = 45)
  $Uri = Get-HealthUri
  $Deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
  do {
    try {
      $Health = Invoke-RestMethod -Uri $Uri -Method Get -TimeoutSec 2
      if ($Health.ok -eq $true -and $Health.serviceId -eq "membrane-hub" -and $Health.nativeOnly -eq $true -and $Health.releaseGeneration -eq $ExpectedGeneration) {
        return $Health
      }
    } catch {}
    Start-Sleep -Milliseconds 500
  } while ([DateTime]::UtcNow -lt $Deadline)
  throw "Membrane health did not reach expected generation $ExpectedGeneration"
}

function Invoke-Activation {
  param([string]$Root)
  $Membrane = Join-Path $Root "membrane.exe"
  $Output = & $Membrane activate --install-root $Root 2>&1
  $ExitCode = $LASTEXITCODE
  $Output | Out-Host
  if ($ExitCode -ne 0) { throw "Membrane activation failed with exit $ExitCode" }
}

function Add-UserPath {
  param([string]$Root)
  $Current = [Environment]::GetEnvironmentVariable("Path", "User")
  $Entries = @($Current -split ';' | Where-Object { $_ -and $_.Trim() })
  if (-not ($Entries | Where-Object { [IO.Path]::GetFullPath($_).TrimEnd('\').Equals($Root.TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase) })) {
    $Updated = (@($Root) + $Entries) -join ';'
    [Environment]::SetEnvironmentVariable("Path", $Updated, "User")
  }
  if (-not (($env:Path -split ';') -contains $Root)) { $env:Path = "$Root;$env:Path" }
}

Write-Host ""
Write-Host "  Membrane portable installer" -ForegroundColor Blue
$TemporaryRoot = Join-Path $env:TEMP ("membrane-install-" + [guid]::NewGuid().ToString("N"))
$DownloadRoot = Join-Path $TemporaryRoot "download"
$StageRoot = Join-Path $TemporaryRoot "stage"
$BackupRoot = "$InstallRoot.membrane-backup-$([guid]::NewGuid().ToString('N'))"
$HadExisting = Test-Path $InstallRoot
$LiveMoved = $false
$StageMoved = $false
$Installed = $false

try {
  New-Item -ItemType Directory -Path $DownloadRoot -Force | Out-Null
  New-Item -ItemType Directory -Path $StageRoot -Force | Out-Null
  if ($ArchivePath) {
    $ArchivePath = [IO.Path]::GetFullPath($ArchivePath)
    if (-not (Test-Path $ArchivePath -PathType Leaf)) { throw "archive not found: $ArchivePath" }
    if (-not $ChecksumPath) { $ChecksumPath = "$ArchivePath.sha256" }
    $ChecksumPath = [IO.Path]::GetFullPath($ChecksumPath)
    if (-not (Test-Path $ChecksumPath -PathType Leaf)) { throw "checksum not found: $ChecksumPath" }
  } else {
    $BaseUri = if ($Version -eq "latest") {
      "https://github.com/$Repository/releases/latest/download"
    } else {
      "https://github.com/$Repository/releases/download/v$Version"
    }
    $ArchivePath = Join-Path $DownloadRoot $AssetName
    $ChecksumPath = Join-Path $DownloadRoot "checksums.json"
    Write-Info "Downloading $AssetName"
    Invoke-WebRequest -Uri "$BaseUri/$AssetName" -OutFile $ArchivePath -UseBasicParsing
    Invoke-WebRequest -Uri "$BaseUri/checksums.json" -OutFile $ChecksumPath -UseBasicParsing
  }

  if ([IO.Path]::GetExtension($ChecksumPath) -eq ".json") {
    $Checksums = Get-Content $ChecksumPath -Raw -Encoding UTF8 | ConvertFrom-Json
    $ExpectedHash = $Checksums.assets.$AssetName.sha256
  } else {
    $ExpectedHash = (Get-Content $ChecksumPath -Raw -Encoding UTF8).Trim().Split(' ')[0]
  }
  if (-not $ExpectedHash -or $ExpectedHash -notmatch '^[0-9a-fA-F]{64}$') { throw "checksum file invalid" }
  $ActualHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $ArchivePath).Hash
  if ($ActualHash -ne $ExpectedHash) { throw "archive checksum mismatch" }
  Write-Success "Archive checksum verified"

  Expand-Archive -LiteralPath $ArchivePath -DestinationPath $StageRoot -Force
  $Manifest = Test-Payload -Root $StageRoot
  Write-Success "Payload signatures & file hashes verified"

  Stop-MembraneProcesses -Root $InstallRoot
  if ($HadExisting) {
    Move-Item -LiteralPath $InstallRoot -Destination $BackupRoot -Force
    $LiveMoved = $true
  }
  New-Item -ItemType Directory -Path (Split-Path $InstallRoot -Parent) -Force | Out-Null
  Move-Item -LiteralPath $StageRoot -Destination $InstallRoot -Force
  $StageMoved = $true

  Invoke-Activation -Root $InstallRoot
  $Health = Wait-MembraneHealth -ExpectedGeneration $Manifest.releaseGeneration
  Add-UserPath -Root $InstallRoot
  $Installed = $true
  Write-Success "Membrane $($Manifest.version) active at $InstallRoot"
  Write-Success "Health verified: $($Health.releaseGeneration)"
} catch {
  $Failure = $_
  if ($StageMoved -and (Test-Path $InstallRoot)) {
    Stop-MembraneProcesses -Root $InstallRoot
    Remove-Item -LiteralPath $InstallRoot -Recurse -Force -ErrorAction SilentlyContinue
  }
  if ($LiveMoved -and (Test-Path $BackupRoot)) {
    Move-Item -LiteralPath $BackupRoot -Destination $InstallRoot -Force
    try { Invoke-Activation -Root $InstallRoot } catch {}
  }
  throw $Failure
} finally {
  if ($Installed -and (Test-Path $BackupRoot)) {
    Remove-Item -LiteralPath $BackupRoot -Recurse -Force -ErrorAction SilentlyContinue
  }
  if (Test-Path $TemporaryRoot) {
    Remove-Item -LiteralPath $TemporaryRoot -Recurse -Force -ErrorAction SilentlyContinue
  }
}
