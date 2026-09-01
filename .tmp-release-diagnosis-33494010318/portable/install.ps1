# rightkit-direct-bootstrap:v1
# generated contract: generator=rightkit-direct-bootstrap contractSha256-bound
[CmdletBinding()] param([string]$Version, [switch]$AllowDowngrade, [string]$ReleaseRoot)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$Contract = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('eyJzY2hlbWFWZXJzaW9uIjoxLCJnZW5lcmF0b3IiOiJyaWdodGtpdC1kaXJlY3QtYm9vdHN0cmFwIiwicHJvZHVjdCI6Im1lbWJyYW5lIiwicmVwb3NpdG9yeSI6Ik9ydGhpYy1MYWJzL01lbWJyYW5lIiwiYm9vdHN0cmFwVmVyc2lvbiI6IjAuMS4yNCIsImluc3RhbGxSb290U3ViZGlyIjoiT3J0aGljIExhYnNcXE1lbWJyYW5lIiwiYWNjZXB0ZWRNYW5pZmVzdFNpZ25lcnMiOnsiYXp1cmUtYXJ0aWZhY3Qtc2lnbmluZy1kYW1uZWQtdmVudHVyZXMtdjEiOnsic3ViamVjdCI6IkNOPURhbW5lZCBWZW50dXJlcyBMTEMiLCJjZXJ0aWZpY2F0ZVNoYTI1NiI6ImQwZjM3NjdiYWI2NzJhNGI3M2RkYjI1Y2Q5NTc1OThhMjQzYmZlYWY5YjFkYTk4MjE4NDJhZTg3NjViZDM5N2YifX0sImV4ZWN1dGFibGVQYXRoIjoibWVtYnJhbmUuZXhlIiwiYWN0aXZhdGlvbkFyZ3MiOlsiYWN0aXZhdGUiLCItLWluc3RhbGwtcm9vdCIsIntjdXJyZW50fSJdLCJzdGF0dXNBcmdzIjpbImFjdGl2YXRlIiwiLS1pbnN0YWxsLXJvb3QiLCJ7Y3VycmVudH0iLCItLWRyeS1ydW4iXSwiaGVhbHRoQXNzZXJ0aW9ucyI6W3sicGF0aCI6InNjaGVtYVZlcnNpb24iLCJlcXVhbHMiOjF9LHsicGF0aCI6ImRyeVJ1biIsImVxdWFscyI6dHJ1ZX0seyJwYXRoIjoic2VydmljZS5zZXJ2aWNlSWQiLCJlcXVhbHMiOiJtZW1icmFuZS1odWIifSx7InBhdGgiOiJzZXJ2aWNlLnJlbGVhc2VHZW5lcmF0aW9uIiwibm9uZW1wdHkiOnRydWV9LHsicGF0aCI6ImNsaWVudHMiLCJtaW5Db3VudCI6Mn1dLCJwcmVmbGlnaHRBcmdzIjpbXSwiYWxsb3dMb2NhbFJlbGVhc2VSb290Ijp0cnVlLCJjb250cmFjdFNoYTI1NiI6ImRkODYxYzcyMjFiMDhmNzJmNzI1NDliNGQwOWVjZTllMDlkN2UyN2EwNWU3MjljZDdiM2M4ZTg0Y2M2NTkzYzEifQ==')) | ConvertFrom-Json
$AcceptedSigners = @{}; $Contract.acceptedManifestSigners.PSObject.Properties | ForEach-Object { $AcceptedSigners[$_.Name] = $_.Value }
$InstallRoot = Join-Path $env:LOCALAPPDATA 'Orthic Labs\Membrane'
$VersionsRoot = Join-Path $InstallRoot 'versions'
$Current = Join-Path $InstallRoot 'current'
$PreviousLink = Join-Path $InstallRoot '.current-previous'
$NextLink = Join-Path $InstallRoot '.current-next'
$Journal = Join-Path $InstallRoot 'integration-journal.json'
function Expand-BootstrapArgs($Values, $ActiveVersionRoot, $ActiveVersion) { foreach ($Value in $Values) { ([string]$Value).Replace('{current}', $Current).Replace('{installRoot}', $InstallRoot).Replace('{versionRoot}', $ActiveVersionRoot).Replace('{version}', $ActiveVersion) } }
function Resolve-HealthPath($Value, [string]$JsonPath) { $CurrentValue = $Value; foreach ($Segment in $JsonPath.Split('.')) { if ($Segment -match '^\d+$') { $Index = [int]$Segment; if ($null -eq $CurrentValue -or $Index -ge @($CurrentValue).Count) { throw ('Health path is missing: ' + $JsonPath) }; $CurrentValue = @($CurrentValue)[$Index] } else { $Property = $CurrentValue.PSObject.Properties[$Segment]; if ($null -eq $Property) { throw ('Health path is missing: ' + $JsonPath) }; $CurrentValue = $Property.Value } }; return $CurrentValue }
function Assert-Health($Health, [string]$ActiveVersion) { foreach ($Assertion in $Contract.healthAssertions) { $Actual = Resolve-HealthPath $Health ([string]$Assertion.path); if ($Assertion.PSObject.Properties.Name -contains 'equals') { $Expected = $Assertion.equals; if ($Expected -is [string]) { $Expected = $Expected.Replace('{version}', $ActiveVersion).Replace('{current}', $Current).Replace('{installRoot}', $InstallRoot) }; if ($Actual -ne $Expected) { throw ('Health assertion failed: ' + $Assertion.path) } } elseif ($Assertion.nonempty -eq $true) { if ($null -eq $Actual -or ([string]$Actual).Trim().Length -eq 0) { throw ('Health assertion failed: ' + $Assertion.path) } } elseif (@($Actual).Count -lt [int]$Assertion.minCount) { throw ('Health assertion failed: ' + $Assertion.path) } } }
New-Item -ItemType Directory -Force -Path $VersionsRoot | Out-Null
$LocalReleaseRoot = $null; if ($ReleaseRoot) { $LocalReleaseRoot = [IO.Path]::GetFullPath($ReleaseRoot); if (-not (Test-Path -LiteralPath $LocalReleaseRoot -PathType Container)) { throw 'Local release root is missing' } }
if (-not $Version -and -not $LocalReleaseRoot) { $Version = ((Invoke-RestMethod 'https://api.github.com/repos/Orthic-Labs/Membrane/releases/latest').tag_name -replace '^v','') }
if ($Version -and $Version -notmatch '^(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)$') { throw 'Invalid stable release version' }
$ReleaseBase = if ($Version) { 'https://github.com/Orthic-Labs/Membrane/releases/download/v' + $Version } else { $null }
$Work = Join-Path ([IO.Path]::GetTempPath()) ('membrane-install-' + [guid]::NewGuid())
New-Item -ItemType Directory -Force -Path $Work | Out-Null
$ManifestPath = Join-Path $Work 'release-manifest.json'; $SignaturePath = Join-Path $Work 'release-manifest.cat'; $ChecksumsPath = Join-Path $Work 'checksums.json'
function Receive-ReleaseFile([string]$Name, [string]$Destination) { if ($LocalReleaseRoot) { $RootPrefix = $LocalReleaseRoot.TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar; $Source = [IO.Path]::GetFullPath((Join-Path $LocalReleaseRoot $Name)); if (-not $Source.StartsWith($RootPrefix, [StringComparison]::OrdinalIgnoreCase) -or -not (Test-Path -LiteralPath $Source -PathType Leaf)) { throw ('Local release asset is missing or escapes root: ' + $Name) }; Copy-Item -LiteralPath $Source -Destination $Destination } else { Invoke-WebRequest ($ReleaseBase + '/' + $Name) -OutFile $Destination } }
Receive-ReleaseFile 'release-manifest.json' $ManifestPath
Receive-ReleaseFile 'release-manifest.cat' $SignaturePath
$ManifestBytes = [IO.File]::ReadAllBytes($ManifestPath); Import-Module Microsoft.PowerShell.Security -ErrorAction Stop
$Catalog = Test-FileCatalog -CatalogFilePath $SignaturePath -Path $ManifestPath -Detailed; if ([string]$Catalog.Status -ne 'Valid') { throw 'Release manifest catalog binding is invalid' }
$CatalogSignature = Get-AuthenticodeSignature -FilePath $SignaturePath; if ([string]$CatalogSignature.Status -ne 'Valid' -or -not $CatalogSignature.SignerCertificate) { throw 'Release manifest signature is invalid' }; $SignerCertificate = $CatalogSignature.SignerCertificate
$SignerSubject = 'CN=' + $SignerCertificate.GetNameInfo([Security.Cryptography.X509Certificates.X509NameType]::SimpleName, $false)
$Sha256 = [Security.Cryptography.SHA256]::Create(); try { $SignerFingerprint = ([BitConverter]::ToString($Sha256.ComputeHash($SignerCertificate.RawData))).Replace('-','').ToLowerInvariant() } finally { $Sha256.Dispose() }
$VerifiedKeyId = $null; foreach ($KeyId in $AcceptedSigners.Keys) { $Accepted = $AcceptedSigners[$KeyId]; if ($SignerSubject -eq [string]$Accepted.subject -and $SignerFingerprint -eq [string]$Accepted.certificateSha256) { $VerifiedKeyId = $KeyId; break } }
if (-not $VerifiedKeyId) { throw 'Release manifest signer is not accepted' }
$Manifest = [Text.Encoding]::UTF8.GetString($ManifestBytes) | ConvertFrom-Json
if (-not $Version) { $Version = [string]$Manifest.version }
if ($Version -notmatch '^(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)$') { throw 'Invalid stable release version' }
$ReleaseBase = 'https://github.com/Orthic-Labs/Membrane/releases/download/v' + $Version
$PriorTarget = $null; if (Test-Path $Current) { $PriorTarget = (Get-Item $Current).Target }
$PriorVersion = if ($PriorTarget) { ((Split-Path ([string]$PriorTarget) -Leaf) -split '-')[0] } else { $null }
if ($PriorVersion -and -not $AllowDowngrade -and ([version]$Version -lt [version]$PriorVersion)) { throw 'Downgrade requires -AllowDowngrade' }
if ($Manifest.signingKeyId -ne $VerifiedKeyId -or $Manifest.signatureAlgorithm -ne 'authenticode-catalog-sha256' -or $Manifest.signatureProvider -ne 'windows-authenticode-catalog' -or $Manifest.signatureProviderVersion -ne 1) { throw 'Release manifest signer identity mismatch' }
if ($Manifest.product -ne 'membrane' -or $Manifest.version -ne $Version -or $Manifest.tag -ne ('v' + $Version)) { throw 'Release identity mismatch' }
if ([version]$Contract.bootstrapVersion -lt [version]$Manifest.minimumBootstrapVersion) { throw 'Bootstrap update required' }
$Machine = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString(); $Target = if ($Machine -eq 'Arm64') { 'windows-arm64' } elseif ($Machine -eq 'X64') { 'windows-x86_64' } else { throw ('Unsupported architecture: ' + $Machine) }
$Matches = @($Manifest.assets | Where-Object { $_.target -eq $Target }); if ($Matches.Count -ne 1) { throw 'Release has no unique exact target' }; $Asset = $Matches[0]
if ($Asset.url -ne ($ReleaseBase + '/' + $Asset.name)) { throw 'Asset URL is not exact immutable GitHub release URL' }
Receive-ReleaseFile 'checksums.json' $ChecksumsPath
$ChecksumHash = (Get-FileHash -Algorithm SHA256 $ChecksumsPath).Hash.ToLowerInvariant(); if ($ChecksumHash -ne $Manifest.checksumsSha256) { throw 'checksums.json is not manifest-bound' }
$Checksums = Get-Content -Raw $ChecksumsPath | ConvertFrom-Json; if ($Checksums.assets.($Asset.name) -ne $Asset.sha256) { throw 'Asset checksum differs from bound checksums' }
$Archive = Join-Path $Work $Asset.name; Receive-ReleaseFile ([string]$Asset.name) $Archive
if ((Get-FileHash -Algorithm SHA256 $Archive).Hash.ToLowerInvariant() -ne $Asset.sha256) { throw 'Archive checksum mismatch' }
if ($Checksums.assets.($Asset.provenanceName) -ne $Asset.provenanceSha256 -or $Checksums.assets.($Asset.sbomName) -ne $Asset.sbomSha256) { throw 'Bound provenance/SBOM checksums are missing' }
$ProvenancePath = Join-Path $Work $Asset.provenanceName; $SbomPath = Join-Path $Work $Asset.sbomName
Receive-ReleaseFile ([string]$Asset.provenanceName) $ProvenancePath; Receive-ReleaseFile ([string]$Asset.sbomName) $SbomPath
if ((Get-FileHash -Algorithm SHA256 $ProvenancePath).Hash.ToLowerInvariant() -ne $Asset.provenanceSha256) { throw 'Provenance checksum mismatch' }
if ((Get-FileHash -Algorithm SHA256 $SbomPath).Hash.ToLowerInvariant() -ne $Asset.sbomSha256) { throw 'SBOM checksum mismatch' }
$Stage = Join-Path $Work 'stage'; New-Item -ItemType Directory -Force -Path $Stage | Out-Null
Add-Type -AssemblyName System.IO.Compression.FileSystem; $Zip = [IO.Compression.ZipFile]::OpenRead($Archive); try { foreach ($Entry in $Zip.Entries) { $Dest = [IO.Path]::GetFullPath((Join-Path $Stage $Entry.FullName)); if (-not $Dest.StartsWith(([IO.Path]::GetFullPath($Stage) + [IO.Path]::DirectorySeparatorChar), [StringComparison]::OrdinalIgnoreCase)) { throw 'Archive path escapes staging root' } } } finally { $Zip.Dispose() }
Expand-Archive -Path $Archive -DestinationPath $Stage -Force
$VersionRoot = Join-Path $VersionsRoot ($Version + '-' + $Asset.sha256.Substring(0,12) + '-' + [guid]::NewGuid()); Move-Item $Stage $VersionRoot
$VersionExe = Join-Path $VersionRoot ($Asset.executablePath -replace '/', [IO.Path]::DirectorySeparatorChar); if (-not (Test-Path $VersionExe)) { throw 'Release executable missing' }
$Authenticode = Get-AuthenticodeSignature $VersionExe; if ($Asset.nativeSignaturePolicy -eq 'authenticode-valid' -and $Authenticode.Status -ne 'Valid') { throw 'Native signature invalid' }
$ActivationArgsTemplate = @('activate','--install-root','{current}')
$StatusArgsTemplate = @('activate','--install-root','{current}','--dry-run')
$PreflightArgsTemplate = @()
$PreflightArgs = @(Expand-BootstrapArgs $PreflightArgsTemplate $VersionRoot $Version)
$PriorStatus = $null; if ($PriorTarget) { $PriorExe = Join-Path $Current 'membrane.exe'; $PriorStatusArgs = @(Expand-BootstrapArgs $StatusArgsTemplate ([string]$PriorTarget) $PriorVersion); try { $PriorStatus = (& $PriorExe @PriorStatusArgs | Out-String).Trim() } catch {} }
@{ schemaVersion=1; priorTarget=$PriorTarget; priorVersion=$PriorVersion; targetVersion=$Version; priorStatus=$PriorStatus } | ConvertTo-Json -Depth 8 | Set-Content -Encoding UTF8 $Journal
if ($PreflightArgs.Count) { & $VersionExe @PreflightArgs; if ($LASTEXITCODE -ne 0) { throw 'Activation preflight failed' } }
if (Test-Path $NextLink) { [IO.Directory]::Delete($NextLink) }; if (Test-Path $PreviousLink) { [IO.Directory]::Delete($PreviousLink) }
New-Item -ItemType Junction -Path $NextLink -Target $VersionRoot | Out-Null
$Switched = $false; $PriorDetached = $false
try {
  if (Test-Path $Current) { Rename-Item -LiteralPath $Current -NewName (Split-Path -Leaf $PreviousLink); $PriorDetached = $true }; Rename-Item -LiteralPath $NextLink -NewName (Split-Path -Leaf $Current); $Switched = $true
  $StableExe = Join-Path $Current 'membrane.exe'
  $ActivationArgs = @(Expand-BootstrapArgs $ActivationArgsTemplate $VersionRoot $Version); $StatusArgs = @(Expand-BootstrapArgs $StatusArgsTemplate $VersionRoot $Version)
  & $StableExe @ActivationArgs; if ($LASTEXITCODE -ne 0) { throw 'Activation failed' }
  $HealthText = (& $StableExe @StatusArgs | Out-String).Trim(); if ($LASTEXITCODE -ne 0) { throw 'Health command failed' }; $Health = $HealthText | ConvertFrom-Json
  Assert-Health $Health $Version
  $StableBin = Split-Path $StableExe -Parent; $UserPath = [Environment]::GetEnvironmentVariable('Path','User'); if (-not (($UserPath -split ';') -contains $StableBin)) { [Environment]::SetEnvironmentVariable('Path', (($UserPath.TrimEnd(';') + ';' + $StableBin).Trim(';')), 'User') }
  Remove-Item -Force $Journal; if (Test-Path $PreviousLink) { [IO.Directory]::Delete($PreviousLink) }; Write-Output ('membrane ' + $Version + ' installed')
} catch {
  Write-Host ('ORIGINAL FAILURE: ' + $_.Exception.Message); if (($Switched -or $PriorDetached) -and (Test-Path $Current)) { [IO.Directory]::Delete($Current) }; if ($PriorDetached -and (Test-Path $PreviousLink)) { Rename-Item -LiteralPath $PreviousLink -NewName (Split-Path -Leaf $Current) }
  if ($PriorTarget -and (Test-Path $Current)) { $PriorExe = Join-Path $Current 'membrane.exe'; $PriorActivationArgs = @(Expand-BootstrapArgs $ActivationArgsTemplate ([string]$PriorTarget) $PriorVersion); $PriorStatusArgs = @(Expand-BootstrapArgs $StatusArgsTemplate ([string]$PriorTarget) $PriorVersion); & $PriorExe @PriorActivationArgs; if ($LASTEXITCODE -ne 0) { throw 'Rollback activation failed' }; $PriorHealthText = (& $PriorExe @PriorStatusArgs | Out-String).Trim(); if ($LASTEXITCODE -ne 0) { throw 'Rollback health failed' }; $PriorHealth = $PriorHealthText | ConvertFrom-Json; try { Assert-Health $PriorHealth $PriorVersion } catch { throw 'Prior health was not restored' } }
  if (Test-Path $VersionRoot) { Remove-Item -Recurse -Force $VersionRoot }; throw
} finally { if (Test-Path $Work) { Remove-Item -Recurse -Force $Work } }
# source-sha256:4bd827d1bcdf7de520fced89cff6262a28b622e2ea1da49e50900a2f973c5d49
