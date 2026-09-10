[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string]$Installer,
  [string]$PreviousInstaller = '',
  [Parameter(Mandatory = $true)][string]$ReleaseManifest,
  [Parameter(Mandatory = $true)][string]$Sbom,
  [Parameter(Mandatory = $true)][string]$EvidencePath,
  [string]$InstallRoot = (Join-Path $env:LOCALAPPDATA 'Orthic Labs\Membrane\current'),
  [int]$TimeoutSeconds = 45,
  [int]$SteadyStateSamples = 4,
  # Default stays the production route: a signed installer is mandatory and
  # every result is a signed-release PASS. 'internal-unsigned' is an explicit,
  # separate route for this-machine internal candidates that are not
  # Authenticode-signed; it verifies exact artifact/installer/source/
  # installation hashes and the internal stable `current` layout, but every
  # result it can produce is labeled unsigned-functional. It never disables
  # production signing verification -- it simply never runs it, because an
  # internal-unsigned candidate is not expected to carry a valid signature.
  # This parameter never flips the signed-release branch's own behavior.
  [ValidateSet('signed-release', 'internal-unsigned')][string]$Profile = 'signed-release'
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
$script:TrayProcess = $null
$script:DaemonProcess = $null
$script:DashboardProcess = $null
$script:TrayPath = $null
$script:DaemonPath = $null
$script:DashboardPath = $null
$script:DashboardIdentity = $null
# Keep qualification isolated from checkout-local runtimes while retaining
# system Git, which Blueprint uses for repository fingerprinting.
$script:GitPath = (Get-Command git.exe -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty Source)
$script:GitBin = if ($script:GitPath) { Split-Path -Parent $script:GitPath } else { $null }
$script:SafePath = ((@("$env:WINDIR\System32", "$env:WINDIR", $script:GitBin) | Where-Object { $_ }) -join ';')
# Hosted runners have no GPU-backed OpenGL (run 33646458218: the tray died with
# "Could not locate glCreateShader symbol"). The tray honours this override and
# selects Slint's software renderer before creating any window; a desktop with
# a GPU never sets it. membrane activate inherits it into the tray it launches.
if ($env:GITHUB_ACTIONS -eq 'true' -and [string]::IsNullOrWhiteSpace($env:MEMBRANE_TRAY_RENDERER)) { $env:MEMBRANE_TRAY_RENDERER = 'software' }
$script:QualificationWorkspace = $null
$script:AdaptEvidence = $null
$script:State = $null
$script:InitialInstallRoot = $null
$script:InitialEvidence = $null
$script:NativeInitEvidence = $null
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
  Write-Host "[qualification] Invoke-Activation $(Get-Date -Format 'HH:mm:ss')"
  # Silent installation reconciles bindings only. Resident qualification
  # subsequently opens the real Hub holder before waiting for service health.
  # Output is evidence either way; a non-zero exit fails qualification with it.
  $membrane = Join-Path $Root 'membrane.exe'
  Require (Test-Path -LiteralPath $membrane -PathType Leaf) "installed membrane.exe is missing at $membrane"
  $evidenceRoot = $env:RIGHT_GIT_QUALIFICATION_EVIDENCE_ROOT
  if (-not $evidenceRoot) { $evidenceRoot = $EvidencePath }
  $activationTimeoutMs = 90000
  $processExitBoundMs = 5000
  $captureRoot = Join-Path ([IO.Path]::GetTempPath()) "membrane-activation-$([guid]::NewGuid().ToString('N'))"
  $stdoutPath = Join-Path $captureRoot 'stdout.log'
  $stderrPath = Join-Path $captureRoot 'stderr.log'
  $stdout = ''
  $stderr = ''
  $exit = $null
  $failure = $null
  $timedOut = $false
  $process = $null
  try {
    New-Item -ItemType Directory -Path $captureRoot -Force -ErrorAction Stop | Out-Null
    $commandProcessor = Join-Path $env:WINDIR 'System32\cmd.exe'
    Require (Test-Path -LiteralPath $commandProcessor -PathType Leaf) "system command processor is missing: $commandProcessor"
    # Let cmd.exe own direct file redirection so Start-Process creates no
    # asynchronous stream bookkeeping retained by resident descendants. Every
    # token is a fixed canonical path or integer & remains quoted inside one
    # hidden system command-processor invocation; no PATH lookup is involved.
    $commandLine = '""' + $membrane + '" activate --install-root "' + $Root +
      '" --bindings-only --timeout-ms ' + [string]$activationTimeoutMs + ' 1>"' + $stdoutPath +
      '" 2>"' + $stderrPath + '"'
    $process = Start-Process -FilePath $commandProcessor -ArgumentList @('/d', '/s', '/c', $commandLine) `
      -WorkingDirectory $Root -PassThru -WindowStyle Hidden
    if (-not $process.WaitForExit($activationTimeoutMs)) {
      $timedOut = $true
      try { & (Join-Path $env:WINDIR 'System32\taskkill.exe') /PID $process.Id /T /F 2>$null | Out-Null } catch { }
      if (-not $process.WaitForExit($processExitBoundMs)) {
        $failure = "membrane activate timed out after $activationTimeoutMs ms and did not exit within $processExitBoundMs ms"
      } else {
        $failure = "membrane activate timed out after $activationTimeoutMs ms"
      }
    } else {
      # Give the direct process handle a tight second bound to settle before
      # reading files; this never waits on the resident tray descendant.
      Require ($process.WaitForExit($processExitBoundMs)) 'membrane activate process did not settle within 5000 ms'
      $process.Refresh()
      Require $process.HasExited 'membrane activate process exit state is unavailable'
      $candidateExit = $process.ExitCode
      Require ($null -ne $candidateExit) 'membrane activate exit code is unavailable'
      $exit = [int]$candidateExit
    }
  } catch {
    if (-not $failure) { $failure = $_.Exception.Message }
  }
  $readCapture = {
    param([string]$Path)
    $stream = $null
    $reader = $null
    try {
      # Resident tray inherits these handles; permit shared reads while it
      # remains alive, without changing captured bytes or waiting on it.
      $share = [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete
      $stream = [IO.FileStream]::new($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, $share)
      $reader = [IO.StreamReader]::new($stream, [Text.UTF8Encoding]::new($false), $true)
      return $reader.ReadToEnd()
    } finally {
      if ($reader) { $reader.Dispose() }
      elseif ($stream) { $stream.Dispose() }
    }
  }
  try { if (Test-Path -LiteralPath $stdoutPath -PathType Leaf) { $stdout = & $readCapture $stdoutPath } } catch { $stdout = "<stdout unavailable: $($_.Exception.Message)>" }
  try { if (Test-Path -LiteralPath $stderrPath -PathType Leaf) { $stderr = & $readCapture $stderrPath } } catch { $stderr = "<stderr unavailable: $($_.Exception.Message)>" }
  $output = $stdout
  $combinedOutput = if ([string]::IsNullOrEmpty($stderr)) { $stdout } else { "$stdout`r`n$stderr" }
  try {
    New-Item -ItemType Directory -Path $evidenceRoot -Force -ErrorAction Stop | Out-Null
    $stdout | Set-Content -LiteralPath (Join-Path $evidenceRoot 'activation-stdout.log') -Encoding utf8
    $stderr | Set-Content -LiteralPath (Join-Path $evidenceRoot 'activation-stderr.log') -Encoding utf8
    $combinedOutput | Set-Content -LiteralPath (Join-Path $evidenceRoot 'activation.log') -Encoding utf8
    [ordered]@{
      schema = 'membrane.activation-process.v1'
      executable = $membrane
      exitCode = $exit
      timedOut = $timedOut
      stdout = (Join-Path $evidenceRoot 'activation-stdout.log')
      stderr = (Join-Path $evidenceRoot 'activation-stderr.log')
    } | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $evidenceRoot 'activation-process.json') -Encoding utf8
  } catch {}
  if ($failure) { [void](Save-RuntimeLogEvidence 'activation'); throw "$failure`nstdout:`n$stdout`nstderr:`n$stderr" }
  if ($exit -ne 0) { [void](Save-RuntimeLogEvidence 'activation') }
  Require ($exit -eq 0) "membrane activate exited $exit`n$combinedOutput"
  $parsed = $null
  try { $parsed = $output | ConvertFrom-Json } catch { throw "membrane activate did not emit JSON:`n$output" }
  Require ([string]$parsed.runtimeOrigin -eq 'installed') "activation reported runtimeOrigin $($parsed.runtimeOrigin)"
  try { if (Test-Path -LiteralPath $captureRoot) { Remove-Item -LiteralPath $captureRoot -Recurse -Force -ErrorAction SilentlyContinue } } catch {}
  return [ordered]@{ exitCode = $exit; runtimeOrigin = [string]$parsed.runtimeOrigin; service = $parsed.service; clients = @($parsed.clients | ForEach-Object { [ordered]@{ client = $_.client; before = $_.before; after = $_.after; changed = $_.changed } }) }
}

