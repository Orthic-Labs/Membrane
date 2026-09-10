param(
  [string]$InstalledRoot = $(Join-Path $env:LOCALAPPDATA 'Orthic Labs\Membrane\current'),
  [string]$Output = $(Join-Path $env:TEMP "membrane-windows-observation-$([guid]::NewGuid().ToString('N')).json"),
  [string]$NativeOutput = $(Join-Path $env:TEMP "membrane-windows-native-$([guid]::NewGuid().ToString('N')).json"),
  [string]$InterpreterDispositions = 'D:\Claude\review\windows-r5\interpreter-dispositions.json',
  [string]$QualificationEvidence = '',
  [int]$TimeoutMs = 5000
)

$ErrorActionPreference = 'Stop'
$started = [DateTime]::UtcNow
$platform = [Environment]::OSVersion.VersionString

function Snapshot {
  @(Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name, ExecutablePath, CommandLine | ForEach-Object {
    [ordered]@{ pid = [int]$_.ProcessId; ppid = [int]$_.ParentProcessId; name = $_.Name; executable = $_.ExecutablePath; commandLine = $_.CommandLine }
  })
}

function Descendants([object[]]$Snapshot, [int]$RootPid) {
  $result = [System.Collections.Generic.List[object]]::new()
  $pending = [System.Collections.Generic.Queue[int]]::new()
  $pending.Enqueue($RootPid)
  while ($pending.Count -gt 0) {
    $parent = $pending.Dequeue()
    foreach ($item in @($Snapshot | Where-Object { $_.ppid -eq $parent })) {
      $result.Add($item)
      $pending.Enqueue([int]$item.pid)
    }
  }
  $result.ToArray()
}

function Run-Native([string]$Path, [string[]]$Arguments) {
  if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
    return [ordered]@{ path = $Path; status = 'missing'; processTree = @() }
  }
  $oldPath = $env:PATH
  $out = $null
  $err = $null
  try {
    $env:PATH = 'C:\Windows\System32;C:\Windows'
    $out = Join-Path $env:TEMP "membrane-native-$([guid]::NewGuid().ToString('N')).out"
    $err = Join-Path $env:TEMP "membrane-native-$([guid]::NewGuid().ToString('N')).err"
    $process = Start-Process -FilePath $Path -ArgumentList $Arguments -RedirectStandardOutput $out -RedirectStandardError $err -PassThru -WindowStyle Hidden
    $observed = [System.Collections.Generic.List[object]]::new()
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    while (-not $process.HasExited -and $watch.ElapsedMilliseconds -lt [Math]::Max(1, $TimeoutMs)) {
      foreach ($item in @(Descendants (Snapshot) $process.Id)) {
        if (-not ($observed | Where-Object { $_.pid -eq $item.pid })) { $observed.Add($item) }
      }
      [void]$process.WaitForExit(50)
    }
    $timedOut = -not $process.HasExited
    if ($timedOut) {
      # Kill only this owned process tree through its retained Process handle.
      # Never terminate a saved list of PIDs that may have been reused.
      try { $process.Kill($true) } catch { }
      [void]$process.WaitForExit(1000)
    }
    $tree = @($observed.ToArray())
    $forbidden = @($tree | Where-Object { $_.name -match '^(node|node_repl|python|python3|sh|bash)(\.exe)?$' })
    $stdout = if (Test-Path -LiteralPath $out) { Get-Content -LiteralPath $out -Raw } else { '' }
    $stderr = if (Test-Path -LiteralPath $err) { Get-Content -LiteralPath $err -Raw } else { '' }
    $failure = if ($timedOut) { 'timeout' } elseif ($forbidden.Count -gt 0) { 'forbidden-child' } elseif ($process.ExitCode -ne 0) { 'nonzero-exit' } elseif ([string]::IsNullOrWhiteSpace($stdout)) { 'empty-output' } else { $null }
    [ordered]@{ path = $Path; status = if ($null -eq $failure) { 'passed' } else { 'failed' }; failureType = $failure; exitCode = if ($process.HasExited) { $process.ExitCode } else { $null }; pid = $process.Id; stdout = $stdout; stderr = $stderr; processTree = $tree; forbiddenChildren = $forbidden }
  } catch {
    [ordered]@{ path = $Path; status = 'failed'; error = $_.Exception.Message; processTree = @() }
  } finally {
    $temporaryPaths = @($out, $err) | Where-Object { $_ }
    if ($temporaryPaths) { Remove-Item -LiteralPath $temporaryPaths -Force -ErrorAction SilentlyContinue }
    $env:PATH = $oldPath
  }
}

function Run-Scenario([string]$Lane, [string]$Id) {
  [ordered]@{ id = $Id; lane = $Lane; status = 'blocked'; reason = 'install-release.ps1 output has no executable observation for this semantic scenario'; actions = @(); processTreeBefore = @(); processTreeDuring = @(); processTreeAfter = @() }
}

