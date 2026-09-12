[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$hub = Join-Path $repo 'apps\membrane-hub'
$package = Get-Content -Raw -LiteralPath (Join-Path $hub 'package.json') | ConvertFrom-Json
$bundle = Join-Path $hub 'src-tauri\target\x86_64-pc-windows-msvc\release\bundle\nsis'
$installer = Join-Path $bundle "Membrane_Hub_$($package.version)_x64-setup.exe"
$manifest = Join-Path $bundle 'candidate.json'
$sbom = Join-Path $bundle 'sbom.json'
$evidence = Join-Path ([IO.Path]::GetTempPath()) "membrane-local-windows-$([DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ'))"
$installRoot = Join-Path $env:LOCALAPPDATA 'Orthic Labs\Membrane\current'

Push-Location $repo
try {
  & pnpm.cmd --dir apps/membrane-hub run release:build:win:unsigned
  if ($LASTEXITCODE -ne 0) { throw "unsigned installer build failed: $LASTEXITCODE" }
  foreach ($path in @($installer, $manifest, $sbom)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "local installer evidence missing: $path" }
  }

  & powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/qualification/install-release.ps1 `
    -Installer $installer -ReleaseManifest $manifest -Sbom $sbom -EvidencePath $evidence -Profile internal-unsigned
  if ($LASTEXITCODE -ne 0) { throw "installed qualification failed: $LASTEXITCODE" }

  $install = Start-Process -FilePath $installer -ArgumentList '/S' -Wait -PassThru -WindowStyle Hidden
  if ($install.ExitCode -ne 0) { throw "final installer failed: $($install.ExitCode)" }
  if (-not (Test-Path -LiteralPath (Join-Path $installRoot 'membrane-hub.exe') -PathType Leaf)) {
    throw "installed Membrane Hub missing from stable current root: $installRoot"
  }
  $current = Get-Item -Force -LiteralPath $installRoot
  if ($current.LinkType -ne 'Junction') { throw "installed current root is not a junction: $installRoot" }

  [ordered]@{
    status = 'pass'
    profile = 'internal-unsigned'
    installer = $installer
    installerSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $installer).Hash.ToLowerInvariant()
    evidence = $evidence
    installedCurrent = $installRoot
    installedVersion = $package.version
  } | ConvertTo-Json -Depth 4
} finally {
  Pop-Location
}
