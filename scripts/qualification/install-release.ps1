[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string]$Installer,
  [string]$PreviousInstaller = '',
  [Parameter(Mandatory = $true)][string]$ReleaseManifest,
  [Parameter(Mandatory = $true)][string]$Sbom,
  [Parameter(Mandatory = $true)][string]$EvidencePath,
  [string]$InstallRoot = (Join-Path $env:LOCALAPPDATA 'Orthic Labs\Membrane\current'),
  [int]$TimeoutSeconds = 45,
  [int]$SteadyStateSamples = 4
)

# Installed Windows qualification is deliberately a runner, never a builder.
# It accepts already-created package/evidence paths and exercises one exact
# signed package through clean install, repair/upgrade, rollback & uninstall.
$ErrorActionPreference = 'Stop'
# Native command diagnostics are checked through explicit exit codes below;
# keep non-fatal Git warnings from becoming terminating PowerShell errors.
$PSNativeCommandUseErrorActionPreference = $false
# Qualification can be launched by Windows PowerShell from a pwsh-hosted CI
# step. Pin the host-native security module so an inherited pwsh module path
# cannot make Windows PowerShell select its incompatible PowerShell 7 copy.
# Same hazard applies to Utility (Get-FileHash, Invoke-RestMethod, ...) and
# Management (Get-ChildItem, Get-Content, ...): an inherited pwsh PSModulePath
# makes Windows PowerShell resolve the PowerShell 7 copies, which do not export
# their commands into a 5.1 host. Pin all three to $PSHOME.
foreach ($module in @('Microsoft.PowerShell.Security', 'Microsoft.PowerShell.Utility', 'Microsoft.PowerShell.Management')) {
  Import-Module (Join-Path $PSHOME "Modules\$module\$module.psd1") -Force -ErrorAction Stop
}
Add-Type -AssemblyName System.Net.Http
$script:HubProcess = $null
# Keep qualification isolated from checkout-local runtimes while retaining
# system Git, which Blueprint uses for repository fingerprinting.
$script:GitPath = (Get-Command git.exe -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty Source)
$script:GitBin = if ($script:GitPath) { Split-Path -Parent $script:GitPath } else { $null }
$script:SafePath = ((@("$env:WINDIR\System32", "$env:WINDIR", $script:GitBin) | Where-Object { $_ }) -join ';')
$script:QualificationWorkspace = $null
$script:AdaptEvidence = $null
$script:State = $null
$script:InitialInstallRoot = $null
$script:InitialEvidence = $null
$script:UpgradeEvidence = $null
$script:ActivationDryRun = $null
$script:Activation = $null
$script:PreviousMembraneWorkspaceConfig = $null
$script:WorkspaceConfigPath = $null
$script:WorkspaceConfigInitialSha256 = $null
$script:WorkspaceMigrationEvidence = $null
$script:ActiveHubHealth = $null
$script:ActiveHubPort = $null

function Require([bool]$Condition, [string]$Message) {
  if (-not $Condition) { throw $Message }
}

function Resolve-File([string]$Path, [string]$Label) {
  Require (-not [string]::IsNullOrWhiteSpace($Path)) "$Label path is empty"
  Require (Test-Path -LiteralPath $Path -PathType Leaf) "$Label is missing: $Path"
  return (Resolve-Path -LiteralPath $Path).Path
}