$required = @('membrane.exe', 'membrane-daemon.exe', 'membrane-tray.exe', 'membrane-hub.exe', 'cortex.exe')
$files = foreach ($name in $required) {
  $path = Join-Path $InstalledRoot $name
  if (Test-Path -LiteralPath $path -PathType Leaf) {
    $item = Get-Item -LiteralPath $path
    [ordered]@{ name = $name; path = $path; bytes = $item.Length; lastWriteTimeUtc = $item.LastWriteTimeUtc.ToString('o') }
  }
}
$payloadInterpreters = @(Get-ChildItem -LiteralPath $InstalledRoot -Recurse -File -ErrorAction SilentlyContinue | Where-Object { $_.Name -match '^(node|nodejs|python|python3|sh|bash)(\.exe)?$' } | ForEach-Object FullName)
$membrane = Join-Path $InstalledRoot 'membrane.exe'
$qualification = $null
if ($QualificationEvidence -and (Test-Path -LiteralPath $QualificationEvidence -PathType Leaf)) { $qualification = Get-Content -LiteralPath $QualificationEvidence -Raw | ConvertFrom-Json }
$cli = Run-Native $membrane @('diagnostics', 'capabilities')
$daemon = [ordered]@{
  path = Join-Path $InstalledRoot 'membrane-daemon.exe'
  status = 'blocked'
  reason = 'Daemon accepts tray-owned lifecycle IPC, not CLI flags; qualify through install-release.ps1 owner-supervised launch, health & drain observations.'
  processTree = @()
}

$allScenarios = @{
  'LC-01' = @('hub-only','coderight-only','both','holder-crash','holder-exit','final-holder-shutdown','concurrent-acquire-renew-release','drain-acquire-race','restart-during-acquire','stale-fencing','survivor-continuity')
  'LC-02' = @('idle-refresh','mid-build-refresh','watcher-disabled-refresh','hub-off-refresh')
  'LC-03' = @('fair-service','deadline-cancellation','scope-isolation','deduplicated-work')
  'LC-04' = @('hub-off-explicit','hub-background','coderight-adopt','provision-missing','reject-corrupt','reject-denied','reject-unverifiable','reject-development-checkout')
  'LC-05' = @('credential-race','lease-incarnation','tombstone','reordered-response','lost-response','clock-rewind','replay-bound')
  'LC-06' = @('canonical-roots','health-probe','startup-lock','atomic-promotion','hook-containment')
}
$recorded = @(); if ($qualification -and $qualification.runtime -and $qualification.runtime.lifecycleObservations) { $recorded = @($qualification.runtime.lifecycleObservations) }
$scenarios = foreach ($lane in $allScenarios.Keys) { foreach ($id in $allScenarios[$lane]) {
  $match = $recorded | Where-Object { $_.lane -eq $lane -and $_.id -eq $id } | Select-Object -First 1
  if ($match) {
    [ordered]@{ id = $id; lane = $lane; status = 'observed'; action = $match.action; evidence = $match; actions = @([ordered]@{ action = $match.action; observed = $match.observed; evidence = $match }); processTreeBefore = @($match.before); processTreeDuring = @(); processTreeAfter = @($match.after) }
  } else { Run-Scenario $lane $id }
} }
$dispositionRows = @(); if (Test-Path -LiteralPath $InterpreterDispositions) { $dispositionRows = @(Get-Content -LiteralPath $InterpreterDispositions -Raw | ConvertFrom-Json) }
$accounting = [ordered]@{ interpretedFiles = $dispositionRows.Count; installedPayloadFiles = $files.Count; interpreterDispositions = $dispositionRows.Count }
$surfaces = @(
  [ordered]@{ name = 'cli'; status = $cli.status; processTree = $cli.processTree; executable = $cli.path },
  [ordered]@{ name = 'mcp'; status = 'blocked'; processTree = @(); reason = 'no installed membrane-mcp.exe surface' },
  [ordered]@{ name = 'sdk'; status = 'blocked'; processTree = @(); reason = 'SDK requires an installed native caller' },
  [ordered]@{ name = 'federation'; status = 'blocked'; processTree = @(); reason = 'federation requires an installed native caller' }
)
$receipt = [ordered]@{
  schema = 'membrane.windows-lifecycle-observation.v1'; platform = 'windows'; installed = (Test-Path -LiteralPath $membrane -PathType Leaf)
  generatedAt = [DateTime]::UtcNow.ToString('o'); host = $env:COMPUTERNAME; os = $platform; installedRoot = $InstalledRoot
  buildIdentity = [ordered]@{ root = $InstalledRoot; generation = if (Test-Path -LiteralPath (Join-Path $InstalledRoot 'release.json')) { (Get-Content (Join-Path $InstalledRoot 'release.json') -Raw | ConvertFrom-Json).version } else { $null }; membraneSha256 = if (Test-Path -LiteralPath $membrane) { (Get-FileHash -LiteralPath $membrane -Algorithm SHA256).Hash } else { $null }; files = $files }
  processTree = @(Snapshot); nativeSurfaces = @($cli, $daemon); surfaces = $surfaces; scenarios = @($scenarios)
  accounting = $accounting
  storage = [ordered]@{ compatibility = 'unmeasured'; database = $null }; vectorScale = [ordered]@{ status = 'unmeasured' }; packageSize = [ordered]@{ bytes = ($files | Measure-Object bytes -Sum).Sum; status = 'measured' }
  qualificationEvidence = if ($qualification) { [ordered]@{ path = $QualificationEvidence; schema = $qualification.schema; generatedAt = $qualification.generatedAt; artifactSha256 = $qualification.artifact.sha256; installedRoot = $qualification.installedCurrent.root } } else { $null }
  payloadInterpreters = $payloadInterpreters; producer = 'observe-windows-lifecycle.ps1'; startedAt = $started.ToString('o')
}
$receipt | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $Output -Encoding utf8
$nativeReceipt = [ordered]@{ schema = 'membrane.windows-native-observation.v1'; platform = 'windows'; generatedAt = $receipt.generatedAt; installedRoot = $InstalledRoot; processTree = @(Snapshot); surfaces = $surfaces; payloadInterpreters = $payloadInterpreters; buildIdentity = $receipt.buildIdentity }
$nativeReceipt | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $NativeOutput -Encoding utf8
Write-Output $Output
Write-Output $NativeOutput