function Invoke-ActivationDryRun([string]$Root) {
  Write-Host "[qualification] Invoke-ActivationDryRun $(Get-Date -Format 'HH:mm:ss')"
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
  Write-Host "[qualification] Invoke-Installer $(Get-Date -Format 'HH:mm:ss')"
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

function Assert-BlueprintResident([string]$Root, [string]$WorkspaceRoot) {
  Write-Host "[qualification] Assert-BlueprintResident $(Get-Date -Format 'HH:mm:ss')"
  # Hub health/process identity proves resident watcher ownership. Blueprint's
  # supported production surface is membrane.exe's bounded native CLI; no
  # retired standalone endpoint protocol is part of qualification.
  $result = Invoke-BlueprintOneShot $Root $WorkspaceRoot
  Require ($result.status -eq 'pass') 'Hub-hosted Blueprint native status did not pass'
  Require ($script:ActiveHubHealth -and $script:ActiveHubHealth.subsystems -contains 'blueprint') 'Hub health omitted Blueprint watcher subsystem'
  $membrane = Join-Path $Root 'membrane.exe'
  $statusPayload = $result.Payload
  $graphGeneration = [string]$statusPayload.generationId
  if (-not $graphGeneration -and $statusPayload.result) { $graphGeneration = [string]$statusPayload.result.generationId }
  Require ($graphGeneration -match '^xxh128:[0-9a-f]{32}$') "native Blueprint status returned invalid graph generation: $graphGeneration"
  $freshnessState = if ($statusPayload.state) { [string]$statusPayload.state } elseif ($statusPayload.result) { [string]$statusPayload.result.state } else { '' }
  Require ($freshnessState -eq 'fresh') "native Blueprint watcher freshness is not current: $freshnessState"
  $watchMarker = "watcher_marker_$([guid]::NewGuid().ToString('N'))"
  $watchFile = Join-Path $WorkspaceRoot 'watcher-qualification.mjs'
  Write-NativeText $watchFile "export function $watchMarker() { return '$watchMarker'; }`n"
  $watchDeadline = (Get-Date).AddSeconds($TimeoutSeconds)
  $watchPayload = $null
  do {
    Start-Sleep -Milliseconds 500
    try {
      $candidate = Invoke-BlueprintOneShot $Root $WorkspaceRoot
      $candidatePayload = $candidate.Payload
      $candidateGeneration = [string]$candidatePayload.generationId
      $candidateState = [string]$candidatePayload.state
      if ($candidateGeneration -and $candidateGeneration -ne $graphGeneration -and $candidateState -eq 'fresh') { $watchPayload = $candidatePayload; break }
    } catch { }
  } while ((Get-Date) -lt $watchDeadline)
  Require ($null -ne $watchPayload) 'Blueprint watcher did not publish a newer fresh generation after isolated file mutation'
  $watchGeneration = [string]$watchPayload.generationId
  $search = Invoke-NativeProcess $membrane ("cli blueprint search --repo-root $(Quote-NativeArgument $WorkspaceRoot) --query " + (Quote-NativeArgument $watchMarker)) '' $Root
  Require ([string]$search.Stdout -match [regex]::Escape($watchMarker)) 'Blueprint watcher generation does not expose mutated symbol through native query'
  $recall = Invoke-NativeProcess $membrane ("cli blueprint recall --repo-root $(Quote-NativeArgument $WorkspaceRoot) --query " + (Quote-NativeArgument $watchMarker)) '' $Root
  $recallPayload = Read-NativeOutput $recall.Stdout 'native Blueprint recall'
  Require ([string]$recallPayload.generationId -eq $watchGeneration) 'native Blueprint recall returned a different graph generation'
  Require ([string]$recallPayload.state -eq 'complete' -and $null -ne $recallPayload.candidateSet -and $null -ne $recallPayload.resolution) 'native Blueprint recall omitted complete graph resolution'
  Require (@($recallPayload.nodes).Count -gt 0 -and [string]$recall.Stdout -match [regex]::Escape($watchMarker)) 'native Blueprint recall did not recover watched symbol'
  $mismatchGeneration = 'xxh128:' + [string]::new('0', 32)
  $mismatch = Invoke-NativeProcessAllowFailure $membrane ("cli blueprint findings.get --repo-root $(Quote-NativeArgument $WorkspaceRoot) --generation $mismatchGeneration") '' $Root
  $mismatchPayload = if (-not [string]::IsNullOrWhiteSpace($mismatch.Stdout)) { Read-NativeOutput $mismatch.Stdout 'native Blueprint generation mismatch' } else { $null }
  $mismatchCode = if ($mismatchPayload.error.code) { [string]$mismatchPayload.error.code } elseif ($mismatchPayload.result.error.code) { [string]$mismatchPayload.result.error.code } else { [string]$mismatch.Stderr }
  Require ($mismatch.ExitCode -ne 0 -and $mismatchCode -match '(?i)generation_mismatch|stale_blocked') "native Blueprint generation mismatch did not fail closed: exit=$($mismatch.ExitCode) response=$mismatchCode"
  return [ordered]@{ transport = 'membrane.exe cli blueprint'; status = 'pass'; enrollment = 'native'; graph = $freshnessState; generation = $graphGeneration; watcher = 'hub-health-and-freshness'; watcherMutation = 'pass'; watcherGeneration = $watchGeneration; watcherQuery = 'pass'; findings = 'generation_mismatch'; recall = 'success'; generationMismatch = 'pass'; hubOwned = $true }
}

function Invoke-BlueprintOneShot([string]$Root, [string]$WorkspaceRoot) {
  Write-Host "[qualification] Invoke-BlueprintOneShot $(Get-Date -Format 'HH:mm:ss')"
  $membrane = Join-Path $Root 'membrane.exe'
  Require (Test-Path -LiteralPath $membrane -PathType Leaf) 'installed native Blueprint host is missing'
  $arguments = "cli blueprint status --repo-root $(Quote-NativeArgument $WorkspaceRoot)"
  $result = Invoke-NativeProcess $membrane $arguments '' $Root
  $stdout = [string]$result.Stdout
  try { $payload = $stdout | ConvertFrom-Json } catch { throw "bounded native Blueprint one-shot returned invalid JSON: $($_.Exception.Message)`n$stdout" }
  Require ($null -ne $payload) 'bounded native Blueprint one-shot returned no status payload'
  $typedMissing = @('missing', 'not_configured', 'root_not_enrolled', 'graph_missing', 'missing_graph')
  $state = [string]$payload.state
  if (-not $state -and $payload.result) { $state = [string]$payload.result.state }
  $errorCode = [string]$payload.error.code
  if (-not $errorCode -and $payload.result) { $errorCode = [string]$payload.result.error.code }
  if ($state -notin @('fresh', 'degraded', 'running')) {
    Require ($typedMissing -contains $state -or $typedMissing -contains $errorCode) "bounded Blueprint one-shot returned untyped status: state=$state code=$errorCode"
  }
  $outputHash = [Security.Cryptography.SHA256]::Create()
  try { $outputSha256 = ([BitConverter]::ToString($outputHash.ComputeHash([Text.Encoding]::UTF8.GetBytes($stdout))) -replace '-', '').ToLowerInvariant() }
  finally { $outputHash.Dispose() }
  return [ordered]@{ status = 'pass'; executable = $membrane; arguments = $arguments; exitCode = 0; Payload = $payload; state = if ($state) { $state } else { $errorCode }; availability = if ($state -in @('fresh', 'degraded', 'running')) { 'available' } else { 'not_configured' }; outputSha256 = $outputSha256 }
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

function Get-InstalledProcessRows([string]$ExecutablePath) {
  $full = [IO.Path]::GetFullPath($ExecutablePath)
  return @(Get-CimInstance Win32_Process -ErrorAction Stop | Where-Object {
    $_.ExecutablePath -and ([IO.Path]::GetFullPath([string]$_.ExecutablePath) -ieq $full)
  })
}

function Capture-InstalledProcess([string]$ExecutablePath, [string]$Label, [int]$ExpectedParentId = -1) {
  $rows = @(Get-InstalledProcessRows $ExecutablePath)
  Require ($rows.Count -eq 1) "$Label is not exactly one installed process (observed $($rows.Count))"
  $row = $rows[0]
  if ($ExpectedParentId -ge 0) {
    Require ([int]$row.ParentProcessId -eq $ExpectedParentId) "$Label parent $($row.ParentProcessId) does not match expected $ExpectedParentId"
  }
  $process = Get-Process -Id ([int]$row.ProcessId) -ErrorAction Stop
  Require (-not $process.HasExited) "$Label exited during identity capture"
  $creation = [string]$row.CreationDate
  Require (-not [string]::IsNullOrWhiteSpace($creation)) "$Label creation identity is missing"
  return [pscustomobject]@{
    Process = $process
    ProcessId = [int]$row.ProcessId
    ParentProcessId = [int]$row.ParentProcessId
    ExecutablePath = [IO.Path]::GetFullPath([string]$row.ExecutablePath)
    Creation = $creation
  }
}

function Assert-InstalledProcessIdentity($Identity, [string]$Label) {
  $rows = @(Get-CimInstance Win32_Process -ErrorAction Stop | Where-Object { [int]$_.ProcessId -eq $Identity.ProcessId })
  Require ($rows.Count -eq 1) "$Label process identity disappeared"
  $row = $rows[0]
  Require ([IO.Path]::GetFullPath([string]$row.ExecutablePath) -ieq $Identity.ExecutablePath) "$Label executable path changed"
  Require ([int]$row.ParentProcessId -eq $Identity.ParentProcessId) "$Label parent process changed"
  Require ([string]$row.CreationDate -eq $Identity.Creation) "$Label creation identity changed"
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

function Assert-NativeSteadyState([int]$TrayProcessId, [int]$DaemonProcessId, [string]$TrayExecutable, [string]$DaemonExecutable, $DashboardIdentity) {
  Write-Host "[qualification] Assert-NativeSteadyState $(Get-Date -Format 'HH:mm:ss')"
  $latest = @()
  $trayFull = [IO.Path]::GetFullPath($TrayExecutable)
  $daemonFull = [IO.Path]::GetFullPath($DaemonExecutable)
  $webViewPath = $null
  for ($sample = 0; $sample -lt [Math]::Max(1, $SteadyStateSamples); $sample++) {
    $latest = @(Get-ProcessTree $TrayProcessId | Where-Object {
      $candidate = Get-Process -Id ([int]$_.ProcessId) -ErrorAction SilentlyContinue
      $null -ne $candidate -and -not $candidate.HasExited
    })
    $tray = @($latest | Where-Object { [int]$_.ProcessId -eq $TrayProcessId })
    Require ($tray.Count -eq 1) 'installed tray exited during native steady-state sampling'
    Require ([IO.Path]::GetFullPath([string]$tray[0].ExecutablePath) -ieq $trayFull) 'native steady-state tray path is not the installed tray'
    $daemon = @($latest | Where-Object { [int]$_.ProcessId -eq $DaemonProcessId })
    Require ($daemon.Count -eq 1) 'tray-owned installed daemon is missing during native steady-state sampling'
    Require ([IO.Path]::GetFullPath([string]$daemon[0].ExecutablePath) -ieq $daemonFull) 'native steady-state daemon path is not the installed daemon'
    Require ([int]$daemon[0].ParentProcessId -eq $TrayProcessId) 'installed daemon is not owned by the installed tray'
    $forbidden = @($latest | Where-Object { $_.Name -match '(?i)^(node|nodejs|python|pythonw|python3|pip|npm|npx)(\.exe)?$' })
    Require ($forbidden.Count -eq 0) "native-only steady-state contains retired interpreter process: $($forbidden.Name -join ', ')"
    # Dashboard is the active Hub holder during this sample. Its renderer
    # subtree is allowed only under the exact bootstrapped Hub identity.
    Require ($null -ne $DashboardIdentity) 'dashboard holder identity is missing during steady-state sampling'
    $dashboard = @($latest | Where-Object { [int]$_.ProcessId -eq [int]$DashboardIdentity.ProcessId })
    Require ($dashboard.Count -eq 1) 'dashboard holder exited during steady-state sampling'
    Require ([int]$dashboard[0].ParentProcessId -eq $TrayProcessId) 'dashboard holder has unexpected parent'
    Require ([IO.Path]::GetFullPath([string]$dashboard[0].ExecutablePath) -ieq $DashboardIdentity.ExecutablePath) 'dashboard holder executable changed'
    Require ([string]$dashboard[0].CreationDate -eq $DashboardIdentity.Creation) 'dashboard holder process identity changed'
    $allowed = @($TrayProcessId, $DaemonProcessId, [int]$DashboardIdentity.ProcessId)
    $pendingChildren = @([int]$DashboardIdentity.ProcessId)
    while ($pendingChildren.Count -gt 0) {
      $children = @($latest | Where-Object { [int]$_.ParentProcessId -in $pendingChildren })
      $pendingChildren = @()
      foreach ($child in $children) {
        Require ($child.Name -ieq 'msedgewebview2.exe') "dashboard contains unexpected helper: $($child.Name)"
        Require (-not [string]::IsNullOrWhiteSpace([string]$child.ExecutablePath)) 'WebView2 executable identity is missing'
        $childPath = [IO.Path]::GetFullPath([string]$child.ExecutablePath)
        if ($null -eq $webViewPath) {
          $signature = Get-AuthenticodeSignature -LiteralPath $childPath
          Require ($signature.Status -eq 'Valid' -and $signature.SignerCertificate.Subject -match 'O=Microsoft Corporation(?:,|$)') 'dashboard WebView2 helper is not Microsoft-signed'
          $webViewPath = $childPath
        }
        Require ($childPath -ieq $webViewPath) 'dashboard WebView2 helper path changed'
        $allowed += [int]$child.ProcessId
        $pendingChildren += [int]$child.ProcessId
      }
    }
    $unexpected = @($latest | Where-Object { [int]$_.ProcessId -notin $allowed })
    Require ($unexpected.Count -eq 0) "native-only steady-state contains unexpected resident process: $($unexpected.Name -join ', ')"
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
  Write-Host "[qualification] Assert-TrayAndPopup $(Get-Date -Format 'HH:mm:ss')"
  Add-NativeWindowProbe
  $shell = [MembraneQualification.NativeWindowProbe]::Find('Shell_TrayWnd', $null)
  Require ($shell -ne [IntPtr]::Zero) 'Windows notification area is unavailable'
  $element = Find-TrayElement
  $pattern = $null
  if ($null -ne $element) {
    # A discoverable notification element need not expose InvokePattern.
    $available = $element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)
    if (-not $available) { $pattern = $null }
  }
  if ($null -ne $pattern) {
    try {
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
  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  do {
    $windows = @(Get-WindowRows $ProcessId)
    $popover = @($windows | Where-Object { $_.Visible -and $_.Title -eq 'Membrane' })
    if ($popover.Count -eq 1) { break }
    Start-Sleep -Milliseconds 250
  } while ((Get-Date) -lt $deadline)
  Require ($popover.Count -eq 1) 'exactly one Membrane tray popover did not become visible after tray activation'
  Require (-not [string]::IsNullOrWhiteSpace([string]$popover[0].ClassName)) 'Membrane tray popover native window class is missing'
}

function Assert-Dashboard([string]$HubExecutable, [int]$TrayProcessId) {
  Write-Host "[qualification] Assert-Dashboard $(Get-Date -Format 'HH:mm:ss')"
  Require ($script:TrayPath -and (Test-Path -LiteralPath $script:TrayPath -PathType Leaf)) 'installed tray path is unavailable for dashboard bootstrap'
  $before = @(Get-InstalledProcessRows $HubExecutable | ForEach-Object { [int]$_.ProcessId })
  $signal = Start-Process -FilePath $script:TrayPath -ArgumentList @('--open-dashboard') -WorkingDirectory $InstallRoot -PassThru -WindowStyle Hidden
  try {
    Require ($signal.WaitForExit(10000)) 'tray dashboard signal did not exit through single-instance cutover'
    Require ($signal.ExitCode -eq 0) "tray dashboard signal failed with exit code $($signal.ExitCode)"
  } finally {
    if ($signal -and -not $signal.HasExited) { Stop-Process -Id $signal.Id -Force -ErrorAction SilentlyContinue }
  }
  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  do {
    $hubRows = @(Get-InstalledProcessRows $HubExecutable | Where-Object {
      ([int]$_.ProcessId -notin $before) -and ([int]$_.ParentProcessId -eq $TrayProcessId)
    })
    if ($hubRows.Count -eq 1) { break }
    Start-Sleep -Milliseconds 250
  } while ((Get-Date) -lt $deadline)
  Require ($hubRows.Count -eq 1) "tray did not bootstrap exactly one installed Hub child (observed $($hubRows.Count))"
  $script:DashboardProcess = Get-Process -Id ([int]$hubRows[0].ProcessId) -ErrorAction Stop
  $script:DashboardPath = [IO.Path]::GetFullPath([string]$hubRows[0].ExecutablePath)
  Require (-not $script:DashboardProcess.HasExited) 'bootstrapped Hub exited before dashboard assertion'
  $script:DashboardIdentity = [pscustomobject]@{
    ProcessId = [int]$hubRows[0].ProcessId
    ParentProcessId = [int]$hubRows[0].ParentProcessId
    ExecutablePath = $script:DashboardPath
    Creation = [string]$hubRows[0].CreationDate
  }
  Require (-not [string]::IsNullOrWhiteSpace($script:DashboardIdentity.Creation)) 'bootstrapped Hub creation identity is missing'
  Start-Sleep -Milliseconds 500
  $windows = @(Get-WindowRows $script:DashboardProcess.Id)
  Require (@($windows | Where-Object { $_.Visible -and $_.Title -match '(?i)Membrane Hub' }).Count -gt 0) 'Hub dashboard did not become visible through tray bootstrap'
  return [ordered]@{ processId = $script:DashboardProcess.Id; parentProcessId = $TrayProcessId; executablePath = $script:DashboardPath }
}

function Assert-RendererWindows([int]$ProcessId) {
  Write-Host "[qualification] Assert-RendererWindows $(Get-Date -Format 'HH:mm:ss')"
  $windows = @(Get-WindowRows $ProcessId)
  $hubWindows = @($windows | Where-Object { $_.Visible -and $_.Title -eq 'Membrane Hub' })
  Require ($hubWindows.Count -eq 1) 'installed Hub did not create exactly one visible on-demand dashboard renderer window'
  Require (-not [string]::IsNullOrWhiteSpace([string]$hubWindows[0].ClassName)) 'installed dashboard renderer window class is missing'
  return @($hubWindows | ForEach-Object {
    [ordered]@{ title = [string]$_.Title; className = [string]$_.ClassName; visible = [bool]$_.Visible }
  })
}

function Assert-NativeHostCutover([string]$Root, [string]$HubExecutable) {
  Write-Host "[qualification] Assert-NativeHostCutover $(Get-Date -Format 'HH:mm:ss')"
  $hubPublisher = ''
  if ($Profile -eq 'signed-release') {
    $hubPublisher = Assert-SignedFile $HubExecutable 'installed Hub'
  }
  $forbidden = @(Get-ChildItem -LiteralPath $Root -File -Recurse -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -match '(?i)^(node|nodejs|python|pythonw|python3|pip|npm|npx)(\.exe)?$' })
  Require ($forbidden.Count -eq 0) "installed package carries a retired interpreter runtime: $($forbidden.FullName -join ', ')"
  $inventoryPath = Join-Path $Root 'runtime\runtime-inventory.json'
  Require (Test-Path -LiteralPath $inventoryPath -PathType Leaf) 'installed runtime inventory is missing'
  $inventory = Read-JsonFile $inventoryPath 'installed runtime inventory'
  Require ($inventory.schemaVersion -eq 3 -and $inventory.target -eq 'x86_64-pc-windows-msvc') 'installed runtime inventory identity is invalid'
  Require (@($inventory.composition | Where-Object { $_ -eq 'blueprint' }).Count -eq 1) 'installed runtime composition does not contain exactly one Blueprint axis'
  $blueprintContract = @($inventory.entries | Where-Object { $_.component -eq 'blueprint-contract' })
  Require ($blueprintContract.Count -eq 1 `
    -and [string]$blueprintContract[0].axis -eq 'blueprint' `
    -and [string]$blueprintContract[0].delivery -eq 'resource' `
    -and -not [string]::IsNullOrWhiteSpace([string]$blueprintContract[0].installerPath) `
    -and [string]$blueprintContract[0].sha256 -match '^[0-9a-f]{64}$') 'installed Blueprint contract inventory entry is invalid'
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
  if ($Profile -eq 'signed-release') {
    [void](Assert-SignedFile $membrane 'installed membrane native host' $hubPublisher)
    [void](Assert-SignedFile $cortex 'installed cortex native host' $hubPublisher)
    [void](Assert-SignedFile $tray 'installed membrane tray sidecar' $hubPublisher)
    [void](Assert-SignedFile $daemon 'installed membrane daemon sidecar' $hubPublisher)
  }
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
  return [pscustomobject]@{ Hub = $HubExecutable; Membrane = $membrane; Cortex = $cortex; Tray = $tray; Daemon = $daemon; RuntimeInventory = $inventoryPath; RuntimeInventoryEvidence = $inventoryEvidence; Publisher = $hubPublisher }
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

function Invoke-NativeProcessAllowFailure([string]$Executable, [string]$Arguments, [string]$InputText = '', [string]$WorkingDirectory = $InstallRoot) {
  $start = [Diagnostics.ProcessStartInfo]::new(); $start.FileName = $Executable; $start.Arguments = $Arguments; $start.WorkingDirectory = $WorkingDirectory
  $start.UseShellExecute = $false; $start.CreateNoWindow = $true; $start.RedirectStandardInput = $true; $start.RedirectStandardOutput = $true; $start.RedirectStandardError = $true
  $start.EnvironmentVariables['PATH'] = $script:SafePath
  $process = [Diagnostics.Process]::new(); $process.StartInfo = $start
  Require ($process.Start()) "could not start native process: $Executable"
  if (-not [string]::IsNullOrEmpty($InputText)) { $process.StandardInput.Write($InputText) }
  $process.StandardInput.Close(); $stdout = $process.StandardOutput.ReadToEnd(); $stderr = $process.StandardError.ReadToEnd()
  Require ($process.WaitForExit($TimeoutSeconds * 1000)) "native process timed out: $Executable $Arguments"
  return [pscustomobject]@{ Stdout = $stdout; Stderr = $stderr; ExitCode = [int]$process.ExitCode; Executable = $Executable; Arguments = $Arguments; WorkingDirectory = $WorkingDirectory }
}

function Invoke-InstalledHealth([string]$Executable, [int]$TimeoutSeconds, [string]$Phase) {
  $result = Invoke-NativeProcessAllowFailure $Executable "cli health --timeout-seconds $TimeoutSeconds" '' $InstallRoot
  Require ($result.ExitCode -eq 0) "installed authenticated health failed during ${Phase}: $($result.Stderr)"
  try { return ($result.Stdout | ConvertFrom-Json) } catch { throw "installed authenticated health returned invalid JSON during $Phase" }
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
  Write-Host "[qualification] Invoke-InstalledAdaptQualification $(Get-Date -Format 'HH:mm:ss')"
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
  Write-Host "[qualification] Invoke-NativeMcp $(Get-Date -Format 'HH:mm:ss')"
  $caller = [ordered]@{
    root = $script:QualificationWorkspace
    repositoryId = 'windows-qualification'
    scopeId = 'windows-qualification'
  }
  $allTools = @(
    'membrane_context', 'membrane_source_read', 'membrane_blueprint',
    'membrane_knowledge_propose', 'membrane_checkpoint_save', 'membrane_checkpoint_load',
    'membrane_working_context', 'membrane_temporal_fact', 'membrane_scratchpad',
    'membrane_feedback', 'membrane_memory', 'membrane_memory_read', 'membrane_ledger',
    'membrane_diagnostic_workspace', 'membrane_diagnostic_mutation',
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
    membrane_memory = [ordered]@{ repository = 'windows-qualification'; caller = $caller; operation = 'recall'; query = 'qualification'; limit = 1 }
    membrane_memory_read = [ordered]@{ repository = 'windows-qualification'; caller = $caller; id = 'qualification-missing-memory' }
    membrane_ledger = [ordered]@{ repository = 'windows-qualification'; caller = $caller; operation = 'status' }
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
  Require ($tools.Count -eq $allTools.Count) "MCP tools/list returned $($tools.Count) tools; expected $($allTools.Count)"
  $actualNames = (@($tools.name) | Sort-Object) -join ','
  $expectedNames = ($allTools | Sort-Object) -join ','
  Require ($actualNames -eq $expectedNames) 'MCP tools/list does not match the exact qualification registry'
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

function Get-InstalledResidentAuthPaths {
  Require ($null -ne $script:ActiveHubHealth) 'Hub MCP authentication requires health identity'
  $stableCurrent = [string]$script:ActiveHubHealth.stableInstallRoot
  Require (-not [string]::IsNullOrWhiteSpace($stableCurrent)) 'Hub health omitted stable install root'
  $stableCurrent = Normalize-ComparablePath $stableCurrent
  $activeCurrent = Normalize-ComparablePath $InstallRoot
  Require ($stableCurrent -ieq $activeCurrent) 'Hub health stable install root is not the active installed current'
  $stateRoot = Join-Path (Split-Path -Parent $stableCurrent) 'state'
  [pscustomobject]@{
    TokenPath = Join-Path $stateRoot 'tools\.cache\memory\api-token'
    IdentityPath = Join-Path $stateRoot 'tools\.cache\memory\installation.json'
  }
}

function Invoke-HubMcpCall([string]$Name, $Payload) {
  Require ($null -ne $script:ActiveHubHealth -and $script:ActiveHubPort) "Hub MCP call $Name requires an active Hub"
  $authPaths = Get-InstalledResidentAuthPaths
  $tokenPath = $authPaths.TokenPath
  $identityPath = $authPaths.IdentityPath
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
  Write-Host "[qualification] Get-DoctorPaths $(Get-Date -Format 'HH:mm:ss')"
  $result = Invoke-NativeProcess $MembraneExecutable 'cli doctor paths'
  try { return ($result.Stdout | ConvertFrom-Json) }
  catch { throw "native doctor paths emitted invalid JSON: $($result.Stdout)" }
}

function Save-State([string]$MembraneExecutable) {
  Write-Host "[qualification] Save-State $(Get-Date -Format 'HH:mm:ss')"
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
  Write-Host "[qualification] Assert-State $(Get-Date -Format 'HH:mm:ss')"
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
  Write-Host "[qualification] Assert-WorkspaceConfigMigrated $(Get-Date -Format 'HH:mm:ss')"
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
  Write-Host "[qualification] Start-AndVerifyHub $(Get-Date -Format 'HH:mm:ss')"
  $hub = Get-InstalledExecutable $InstallRoot
  $installedVersion = Get-ArtifactVersion $hub "installed Hub during $Phase"
  Require ($installedVersion -eq $ExpectedVersion) "installed Hub version $installedVersion does not match expected $ExpectedVersion during $Phase"
  $native = Assert-NativeHostCutover $InstallRoot $hub
  $script:TrayPath = $native.Tray
  $script:DaemonPath = $native.Daemon
  $trayRows = @(Get-InstalledProcessRows $script:TrayPath)
  if ($trayRows.Count -eq 0) {
    # Tray starts transport within its bounded pre-holder grace. The real
    # dashboard below acquires Hub ownership before semantic catch-up finishes.
    $script:TrayProcess = Start-Process -FilePath $script:TrayPath -ArgumentList @('--activate') -WorkingDirectory $InstallRoot -PassThru -WindowStyle Hidden
    $trayDeadline = (Get-Date).AddSeconds(10)
    do {
      $trayRows = @(Get-InstalledProcessRows $script:TrayPath)
      if ($trayRows.Count -eq 1) { break }
      Start-Sleep -Milliseconds 100
    } while ((Get-Date) -lt $trayDeadline)
  }
  Require ($trayRows.Count -eq 1) "installed tray is not exactly one resident process during $Phase (observed $($trayRows.Count))"
  $script:TrayProcess = Get-Process -Id ([int]$trayRows[0].ProcessId) -ErrorAction Stop
  Require (-not $script:TrayProcess.HasExited) "installed tray exited during $Phase"
  $trayIdentity = Capture-InstalledProcess $script:TrayPath "installed tray during $Phase"
  [void](Assert-Dashboard $hub $trayIdentity.ProcessId)
  $daemonIdentity = $null
  $port = Get-RuntimePort $InstallRoot
  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  do {
    $daemonRows = @(Get-InstalledProcessRows $script:DaemonPath | Where-Object { [int]$_.ParentProcessId -eq $trayIdentity.ProcessId })
    if ($daemonRows.Count -eq 1) {
      try { $daemonIdentity = Capture-InstalledProcess $script:DaemonPath "tray-owned daemon during $Phase" $trayIdentity.ProcessId } catch { $daemonIdentity = $null }
    }
    try {
      $health = Invoke-InstalledHealth (Join-Path $InstallRoot 'membrane.exe') 3 $Phase
      if ($health.ok -eq $true -and $null -ne $daemonIdentity) { break }
    } catch { }
    Start-Sleep -Milliseconds 500
  } while ((Get-Date) -lt $deadline)
  Require ($null -ne $daemonIdentity) "tray-owned installed daemon did not become resident during $Phase"
  $script:DaemonProcess = $daemonIdentity.Process
  try { $health = Invoke-InstalledHealth (Join-Path $InstallRoot 'membrane.exe') 5 $Phase } catch {
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
  $trayIdentity | Add-Member -NotePropertyName Generation -NotePropertyValue $generation
  $daemonIdentity | Add-Member -NotePropertyName Generation -NotePropertyValue $generation
  Require ([int]$health.protocolVersion -eq 1 -and [int]$health.schemaVersion -eq 1) "Hub protocol/schema handshake is invalid during $Phase"
  Require ($health.nativeOnly -eq $true) "Hub did not attest nativeOnly during $Phase"
  $script:ActiveHubHealth = $health
  $script:ActiveHubPort = $port
  $subsystems = @($health.subsystems | Sort-Object)
  Require (($subsystems -join ',') -eq 'adapt,blueprint,cortex,ledger,pull,push') "Hub six-subsystem health is invalid during $Phase"
  Require (@($health.capabilities) -contains 'memory') "Hub health omitted memory capability during $Phase"
  Assert-InstalledProcessIdentity $trayIdentity "installed tray during $Phase"
  Assert-InstalledProcessIdentity $daemonIdentity "tray-owned daemon during $Phase"
  $tree = @(Assert-NativeSteadyState $trayIdentity.ProcessId $daemonIdentity.ProcessId $native.Tray $native.Daemon $script:DashboardIdentity)
  $assets = @()
  $mcp = $null
  $blueprint = $null
  if ($Full) {
    Assert-TrayAndPopup $trayIdentity.ProcessId
    $assets = @(Assert-RendererWindows $script:DashboardProcess.Id)
    $mcp = Invoke-NativeMcp $native.Membrane -ExerciseAll
    $blueprint = Assert-BlueprintResident $InstallRoot $script:QualificationWorkspace
  }
  $script:HubProcess = $script:DashboardProcess
  return [pscustomobject]@{
    Hub = $hub
    Tray = $native.Tray
    Daemon = $native.Daemon
    TrayProcess = $trayIdentity
    DaemonProcess = $daemonIdentity
    DashboardProcess = $script:DashboardProcess
    DashboardIdentity = $script:DashboardIdentity
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
  Write-Host "[qualification] Start-AndVerifyPreviousHub $(Get-Date -Format 'HH:mm:ss')"
  $verified = Start-AndVerifyHub 'downgrade' $ExpectedVersion '' $currentGeneration
  return [pscustomobject]@{
    hub = $verified.Hub
    version = $verified.Version
    processTree = @($verified.ProcessTree)
    installedContent = @(Get-InstalledContentEvidence $InstallRoot)
  }
}

function Assert-QualificationProcessTreeGone([int[]]$ProcessIds) {
  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  do {
    $live = @(Get-CimInstance Win32_Process -ErrorAction Stop | Where-Object {
      $ProcessIds -contains [int]$_.ProcessId
    })
    if ($live.Count -eq 0) { return }
    Start-Sleep -Milliseconds 250
  } while ((Get-Date) -lt $deadline)
  throw "Hub process descendants remain after shutdown: $($live.Name -join ', ')"
}

function Stop-QualificationHub {
  if ($null -eq $script:TrayProcess -and $null -eq $script:DashboardProcess) { $script:ActiveHubHealth = $null; $script:ActiveHubPort = $null; return }
  $trayPid = if ($script:TrayProcess) { $script:TrayProcess.Id } else { -1 }
  $daemonPid = if ($script:DaemonProcess) { $script:DaemonProcess.Id } else { -1 }
  $hubPid = if ($script:DashboardProcess) { $script:DashboardProcess.Id } else { -1 }
  $ids = @($trayPid, $daemonPid, $hubPid) | Where-Object { $_ -gt 0 }
  # Snapshot owned descendants before parents exit; unrelated installed clients
  # are not members of this qualification's presentation/controller tree.
  $ids = @($ids; foreach ($ownerId in $ids) {
    Get-ProcessTree $ownerId | ForEach-Object { [int]$_.ProcessId }
  }) | Sort-Object -Unique
  # Closing presentation first releases authenticated Hub holder lease; daemon
  # then performs final-holder drain under tray supervision.
  if ($script:DashboardProcess) {
    $liveHub = Get-Process -Id $hubPid -ErrorAction SilentlyContinue
    if ($liveHub -and -not $liveHub.HasExited) {
      [void]$liveHub.CloseMainWindow()
      if (-not $liveHub.WaitForExit([Math]::Min(10000, $TimeoutSeconds * 1000))) {
        # Failure-only cleanup for an unresponsive presentation process.
        $taskkill = Join-Path $env:WINDIR 'System32\taskkill.exe'
        [void](Start-Process -FilePath $taskkill -ArgumentList @('/PID', [string]$hubPid, '/T', '/F') -Wait -PassThru -WindowStyle Hidden)
      }
    }
  }
  $drainDeadline = (Get-Date).AddSeconds($TimeoutSeconds)
  $daemonExited = $false
  do {
    $daemonLive = if ($daemonPid -gt 0) { Get-Process -Id $daemonPid -ErrorAction SilentlyContinue } else { $null }
    if ($null -eq $daemonLive -or $daemonLive.HasExited) { $daemonExited = $true; break }
    Start-Sleep -Milliseconds 250
  } while ((Get-Date) -lt $drainDeadline)
  Require $daemonExited 'final-holder daemon drain did not complete within qualification timeout'
  if ($script:TrayProcess) {
    $liveTray = Get-Process -Id $trayPid -ErrorAction SilentlyContinue
    if ($liveTray -and -not $liveTray.HasExited) {
      [void]$liveTray.CloseMainWindow()
      [void]$liveTray.WaitForExit([Math]::Min(5000, $TimeoutSeconds * 1000))
    }
  }
  $remainingTray = if ($trayPid -gt 0) { Get-Process -Id $trayPid -ErrorAction SilentlyContinue } else { $null }
  if ($remainingTray -and -not $remainingTray.HasExited) {
    # Failure-only exact cleanup after typed daemon drain; never kill runtime
    # before holder release has been observed.
    $taskkill = Join-Path $env:WINDIR 'System32\taskkill.exe'
    [void](Start-Process -FilePath $taskkill -ArgumentList @('/PID', [string]$trayPid, '/T', '/F') -Wait -PassThru -WindowStyle Hidden)
  }
  Assert-QualificationProcessTreeGone $ids
  $script:TrayProcess = $null
  $script:DaemonProcess = $null
  $script:DashboardProcess = $null
  $script:DashboardIdentity = $null
  $script:HubProcess = $null
  $script:ActiveHubHealth = $null; $script:ActiveHubPort = $null
}

function Assert-UninstallResidue([string]$Root, $Doctor, [string]$DataMarker, [string]$DataHash) {
  Write-Host "[qualification] Assert-UninstallResidue $(Get-Date -Format 'HH:mm:ss')"
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

$installerPublisher = $null
if ($Profile -eq 'signed-release') {
  # Production route: unconditional, never bypassed by any parameter.
  $installerPublisher = Assert-SignedFile $installerPath 'current installer'
  if ($previousPath) { [void](Assert-SignedFile $previousPath 'previous installer' $installerPublisher) }
} else {
  # internal-unsigned route: signature verification is not run (the candidate
  # is not expected to carry one). Exact hash binding below still applies in
  # full, and this branch can never mark the run as a signed-release PASS.
  Write-Host "[qualification] profile=internal-unsigned: Authenticode verification skipped by design; hash-bound identity checks below still apply"
}
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
$script:PreviousMembraneProjectRegistry = [Environment]::GetEnvironmentVariable('MEMBRANE_PROJECT_REGISTRY', 'Process')
$script:WorkspaceConfigPath = Join-Path $script:QualificationWorkspace 'workspace.json'
Seed-WorkspaceV2Config $script:WorkspaceConfigPath $script:QualificationWorkspace
Initialize-QualificationRepository $script:QualificationWorkspace
# Isolated canonical registry input exercises resident watching; native
# enrollment behavior requires separate qualification.
$registryBindings = @{}
$registryBindings[$script:QualificationWorkspace] = [ordered]@{
  repository_id = 'windows-qualification'
  scope_id = 'windows-qualification'
  scope_descriptor = [ordered]@{ kind = 'filesystem'; path = 'windows-qualification' }
  grant_policy = [ordered]@{ level = 'read-only' }
}
$registryPath = Join-Path $script:QualificationWorkspace 'project-registry.json'
Write-NativeText $registryPath ([ordered]@{ schema_version = 2; bindings = $registryBindings } | ConvertTo-Json -Depth 10)
$env:MEMBRANE_PROJECT_REGISTRY = $registryPath
$env:MEMBRANE_WORKSPACE_ROOT = $script:QualificationWorkspace
$env:MEMBRANE_WORKSPACE_CONFIG = $script:WorkspaceConfigPath

$doctor = $null
$dataMarker = $null
$dataHash = $null
try {
  $initialTarget = Invoke-Installer $installerPath
  # Prove the installed layout passes the
  # product's own activation validation, with its output on record, before the
  # Hub is started.
  $script:ActivationDryRun = Invoke-ActivationDryRun $InstallRoot
  # Reconcile bindings through silent-install activation. Start-AndVerifyHub
  # then opens the real Hub holder before waiting for resident service health.
  $script:Activation = Invoke-Activation $InstallRoot
  $first = Start-AndVerifyHub 'initial install' $currentVersion $currentGeneration '' -Full
  $script:InitialEvidence = $first
  $initArgs = 'init ' + (Quote-NativeArgument $script:QualificationWorkspace) + ' --repository windows-qualification --scope windows-qualification'
  $initResult = Invoke-NativeProcess $first.Native.Membrane $initArgs '' $InstallRoot
  $initReceipt = Read-NativeOutput $initResult.Stdout 'installed native init'
  Require ($initReceipt.action -eq 'enroll') 'native init omitted enrollment receipt'
  Require ((Normalize-ComparablePath ([string]$initReceipt.root)) -eq (Normalize-ComparablePath $script:QualificationWorkspace)) 'native init enrolled wrong root'
  $initRegistry = Read-JsonFile $registryPath 'native init registry'
  $matchingBindings = @($initRegistry.bindings.PSObject.Properties | Where-Object {
    (Normalize-ComparablePath $_.Name) -eq (Normalize-ComparablePath $script:QualificationWorkspace)
  })
  Require ($matchingBindings.Count -eq 1) 'native init must persist exactly one canonical root binding'
  $initBinding = $matchingBindings[0].Value
  Require ($initBinding.repository_id -eq 'windows-qualification' -and $initBinding.scope_id -eq 'windows-qualification') 'native init persisted wrong identity'
  $installedBinding = $initBinding.provider_config.installation_binding
  Require ($null -ne $installedBinding) 'native init omitted installed binding'
  Require ((Normalize-ComparablePath ([string]$installedBinding.stableCurrent)) -eq (Normalize-ComparablePath $InstallRoot)) 'native init selected wrong installed current'
  Require (-not [string]::IsNullOrWhiteSpace([string]$installedBinding.installationId)) 'native init omitted installation identity'
  Require (-not [string]::IsNullOrWhiteSpace([string]$installedBinding.serviceInstanceId)) 'native init omitted service identity'
  Require (($installedBinding | ConvertTo-Json -Depth 20 -Compress) -eq ($initReceipt.installation_binding | ConvertTo-Json -Depth 20 -Compress)) 'native init receipt differs from persisted installed binding'
  $script:NativeInitEvidence = [ordered]@{ receipt = $initReceipt; registry = $registryPath; binding = $initBinding }
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
  # Authenticode status is captured for the record on both routes; only the
  # signed-release route (above) ever REQUIREs it to be Valid. Recording it
  # here never re-derives or asserts a signed-release PASS for the
  # internal-unsigned route.
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
  $certification = if ($Profile -eq 'signed-release') { 'signed-release' } else { 'unsigned-functional' }
  $installedContentEvidence = Get-InstalledContentEvidence $InstallRoot
  # Preserve concrete lifecycle actions performed by this runner. These records
  # are observations from the native qualification path, never copied status
  # claims; consumers must still require every scenario they need.
  $lifecycleObservations = @(
    [ordered]@{ id = 'holder-exit'; lane = 'LC-01'; action = 'Stop-QualificationHub'; observed = ($null -ne $script:UpgradeEvidence -and $null -ne $script:UpgradeEvidence.DaemonProcess); before = @($script:UpgradeEvidence.ProcessTree); after = @(); processIdentity = $script:UpgradeEvidence.DaemonProcess }
    [ordered]@{ id = 'survivor-continuity'; lane = 'LC-01'; action = 'durable-state-hash-compare'; observed = ($dataHash -and (Hash-File $dataMarker) -eq $dataHash); beforeHash = $dataHash; afterHash = if (Test-Path -LiteralPath $dataMarker) { Hash-File $dataMarker } else { $null } }
    [ordered]@{ id = 'source-mutation-watcher'; lane = 'LC-01'; action = 'Assert-BlueprintResident'; observed = ($null -ne $script:UpgradeEvidence.Blueprint -and $script:UpgradeEvidence.Blueprint.watcherMutation -eq 'pass'); evidence = $script:UpgradeEvidence.Blueprint }
    [ordered]@{ id = 'native-only-process-tree'; lane = 'NCL-05'; action = 'Assert-NativeSteadyState'; observed = ($script:UpgradeEvidence.Native -and $script:UpgradeEvidence.ProcessTree); before = @($script:UpgradeEvidence.ProcessTree); artifact = $installedContentEvidence }
  )
  $receipt = [ordered]@{
    schema = 'membrane.windows-installed-qualification.v1'
    generatedAt = [DateTime]::UtcNow.ToString('o')
    platform = 'windows-x86_64'
    profile = if ($Profile -eq 'signed-release') { 'installed-local' } else { 'internal-unsigned' }
    # Explicit, separate label from `profile`/`lifecycle` so no consumer can
    # mistake an internal-unsigned run's result for a signed-release PASS.
    certification = $certification
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
    # Exact installation content hashes for the internal stable `current`
    # layout under test; present on both routes, required reading for the
    # internal-unsigned route's own hash-bound identity claim.
    installedCurrent = [ordered]@{
      root = $InstallRoot
      files = $installedContentEvidence
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
      blueprint = [ordered]@{ host = $script:UpgradeEvidence.Native.Membrane; resident = $false; oneShot = $script:BlueprintOneShot }
      adapt = $script:AdaptEvidence
      workspaceConfigMigration = $script:WorkspaceMigrationEvidence
      hubHosted = $script:UpgradeEvidence.Blueprint
      hubOffOneShot = $script:BlueprintOneShot
      lifecycleObservations = $lifecycleObservations
    }
    environment = [ordered]@{
      path = $script:SafePath
      developmentCheckoutRequired = $false
      networkInterpreterFetch = $false
      forbiddenInterpreterDescendants = @('node', 'nodejs', 'python', 'pythonw', 'python3', 'py')
      blueprintInterpreter = 'none-resident-native-cli'
    }
    initial = $script:InitialEvidence
    nativeInit = $script:NativeInitEvidence
    downgradeContract = $transitionContract
    downgrade = $rollback
    upgradeContract = 'full-native-upgrade-uninstall-v1'
    upgrade = $script:UpgradeEvidence
    uninstallEvidence = $uninstallEvidence
    lifecycle = [ordered]@{
      install = $certification
      startup = $certification
      hubHealth = $certification
      tray = $certification
      popup = $certification
      renderer = $certification
      mcp17 = $certification
      nativeHostCutover = $certification
      blueprintHubHosted = $certification
      blueprintHubOffOneShot = $certification
      downgrade = if ($previousPath) { $certification } else { 'not_applicable' }
      repair = if ($previousPath) { 'not_applicable' } else { $certification }
      upgrade = $certification
      stateContinuity = $certification
      uninstall = $certification
      residue = $certification
      nativeOnlyProcessTree = $certification
      runtimeInventory = $certification
      adapt = $certification
      workspaceConfigMigration = $certification
    }
  }
  Write-JsonAtomic $EvidencePath $receipt
  if ($Profile -eq 'signed-release') {
    Write-Output "Windows installed qualification passed (signed-release): $(Hash-File $installerPath)"
  } else {
    # Never phrase this as a signed-release PASS.
    Write-Output "Windows installed qualification passed (unsigned-functional, internal profile only): $(Hash-File $installerPath)"
  }
} catch {
  $script:PrimaryQualificationFailure = $_
  Write-Host "[qualification] primary failure: $($_.Exception.Message)"
  throw
} finally {
  try { Stop-QualificationHub } catch {
    if ($script:PrimaryQualificationFailure) {
      Write-Warning "qualification cleanup also failed: $($_.Exception.Message)"
    } else { throw }
  }
  if ($dataMarker -and (Test-Path -LiteralPath $dataMarker)) { Remove-Item -LiteralPath $dataMarker -Force -ErrorAction SilentlyContinue }
  if ($script:QualificationWorkspace -and (Test-Path -LiteralPath $script:QualificationWorkspace)) { Remove-Item -LiteralPath $script:QualificationWorkspace -Recurse -Force -ErrorAction SilentlyContinue }
  if ($null -eq $script:PreviousMembraneWorkspaceRoot) { Remove-Item Env:MEMBRANE_WORKSPACE_ROOT -ErrorAction SilentlyContinue } else { $env:MEMBRANE_WORKSPACE_ROOT = $script:PreviousMembraneWorkspaceRoot }
  if ($null -eq $script:PreviousMembraneWorkspaceConfig) { Remove-Item Env:MEMBRANE_WORKSPACE_CONFIG -ErrorAction SilentlyContinue } else { $env:MEMBRANE_WORKSPACE_CONFIG = $script:PreviousMembraneWorkspaceConfig }
  if ($null -eq $script:PreviousMembraneProjectRegistry) { Remove-Item Env:MEMBRANE_PROJECT_REGISTRY -ErrorAction SilentlyContinue } else { $env:MEMBRANE_PROJECT_REGISTRY = $script:PreviousMembraneProjectRegistry }
}