function Hash-File([string]$Path) {
  return (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
}

function Normalize-ComparablePath([string]$Path) {
  $full = [IO.Path]::GetFullPath($Path)
  if ($full.StartsWith('\\?\UNC\', [StringComparison]::OrdinalIgnoreCase)) { return ('\\' + $full.Substring(8)) }
  if ($full.StartsWith('\\?\', [StringComparison]::OrdinalIgnoreCase)) { return $full.Substring(4) }
  return $full
}

function Normalize-Version([string]$Value, [string]$Label) {
  $match = [regex]::Match(([string]$Value).Trim(), '(?<!\d)(\d+\.\d+\.\d+)(?:[-+][0-9A-Za-z.-]+)?')
  Require ($match.Success) "$Label is not a semantic version: $Value"
  return "v$($match.Groups[1].Value)"
}

function Get-ArtifactVersion([string]$Path, [string]$Label) {
  try { $value = [Diagnostics.FileVersionInfo]::GetVersionInfo($Path).ProductVersion }
  catch { throw "$Label version metadata could not be read: $($_.Exception.Message)" }
  return Normalize-Version $value "$Label ProductVersion"
}

function Normalize-Generation([string]$Value, [string]$Label) {
  $generation = ([string]$Value).Trim()
  if ($generation.StartsWith('sha256:', [StringComparison]::OrdinalIgnoreCase)) { $generation = $generation.Substring(7) }
  Require ($generation -match '^[0-9a-f]{64}$') "$Label is not a SHA-256 generation: $Value"
  return $generation.ToLowerInvariant()
}

function Assert-SignedFile([string]$Path, [string]$Label, [string]$ExpectedPublisher = '') {
  $signature = Get-AuthenticodeSignature -LiteralPath $Path
  Require ($signature.Status -eq 'Valid') "$Label Authenticode status is $($signature.Status)"
  Require ($null -ne $signature.SignerCertificate) "$Label has no signer certificate"
  Require ($null -ne $signature.TimeStamperCertificate) "$Label has no trusted timestamp certificate"
  $publisher = [string]$signature.SignerCertificate.Subject
  Require (-not [string]::IsNullOrWhiteSpace($publisher)) "$Label signer publisher is empty"
  if (-not [string]::IsNullOrWhiteSpace($ExpectedPublisher)) {
    Require ($publisher -eq $ExpectedPublisher) "$Label publisher does not match installer publisher"
  }
  return $publisher
}

function Write-JsonAtomic([string]$Path, $Value) {
  $full = [System.IO.Path]::GetFullPath($Path)
  $parent = Split-Path -Parent $full
  if (-not (Test-Path -LiteralPath $parent)) { New-Item -ItemType Directory -Path $parent -Force | Out-Null }
  $temporary = "$full.tmp-$([guid]::NewGuid().ToString('N'))"
  try {
    [IO.File]::WriteAllText($temporary, ($Value | ConvertTo-Json -Depth 40), [Text.UTF8Encoding]::new($false))
    Move-Item -LiteralPath $temporary -Destination $full -Force
  } finally {
    if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -Force -ErrorAction SilentlyContinue }
  }
}

function Read-JsonFile([string]$Path, [string]$Label) {
  try { return (Get-Content -Raw -LiteralPath $Path | ConvertFrom-Json) }
  catch { throw "$Label is not valid JSON: $($_.Exception.Message)" }
}

function Assert-BoundEvidence([string]$InstallerPath, [string]$ManifestPath, [string]$SbomPath) {
  $installerHash = Hash-File $InstallerPath
  $manifestText = Get-Content -Raw -LiteralPath $ManifestPath
  $sbomText = Get-Content -Raw -LiteralPath $SbomPath
  Require ($manifestText -match [regex]::Escape($installerHash)) 'release manifest does not bind installer SHA-256'
  Require ($sbomText -match [regex]::Escape($installerHash)) 'SBOM does not bind installer SHA-256'
  Require ($manifestText -match '(?i)membrane[-_ ]hub') 'release manifest has no Membrane Hub identity'
  Require ($sbomText -match '(?i)membrane[-_ ]hub|membrane') 'SBOM has no Membrane package identity'

  $manifest = Read-JsonFile $ManifestPath 'release manifest'
  $sbom = Read-JsonFile $SbomPath 'SBOM'
  Require ($manifest.schema -eq 'membrane.release-evidence.v1') 'release manifest schema is invalid'
  Require ($sbom.schema -eq 'membrane.sbom.v1') 'SBOM schema is invalid'
  Require ([string]$manifest.artifact.sha256 -ieq $installerHash) 'release manifest artifact digest does not bind installer'
  Require ([string]$sbom.artifact.sha256 -ieq $installerHash) 'SBOM artifact digest does not bind installer'
  Require ([string]$manifest.artifact.path -and [string]$sbom.artifact.path) 'digest-bound artifact paths are missing'
  $bound = @()
  function Find-BoundDigest($Value) {
    if ($null -eq $Value) { return }
    if ($Value -is [System.Collections.IDictionary]) {
      if ($Value.Contains('sha256') -and [string]$Value.sha256 -ieq $installerHash) { $script:bound += $Value }
      foreach ($item in $Value.Values) { Find-BoundDigest $item }
      return
    }
    if ($Value -is [System.Collections.IEnumerable] -and -not ($Value -is [string])) {
      foreach ($item in $Value) { Find-BoundDigest $item }
    }
    if ($Value -is [pscustomobject]) {
      $digest = $Value.PSObject.Properties['sha256']
      if ($digest -and [string]$digest.Value -ieq $installerHash) { $script:bound += $Value }
      foreach ($property in $Value.PSObject.Properties) { Find-BoundDigest $property.Value }
    }
  }
  $script:bound = @()
  Find-BoundDigest $manifest
  Require ($script:bound.Count -gt 0) 'release manifest JSON has no installer digest-bound entry'
  Remove-Variable bound -Scope Script -ErrorAction SilentlyContinue
}

function Save-RuntimeLogEvidence([string]$Label) {
  # Copy every log the installed product can have written so a health failure
  # carries the runtime's own words, not only the probe's.
  $evidenceRoot = $env:RIGHT_GIT_QUALIFICATION_EVIDENCE_ROOT
  if (-not $evidenceRoot) { $evidenceRoot = $EvidencePath }
  $target = Join-Path $evidenceRoot "runtime-logs-$Label"
  try {
    New-Item -ItemType Directory -Path $target -Force -ErrorAction Stop | Out-Null
    $sources = @(
      (Join-Path $env:LOCALAPPDATA 'Orthic Labs\Membrane\logs'),
      (Join-Path $env:LOCALAPPDATA 'Orthic Labs\Membrane\log'),
      (Join-Path $env:LOCALAPPDATA 'Membrane\logs'),
      (Join-Path $env:LOCALAPPDATA 'Membrane\log'),
      (Join-Path $env:APPDATA 'Orthic Labs\Membrane\logs'),
      (Join-Path $env:APPDATA 'Membrane\logs')
    )
    if ($script:QualificationWorkspace) { $sources += (Join-Path $script:QualificationWorkspace 'logs') }
    foreach ($source in $sources) {
      if (Test-Path -LiteralPath $source -PathType Container) {
        $dest = Join-Path $target (($source -replace '[:\\]+', '_'))
        Copy-Item -LiteralPath $source -Destination $dest -Recurse -Force -ErrorAction SilentlyContinue
      }
    }
    $processes = @(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object { $_.Name -match '(?i)^(membrane|cortex|node)' } | Select-Object ProcessId, ParentProcessId, Name, ExecutablePath, CommandLine)
    $processes | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath (Join-Path $target 'processes.json') -Encoding utf8
  } catch {
    try { "could not collect runtime logs: $($_.Exception.Message)" | Set-Content -LiteralPath (Join-Path $target 'collection-error.txt') -Encoding utf8 } catch {}
  }
  return $target
}

function Invoke-Activation([string]$Root) {
  # The real activation a customer install performs: reconciles every client
  # config, registers PATH, launches the resident tray and waits for health.
  # Output is evidence either way; a non-zero exit fails qualification with it.
  $membrane = Join-Path $Root 'membrane.exe'
  Require (Test-Path -LiteralPath $membrane -PathType Leaf) "installed membrane.exe is missing at $membrane"
  # Native stderr must not become a terminating error before it is captured.
  $previousErrorAction = $ErrorActionPreference
  $ErrorActionPreference = 'Continue'
  try {
    $output = & $membrane activate --install-root $Root --timeout-ms 90000 2>&1 | ForEach-Object { "$_" } | Out-String
    $exit = $LASTEXITCODE
  } finally { $ErrorActionPreference = $previousErrorAction }
  $evidenceRoot = $env:RIGHT_GIT_QUALIFICATION_EVIDENCE_ROOT
  if (-not $evidenceRoot) { $evidenceRoot = $EvidencePath }
  try {
    New-Item -ItemType Directory -Path $evidenceRoot -Force -ErrorAction Stop | Out-Null
    $output | Set-Content -LiteralPath (Join-Path $evidenceRoot 'activation.log') -Encoding utf8
  } catch {}
  if ($exit -ne 0) { [void](Save-RuntimeLogEvidence 'activation') }
  Require ($exit -eq 0) "membrane activate exited $exit`n$output"
  $parsed = $null
  try { $parsed = $output | ConvertFrom-Json } catch { throw "membrane activate did not emit JSON:`n$output" }
  Require ([string]$parsed.runtimeOrigin -eq 'installed') "activation reported runtimeOrigin $($parsed.runtimeOrigin)"
  return [ordered]@{ exitCode = $exit; runtimeOrigin = [string]$parsed.runtimeOrigin; service = $parsed.service; clients = @($parsed.clients | ForEach-Object { [ordered]@{ client = $_.client; before = $_.before; after = $_.after; changed = $_.changed } }) }
}

function Invoke-ActivationDryRun([string]$Root) {
  # `membrane activate --dry-run` validates the stable root, the version
  # pointer and every client's config without launching or mutating anything.
  # Its full output is kept as evidence either way; a non-zero exit fails
  # qualification with that output in the message.
  $membrane = Join-Path $Root 'membrane.exe'
  Require (Test-Path -LiteralPath $membrane -PathType Leaf) "installed membrane.exe is missing at $membrane"
  $output = & $membrane activate --install-root $Root --dry-run 2>&1 | Out-String
  $exit = $LASTEXITCODE
  $evidenceRoot = $env:RIGHT_GIT_QUALIFICATION_EVIDENCE_ROOT
  if (-not $evidenceRoot) { $evidenceRoot = $EvidencePath }
  try {
    New-Item -ItemType Directory -Path $evidenceRoot -Force -ErrorAction Stop | Out-Null
    $output | Set-Content -LiteralPath (Join-Path $evidenceRoot 'activation-dry-run.log') -Encoding utf8
  } catch {}
  Require ($exit -eq 0) "membrane activate --dry-run exited $exit`n$output"
  $parsed = $null
  try { $parsed = $output | ConvertFrom-Json } catch { throw "membrane activate --dry-run did not emit JSON:`n$output" }
  Require ([string]$parsed.runtimeOrigin -eq 'installed') "activation dry run reported runtimeOrigin $($parsed.runtimeOrigin)"
  return [ordered]@{ exitCode = $exit; runtimeOrigin = [string]$parsed.runtimeOrigin; clients = @($parsed.clients | ForEach-Object { [ordered]@{ client = $_.client; before = $_.before; after = $_.after } }) }
}

function Save-InstallerFailureEvidence([string]$InstallerPath, [string]$Version, [int]$ExitCode) {
  # Section Install writes a step-level failure line ("<step> exit=<code>") to
  # %LOCALAPPDATA%\Orthic Labs\Membrane\logs\install-<version>.log. Collect the
  # newest such log into evidence instead of extracting and re-running an
  # embedded payload; the installer no longer carries one.
  $evidenceRoot = $env:RIGHT_GIT_QUALIFICATION_EVIDENCE_ROOT
  if (-not $evidenceRoot) { $evidenceRoot = $EvidencePath }
  $logPath = Join-Path $evidenceRoot 'installer-failure.log'
  $jsonPath = Join-Path $evidenceRoot 'installer-failure.json'
  try {
    New-Item -ItemType Directory -Path $evidenceRoot -Force -ErrorAction Stop | Out-Null
    $logsDir = Join-Path $env:LOCALAPPDATA 'Orthic Labs\Membrane\logs'
    $sourceLog = $null
    # Bind to the exact version under test; never attach another version's log.
    $exactLog = Join-Path $logsDir "install-$($Version.TrimStart('v')).log"
    if (Test-Path -LiteralPath $exactLog -PathType Leaf) {
      $sourceLog = Get-Item -LiteralPath $exactLog
    }
    if ($sourceLog) {
      Copy-Item -LiteralPath $sourceLog.FullName -Destination $logPath -Force
    } else {
      "no NSIS install step log found under $logsDir for NSIS exit code $ExitCode" | Set-Content -LiteralPath $logPath -Encoding utf8
    }
    $record = [ordered]@{
      schema = 'membrane.installer-failure.v1'
      installer = $InstallerPath
      version = $Version
      nsisExitCode = $ExitCode
      installStepLog = if ($sourceLog) { $sourceLog.FullName } else { $null }
      evidenceLog = $logPath
    }
    $record | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $jsonPath -Encoding utf8
    return $logPath
  } catch {
    try { "could not capture installer failure evidence: $($_.Exception.Message)" | Set-Content -LiteralPath $logPath -Encoding utf8 } catch {}
    return $logPath
  }
}

function Invoke-Installer([string]$Path) {
  $resolved = Resolve-File $Path 'installer'
  $expectedVersion = Get-ArtifactVersion $resolved 'installer'
  # Product root is fixed by installer; qualification must not override it.
  # A just-terminated downgrade process can briefly retain an executable
  # handle; NSIS may then report success while leaving that old binary in
  # place. Retry only when installed Hub identity proves replacement did not
  # happen, keeping upgrade evidence fail-closed and bounded.
  $actualVersion = $null
  for ($attempt = 1; $attempt -le 3; $attempt++) {
    $process = Start-Process -FilePath $resolved -ArgumentList @('/S') -Wait -PassThru -WindowStyle Hidden
    if ($process.ExitCode -ne 0) {
      $logPath = Save-InstallerFailureEvidence -InstallerPath $resolved -Version $expectedVersion -ExitCode $process.ExitCode
      throw "installer failed with exit code $($process.ExitCode): $resolved (payload log: $logPath)"
    }
    Start-Sleep -Milliseconds 750
    $installedHub = Join-Path $InstallRoot 'membrane-hub.exe'
    if (Test-Path -LiteralPath $installedHub -PathType Leaf) {
      try { $actualVersion = Get-ArtifactVersion $installedHub 'installed Hub after installer' } catch { $actualVersion = $null }
    }
    if ($actualVersion -eq $expectedVersion) {
      $link = Get-Item -Force -LiteralPath $InstallRoot
      Require ($link.LinkType -eq 'Junction') 'installed current path is not a junction'
      $target = [IO.Path]::GetFullPath([string]$link.Target)
      $versionsRoot = [IO.Path]::GetFullPath((Join-Path (Split-Path -Parent $InstallRoot) 'versions')).TrimEnd('\') + '\'
      Require ($target.StartsWith($versionsRoot, [StringComparison]::OrdinalIgnoreCase)) 'installed current target escapes versions root'
      Require ((Split-Path -Parent $target).TrimEnd('\') -ieq $versionsRoot.TrimEnd('\')) 'installed current target is not one direct versions child'
      return $target
    }
    if ($attempt -lt 3) { Start-Sleep -Milliseconds 750 }
  }
  throw "installer completed but installed Hub version $actualVersion does not match expected $($expectedVersion): $resolved"
}

function Get-InstalledExecutable([string]$Root, [string]$Label = 'Hub executable') {
  $preferred = @('membrane-hub.exe', 'Membrane Hub.exe', 'MembraneHub.exe')
  foreach ($name in $preferred) {
    $candidate = Join-Path $Root $name
    if (Test-Path -LiteralPath $candidate -PathType Leaf) { return (Resolve-Path -LiteralPath $candidate).Path }
  }
  $candidates = @(Get-ChildItem -LiteralPath $Root -Filter '*.exe' -File -Recurse -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -notin @('uninstall.exe', 'membrane.exe', 'cortex.exe') })
  foreach ($candidate in $candidates) {
    try {
      if ($candidate.VersionInfo.ProductName -match '(?i)Membrane Hub') { return $candidate.FullName }
    } catch { }
  }
  throw "$Label not found under $Root"
}

function Get-InstalledSidecar([string]$Root, [string]$Name, $Entry) {
  $declared = ([string]$Entry.installerPath).Replace('/', '\')
  Require ($declared -ieq "$Name.exe") "installed inventory sidecar path is invalid for $($Name): $declared"
  $declaredPath = [IO.Path]::GetFullPath((Join-Path $Root $declared))
  $rootPrefix = [IO.Path]::GetFullPath($Root).TrimEnd('\') + '\'
  Require ($declaredPath.StartsWith($rootPrefix, [StringComparison]::OrdinalIgnoreCase)) "installed inventory sidecar path escapes install root: $declared"
  Require (Test-Path -LiteralPath $declaredPath -PathType Leaf) "native $Name sidecar is missing at inventory path: $declared"
  return (Resolve-Path -LiteralPath $declaredPath).Path
}

function Get-RuntimePort([string]$Root) {
  $configs = @(Get-ChildItem -LiteralPath $Root -Filter 'runtime.json' -File -Recurse -ErrorAction SilentlyContinue)
  foreach ($configPath in $configs) {
    try {
      $config = Read-JsonFile $configPath.FullName 'installed runtime config'
      if ($config.schemaVersion -eq 1 -and $config.host -eq '127.0.0.1' -and [int]$config.port -ge 1024) {
        return [int]$config.port
      }
    } catch { }
  }
  return 47851
}

function Get-BlueprintEndpoint {
  Require (-not [string]::IsNullOrWhiteSpace($env:USERPROFILE)) 'USERPROFILE is unavailable for Blueprint endpoint'
  $sha = [Security.Cryptography.SHA256]::Create()
  try { $hex = ([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($env:USERPROFILE))) -replace '-', '').ToLowerInvariant() }
  finally { $sha.Dispose() }
  return "\\.\pipe\membrane-blueprint-$($hex.Substring(0, 16))"
}

function Invoke-BlueprintPipe([string]$Endpoint, [string]$Method, [hashtable]$Payload = @{}, [int]$DeadlineMs = 30000) {
  $pipeName = $Endpoint.Substring($Endpoint.LastIndexOf('\') + 1)
  $client = [IO.Pipes.NamedPipeClientStream]::new('.', $pipeName, [IO.Pipes.PipeDirection]::InOut, [IO.Pipes.PipeOptions]::Asynchronous)
  try {
    $client.Connect([Math]::Min($DeadlineMs, 5000))
    # NamedPipeClientStream does not implement stream timeouts on Windows;
    # connect deadline plus service protocol deadline provides bounded calls.
    try { $client.ReadTimeout = $DeadlineMs + 500 } catch { }
    try { $client.WriteTimeout = $DeadlineMs } catch { }
    $writer = [IO.StreamWriter]::new($client, [Text.UTF8Encoding]::new($false), 4096, $true); $writer.AutoFlush = $true
    $reader = [IO.StreamReader]::new($client, [Text.UTF8Encoding]::new($false), $false, 4096, $true)
    $request = [ordered]@{ protocolVersion = 1; requestId = [guid]::NewGuid().ToString(); repoId = $null; generation = $null; method = $Method; deadlineMs = $DeadlineMs; input = $Payload }
    $writer.WriteLine(($request | ConvertTo-Json -Compress -Depth 20)); $line = $reader.ReadLine(); Require (-not [string]::IsNullOrWhiteSpace($line)) "Blueprint $Method returned no response"; return $line | ConvertFrom-Json
  } finally { $client.Dispose() }
}

function Invoke-BlueprintPipeUntilReady([string]$Endpoint, [string]$Method, [hashtable]$Payload, [int]$Timeout) {
  $deadline = (Get-Date).AddSeconds($Timeout); $response = $null
  do { try { $response = Invoke-BlueprintPipe $Endpoint $Method $Payload ([Math]::Min(30000, $Timeout * 1000)) } catch { $response = $null }; if ($response.ok -eq $true) { return $response }; Start-Sleep -Milliseconds 250 } while ((Get-Date) -lt $deadline)
  return $response
}

function Assert-BlueprintResident([string]$Root, [string]$WorkspaceRoot) {
  $typedStates = @('root_not_enrolled', 'not_configured', 'graph_missing', 'missing_graph', 'stale_blocked', 'generation_mismatch')
  $endpoint = Get-BlueprintEndpoint; $status = $null; $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  do {
    try {
      $status = Invoke-BlueprintPipe $endpoint 'status' @{ repoRoot = $WorkspaceRoot }
      if ($status.ok -eq $true -or $typedStates -contains [string]$status.error.code) { break }
    } catch {
      $script:LastBlueprintPipeError = $_.Exception.Message
      $status = $null
    }
    Start-Sleep -Milliseconds 250
  } while ((Get-Date) -lt $deadline)
  $pipeDetail = if ($script:LastBlueprintPipeError) { ": $script:LastBlueprintPipeError" } else { '' }
  Require ($null -ne $status) "Hub-hosted Blueprint returned no status envelope$pipeDetail"
  if ($status.ok -eq $true) {
    Require ($status.result -and [string]$status.result.state -in @('fresh', 'degraded', 'running')) 'Hub-hosted Blueprint status is not serving'
    $enrollment = if ([int]$status.result.runtime.enrolledRepoCount -gt 0) { 'enrolled' } else { 'not_configured' }
    $graph = [string]$status.result.state
  } else {
    Require ($typedStates -contains [string]$status.error.code) "Hub-hosted Blueprint status failed with untyped error: $($status.error.code)"
    $enrollment = if ([string]$status.error.code -in @('root_not_enrolled', 'not_configured')) { 'not_configured' } else { 'unknown' }
    $graph = [string]$status.error.code
  }
  $findings = Invoke-BlueprintPipe $endpoint 'findings.get' @{ repoRoot = $WorkspaceRoot; allowStale = $false }
  Require ($null -ne $findings) 'Hub-hosted Blueprint findings.get returned no envelope'
  if ($findings.ok -eq $true) { Require ($findings.result.kind -eq 'findings.get') 'Hub-hosted Blueprint findings.get result is invalid'; $findingsState = 'success' }
  else { Require ($typedStates -contains [string]$findings.error.code) "Hub-hosted Blueprint findings.get failed with untyped error: $($findings.error.code)"; $findingsState = [string]$findings.error.code }
  $recall = Invoke-BlueprintPipe $endpoint 'recall' @{ repoRoot = $WorkspaceRoot; query = 'native qualification' }
  Require ($null -ne $recall) 'Hub-hosted Blueprint recall returned no envelope'
  if ($recall.ok -eq $true) { Require ([string]$recall.result.action -in @('allow', 'continue', 'block', 'noop')) 'Hub-hosted Blueprint recall result is invalid'; $recallState = 'success' }
  else { Require ($typedStates -contains [string]$recall.error.code) "Hub-hosted Blueprint recall failed with untyped error: $($recall.error.code)"; $recallState = [string]$recall.error.code }
  $mismatch = Invoke-BlueprintPipe $endpoint 'findings.get' @{ repoRoot = $WorkspaceRoot; generation = "sha256:$([string]::new('0', 64))"; allowStale = $false }
  Require ($mismatch.ok -eq $false -and @('generation_mismatch', 'stale_blocked') -contains [string]$mismatch.error.code) 'Blueprint generation mismatch did not fail closed'
  return [ordered]@{ endpoint = $endpoint; status = 'pass'; enrollment = $enrollment; graph = $graph; findings = $findingsState; recall = $recallState; generationMismatch = 'pass'; hubOwned = $true }
}

function Invoke-BlueprintOneShot([string]$Root, [string]$WorkspaceRoot) {
  $launcher = Join-Path $Root 'runtime\blueprint\bin\blueprint.cmd'; Require (Test-Path -LiteralPath $launcher -PathType Leaf) 'installed Blueprint launcher is missing'
  $psi = [Diagnostics.ProcessStartInfo]::new(); $psi.FileName = $env:ComSpec; $psi.WorkingDirectory = Split-Path -Parent $launcher; $psi.UseShellExecute = $false; $psi.RedirectStandardOutput = $true; $psi.RedirectStandardError = $true; $psi.CreateNoWindow = $true
  $psi.Arguments = '/d /s /c ""{0}" status --json --root "{1}""' -f $launcher.Replace('"', '""'), $WorkspaceRoot.Replace('"', '""')
  $process = [Diagnostics.Process]::new(); $process.StartInfo = $psi; Require $process.Start() 'could not start bounded Blueprint one-shot'; $stdout = $process.StandardOutput.ReadToEnd(); $stderr = $process.StandardError.ReadToEnd(); Require ($process.WaitForExit($TimeoutSeconds * 1000)) 'bounded Blueprint one-shot timed out'; Require ($process.ExitCode -in @(0, 2)) "bounded Blueprint one-shot failed: $stderr"
  try { $payload = $stdout | ConvertFrom-Json } catch { throw "bounded Blueprint one-shot returned invalid JSON: $($_.Exception.Message)" }
  Require ($null -ne $payload) 'bounded Blueprint one-shot returned no status payload'
  $typedMissing = @('missing', 'not_configured', 'root_not_enrolled', 'graph_missing', 'missing_graph')
  $state = [string]$payload.state
  $errorCode = [string]$payload.error.code
  if ($state -notin @('fresh', 'degraded', 'running')) {
    Require ($typedMissing -contains $state -or $typedMissing -contains $errorCode) "bounded Blueprint one-shot returned untyped status: state=$state code=$errorCode"
  }
  $outputHash = [Security.Cryptography.SHA256]::Create()
  try { $outputSha256 = ([BitConverter]::ToString($outputHash.ComputeHash([Text.Encoding]::UTF8.GetBytes($stdout))) -replace '-', '').ToLowerInvariant() }
  finally { $outputHash.Dispose() }
  return [ordered]@{ status = 'pass'; exitCode = $process.ExitCode; state = if ($state) { $state } else { $errorCode }; availability = if ($state -in @('fresh', 'degraded', 'running')) { 'available' } else { 'not_configured' }; outputSha256 = $outputSha256 }
}

function Get-ProcessTree([int]$ProcessId) {
  $all = @(Get-CimInstance Win32_Process)
  $root = @($all | Where-Object { [uint32]$_.ProcessId -eq [uint32]$ProcessId })
  Require ($root.Count -eq 1) "process $ProcessId is no longer present"
  $pending = New-Object 'System.Collections.Generic.Queue[uint32]'
  $pending.Enqueue([uint32]$ProcessId)
  $rows = New-Object 'System.Collections.Generic.List[object]'
  while ($pending.Count -gt 0) {
    $parent = $pending.Dequeue()
    foreach ($child in @($all | Where-Object { [uint32]$_.ParentProcessId -eq $parent })) {
      $rows.Add($child)
      $pending.Enqueue([uint32]$child.ProcessId)
    }
  }
  return @($root + $rows)
}

function Add-NativeWindowProbe {
  if ('MembraneQualification.NativeWindowProbe' -as [type]) { return }
  Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

namespace MembraneQualification {
  public sealed class WindowRecord {
    public IntPtr Handle { get; set; }
    public int ProcessId { get; set; }
    public string Title { get; set; }
    public string ClassName { get; set; }
    public bool Visible { get; set; }
  }

  public static class NativeWindowProbe {
    private delegate bool EnumWindowsProc(IntPtr handle, IntPtr state);
    [DllImport("user32.dll")] private static extern bool EnumWindows(EnumWindowsProc callback, IntPtr state);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern int GetWindowText(IntPtr handle, StringBuilder text, int max);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern int GetClassName(IntPtr handle, StringBuilder text, int max);
    [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr handle);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr handle, out uint processId);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern IntPtr FindWindow(string className, string title);
    [DllImport("user32.dll")] private static extern bool PostMessage(IntPtr handle, uint message, IntPtr wParam, IntPtr lParam);

    private static string Text(IntPtr handle, bool title) {
      var buffer = new StringBuilder(512);
      if (title) GetWindowText(handle, buffer, buffer.Capacity); else GetClassName(handle, buffer, buffer.Capacity);
      return buffer.ToString();
    }

    public static WindowRecord[] ForProcess(int processId) {
      var rows = new List<WindowRecord>();
      EnumWindows((handle, state) => {
        uint owner;
        GetWindowThreadProcessId(handle, out owner);
        if (owner == processId) rows.Add(new WindowRecord { Handle = handle, ProcessId = (int)owner, Title = Text(handle, true), ClassName = Text(handle, false), Visible = IsWindowVisible(handle) });
        return true;
      }, IntPtr.Zero);
      return rows.ToArray();
    }

    public static IntPtr Find(string className, string title) { return FindWindow(className, title); }
    public static bool PostTrayClick(IntPtr handle, bool up) { return PostMessage(handle, 6002u, IntPtr.Zero, (IntPtr)(up ? 514 : 513)); }
  }
}
'@
}

function Assert-NativeSteadyState([int]$ProcessId, [string]$BlueprintNode, [string]$BlueprintNodeSha256) {
  $latest = @()
  $rendererPath = $null
  # Rust Hub publishes health as soon as its service child handshakes, while
  # Windows may materialize that child's Blueprint watcher a little later.
  # Wait for the one required resident watcher inside the same bounded
  # qualification deadline before sampling steady state; a missing watcher
  # still fails typed instead of being hidden by an unbounded wait.
  $readyDeadline = (Get-Date).AddSeconds([Math]::Max(5, $TimeoutSeconds))
  $readyService = @()
  $readyWatcher = @()
  do {
    $latest = @(Get-ProcessTree $ProcessId)
    $readyDescendants = @($latest | Where-Object { [uint32]$_.ProcessId -ne [uint32]$ProcessId })
    $readyService = @($readyDescendants | Where-Object { $_.Name -match '(?i)^node(?:\.exe)?$' -and $_.CommandLine -match '(?i)blueprint\.mjs.*\bservice\b.*\brun\b' })
    $readyWatcher = @($readyDescendants | Where-Object { $_.Name -match '(?i)^node(?:\.exe)?$' -and $_.CommandLine -match '(?i)blueprint-watch\.mjs.*\bstart\b' })
    if ($readyService.Count -eq 1 -and $readyWatcher.Count -eq 1) { break }
    $hubProcess = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    Require ($null -ne $hubProcess -and -not $hubProcess.HasExited) 'Hub exited while waiting for Blueprint watcher readiness'
    Start-Sleep -Milliseconds 250
  } while ((Get-Date) -lt $readyDeadline)
  Require ($readyService.Count -eq 1) "Hub did not expose exactly one Blueprint service process before steady-state sampling (observed $($readyService.Count))"
  Require ($readyWatcher.Count -eq 1) "Hub did not expose exactly one Blueprint watcher process before steady-state sampling (observed $($readyWatcher.Count))"
  for ($sample = 0; $sample -lt [Math]::Max(1, $SteadyStateSamples); $sample++) {
    # WMI can retain a just-exited short-lived child for one query tick. Keep
    # only processes still present in the OS before classifying ancestry so a
    # completed Git fingerprint cannot appear as an orphan.
    $latest = @(Get-ProcessTree $ProcessId | Where-Object {
      $candidate = Get-Process -Id ([int]$_.ProcessId) -ErrorAction SilentlyContinue
      $null -ne $candidate -and -not $candidate.HasExited
    })
    $descendants = @($latest | Where-Object { [uint32]$_.ProcessId -ne [uint32]$ProcessId })
    $blueprintService = @($descendants | Where-Object { $_.Name -match '(?i)^node(?:\.exe)?$' -and $_.CommandLine -match '(?i)blueprint\.mjs.*\bservice\b.*\brun\b' })
    $blueprintWatcher = @($descendants | Where-Object { $_.Name -match '(?i)^node(?:\.exe)?$' -and $_.CommandLine -match '(?i)blueprint-watch\.mjs.*\bstart\b' })
    $processSummary = (@($descendants | ForEach-Object { "[$($_.Name)] $($_.CommandLine)" }) -join ' | ')
    Require ($blueprintService.Count -eq 1) "Hub did not expose exactly one Blueprint service process (observed $($blueprintService.Count); descendants: $processSummary)"
    Require ($blueprintWatcher.Count -eq 1) "Hub did not expose exactly one Blueprint watcher process (observed $($blueprintWatcher.Count); descendants: $processSummary)"
    # First-use graph recovery is a Hub-owned, singleflight Blueprint build.
    # It runs as another child of service run using the same inventory-bound
    # Node executable; admit that worker (and any equivalent Blueprint helper)
    # without weakening the one-service/one-watcher residency invariant.
    $residentIds = @($blueprintService.ProcessId + $blueprintWatcher.ProcessId)
    $blueprintWorkers = @($descendants | Where-Object {
      $_.Name -match '(?i)^node(?:\.exe)?$' -and
        $_.ExecutablePath -and
        ([IO.Path]::GetFullPath($_.ExecutablePath) -ieq [IO.Path]::GetFullPath($BlueprintNode)) -and
        ([uint32]$_.ProcessId -notin @($residentIds)) -and
        $_.CommandLine -match '(?i)blueprint(?:-[A-Za-z0-9_-]+)?\.mjs\b'
    })
    $blueprintProcesses = @($blueprintService + $blueprintWatcher + $blueprintWorkers)
    foreach ($blueprintProcess in $blueprintProcesses) {
      Require ($blueprintProcess.ExecutablePath -and ([IO.Path]::GetFullPath($blueprintProcess.ExecutablePath) -ieq [IO.Path]::GetFullPath($BlueprintNode))) 'Blueprint process executable is not the inventory-bound node.exe'
      Require ((Hash-File $blueprintProcess.ExecutablePath) -ieq $BlueprintNodeSha256) 'Blueprint process executable hash does not match inventory'
    }
    $rendererProcesses = @($descendants | Where-Object { $_.Name -match '(?i)^msedgewebview2(?:\.exe)?$' -and $_.ExecutablePath })
    Require ($rendererProcesses.Count -gt 0) 'installed Hub has no WebView2 renderer descendant'
    foreach ($rendererProcess in $rendererProcesses) {
      $path = [IO.Path]::GetFullPath($rendererProcess.ExecutablePath)
      Require (Test-Path -LiteralPath $path -PathType Leaf) 'WebView2 renderer executable path is missing'
      $signature = Get-AuthenticodeSignature -LiteralPath $path
      Require ($signature.Status -eq 'Valid') "WebView2 renderer is not signed: $path"
      if ($null -eq $rendererPath) { $rendererPath = $path } else { Require ($rendererPath -ieq $path) 'WebView2 renderer path changed during steady-state sampling' }
    }
    # Windows may materialize a hidden console host for a bundled Blueprint
    # Node process even with CREATE_NO_WINDOW/windowsHide. It is an OS host,
    # not a Membrane-owned runtime; admit only signed System32 hosts directly
    # parented by inventory-bound Blueprint actors.
    $blueprintConsoleHosts = @($descendants | Where-Object {
      $_.Name -match '(?i)^conhost(?:\.exe)?$' -and
        $_.ExecutablePath -and
        ([IO.Path]::GetFullPath($_.ExecutablePath) -ieq ([IO.Path]::Combine($env:WINDIR, 'System32', 'conhost.exe'))) -and
        ([uint32]$_.ParentProcessId -in @($blueprintProcesses.ProcessId))
    })
    # Repository fingerprinting is delegated to the host Git executable. Keep
    # this bounded to the PATH-resolved binary and its direct Blueprint parent;
    # Git may itself receive a System32 console host on Windows.
    $blueprintGitProcesses = @($descendants | Where-Object {
      $_.Name -match '(?i)^git(?:\.exe)?$' -and
        $_.ExecutablePath -and
        $script:GitPath -and
        ([IO.Path]::GetFullPath($_.ExecutablePath) -ieq [IO.Path]::GetFullPath($script:GitPath)) -and
        ((Hash-File $_.ExecutablePath) -ieq (Hash-File $script:GitPath)) -and
        ([uint32]$_.ParentProcessId -in @($blueprintProcesses.ProcessId))
    })
    $blueprintGitConsoleHosts = @($descendants | Where-Object {
      $_.Name -match '(?i)^conhost(?:\.exe)?$' -and
        $_.ExecutablePath -and
        ([IO.Path]::GetFullPath($_.ExecutablePath) -ieq ([IO.Path]::Combine($env:WINDIR, 'System32', 'conhost.exe'))) -and
        ([uint32]$_.ParentProcessId -in @($blueprintGitProcesses.ProcessId))
    })
    $unexpected = @($descendants | Where-Object {
      $blueprint = $_.ProcessId -in @($blueprintProcesses.ProcessId)
      $renderer = $_.ProcessId -in @($rendererProcesses.ProcessId)
      $consoleHost = $_.ProcessId -in @($blueprintConsoleHosts.ProcessId + $blueprintGitConsoleHosts.ProcessId)
      $git = $_.ProcessId -in @($blueprintGitProcesses.ProcessId)
      -not ($blueprint -or $renderer -or $consoleHost -or $git)
    })
    $unexpectedSummary = (@($unexpected | ForEach-Object { "[$($_.Name)] path=$($_.ExecutablePath) parent=$($_.ParentProcessId) cmd=$($_.CommandLine)" }) -join ' | ')
    Require ($unexpected.Count -eq 0) "native-only steady-state process tree violated: $unexpectedSummary"
    if ($sample + 1 -lt [Math]::Max(1, $SteadyStateSamples)) { Start-Sleep -Milliseconds 500 }
  }
  return $latest
}

function Convert-ProcessEvidence($Rows) {
  return @($Rows | ForEach-Object {
    $path = [string]$_.ExecutablePath
    # Win32_Process can briefly omit ExecutablePath for short-lived children
    # (notably taskkill.exe) even though process identity is still known.
    # Resolve live rows through Process.Path, then bind known OS executables
    # to System32; never emit an unbound process row into sealed evidence.
    if (-not $path) {
      try {
        $live = Get-Process -Id ([int]$_.ProcessId) -ErrorAction Stop
        $path = [string]$live.Path
      } catch { $path = '' }
    }
    if (-not $path -and [string]$_.Name -match '(?i)^[A-Za-z0-9._-]+$') {
      $systemPath = Join-Path (Join-Path $env:WINDIR 'System32') ([string]$_.Name)
      if (Test-Path -LiteralPath $systemPath -PathType Leaf) { $path = $systemPath }
    }
    Require (-not [string]::IsNullOrWhiteSpace($path)) "process $($_.ProcessId) [$($_.Name)] has no resolvable executable path"
    [ordered]@{
      processId = [int]$_.ProcessId
      parentProcessId = [int]$_.ParentProcessId
      name = [string]$_.Name
      executablePath = $path
      executableSha256 = if ($path -and (Test-Path -LiteralPath $path -PathType Leaf)) { Hash-File $path } else { $null }
    }
  })
}

function Get-InstalledContentEvidence([string]$Root) {
  return @(Get-ChildItem -LiteralPath $Root -File -Recurse -Force | Sort-Object FullName | ForEach-Object {
    [ordered]@{
      path = $_.FullName.Substring($Root.TrimEnd('\').Length).TrimStart('\').Replace('\', '/')
      size = [int64]$_.Length
      sha256 = Hash-File $_.FullName
    }
  })
}

function Get-WindowRows([int]$ProcessId) {
  Add-NativeWindowProbe
  return @([MembraneQualification.NativeWindowProbe]::ForProcess($ProcessId))
}

function Find-TrayElement {
  try {
    Add-Type -AssemblyName UIAutomationClient -ErrorAction Stop
    Add-Type -AssemblyName UIAutomationTypes -ErrorAction Stop
    $trayHandle = [MembraneQualification.NativeWindowProbe]::Find('Shell_TrayWnd', $null)
    Require ($trayHandle -ne [IntPtr]::Zero) 'Windows notification area is unavailable'
    $tray = [System.Windows.Automation.AutomationElement]::FromHandle($trayHandle)
    $items = $tray.FindAll(
      [System.Windows.Automation.TreeScope]::Descendants,
      [System.Windows.Automation.Condition]::TrueCondition)
    foreach ($item in $items) {
      try {
        if ($item.Current.Name -match '(?i)Membrane') { return $item }
      } catch { }
    }
  } catch { return $null }
  return $null
}

function Assert-TrayAndPopup([int]$ProcessId) {
  Add-NativeWindowProbe
  $shell = [MembraneQualification.NativeWindowProbe]::Find('Shell_TrayWnd', $null)
  Require ($shell -ne [IntPtr]::Zero) 'Windows notification area is unavailable'
  $element = Find-TrayElement
  if ($null -ne $element) {
    try {
      $pattern = $element.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
      $pattern.Invoke()
    } catch { throw "Membrane tray icon could not be activated: $($_.Exception.Message)" }
  } else {
    # Windows 11 can omit a valid Tauri tray icon from UI Automation (notably
    # when notification overflow is collapsed). Exercise same callback via
    # tray-icon's documented WM_USER_TRAYICON contract, still requiring one
    # process-owned tray window before activation.
    $trayWindows = @(Get-WindowRows $ProcessId | Where-Object { $_.ClassName -eq 'tray_icon_app' })
    Require ($trayWindows.Count -eq 1) 'Membrane tray icon window is missing'
    Require ([MembraneQualification.NativeWindowProbe]::PostTrayClick($trayWindows[0].Handle, $false)) 'Membrane tray icon press could not be posted'
    Start-Sleep -Milliseconds 100
    Require ([MembraneQualification.NativeWindowProbe]::PostTrayClick($trayWindows[0].Handle, $true)) 'Membrane tray icon release could not be posted'
  }
  Start-Sleep -Milliseconds 750
  $windows = @(Get-WindowRows $ProcessId)
  $visible = @($windows | Where-Object { $_.Visible })
  Require ($visible.Count -gt 0) 'Membrane popup did not become visible after tray activation'
  Require (@($windows | Where-Object { $_.Title -match '(?i)Membrane Hub' }).Count -ge 2) 'Hub and popup windows were not both created'
}

function Assert-Dashboard([string]$HubExecutable, [int]$ProcessId) {
  $previousPath = $env:PATH
  try {
    $env:PATH = $script:SafePath
    $second = Start-Process -FilePath $HubExecutable -PassThru -WindowStyle Hidden
  } finally {
    $env:PATH = $previousPath
  }
  try {
    Require ($second.WaitForExit(10000)) 'second Hub invocation did not exit through single-instance cutover'
    Require ($second.ExitCode -eq 0) "second Hub invocation failed with exit code $($second.ExitCode)"
  } finally {
    if ($second -and -not $second.HasExited) { Stop-Process -Id $second.Id -Force -ErrorAction SilentlyContinue }
  }
  Start-Sleep -Milliseconds 500
  $windows = @(Get-WindowRows $ProcessId)
  Require (@($windows | Where-Object { $_.Visible -and $_.Title -match '(?i)Membrane Hub' }).Count -gt 0) 'Hub dashboard did not become visible through single-instance cutover'
}

function Assert-RendererWindows([int]$ProcessId) {
  $windows = @(Get-WindowRows $ProcessId)
  $hubWindows = @($windows | Where-Object { $_.Title -match '(?i)^Membrane Hub' })
  Require ($hubWindows.Count -ge 2) 'installed Hub did not create both embedded dashboard and popup renderer windows'
  Require (@($hubWindows | Where-Object { [string]::IsNullOrWhiteSpace($_.ClassName) }).Count -eq 0) 'installed renderer window class is missing'
  return @($hubWindows | ForEach-Object {
    [ordered]@{ title = [string]$_.Title; className = [string]$_.ClassName; visible = [bool]$_.Visible }
  })
}

function Assert-NativeHostCutover([string]$Root, [string]$HubExecutable) {
  $hubPublisher = Assert-SignedFile $HubExecutable 'installed Hub'
  $blueprintRoot = Join-Path $Root 'runtime\blueprint'
  $blueprintNode = Join-Path $blueprintRoot 'lib\node.exe'
  $blueprintLauncher = Join-Path $blueprintRoot 'bin\blueprint.cmd'
  Require (Test-Path -LiteralPath $blueprintNode -PathType Leaf) 'installed Blueprint runtime node.exe is missing'
  Require (Test-Path -LiteralPath $blueprintLauncher -PathType Leaf) 'installed Blueprint launcher is missing'
  $forbidden = @(Get-ChildItem -LiteralPath $Root -File -Recurse -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -match '(?i)^(node|nodejs|python|pythonw|python3|pip|npm|npx)(\.exe)?$' -and -not $_.FullName.StartsWith($blueprintRoot, [System.StringComparison]::OrdinalIgnoreCase) })
  Require ($forbidden.Count -eq 0) "installed package carries an undeclared interpreter runtime: $($forbidden.FullName -join ', ')"
  $inventoryPath = Join-Path $Root 'runtime\runtime-inventory.json'
  Require (Test-Path -LiteralPath $inventoryPath -PathType Leaf) 'installed runtime inventory is missing'
  $inventory = Read-JsonFile $inventoryPath 'installed runtime inventory'
  Require ($inventory.schemaVersion -eq 3 -and $inventory.target -eq 'x86_64-pc-windows-msvc') 'installed runtime inventory identity is invalid'
  Require ([string]$inventory.components.blueprint.treeSha256 -match '^[0-9a-f]{64}$' -and [int]$inventory.components.blueprint.fileCount -gt 0) 'installed Blueprint inventory metadata is invalid'
  $runtimeRoot = [IO.Path]::GetFullPath((Join-Path $Root 'runtime')).TrimEnd('\') + '\'
  Require ($inventory.entries -is [array] -and $inventory.entries.Count -gt 0) 'installed runtime inventory entries are missing'
  $membraneEntry = @($inventory.entries | Where-Object { $_.delivery -eq 'externalBin' -and $_.component -eq 'membrane-command' })
  $cortexEntry = @($inventory.entries | Where-Object { $_.delivery -eq 'externalBin' -and $_.component -eq 'cortex-cli' })
  $trayEntry = @($inventory.entries | Where-Object { $_.delivery -eq 'externalBin' -and $_.component -eq 'membrane-tray' })
  $daemonEntry = @($inventory.entries | Where-Object { $_.delivery -eq 'externalBin' -and $_.component -eq 'membrane-daemon' })
  Require (($membraneEntry.Count -eq 1) -and ($cortexEntry.Count -eq 1) -and ($trayEntry.Count -eq 1) -and ($daemonEntry.Count -eq 1)) 'installed runtime inventory sidecar entries are not exact and unique'
  $membrane = Get-InstalledSidecar $Root 'membrane' $membraneEntry[0]
  $cortex = Get-InstalledSidecar $Root 'cortex' $cortexEntry[0]
  $tray = Get-InstalledSidecar $Root 'membrane-tray' $trayEntry[0]
  $daemon = Get-InstalledSidecar $Root 'membrane-daemon' $daemonEntry[0]
  [void](Assert-SignedFile $membrane 'installed membrane native host' $hubPublisher)
  [void](Assert-SignedFile $cortex 'installed cortex native host' $hubPublisher)
  [void](Assert-SignedFile $tray 'installed membrane tray sidecar' $hubPublisher)
  [void](Assert-SignedFile $daemon 'installed membrane daemon sidecar' $hubPublisher)
  foreach ($component in @('membrane-command', 'cortex-cli', 'membrane-tray', 'membrane-daemon')) {
    Require (@($inventory.entries | Where-Object { $_.delivery -eq 'externalBin' -and $_.component -eq $component }).Count -eq 1) "installed runtime inventory sidecar entry is missing or duplicated: $component"
  }
  $inventoryEvidence = @(); $seenInventoryPaths = @{}
  foreach ($entry in @($inventory.entries | Where-Object { $_.delivery -ne 'tauriBundle' })) {
    Require (-not [string]::IsNullOrWhiteSpace([string]$entry.installerPath)) 'installed inventory entry path is missing'
    if ($entry.delivery -eq 'externalBin') {
      $path = switch ([string]$entry.component) {
        'membrane-command' { $membrane; break }
        'cortex-cli' { $cortex; break }
        'membrane-tray' { $tray; break }
        'membrane-daemon' { $daemon; break }
        default { throw "installed inventory has unknown external sidecar: $($entry.component)" }
      }
      $relative = $path.Substring($Root.TrimEnd('\').Length).TrimStart('\')
      $expectedPath = switch ([string]$entry.component) {
        'membrane-command' { $membrane; break }
        'cortex-cli' { $cortex; break }
        'membrane-tray' { $tray; break }
        'membrane-daemon' { $daemon; break }
      }
      Require ($path -ieq $expectedPath) "installed sidecar path resolution failed: $($entry.component)"
    } else {
      $relative = ([string]$entry.installerPath).Replace('/', '\')
      $path = [IO.Path]::GetFullPath((Join-Path $runtimeRoot $relative))
      Require ($path.StartsWith($runtimeRoot, [StringComparison]::OrdinalIgnoreCase)) "installed inventory path escapes runtime root: $relative"
    }
    Require (-not $seenInventoryPaths.ContainsKey($path.ToLowerInvariant())) "installed inventory contains duplicate path: $relative"
    $seenInventoryPaths[$path.ToLowerInvariant()] = $true
    Require (Test-Path -LiteralPath $path -PathType Leaf) "installed inventory file is missing: $relative"
    $actual = Hash-File $path
    Require ($actual -ieq [string]$entry.sha256) "installed inventory hash mismatch: $relative"
    $inventoryEvidence += [ordered]@{ path = $relative.Replace('\', '/'); sha256 = $actual }
  }
  Require (@($inventory.entries | Where-Object { $_.component -eq 'blueprint-runtime' -and $_.delivery -eq 'installedComponent' }).Count -gt 0) 'installed runtime inventory omits Blueprint component'
  $blueprintEntry = @($inventory.entries | Where-Object { $_.component -eq 'blueprint-runtime' -and $_.installerPath -match '(?i)(^|[\\/])lib[\\/]node\.exe$' })
  Require ($blueprintEntry.Count -eq 1) 'installed runtime inventory does not uniquely bind Blueprint node.exe'
  Require ((Hash-File $blueprintNode) -ieq [string]$blueprintEntry[0].sha256) 'installed Blueprint node.exe hash does not match inventory'
  return [pscustomobject]@{ Hub = $HubExecutable; Membrane = $membrane; Cortex = $cortex; Tray = $tray; Daemon = $daemon; BlueprintNode = $blueprintNode; BlueprintNodeSha256 = [string]$blueprintEntry[0].sha256; BlueprintRuntime = $blueprintRoot; RuntimeInventory = $inventoryPath; RuntimeInventoryEvidence = $inventoryEvidence; Publisher = $hubPublisher }
}

function Invoke-NativeProcess([string]$Executable, [string]$Arguments, [string]$InputText = '', [string]$WorkingDirectory = $InstallRoot, [hashtable]$Environment = @{}) {
  $start = [System.Diagnostics.ProcessStartInfo]::new()
  $start.FileName = $Executable
  $start.Arguments = $Arguments
  $start.WorkingDirectory = $WorkingDirectory
  $start.UseShellExecute = $false
  $start.CreateNoWindow = $true
  $start.RedirectStandardInput = $true
  $start.RedirectStandardOutput = $true
  $start.RedirectStandardError = $true
  $start.EnvironmentVariables['PATH'] = $script:SafePath
  foreach ($name in $Environment.Keys) {
    if ($null -eq $Environment[$name]) { $start.EnvironmentVariables.Remove($name) }
    else { $start.EnvironmentVariables[$name] = [string]$Environment[$name] }
  }
  $process = [System.Diagnostics.Process]::new()
  $process.StartInfo = $start
  Require ($process.Start()) "could not start native process: $Executable"
  if (-not [string]::IsNullOrEmpty($InputText)) { $process.StandardInput.Write($InputText) }
  $process.StandardInput.Close()
  $stdout = $process.StandardOutput.ReadToEnd()
  $stderr = $process.StandardError.ReadToEnd()
  Require ($process.WaitForExit($TimeoutSeconds * 1000)) "native process timed out: $Executable $Arguments"
  Require ($process.ExitCode -eq 0) "native process failed ($($process.ExitCode)): $Executable $Arguments :: $stderr"
  return [pscustomobject]@{ Stdout = $stdout; Stderr = $stderr; Executable = $Executable; Arguments = $Arguments; WorkingDirectory = $WorkingDirectory }
}

function Quote-NativeArgument([string]$Value) {
  Require (-not [string]::IsNullOrWhiteSpace($Value)) 'native Adapt argument is empty'
  return '"' + $Value.Replace('"', '\"') + '"'
}

function Read-NativeOutput([string]$Text, [string]$Label) {
  try { return ($Text | ConvertFrom-Json) }
  catch { throw "$Label emitted invalid JSON: $Text" }
}

function Write-NativeText([string]$Path, [string]$Text) {
  [IO.File]::WriteAllText($Path, $Text, [Text.UTF8Encoding]::new($false))
}

function Invoke-InstalledAdaptQualification([string]$MembraneExecutable, [string]$Root, [string]$Database) {
  # This runner intentionally creates its input under %TEMP%, never under this
  # checkout. The only executable involved is the installed native sidecar;
  # source Adapt, Python, Pi, OpenCode, and Node are absent from PATH.
  $adaptRoot = Join-Path $script:QualificationWorkspace 'adapt-native'
  New-Item -ItemType Directory -Path $adaptRoot -Force | Out-Null
  $transcript = Join-Path $adaptRoot 'selected-transcript.jsonl'
  $minedPath = Join-Path $adaptRoot 'mine.json'
  $reviewPath = Join-Path $adaptRoot 'review.json'
  $pendingPath = Join-Path $adaptRoot 'pending.json'
  $acceptedPath = Join-Path $adaptRoot 'accepted.json'
  $decisionsPath = Join-Path $adaptRoot 'decisions.json'
  $contractPath = Join-Path $adaptRoot 'review-contract.json'
  $transcriptText = @'
{"type":"adapt_event_v1","host":"pi","event":{"sessionId":"native-qualification-session","kind":"user_message","role":"user","timestamp":"2026-08-26T00:00:00Z","text":"never use npm install in this repo"}}
{"type":"adapt_event_v1","host":"pi","event":{"sessionId":"native-qualification-session","kind":"assistant_message","role":"assistant","timestamp":"2026-08-26T00:00:01Z","text":"Understood."}}
'@
  Write-NativeText $transcript $transcriptText.TrimStart("`n")

  $nativeEnv = @{ CORTEX_DB = $Database; MEMBRANE_WORKSPACE_ROOT = $script:QualificationWorkspace }
  $mineArgs = "adapt mine --host pi --scope workspace " + (Quote-NativeArgument $transcript)
  $mine = Invoke-NativeProcess $MembraneExecutable $mineArgs '' $adaptRoot $nativeEnv
  $mineValue = Read-NativeOutput $mine.Stdout 'native Adapt mine'
  Require ($mineValue.response.api_version -eq 'adapt.cli.v1') 'native Adapt mine API contract is invalid'
  Require (@($mineValue.taste_candidates).Count -eq 1) 'native Adapt mine did not produce exact selected-transcript candidate'
  Require ($mineValue.taste_review.contract -eq 'adapt.taste-review-input.v1') 'native Adapt mine omitted Taste source binding'
  Require (@($mineValue.taste_review.sources).Count -eq 1) 'native Adapt mine did not bind one selected source'
  Write-NativeText $minedPath ($mineValue | ConvertTo-Json -Depth 40)

  $reviewArgs = "adapt review --input " + (Quote-NativeArgument $minedPath)
  $review = Invoke-NativeProcess $MembraneExecutable $reviewArgs '' $adaptRoot $nativeEnv
  $reviewValue = Read-NativeOutput $review.Stdout 'native Adapt review'
  Require ($reviewValue.api_version -eq 'adapt.cli.v1') 'native Adapt review API contract is invalid'
  Write-NativeText $reviewPath ($reviewValue | ConvertTo-Json -Depth 40)

  # Local review owns its batch identity. Live canonical-pool identity is
  # computed by installed Membrane from its DB and emitted in pending output.
  $reviewInstallationId = 'installed-local-qualification'

  $reviewTasteArgs = "adapt --db " + (Quote-NativeArgument $Database) + " review-taste --input " + (Quote-NativeArgument $minedPath) +
    " --installation-id " + (Quote-NativeArgument $reviewInstallationId) +
    ' --created-at "2026-08-26T00:00:02Z"'
  $pending = Invoke-NativeProcess $MembraneExecutable $reviewTasteArgs '' $adaptRoot $nativeEnv
  $pendingValue = Read-NativeOutput $pending.Stdout 'native Adapt review-taste'
  Require (@($pendingValue.records).Count -eq 1) 'native Adapt review-taste did not produce one pending record'
  Write-NativeText $pendingPath ($pendingValue | ConvertTo-Json -Depth 40)

  # Explicit caller-selected transcripts use a local, human-review contract.
  # Identity comes from the freshly rebuilt pending manifest; no packaged
  # decision or qualification artifact is trusted.
  $installationId = [string]$pendingValue.installation_id
  $canonicalPoolSha256 = [string]$pendingValue.canonical_pool_sha256
  $pendingManifestSha256 = [string]$pendingValue.manifest_sha256
  Require (-not [string]::IsNullOrWhiteSpace($installationId)) 'native Adapt pending manifest omitted installationId'
  Require ($canonicalPoolSha256 -match '^[0-9a-f]{64}$') 'native Adapt pending manifest omitted canonical pool digest'
  Require ($pendingManifestSha256 -match '^[0-9a-f]{64}$') 'native Adapt pending manifest omitted manifest digest'
  Require ($installationId -eq $reviewInstallationId) 'native Adapt pending installationId changed after review binding'
  $reviewContract = [ordered]@{
    schema = 'adapt.user-taste-review.v1'
    installationId = $installationId
    canonicalPoolSha256 = $canonicalPoolSha256
    pendingManifestSha256 = $pendingManifestSha256
    candidateSetSha256 = [string]$mineValue.taste_review.candidate_set_sha256
    sourceBindings = @($mineValue.taste_review.sources | ForEach-Object { [ordered]@{ path = [string]$_.path; prefixDigest = [string]$_.prefix_digest } })
    selection = 'caller-selected-transcript'
    review = 'local-human-adjudication-required'
  }
  $adjudicateArgs = "adapt adjudicate-taste --manifest " + (Quote-NativeArgument $pendingPath) +
    " --decisions " + (Quote-NativeArgument $decisionsPath) +
    ' --validated-at "2026-08-26T00:00:03Z"'
  $decisions = [ordered]@{
    contract_version = 'adapt.user-taste-review.v1'
    independent = $true
    issuer_id = ''
    key_id = ''
    installation_id = $installationId
    validator_receipt_id = "local-user-review-$($pendingManifestSha256.Substring(0, 16))"
    pending_manifest_sha256 = $pendingManifestSha256
    canonical_pool_sha256 = $canonicalPoolSha256
    validated_at = '2026-08-26T00:00:03Z'
    decisions = @($pendingValue.records | ForEach-Object {
      [ordered]@{ id = [string]$_.id; verdict = 'valid'; reason = 'explicit caller-selected transcript preference reviewed locally' }
    })
    signature_hex = ''
  }
  Write-NativeText $decisionsPath ($decisions | ConvertTo-Json -Depth 40)
  $reviewContract.decisionPath = $decisionsPath
  $reviewContract.decisionSha256 = Hash-File $decisionsPath
  Write-NativeText $contractPath ($reviewContract | ConvertTo-Json -Depth 40)
  $adjudicated = Invoke-NativeProcess $MembraneExecutable $adjudicateArgs '' $adaptRoot $nativeEnv
  $acceptedValue = Read-NativeOutput $adjudicated.Stdout 'native Adapt adjudicate-taste'
  Require (@($acceptedValue.records).Count -eq 1) 'native Adapt adjudicate-taste did not produce one accepted record'
  Write-NativeText $acceptedPath ($acceptedValue | ConvertTo-Json -Depth 40)

  $applyArgs = "adapt --db " + (Quote-NativeArgument $Database) + ' apply --manifest ' + (Quote-NativeArgument $acceptedPath)
  $applied = Invoke-NativeProcess $MembraneExecutable $applyArgs '' $adaptRoot $nativeEnv
  $appliedValue = Read-NativeOutput $applied.Stdout 'native Adapt apply'
  Require ($appliedValue.response.valid -eq $true -and @($appliedValue.response.accepted_record_ids).Count -eq 1) 'native Adapt apply did not admit one record'
  Require ($appliedValue.cortex_receipt.complete -eq $true) 'native Adapt apply omitted complete Cortex receipt'

  $recallArgs = "adapt --db " + (Quote-NativeArgument $Database) + ' recall npm --scope workspace'
  $recalled = Invoke-NativeProcess $MembraneExecutable $recallArgs '' $adaptRoot $nativeEnv
  $recalledValue = Read-NativeOutput $recalled.Stdout 'native Adapt recall'
  Require (@($recalledValue.records).Count -eq 1) 'native Adapt recall did not return admitted record'
  return [ordered]@{
    contract = [ordered]@{ path = $contractPath; sha256 = Hash-File $contractPath; schema = [string]$reviewContract.schema; installationId = $installationId; canonicalPoolSha256 = $canonicalPoolSha256; pendingManifestSha256 = $pendingManifestSha256 }
    selectedTranscript = [ordered]@{ path = $transcript; sha256 = Hash-File $transcript; root = $adaptRoot; source = 'caller-selected'; checkout = $false }
    mine = [ordered]@{ apiVersion = [string]$mineValue.response.api_version; candidates = @($mineValue.taste_candidates).Count; sourceBindings = @($mineValue.taste_review.sources).Count }
    review = [ordered]@{ apiVersion = [string]$reviewValue.api_version; status = 'pass' }
    reviewTaste = [ordered]@{ contract = [string]$mineValue.taste_review.contract; records = @($pendingValue.records).Count; candidateSetSha256 = [string]$mineValue.taste_review.candidate_set_sha256 }
    adjudicate = [ordered]@{ records = @($acceptedValue.records).Count; validatedAt = '2026-08-26T00:00:03Z'; decisionsSha256 = Hash-File $decisionsPath; decisionsPath = $decisionsPath; contract = 'adapt.user-taste-review.v1' }
    apply = [ordered]@{ acceptedRecordIds = @($appliedValue.response.accepted_record_ids); cortexComplete = [bool]$appliedValue.cortex_receipt.complete }
    recall = [ordered]@{ query = 'npm'; records = @($recalledValue.records).Count; lifecycle = [string]$recalledValue.records[0].record.lifecycle_state }
    processPolicy = [ordered]@{ executable = $MembraneExecutable; path = $script:SafePath; python = $false; pi = $false; openCode = $false; node = $false; checkout = $false; nativeOnly = $true }
  }
}

function Invoke-NativeMcp([string]$Executable, [switch]$ExerciseAll, [hashtable]$AdditionalCalls) {
  $caller = [ordered]@{
    root = $script:QualificationWorkspace
    repositoryId = 'windows-qualification'
    scopeId = 'windows-qualification'
  }
  $allTools = @(
    'membrane_context', 'membrane_source_read', 'membrane_blueprint',
    'membrane_knowledge_propose', 'membrane_checkpoint_save', 'membrane_checkpoint_load',
    'membrane_working_context', 'membrane_temporal_fact', 'membrane_scratchpad',
    'membrane_feedback', 'membrane_diagnostic_workspace', 'membrane_diagnostic_mutation',
    'membrane_diagnostic_snapshot', 'membrane_diagnostic_fence',
    'membrane_diagnostic_capabilities', 'membrane_diagnostic_baseline',
    'membrane_diagnostic_provider'
  )
  $arguments = [ordered]@{
    membrane_context = [ordered]@{ task = 'installed Windows qualification'; repository = 'windows-qualification'; caller = $caller; budget = 1 }
    membrane_source_read = [ordered]@{ repository = 'windows-qualification'; caller = $caller }
    membrane_blueprint = [ordered]@{ repository = 'windows-qualification'; caller = $caller; operation = 'changes' }
    membrane_knowledge_propose = [ordered]@{ repository = 'windows-qualification'; caller = $caller; emission = [ordered]@{} }
    membrane_checkpoint_save = [ordered]@{ repository = 'windows-qualification'; caller = $caller; checkpoint = [ordered]@{} }
    membrane_checkpoint_load = [ordered]@{ repository = 'windows-qualification'; caller = $caller; id = 'qualification-missing-checkpoint' }
    membrane_working_context = [ordered]@{ repository = 'windows-qualification'; caller = $caller; operation = 'load'; sessionId = 'qualification-session'; taskId = 'qualification-task' }
    membrane_temporal_fact = [ordered]@{ repository = 'windows-qualification'; caller = $caller; operation = 'query'; subject = 'qualification'; predicate = 'state'; asOf = '2026-01-01T00:00:00Z' }
    membrane_scratchpad = [ordered]@{ repository = 'windows-qualification'; caller = $caller; operation = 'clear'; sessionId = 'qualification-session'; taskId = 'qualification-task' }
    membrane_feedback = [ordered]@{ repository = 'windows-qualification'; caller = $caller; outcome = 'used' }
    membrane_diagnostic_workspace = [ordered]@{ operation = 'status'; repoId = 'windows-qualification'; worktreeId = 'windows-qualification'; projectRoot = $script:QualificationWorkspace }
    membrane_diagnostic_mutation = [ordered]@{ operation = 'unsupported'; repoId = 'windows-qualification'; worktreeId = 'windows-qualification'; projectRoot = $script:QualificationWorkspace }
    membrane_diagnostic_snapshot = [ordered]@{ operation = 'get'; repoId = 'windows-qualification'; worktreeId = 'windows-qualification'; projectRoot = $script:QualificationWorkspace }
    membrane_diagnostic_fence = [ordered]@{ operation = 'evaluate'; repoId = 'windows-qualification'; worktreeId = 'windows-qualification'; projectRoot = $script:QualificationWorkspace }
    membrane_diagnostic_capabilities = [ordered]@{ operation = 'list'; repoId = 'windows-qualification'; worktreeId = 'windows-qualification'; projectRoot = $script:QualificationWorkspace }
    membrane_diagnostic_baseline = [ordered]@{ operation = 'unsupported'; repoId = 'windows-qualification'; worktreeId = 'windows-qualification'; projectRoot = $script:QualificationWorkspace }
    membrane_diagnostic_provider = [ordered]@{ operation = 'list'; repoId = 'windows-qualification'; worktreeId = 'windows-qualification'; projectRoot = $script:QualificationWorkspace }
  }
  $requests = New-Object 'System.Collections.Generic.List[string]'
  $requests.Add((@{ jsonrpc = '2.0'; id = 1; method = 'initialize'; params = @{ protocolVersion = '2025-03-26'; capabilities = @{}; clientInfo = @{ name = 'membrane-windows-qualification'; version = '1' } } } | ConvertTo-Json -Compress -Depth 20))
  $requests.Add((@{ jsonrpc = '2.0'; method = 'notifications/initialized'; params = @{} } | ConvertTo-Json -Compress -Depth 20))
  $requests.Add((@{ jsonrpc = '2.0'; id = 2; method = 'tools/list'; params = @{ _meta = @{ 'membrane.toolsets.v1' = @('memory', 'blueprint', 'diagnostic') } } } | ConvertTo-Json -Compress -Depth 20))
  $callNames = New-Object 'System.Collections.Generic.List[string]'
  if ($ExerciseAll) { foreach ($name in $allTools) { $callNames.Add($name) } }
  if ($AdditionalCalls) { foreach ($name in $AdditionalCalls.Keys) { if (-not $callNames.Contains($name)) { $callNames.Add($name) } } }
  $id = 100
  foreach ($name in $callNames) {
    $payload = if ($AdditionalCalls -and $AdditionalCalls.ContainsKey($name)) { $AdditionalCalls[$name] } else { $arguments[$name] }
    Require ($null -ne $payload) "no MCP qualification payload for $name"
    $requests.Add((@{ jsonrpc = '2.0'; id = $id; method = 'tools/call'; params = @{ name = $name; arguments = $payload } } | ConvertTo-Json -Compress -Depth 30))
    $id++
  }
  $wire = ($requests -join "`n") + "`n"
  $result = Invoke-NativeProcess $Executable 'stdio-mcp' $wire
  $responses = New-Object 'System.Collections.Generic.List[object]'
  foreach ($line in @($result.Stdout -split "`r?`n" | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })) {
    try { $responses.Add(($line | ConvertFrom-Json)) }
    catch { throw "native MCP emitted non-JSON stdout: $line" }
  }
  $initialize = @($responses | Where-Object { $_.id -eq 1 }) | Select-Object -First 1
  $listing = @($responses | Where-Object { $_.id -eq 2 }) | Select-Object -First 1
  Require ($null -ne $initialize -and $null -ne $initialize.result.serverInfo) 'MCP initialize response is invalid'
  $tools = @($listing.result.tools)
  Require ($tools.Count -eq 17) "MCP tools/list returned $($tools.Count) tools; expected 17"
  $actualNames = (@($tools.name) | Sort-Object) -join ','
  $expectedNames = ($allTools | Sort-Object) -join ','
  Require ($actualNames -eq $expectedNames) 'MCP tools/list does not match the 17-tool registry'
  $calls = @($responses | Where-Object { [int]$_.id -ge 100 })
  Require ($calls.Count -eq $callNames.Count) "MCP returned $($calls.Count) tool responses; expected $($callNames.Count)"
  foreach ($call in $calls) {
    Require ($null -ne $call.result.structuredContent) "MCP tool response $($call.id) omitted structuredContent"
    Require (-not [string]::IsNullOrWhiteSpace([string]$call.result.structuredContent.operation)) "MCP tool response $($call.id) omitted operation envelope"
  }
	  # Materialize generic collections explicitly; PowerShell's array
	  # subexpression binder can throw "Argument types do not match" when a
	  # JSON-RPC response carries mixed structured-content shapes.
	  return [pscustomobject]@{ Responses = $responses.ToArray(); Tools = @($tools); Calls = @($calls) }
}

function Invoke-HubMcpCall([string]$Name, $Payload) {
  Require ($null -ne $script:ActiveHubHealth -and $script:ActiveHubPort) "Hub MCP call $Name requires an active Hub"
  $tokenPath = Join-Path $script:QualificationWorkspace 'tools\.cache\memory\api-token'
  $identityPath = Join-Path $script:QualificationWorkspace 'tools\.cache\memory\installation.json'
  Require (Test-Path -LiteralPath $tokenPath -PathType Leaf) 'Hub MCP token is missing'
  Require (Test-Path -LiteralPath $identityPath -PathType Leaf) 'Hub MCP installation identity is missing'
  $token = (Get-Content -LiteralPath $tokenPath -Raw).Trim()
  $identity = Read-JsonFile $identityPath 'Hub MCP installation identity'
  $sessionId = if ($identity.currentServiceInstanceId) { [string]$identity.currentServiceInstanceId } else { [string]$identity.current_service_instance_id }
  Require (-not [string]::IsNullOrWhiteSpace($token)) 'Hub MCP token is empty'
  Require (-not [string]::IsNullOrWhiteSpace($sessionId)) 'Hub MCP session identity is missing'
  $wire = (@{
      jsonrpc = '2.0'; id = 1; method = 'tools/call';
      params = @{ name = $Name; arguments = $Payload }
    } | ConvertTo-Json -Compress -Depth 30)
  $client = [System.Net.Http.HttpClient]::new()
  $request = [System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::Post, "http://127.0.0.1:$($script:ActiveHubPort)/mcp")
  try {
    $request.Content = [System.Net.Http.StringContent]::new($wire, [Text.Encoding]::UTF8, 'application/json')
    [void]$request.Headers.TryAddWithoutValidation('Origin', "http://127.0.0.1:$($script:ActiveHubPort)")
    [void]$request.Headers.TryAddWithoutValidation('Authorization', "Bearer $token")
    [void]$request.Headers.TryAddWithoutValidation('x-membrane-installation-id', [string]$script:ActiveHubHealth.installationId)
    [void]$request.Headers.TryAddWithoutValidation('x-membrane-cortex-store-id', [string]$script:ActiveHubHealth.cortexStoreId)
    [void]$request.Headers.TryAddWithoutValidation('x-membrane-release-generation', [string]$script:ActiveHubHealth.releaseGeneration)
    [void]$request.Headers.TryAddWithoutValidation('x-membrane-session', $sessionId)
    $response = $client.SendAsync($request).GetAwaiter().GetResult()
    $body = $response.Content.ReadAsStringAsync().GetAwaiter().GetResult()
    Require ($response.IsSuccessStatusCode) "Hub MCP call $Name failed with HTTP $([int]$response.StatusCode): $body"
    try { return ($body | ConvertFrom-Json) } catch { throw "Hub MCP call $Name emitted invalid JSON: $body" }
  } finally {
    $request.Dispose(); $client.Dispose()
  }
}

function Get-DoctorPaths([string]$MembraneExecutable) {
  $result = Invoke-NativeProcess $MembraneExecutable 'cli doctor paths'
  try { return ($result.Stdout | ConvertFrom-Json) }
  catch { throw "native doctor paths emitted invalid JSON: $($result.Stdout)" }
}

function Save-State([string]$MembraneExecutable) {
  $contextId = "windows-qualification-$([guid]::NewGuid().ToString('N'))"
  $payload = [ordered]@{
    repository = 'windows-qualification'; caller = [ordered]@{ root = $script:QualificationWorkspace; repositoryId = 'windows-qualification'; scopeId = 'windows-qualification' }
    operation = 'save'; sessionId = 'qualification-session'; taskId = 'qualification-task'; contextId = $contextId
    context = [ordered]@{ contextId = $contextId; marker = 'native-upgrade-continuity'; value = [guid]::NewGuid().ToString('N') }
  }
  $saved = Invoke-HubMcpCall 'membrane_working_context' $payload
  Require ($saved.result.structuredContent.result.kind -eq 'success') "working-context state save did not succeed: $($saved.result.structuredContent | ConvertTo-Json -Compress -Depth 20)"
  $script:State = [pscustomobject]@{ ContextId = $contextId; Marker = $payload.context.marker; Hash = (ConvertTo-Json $payload.context -Compress) }
}

function Assert-State([string]$MembraneExecutable, [string]$Phase) {
  $payload = [ordered]@{
    repository = 'windows-qualification'; caller = [ordered]@{ root = $script:QualificationWorkspace; repositoryId = 'windows-qualification'; scopeId = 'windows-qualification' }
    operation = 'load'; sessionId = 'qualification-session'; taskId = 'qualification-task'
  }
  $loaded = Invoke-HubMcpCall 'membrane_working_context' $payload
  Require ($loaded.result.structuredContent.result.kind -eq 'success') "working-context state load failed after $Phase`: $($loaded.result.structuredContent | ConvertTo-Json -Compress -Depth 20)"
  $contexts = @($loaded.result.structuredContent.result.data.contexts)
  Require (@($contexts | Where-Object { $_.contextId -eq $script:State.ContextId -and $_.marker -eq $script:State.Marker }).Count -eq 1) "working-context state was not continuous after $Phase"
}

function Seed-WorkspaceV2Config([string]$Path, [string]$WorkspaceRoot) {
  $runtimeDirectory = Join-Path $WorkspaceRoot 'tools\lib\memory'
  New-Item -ItemType Directory -Path $runtimeDirectory -Force | Out-Null
  # Installed Hub resolves its in-process native runtime from this canonical
  # workspace identity; qualification must seed the same contract a real
  # workspace install provides before starting the signed package.
  $runtime = [ordered]@{
    schemaVersion = 1
    serviceId = 'membrane-local-v1'
    host = '127.0.0.1'
    port = 47851
  }
  Write-NativeText (Join-Path $runtimeDirectory 'runtime.json') ($runtime | ConvertTo-Json -Compress)
  $legacy = [ordered]@{
    schemaVersion = 2
    workspaceRoot = [IO.Path]::GetFullPath($WorkspaceRoot)
    pythonExecutable = [IO.Path]::GetFullPath((Join-Path $script:QualificationWorkspace 'removed-python.exe'))
  }
  Write-NativeText $Path ($legacy | ConvertTo-Json -Compress -Depth 10)
}

function Initialize-QualificationRepository([string]$WorkspaceRoot) {
  # Blueprint fingerprints repository inputs through Git. Use a fresh local
  # repository under %TEMP%, keeping installed qualification independent from
  # this checkout while exercising real repository semantics.
  Require (-not [string]::IsNullOrWhiteSpace($script:GitPath)) 'system Git is required for Blueprint qualification'
  $readme = Join-Path $WorkspaceRoot 'README.md'
  Write-NativeText $readme "# Windows qualification`n"
  $git = $script:GitPath
  $previousErrorAction = $ErrorActionPreference
  $ErrorActionPreference = 'Continue'
  try {
    $init = & $git -C $WorkspaceRoot init --quiet 2>&1
  } finally { $ErrorActionPreference = $previousErrorAction }
  Require ($LASTEXITCODE -eq 0) "could not initialize qualification repository: $init"
  $previousErrorAction = $ErrorActionPreference
  $ErrorActionPreference = 'Continue'
  try {
    $add = & $git -C $WorkspaceRoot add -- README.md 2>&1
  } finally { $ErrorActionPreference = $previousErrorAction }
  Require ($LASTEXITCODE -eq 0) "could not stage qualification repository: $add"
  $previousErrorAction = $ErrorActionPreference
  $ErrorActionPreference = 'Continue'
  try {
    $commit = & $git -C $WorkspaceRoot -c user.name='Membrane Qualification' -c user.email='qualification@membrane.invalid' commit --quiet -m 'qualification seed' 2>&1
  } finally { $ErrorActionPreference = $previousErrorAction }
  Require ($LASTEXITCODE -eq 0) "could not commit qualification repository: $commit"
}

function Assert-WorkspaceConfigMigrated([string]$Path, [string]$Phase, [string]$ExpectedSha256 = '') {
  Require (Test-Path -LiteralPath $Path -PathType Leaf) "workspace config was not written during $Phase"
  $config = Read-JsonFile $Path "workspace config during $Phase"
  Require ([int]$config.schemaVersion -eq 3) "workspace config did not migrate to strict v3 during $Phase"
  Require ($null -eq $config.PSObject.Properties['pythonExecutable']) "workspace config retained pythonExecutable during $Phase"
  Require ((Normalize-ComparablePath ([string]$config.workspaceRoot)) -eq (Normalize-ComparablePath $script:QualificationWorkspace)) "workspace config root changed during $Phase"
  $temporary = @(Get-ChildItem -LiteralPath (Split-Path -Parent $Path) -Filter '.workspace-*.tmp' -File -ErrorAction Stop)
  Require ($temporary.Count -eq 0) "workspace config migration left temporary files during $Phase"
  $digest = Hash-File $Path
  if (-not [string]::IsNullOrWhiteSpace($ExpectedSha256)) { Require ($digest -eq $ExpectedSha256) "workspace config hash changed during $Phase" }
  return $digest
}

function Start-AndVerifyHub([string]$Phase, [string]$ExpectedVersion, [string]$ExpectedGeneration, [string]$ForbiddenGeneration, [switch]$Full) {
  $hub = Get-InstalledExecutable $InstallRoot
  $installedVersion = Get-ArtifactVersion $hub "installed Hub during $Phase"
  Require ($installedVersion -eq $ExpectedVersion) "installed Hub version $installedVersion does not match expected $ExpectedVersion during $Phase"
  # A real `membrane activate` (Invoke-Activation) launches the resident tray
  # exactly as a customer install does. Adopt that process when it is running
  # from the installed root; start one only when nothing is resident.
  $resident = @(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object { $_.ExecutablePath -and ($_.ExecutablePath -ieq $hub) })
  if ($resident.Count -ge 1) {
    $script:HubProcess = Get-Process -Id ([int]$resident[0].ProcessId)
  } else {
    $previousPath = $env:PATH
    try {
      $env:PATH = $script:SafePath
      $script:HubProcess = Start-Process -FilePath $hub -WorkingDirectory $InstallRoot -PassThru
    } finally {
      $env:PATH = $previousPath
    }
  }
  $port = Get-RuntimePort $InstallRoot
  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  do {
    try {
      $health = Invoke-RestMethod -Uri "http://127.0.0.1:$port/health" -TimeoutSec 3
      if ($health.ok -eq $true) { break }
    } catch { }
    Start-Sleep -Milliseconds 500
  } while ((Get-Date) -lt $deadline)
  Require ($null -ne $script:HubProcess -and -not $script:HubProcess.HasExited) "Hub exited during $Phase"
  try { $health = Invoke-RestMethod -Uri "http://127.0.0.1:$port/health" -TimeoutSec 5 } catch {
    $logs = Save-RuntimeLogEvidence "hub-health-$($Phase -replace '[^a-z0-9]+','-')"
    throw "Hub health unavailable during $Phase (port $port; runtime logs copied to $logs)"
  }
  Require ($health.ok -eq $true) "Hub /health was not ok during $Phase"
  Require ($health.serviceId -eq 'membrane-hub') "Hub native service identity is invalid during $Phase"
  foreach ($name in @('installationId', 'cortexStoreId', 'releaseGeneration')) {
    Require (-not [string]::IsNullOrWhiteSpace([string]$health.$name)) "Hub health omitted $name during $Phase"
  }
  $generation = Normalize-Generation $health.releaseGeneration "Hub releaseGeneration during $Phase"
  if (-not [string]::IsNullOrWhiteSpace($ExpectedGeneration)) { Require ($generation -eq $ExpectedGeneration) "Hub releaseGeneration does not match expected generation during $Phase" }
  if (-not [string]::IsNullOrWhiteSpace($ForbiddenGeneration)) { Require ($generation -ne $ForbiddenGeneration) "Hub downgrade retained current releaseGeneration during $Phase" }
  Require ([int]$health.protocolVersion -eq 1 -and [int]$health.schemaVersion -eq 1) "Hub protocol/schema handshake is invalid during $Phase"
  Require ($health.nativeOnly -eq $true) "Hub did not attest nativeOnly during $Phase"
  $script:ActiveHubHealth = $health
  $script:ActiveHubPort = $port
  $subsystems = @($health.subsystems | Sort-Object)
  Require (($subsystems -join ',') -eq 'adapt,blueprint,cortex,ledger,pull,push') "Hub six-subsystem health is invalid during $Phase"
  Require (@($health.capabilities) -contains 'memory') "Hub health omitted memory capability during $Phase"
  $native = Assert-NativeHostCutover $InstallRoot $hub
  $assets = @(Assert-RendererWindows $script:HubProcess.Id)
  $tree = @(Assert-NativeSteadyState $script:HubProcess.Id $native.BlueprintNode $native.BlueprintNodeSha256)
  $mcp = $null
  $blueprint = $null
  if ($Full) {
    Assert-TrayAndPopup $script:HubProcess.Id
    Assert-Dashboard $hub $script:HubProcess.Id
    $mcp = Invoke-NativeMcp $native.Membrane -ExerciseAll
    $blueprint = Assert-BlueprintResident $InstallRoot $script:QualificationWorkspace
    $tree = @(Assert-NativeSteadyState $script:HubProcess.Id $native.BlueprintNode $native.BlueprintNodeSha256)
  }
  return [pscustomobject]@{
    Hub = $hub
    Native = $native
    Port = $port
    Version = $installedVersion
    ReleaseGeneration = $generation
    Health = $health
    ProcessTree = @(Convert-ProcessEvidence $tree)
    Assets = $assets
    InstalledContent = @(Get-InstalledContentEvidence $InstallRoot)
    Blueprint = $blueprint
    McpTools = if ($mcp) { @($mcp.Tools.name | Sort-Object) } else { @() }
    McpOperations = if ($mcp) { @($mcp.Calls.result.structuredContent.operation | Sort-Object) } else { @() }
  }
}

function Start-AndVerifyPreviousHub([string]$ExpectedVersion) {
  $phase = 'downgrade'
  $hub = Get-InstalledExecutable $InstallRoot
  $installedVersion = Get-ArtifactVersion $hub "installed Hub during $phase"
  Require ($installedVersion -eq $ExpectedVersion) "installed Hub version $installedVersion does not match expected $ExpectedVersion during $phase"
  $previousPath = $env:PATH
  try {
    $env:PATH = $script:SafePath
    $script:HubProcess = Start-Process -FilePath $hub -WorkingDirectory $InstallRoot -PassThru
  } finally {
    $env:PATH = $previousPath
  }
  Start-Sleep -Seconds 2
  Require ($null -ne $script:HubProcess -and -not $script:HubProcess.HasExited) 'previous signed Hub did not remain running during downgrade'
  $tree = @(Get-ProcessTree $script:HubProcess.Id)
  Require ($tree.Count -ge 1) 'previous signed Hub process tree was unavailable during downgrade'
  return [pscustomobject]@{
    hub = $hub
    version = $installedVersion
    processTree = @(Convert-ProcessEvidence $tree)
    installedContent = @(Get-InstalledContentEvidence $InstallRoot)
  }
}

function Test-BlueprintPipeClosed([string]$Endpoint) {
  $pipeName = $Endpoint.Substring($Endpoint.LastIndexOf('\') + 1)
  $client = [IO.Pipes.NamedPipeClientStream]::new('.', $pipeName, [IO.Pipes.PipeDirection]::InOut, [IO.Pipes.PipeOptions]::Asynchronous)
  try {
    try { $client.Connect(250); return $false }
    catch [TimeoutException] { return $true }
    catch [IO.IOException] { return $true }
  } finally { $client.Dispose() }
}

function Assert-QualificationProcessTreeGone([int[]]$ProcessIds, [string]$Root) {
  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  do {
    $live = @(Get-CimInstance Win32_Process -ErrorAction Stop | Where-Object {
      ($ProcessIds -contains [int]$_.ProcessId) -or ($_.ExecutablePath -and $_.ExecutablePath.StartsWith($Root.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase))
    })
    if ($live.Count -eq 0) { return }
    Start-Sleep -Milliseconds 250
  } while ((Get-Date) -lt $deadline)
  throw "Hub process descendants remain after shutdown: $($live.Name -join ', ')"
}

function Stop-QualificationHub {
  if ($null -eq $script:HubProcess) { $script:ActiveHubHealth = $null; $script:ActiveHubPort = $null; return }
  $hubPid = $script:HubProcess.Id
  $rootProcess = Get-Process -Id $hubPid -ErrorAction SilentlyContinue
  if ($null -eq $rootProcess -or $rootProcess.HasExited) {
    Assert-QualificationProcessTreeGone @($hubPid) $InstallRoot
    Require (Test-BlueprintPipeClosed (Get-BlueprintEndpoint)) 'Blueprint named pipe remained open after Hub shutdown'
    $script:HubProcess = $null
    $script:ActiveHubHealth = $null; $script:ActiveHubPort = $null
    return
  }
  $tree = @(Get-ProcessTree $hubPid)
  $ids = @($tree.ProcessId | ForEach-Object { [int]$_ })
  # PowerShell's Process.Kill(bool) overload is unavailable on Windows
  # PowerShell builds used by qualification; killing captured IDs one by one
  # also risks PID reuse. taskkill's scoped tree operation is atomic for this
  # exact Hub root, then process/path assertions prove no orphan remained.
  $taskkill = Join-Path $env:WINDIR 'System32\taskkill.exe'
  $killer = Start-Process -FilePath $taskkill -ArgumentList @('/PID', [string]$hubPid, '/T', '/F') -Wait -PassThru -WindowStyle Hidden
  if ($killer.ExitCode -ne 0) {
    # taskkill reports a race-specific nonzero code when Hub exits between
    # tree capture & scoped termination. Accept only after exact captured IDs
    # have disappeared; any survivor remains a hard qualification failure.
    $remaining = @(Get-CimInstance Win32_Process -ErrorAction Stop | Where-Object { $ids -contains [int]$_.ProcessId })
    Require ($remaining.Count -eq 0) "could not terminate qualification process tree $($ids -join ','): taskkill exit $($killer.ExitCode)"
  }
  Assert-QualificationProcessTreeGone $ids $InstallRoot
  Require (Test-BlueprintPipeClosed (Get-BlueprintEndpoint)) 'Blueprint named pipe remained open after Hub shutdown'
  $script:HubProcess = $null
  $script:ActiveHubHealth = $null; $script:ActiveHubPort = $null
}

function Assert-UninstallResidue([string]$Root, $Doctor, [string]$DataMarker, [string]$DataHash) {
  $productRoot = Split-Path -Parent $Root
  $files = @(); if (Test-Path -LiteralPath $Root) { $files = @(Get-ChildItem -LiteralPath $Root -File -Recurse -Force -ErrorAction Stop) }
  Require ($files.Count -eq 0) "uninstall left files under install root: $($files.FullName -join ', ')"
  Require (-not (Test-Path -LiteralPath (Join-Path $productRoot 'versions'))) 'uninstall left versioned payloads'
  Require (-not (Test-Path -LiteralPath (Join-Path $productRoot 'uninstall.exe'))) 'uninstall left product uninstaller'
  Require (-not (Test-Path -LiteralPath (Join-Path $productRoot 'integration-journal.json'))) 'uninstall left integration journal'
  $processResidue = @(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith($productRoot, [System.StringComparison]::OrdinalIgnoreCase) })
  Require ($processResidue.Count -eq 0) "uninstall left an installed process: $($processResidue.Name -join ', ')"
  if ($Doctor -and $Doctor.receiptOwned) {
    foreach ($entry in @($Doctor.receiptOwned)) {
      $path = [string]$entry.path
      if (-not [string]::IsNullOrWhiteSpace($path)) { Require (-not (Test-Path -LiteralPath $path)) "uninstall left receipt-owned residue: $path" }
    }
  }
  foreach ($rootName in @('config', 'cache', 'log')) {
    $path = [string]$Doctor.roots.$rootName
    if ([string]$Doctor.roots.data -and $path.TrimEnd('\') -ieq ([string]$Doctor.roots.data).TrimEnd('\')) { continue }
    if (-not [string]::IsNullOrWhiteSpace($path) -and (Test-Path -LiteralPath $path)) {
      $left = @(Get-ChildItem -LiteralPath $path -Force -Recurse -ErrorAction Stop)
      Require ($left.Count -eq 0) "uninstall left runtime-owned $rootName residue: $path"
    }
  }
  Require (Test-Path -LiteralPath $DataMarker -PathType Leaf) 'uninstall removed durable data root state'
  Require ((Hash-File $DataMarker) -eq $DataHash) 'uninstall changed durable data root state'

  $shortcutRoots = @(
    (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'),
    (Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs'),
    (Join-Path $env:USERPROFILE 'Desktop'),
    (Join-Path $env:PUBLIC 'Desktop')
  )
  foreach ($shortcutRoot in $shortcutRoots) {
    if (-not (Test-Path -LiteralPath $shortcutRoot)) { continue }
    foreach ($shortcut in @(Get-ChildItem -LiteralPath $shortcutRoot -Filter '*.lnk' -File -Recurse -ErrorAction Stop)) {
      try {
        $shell = New-Object -ComObject WScript.Shell
        $target = $shell.CreateShortcut($shortcut.FullName).TargetPath
        Require (-not ([string]$target).StartsWith($productRoot, [System.StringComparison]::OrdinalIgnoreCase)) "uninstall left shortcut targeting install root: $($shortcut.FullName)"
      } catch { if ($_.Exception.Message -like 'uninstall left shortcut*') { throw }; throw "could not inspect installed shortcut: $($shortcut.FullName): $($_.Exception.Message)" }
    }
  }
  $registryRoots = @('HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*', 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*', 'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*')
  foreach ($registryRoot in $registryRoots) {
    foreach ($entry in @(Get-ItemProperty -Path $registryRoot -ErrorAction Stop)) {
      if ([string]$entry.InstallLocation -and ([string]$entry.InstallLocation).TrimEnd('\') -ieq $productRoot.TrimEnd('\')) { throw "uninstall left registry install entry: $($entry.PSPath)" }
      if ([string]$entry.DisplayName -match '(?i)^Membrane Hub$' -and [string]$entry.UninstallString -match '(?i)Membrane') { throw "uninstall left Membrane Hub registry entry: $($entry.PSPath)" }
    }
  }
  Require (-not (Test-Path -LiteralPath $Root)) "uninstall left current junction: $Root"
  return [ordered]@{
    installRootRemoved = $true
    processesRemoved = $true
    shortcutsRemoved = $true
    registryRemoved = $true
    durableStatePreserved = $true
  }
}

$installerPath = Resolve-File $Installer 'current installer'
$previousPath = if ([string]::IsNullOrWhiteSpace($PreviousInstaller)) { $null } else { Resolve-File $PreviousInstaller 'previous installer' }
$manifestPath = Resolve-File $ReleaseManifest 'release manifest'
$sbomPath = Resolve-File $Sbom 'SBOM'
$InstallRoot = [System.IO.Path]::GetFullPath($InstallRoot)
Require ($InstallRoot -match '(?i)\\Orthic Labs\\Membrane\\current$') "install root must be stable Membrane current path: $InstallRoot"
$script:InitialInstallRoot = $InstallRoot

$installerPublisher = Assert-SignedFile $installerPath 'current installer'
if ($previousPath) { [void](Assert-SignedFile $previousPath 'previous installer' $installerPublisher) }
Assert-BoundEvidence $installerPath $manifestPath $sbomPath
$releaseManifestValue = Read-JsonFile $manifestPath 'release manifest'
$currentVersion = Normalize-Version $releaseManifestValue.release.tag 'release manifest version'
$previousVersion = if ($previousPath) { Get-ArtifactVersion $previousPath 'previous installer' } else { $null }
if ($previousPath) {
  Require ($currentVersion -ne $previousVersion) "current & previous installers are the same version: $currentVersion"
  Require (([Version]$previousVersion.Substring(1)) -lt ([Version]$currentVersion.Substring(1))) "previous installer version $previousVersion is not older than current $currentVersion"
}
$currentGeneration = Normalize-Generation $releaseManifestValue.release.generation 'release manifest generation'
Require ((Get-ArtifactVersion $installerPath 'current installer') -eq $currentVersion) 'current installer version does not match release manifest'
$script:QualificationWorkspace = Join-Path ([System.IO.Path]::GetTempPath()) "membrane-windows-qualification-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $script:QualificationWorkspace -Force | Out-Null
$script:PreviousMembraneWorkspaceRoot = [Environment]::GetEnvironmentVariable('MEMBRANE_WORKSPACE_ROOT', 'Process')
$script:PreviousMembraneWorkspaceConfig = [Environment]::GetEnvironmentVariable('MEMBRANE_WORKSPACE_CONFIG', 'Process')
$script:WorkspaceConfigPath = Join-Path $script:QualificationWorkspace 'workspace.json'
Seed-WorkspaceV2Config $script:WorkspaceConfigPath $script:QualificationWorkspace
Initialize-QualificationRepository $script:QualificationWorkspace
# Hub owns enrollment. Bind every installed Hub phase to this exact configured
# workspace so Blueprint status/findings/recall address an enrolled actor,
# rather than an unrelated fresh root supplied only to request payloads.
$env:MEMBRANE_WORKSPACE_ROOT = $script:QualificationWorkspace
$env:MEMBRANE_WORKSPACE_CONFIG = $script:WorkspaceConfigPath

$doctor = $null
$dataMarker = $null
$dataHash = $null
try {
  $initialTarget = Invoke-Installer $installerPath
  # Silent installs never activate. Prove the installed layout passes the
  # product's own activation validation, with its output on record, before the
  # Hub is started.
  $script:ActivationDryRun = Invoke-ActivationDryRun $InstallRoot
  # Then the real activation a customer's install performs; it launches the
  # resident tray, which Start-AndVerifyHub adopts.
  $script:Activation = Invoke-Activation $InstallRoot
  $first = Start-AndVerifyHub 'initial install' $currentVersion $currentGeneration '' -Full
  $script:InitialEvidence = $first
  $script:WorkspaceConfigInitialSha256 = Assert-WorkspaceConfigMigrated $script:WorkspaceConfigPath 'initial startup'
  $script:WorkspaceMigrationEvidence = [ordered]@{
    contract = 'workspace-config-v2-to-v3-startup-migration-v1'
    path = $script:WorkspaceConfigPath
    schemaVersion = 3
    pythonExecutable = $false
    migratedOnInitialStartup = $true
    atomicTempFiles = $false
    initialSha256 = $script:WorkspaceConfigInitialSha256
  }
  $doctor = Get-DoctorPaths $first.Native.Membrane
  Require ($null -ne $doctor.roots.data) 'native doctor paths omitted durable data root'
  $dataRoot = [string]$doctor.roots.data
  New-Item -ItemType Directory -Path $dataRoot -Force | Out-Null
  $dataMarker = Join-Path $dataRoot "qualification-state-$([guid]::NewGuid().ToString('N')).json"
  $markerBytes = [Text.Encoding]::UTF8.GetBytes((ConvertTo-Json @{ schema = 'membrane.windows-qualification.state.v1'; marker = [guid]::NewGuid().ToString('N') } -Compress))
  [IO.File]::WriteAllBytes($dataMarker, $markerBytes)
  $dataHash = Hash-File $dataMarker
  Save-State $first.Native.Membrane
  Stop-QualificationHub
  # Hub owns runtime storage under configured workspace; installed runtime
  # payload contains executables/contracts, never live Cortex state.
  $nativeDatabase = Join-Path $script:QualificationWorkspace 'tools\.cache\memory\cortex-engine.db'
  Require (Test-Path -LiteralPath $nativeDatabase -PathType Leaf) 'Hub-owned native Cortex database is missing for Adapt qualification'
  $script:AdaptEvidence = Invoke-InstalledAdaptQualification $first.Native.Membrane $InstallRoot $nativeDatabase
  $script:BlueprintOneShot = Invoke-BlueprintOneShot $InstallRoot $script:QualificationWorkspace

  if ($previousPath) {
    $previousTarget = Invoke-Installer $previousPath
    Require ($previousTarget -ne $initialTarget) 'downgrade did not switch current junction target'
    $rollback = Start-AndVerifyPreviousHub $previousVersion
    Require ((Hash-File $dataMarker) -eq $dataHash) 'durable data changed during downgrade'
    $rollback | Add-Member -NotePropertyName durableState -NotePropertyValue 'preserved'
    Stop-QualificationHub
    $upgradeTarget = Invoke-Installer $installerPath
    Require ($upgradeTarget -ne $previousTarget) 'upgrade did not switch current junction target'
    $upgrade = Start-AndVerifyHub 'upgrade' $currentVersion $first.ReleaseGeneration '' -Full
    $transitionContract = 'signed-version-liveness-durable-state-v1'
  } else {
    $repairTarget = Invoke-Installer $installerPath
    # The installer lays versions\<version> down in place; a same-version repair
    # replaces that tree and keeps current pointed at it.
    Require ($repairTarget -eq $initialTarget) 'same-version repair did not reuse the version root'
    $upgrade = Start-AndVerifyHub 'same-version repair' $currentVersion $first.ReleaseGeneration '' -Full
    $rollback = [ordered]@{ status = 'not_applicable'; reason = 'first_stable_layout_release'; durableState = 'preserved' }
    $transitionContract = 'first-stable-layout-repair-v1'
  }
  $script:UpgradeEvidence = $upgrade
  [void](Assert-WorkspaceConfigMigrated $script:WorkspaceConfigPath 'upgrade startup' $script:WorkspaceConfigInitialSha256)
  $script:WorkspaceMigrationEvidence.upgradeIdempotent = $true
  $script:WorkspaceMigrationEvidence.upgradeSha256 = $script:WorkspaceConfigInitialSha256
  Assert-State $upgrade.Native.Membrane 'upgrade'
  Require ((Hash-File $dataMarker) -eq $dataHash) 'durable data changed during upgrade'
  $doctor = Get-DoctorPaths $upgrade.Native.Membrane
  Stop-QualificationHub

  $uninstaller = Resolve-File (Join-Path (Split-Path -Parent $InstallRoot) 'uninstall.exe') 'uninstaller'
  $uninstall = Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait -PassThru -WindowStyle Hidden
  Require ($uninstall.ExitCode -eq 0) "uninstaller failed with exit code $($uninstall.ExitCode)"
  Start-Sleep -Seconds 1
  $uninstallEvidence = Assert-UninstallResidue $InstallRoot $doctor $dataMarker $dataHash
  $installerSignature = Get-AuthenticodeSignature -LiteralPath $installerPath
  $previousArtifactEvidence = $null
  if ($previousPath) {
    $previousSignature = Get-AuthenticodeSignature -LiteralPath $previousPath
    $previousArtifactEvidence = [ordered]@{
      path = $previousPath
      version = $previousVersion
      sha256 = Hash-File $previousPath
      authenticode = [string]$previousSignature.Status
      signerSubject = [string]$previousSignature.SignerCertificate.Subject
      signerThumbprint = [string]$previousSignature.SignerCertificate.Thumbprint
      timestampSubject = [string]$previousSignature.TimeStamperCertificate.Subject
      timestampThumbprint = [string]$previousSignature.TimeStamperCertificate.Thumbprint
    }
  }
  $receipt = [ordered]@{
    schema = 'membrane.windows-installed-qualification.v1'
    generatedAt = [DateTime]::UtcNow.ToString('o')
    platform = 'windows-x86_64'
    profile = 'installed-local'
    artifact = [ordered]@{
      path = $installerPath
      version = $currentVersion
      sha256 = Hash-File $installerPath
      authenticode = [string]$installerSignature.Status
      signerSubject = [string]$installerSignature.SignerCertificate.Subject
      signerThumbprint = [string]$installerSignature.SignerCertificate.Thumbprint
      timestampSubject = [string]$installerSignature.TimeStamperCertificate.Subject
      timestampThumbprint = [string]$installerSignature.TimeStamperCertificate.Thumbprint
    }
    previousArtifact = $previousArtifactEvidence
    activationDryRun = $script:ActivationDryRun
    activation = $script:Activation
    inputs = [ordered]@{
      releaseManifest = [ordered]@{ path = $manifestPath; sha256 = Hash-File $manifestPath }
      sbom = [ordered]@{ path = $sbomPath; sha256 = Hash-File $sbomPath }
    }
    runtime = [ordered]@{
      inventory = [ordered]@{ path = $script:UpgradeEvidence.Native.RuntimeInventory; entries = $script:UpgradeEvidence.Native.RuntimeInventoryEvidence }
      blueprint = [ordered]@{ root = $script:UpgradeEvidence.Native.BlueprintRuntime; node = $script:UpgradeEvidence.Native.BlueprintNode; bounded = $true; hubOwned = $true }
      adapt = $script:AdaptEvidence
      workspaceConfigMigration = $script:WorkspaceMigrationEvidence
      hubHosted = $script:UpgradeEvidence.Blueprint
      hubOffOneShot = $script:BlueprintOneShot
    }
    environment = [ordered]@{
      path = $script:SafePath
      developmentCheckoutRequired = $false
      networkInterpreterFetch = $false
      forbiddenInterpreterDescendants = @('node', 'nodejs', 'python', 'pythonw', 'python3', 'py')
      allowedBlueprintInterpreter = [ordered]@{ root = $script:UpgradeEvidence.Native.BlueprintRuntime; executable = $script:UpgradeEvidence.Native.BlueprintNode; bounded = $true; hubOwned = $true }
    }
    initial = $script:InitialEvidence
    downgradeContract = $transitionContract
    downgrade = $rollback
    upgradeContract = 'full-native-upgrade-uninstall-v1'
    upgrade = $script:UpgradeEvidence
    uninstallEvidence = $uninstallEvidence
    lifecycle = [ordered]@{
      install = 'pass'
      startup = 'pass'
      hubHealth = 'pass'
      tray = 'pass'
      popup = 'pass'
      renderer = 'pass'
      mcp17 = 'pass'
      nativeHostCutover = 'pass'
      blueprintHubHosted = 'pass'
      blueprintHubOffOneShot = 'pass'
      downgrade = if ($previousPath) { 'pass' } else { 'not_applicable' }
      repair = if ($previousPath) { 'not_applicable' } else { 'pass' }
      upgrade = 'pass'
      stateContinuity = 'pass'
      uninstall = 'pass'
      residue = 'pass'
      nativeOnlyProcessTree = 'pass'
      runtimeInventory = 'pass'
      adapt = 'pass'
      workspaceConfigMigration = 'pass'
    }
  }
  Write-JsonAtomic $EvidencePath $receipt
  Write-Output "Windows installed qualification passed: $(Hash-File $installerPath)"
} finally {
  Stop-QualificationHub
  if ($dataMarker -and (Test-Path -LiteralPath $dataMarker)) { Remove-Item -LiteralPath $dataMarker -Force -ErrorAction SilentlyContinue }
  if ($script:QualificationWorkspace -and (Test-Path -LiteralPath $script:QualificationWorkspace)) { Remove-Item -LiteralPath $script:QualificationWorkspace -Recurse -Force -ErrorAction SilentlyContinue }
  if ($null -eq $script:PreviousMembraneWorkspaceRoot) { Remove-Item Env:MEMBRANE_WORKSPACE_ROOT -ErrorAction SilentlyContinue } else { $env:MEMBRANE_WORKSPACE_ROOT = $script:PreviousMembraneWorkspaceRoot }
  if ($null -eq $script:PreviousMembraneWorkspaceConfig) { Remove-Item Env:MEMBRANE_WORKSPACE_CONFIG -ErrorAction SilentlyContinue } else { $env:MEMBRANE_WORKSPACE_CONFIG = $script:PreviousMembraneWorkspaceConfig }
}
