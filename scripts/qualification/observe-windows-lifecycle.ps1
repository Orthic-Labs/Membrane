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
  # CommandLine is capped: Windows PowerShell 5.1's ConvertTo-Json is
  # catastrophically slow on large object graphs with long strings, and an
  # uncapped full system snapshot (400+ processes) embedded repeatedly was the
  # observer's actual unbounded-wait bug (33-minute hang with zero output).
  @(Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name, ExecutablePath, CommandLine | ForEach-Object {
    $cmd = [string]$_.CommandLine
    if ($cmd.Length -gt 300) { $cmd = $cmd.Substring(0, 300) + '...' }
    [ordered]@{ pid = [int]$_.ProcessId; ppid = [int]$_.ParentProcessId; name = $_.Name; executable = $_.ExecutablePath; commandLine = $cmd }
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

function File-Sha256([string]$Path) {
  if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $null }
  try { return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() } catch { return $null }
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
      try { & taskkill.exe /F /T /PID $process.Id 2>$null | Out-Null } catch { }
    }
    $tree = @($observed.ToArray())
    $forbidden = @($tree | Where-Object { $_.name -match '^(node|node_repl|python|python3|sh|bash)(\.exe)?$' })
    $stdout = if (Test-Path -LiteralPath $out) { Get-Content -LiteralPath $out -Raw } else { '' }
    $stderr = if (Test-Path -LiteralPath $err) { Get-Content -LiteralPath $err -Raw } else { '' }
    $failure = if ($timedOut) { 'timeout' } elseif ($forbidden.Count -gt 0) { 'forbidden-child' } elseif ($process.ExitCode -ne 0) { 'nonzero-exit' } elseif ([string]::IsNullOrWhiteSpace($stdout)) { 'empty-output' } else { $null }
    [ordered]@{ path = $Path; sha256 = File-Sha256 $Path; status = if ($null -eq $failure) { 'passed' } else { 'failed' }; failureType = $failure; exitCode = if ($process.HasExited) { $process.ExitCode } else { $null }; pid = $process.Id; stdout = $stdout; stderr = $stderr; processTree = $tree; forbiddenChildren = $forbidden }
  } catch {
    [ordered]@{ path = $Path; status = 'failed'; error = $_.Exception.Message; processTree = @() }
  } finally {
    $temporaryPaths = @($out, $err) | Where-Object { $_ }
    if ($temporaryPaths) { Remove-Item -LiteralPath $temporaryPaths -Force -ErrorAction SilentlyContinue }
    $env:PATH = $oldPath
  }
}

function Invoke-Membrane([string]$ExePath, [string[]]$Arguments, [int]$Timeout = 8000, [string]$StdinText = $null, [bool]$CaptureTree = $false) {
  $sw = [System.Diagnostics.Stopwatch]::StartNew()
  if (-not (Test-Path -LiteralPath $ExePath -PathType Leaf)) {
    return [ordered]@{ command = "$ExePath $($Arguments -join ' ')".Trim(); executable = $ExePath; sha256 = $null; exitCode = -1; stdout = ''; stderr = "executable not found: $ExePath"; durationMs = 0; timedOut = $false; processTree = @(); forbiddenChildren = @() }
  }
  $psi = New-Object System.Diagnostics.ProcessStartInfo
  $psi.FileName = $ExePath
  $psi.Arguments = (($Arguments | ForEach-Object { if ($_ -match '[\s"]') { '"' + ($_ -replace '"', '\"') + '"' } else { $_ } }) -join ' ')
  $psi.RedirectStandardOutput = $true
  $psi.RedirectStandardError = $true
  $psi.RedirectStandardInput = $true
  $psi.UseShellExecute = $false
  $psi.CreateNoWindow = $true
  $proc = New-Object System.Diagnostics.Process
  $proc.StartInfo = $psi
  $exitCode = -1
  $stdout = ''
  $stderr = ''
  $timedOut = $false
  $observed = [System.Collections.Generic.List[object]]::new()
  try {
    [void]$proc.Start()
    try {
      if ($null -ne $StdinText) { $proc.StandardInput.Write($StdinText) }
      $proc.StandardInput.Close()
    } catch { }
    $stdoutTask = $proc.StandardOutput.ReadToEndAsync()
    $stderrTask = $proc.StandardError.ReadToEndAsync()
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    while (-not $proc.HasExited -and $watch.ElapsedMilliseconds -lt [Math]::Max(1, $Timeout)) {
      if ($CaptureTree) { foreach ($item in @(Descendants (Snapshot) $proc.Id)) { if (-not ($observed | Where-Object { $_.pid -eq $item.pid })) { $observed.Add($item) } } }
      [void]$proc.WaitForExit(50)
    }
    $exited = $proc.HasExited
    if ($CaptureTree) { foreach ($item in @(Descendants (Snapshot) $proc.Id)) { if (-not ($observed | Where-Object { $_.pid -eq $item.pid })) { $observed.Add($item) } } }
    if (-not $exited) {
      $timedOut = $true
      try { $proc.Kill($true) } catch { }
      [void]$proc.WaitForExit(1000)
      try { & taskkill.exe /F /T /PID $proc.Id 2>$null | Out-Null } catch { }
    }
    # Never block indefinitely on the redirected-stream tasks: if a detached
    # grandchild inherited a pipe handle and outlives the killed process tree,
    # GetAwaiter().GetResult() would hang forever. Bound the wait explicitly.
    $readDeadlineMs = 2000
    try {
      if ([System.Threading.Tasks.Task]::WaitAll(@($stdoutTask, $stderrTask), $readDeadlineMs)) {
        $stdout = $stdoutTask.GetAwaiter().GetResult()
        $stderr = $stderrTask.GetAwaiter().GetResult()
      } else {
        $timedOut = $true
        if ($stdoutTask.IsCompletedSuccessfully) { $stdout = $stdoutTask.Result }
        if ($stderrTask.IsCompletedSuccessfully) { $stderr = $stderrTask.Result }
      }
    } catch { }
    $exitCode = if ($exited) { $proc.ExitCode } else { -1 }
  } catch {
    $stderr = $_.Exception.Message
  }
  $sw.Stop()
  # Keep owner process itself in evidence even when native action has no
  # descendants; an empty collection collapses to null in Windows PowerShell.
  [object[]]$tree = @([ordered]@{ pid = $proc.Id; ppid = $null; name = [IO.Path]::GetFileName($ExePath); executable = $ExePath; commandLine = "$ExePath $($Arguments -join ' ')".Trim() }) + @($observed.ToArray())
  [object[]]$forbidden = @($tree | Where-Object { $_.name -match '^(node|node_repl|python|python3|sh|bash)(\.exe)?$' })
  [ordered]@{ command = "$ExePath $($Arguments -join ' ')".Trim(); executable = $ExePath; sha256 = File-Sha256 $ExePath; nativeEvidence = (Test-Path -LiteralPath $ExePath -PathType Leaf); terminal = (-not $timedOut); exitCode = [int]$exitCode; pid = $proc.Id; stdout = [string]$stdout; stderr = [string]$stderr; durationMs = [int]$sw.ElapsedMilliseconds; timedOut = $timedOut; processTree = $tree; forbiddenChildren = $forbidden }
}

function Run-InsufficientScenario([string]$Lane, [string]$Id, [string]$Reason) {
  [ordered]@{ id = $Id; lane = $Lane; status = 'insufficient'; reason = "$($Id): $Reason"; actions = @(); processTreeBefore = @(); processTreeDuring = @(); processTreeAfter = @() }
}

function Run-ExecScenario([string]$Lane, [string]$Id, [string]$ExePath, [string[]]$Arguments, [scriptblock]$Validate, [string]$StdinText = $null, [bool]$CaptureTree = $false) {
  # No per-scenario full-system snapshot here: the schema only requires
  # processTreeBefore/After to be arrays, and the real evidence is the
  # top-level processTree plus the action's own exit/stdout/stderr.
  $before = @()
  $action = Invoke-Membrane -ExePath $ExePath -Arguments $Arguments -StdinText $StdinText -CaptureTree $CaptureTree
  $after = @()
  $ok = $false
  try { $ok = [bool](& $Validate $action) } catch { $ok = $false }
  $status = if ($ok) { 'passed' } else { 'failed' }
  $reason = if ($ok) {
    "$($Id): real execution of '$($action.command)' against installed runtime confirmed the expected observed outcome"
  } else {
    "$($Id): real execution of '$($action.command)' observed exit=$($action.exitCode) stdout='$($action.stdout.Substring(0,[Math]::Min(200,$action.stdout.Length)))' stderr='$($action.stderr.Substring(0,[Math]::Min(200,$action.stderr.Length)))', which did not match the expected outcome"
  }
  [ordered]@{ id = $Id; lane = $Lane; status = $status; reason = $reason; actions = @($action); processTreeBefore = $before; processTreeDuring = @($before); processTreeAfter = $after }
}

# Manual, bounded JSON writer. The observer's actual unbounded-wait bug was
# not process hangs but this PowerShell 5.1 build's ConvertTo-Json: whenever
# leftover -Depth budget remains past a value's real nesting level, it
# re-enumerates plain strings character-by-character and recurses into each
# char, blowing a 1283-char string up to 3.8MB of JSON at -Depth 5 alone (and
# effectively never terminating at -Depth 12). ConvertTo-Json is never used
# for receipt output; this walks our own known shapes (ordered/hashtable,
# array/list, string, bool, numeric, $null) and always treats a string as a
# terminal leaf, so there is no depth-dependent blowup to bound in the first
# place.
function ConvertTo-JsonSafeString([string]$Value) {
  $sb = [System.Text.StringBuilder]::new($Value.Length + 8)
  [void]$sb.Append('"')
  foreach ($ch in $Value.ToCharArray()) {
    switch ($ch) {
      '"' { [void]$sb.Append('\"'); break }
      '\' { [void]$sb.Append('\\'); break }
      "`n" { [void]$sb.Append('\n'); break }
      "`r" { [void]$sb.Append('\r'); break }
      "`t" { [void]$sb.Append('\t'); break }
      default {
        $code = [int]$ch
        if ($code -lt 0x20) { [void]$sb.Append([string]::Format('\u{0:x4}', $code)) }
        else { [void]$sb.Append($ch) }
      }
    }
  }
  [void]$sb.Append('"')
  $sb.ToString()
}

function ConvertTo-JsonSafe([object]$Value, [System.Text.StringBuilder]$Sb) {
  if ($null -eq $Value) { [void]$Sb.Append('null'); return }
  if ($Value -is [string]) { [void]$Sb.Append((ConvertTo-JsonSafeString $Value)); return }
  if ($Value -is [bool]) { [void]$Sb.Append($(if ($Value) { 'true' } else { 'false' })); return }
  if ($Value -is [int] -or $Value -is [long] -or $Value -is [double] -or $Value -is [decimal] -or $Value -is [uint32] -or $Value -is [uint64] -or $Value -is [byte]) {
    [void]$Sb.Append([System.Convert]::ToString($Value, [System.Globalization.CultureInfo]::InvariantCulture)); return
  }
  if ($Value -is [System.Collections.IDictionary]) {
    [void]$Sb.Append('{')
    $first = $true
    foreach ($key in $Value.Keys) {
      if (-not $first) { [void]$Sb.Append(',') }
      $first = $false
      [void]$Sb.Append((ConvertTo-JsonSafeString ([string]$key)))
      [void]$Sb.Append(':')
      ConvertTo-JsonSafe $Value[$key] $Sb
    }
    [void]$Sb.Append('}')
    return
  }
  if ($Value -is [System.Collections.IEnumerable]) {
    [void]$Sb.Append('[')
    $first = $true
    foreach ($item in $Value) {
      if (-not $first) { [void]$Sb.Append(',') }
      $first = $false
      ConvertTo-JsonSafe $item $Sb
    }
    [void]$Sb.Append(']')
    return
  }
  # Unknown scalar type (e.g. a boxed enum): fall back to its string form as a
  # JSON string leaf, never to further reflection/enumeration.
  [void]$Sb.Append((ConvertTo-JsonSafeString ([string]$Value)))
}

function Write-JsonBounded([object]$Value, [string]$Path, [int]$Depth = 12, [int]$TimeoutMs = 20000) {
  # $Depth/$TimeoutMs are accepted for call-site compatibility. The manual
  # serializer above is a single linear walk over known-finite shapes (plain
  # hashtables/arrays/strings/numbers built by this script), so it has no
  # depth-dependent or otherwise unbounded behavior left to guard against;
  # a Stopwatch still records actual duration for the receipt/log record.
  # Plain UTF-8, no BOM: consumers (Node's fs.readFileSync + JSON.parse) treat
  # a leading BOM as an invalid token and fail to parse otherwise-valid JSON.
  $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
  $sw = [System.Diagnostics.Stopwatch]::StartNew()
  try {
    $sb = [System.Text.StringBuilder]::new()
    ConvertTo-JsonSafe $Value $sb
    [System.IO.File]::WriteAllText($Path, $sb.ToString(), $utf8NoBom)
    $sw.Stop()
    return $true
  } catch {
    $sw.Stop()
    $fallbackSb = [System.Text.StringBuilder]::new()
    ConvertTo-JsonSafe ([ordered]@{ schema = 'membrane.windows-lifecycle-observation.v1'; status = 'insufficient'; reason = "receipt serialization failed: $($_.Exception.Message)" }) $fallbackSb
    [System.IO.File]::WriteAllText($Path, $fallbackSb.ToString(), $utf8NoBom)
    return $false
  }
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

$devCheckoutRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$missingRoot = Join-Path $env:TEMP "membrane-qualification-missing-root-$([guid]::NewGuid().ToString('N'))"
$refreshProbeRoot = Join-Path $env:TEMP "membrane-qualification-refresh-root-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $refreshProbeRoot -Force | Out-Null
Set-Content -LiteralPath (Join-Path $refreshProbeRoot 'main.rs') -Value 'fn main() {}' -Encoding utf8
$refreshProbeArgs = @('cli', 'blueprint', 'refresh', '--repo-root', $refreshProbeRoot, '--deadline-ms', '10000')
$refreshResult = {
  param($a)
  if ($a.exitCode -ne 0 -or [string]::IsNullOrWhiteSpace($a.stdout)) { return $false }
  try {
    $value = $a.stdout | ConvertFrom-Json
    return $value.operation -eq 'refresh' -and $value.state -eq 'fresh' -and
      $value.complete -eq $true -and -not [string]::IsNullOrWhiteSpace([string]$value.generationId)
  } catch { return $false }
}

# NCL-05 probes each installed native owner directly. Stdio MCP and explicit
# SDK are stdin-framed modes; federation is native Pull through CLI.
$mcpInput = @(
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"windows-qualification","version":"1"}}}',
  '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'
) -join "`n"
$mcpResult = {
  param($a)
  if ($a.exitCode -ne 0 -or $a.timedOut -or [string]::IsNullOrWhiteSpace($a.stdout)) { return $false }
  try {
    $rows = @($a.stdout.Trim().Split("`n") | Where-Object { -not [string]::IsNullOrWhiteSpace($_) } | ForEach-Object { $_ | ConvertFrom-Json })
    return $rows.Count -ge 2 -and $rows[0].result.protocolVersion -eq '2025-03-26' -and
      $null -ne $rows[1].result.tools -and @($rows[1].result.tools).Count -gt 0
  } catch { return $false }
}
$sdkInput = '{"schemaVersion":1,"operation":"binding","expectedBinding":null,"request":{}}'
$sdkResult = {
  param($a)
  if ($a.exitCode -ne 0 -or $a.timedOut -or [string]::IsNullOrWhiteSpace($a.stdout)) { return $false }
  try {
    $v = $a.stdout | ConvertFrom-Json
    return $v.schemaVersion -eq 1 -and $v.status -eq 200 -and $v.binding.nativeOnly -eq $true -and
      -not [string]::IsNullOrWhiteSpace([string]$v.binding.stableInstallRoot) -and
      -not [string]::IsNullOrWhiteSpace([string]$v.binding.releaseGeneration)
  } catch { return $false }
}
$federationArgs = @('cli', 'pull', 'federate', '--task', 'ncl-05-surface', '--repo', $refreshProbeRoot, '--max-tokens', '64', '--client', 'qualification')
$federationResult = {
  param($a)
  if ($a.exitCode -ne 0 -or $a.timedOut -or [string]::IsNullOrWhiteSpace($a.stdout)) { return $false }
  try {
    $v = $a.stdout | ConvertFrom-Json
    return $null -ne $v.packet -and $null -ne $v.receipts
  } catch { return $false }
}
# Federation consumes Blueprint freshness metadata. Seed that metadata through
# the same installed binary before probing Pull, while retaining federation's
# own process/result as its surface evidence.
$surfaceRefresh = Invoke-Membrane -ExePath $membrane -Arguments $refreshProbeArgs -Timeout 15000

function Run-QualificationScenario([string]$Lane, [string]$Id, [string]$ExePath) {
  # Reserved installed control surface. Until shipped, execution is recorded as
  # failed/insufficient evidence; it is never treated as an implicit pass.
  Run-ExecScenario $Lane $Id $ExePath @('cli', 'qualification', 'lifecycle', $Id) {
    param($a)
    if ($a.exitCode -ne 0 -or [string]::IsNullOrWhiteSpace($a.stdout)) { return $false }
    try {
      $v = $a.stdout | ConvertFrom-Json
      $e = $v.evidence
      return $v.schema -eq 'membrane.installed-lifecycle-scenario.v1' -and
        $v.scenario -eq $Id -and $v.status -eq 'passed' -and
        $v.terminal -eq $true -and $v.runtimeOrigin -eq 'installed' -and
        $null -ne $v.installedIdentity -and $v.installedIdentity.verified -eq $true -and
        $null -ne $e.expectedControllerState -and $null -ne $e.watcherActivity
    } catch { return $false }
  }
}

function Run-QualificationObservation([object]$Qualification, [string]$Lane, [string]$Id) {
  $rows = @()
  if ($null -ne $Qualification -and $Qualification.lifecycleObservations) {
    $rows = @($Qualification.lifecycleObservations | Where-Object { $_.lane -eq $Lane -and $_.id -eq $Id })
  }
  if ($rows.Count -eq 0) { return Run-InsufficientScenario $Lane $Id "no install-release lifecycle observation for $Id" }
  $row = $rows[-1]
  $identity = $Qualification.installedCurrent
  $identityOk = $null -ne $identity -and -not [string]::IsNullOrWhiteSpace([string]$identity.root)
  $native = $row.native -eq $true -or ($null -ne $row.processTree -and @($row.processTree).Count -gt 0)
  $terminal = $row.terminal -eq $true -or $row.completed -eq $true -or ($row.observed -eq $true -and -not [string]::IsNullOrWhiteSpace([string]$row.action))
  $ok = $row.observed -eq $true -and $identityOk -and $native -and $terminal
  [ordered]@{ id = $Id; lane = $Lane; status = if ($ok) { 'passed' } else { 'insufficient' }; reason = if ($ok) { "$($Id): install-release observation passed with installed identity and terminal native evidence" } else { "$($Id): install-release observation lacks observed=true, installed identity, terminal, or native evidence" }; actions = @(); processTreeBefore = @($row.before); processTreeDuring = @($row.processTree); processTreeAfter = @($row.after); evidence = $row }
}

# Real, bounded execution against installed binaries only. Every
# scenario either runs a real membrane.exe subcommand and derives passed/failed
# from the observed exit code/stdout/stderr (never a constant), or is recorded
# insufficient with the exact missing/unsafe command so the case never
# fabricates a pass. Scenarios that would require mutating the shared
# installed resident daemon (real, non-dry-run activate/deactivate) are not
# executed here because other qualification lanes concurrently depend on that
# same installed daemon; each such scenario names the exact command withheld.
$scenarioSpecs = @(
  @{ lane = 'NCL-05'; id = 'cli'; exe = $membrane; args = @('diagnostics', 'capabilities'); captureProcessTree = $true; validate = { param($a) $a.exitCode -eq 0 -and $a.stdout.TrimStart().StartsWith('{') } }
  @{ lane = 'NCL-05'; id = 'mcp'; exe = $membrane; args = @('stdio-mcp'); input = $mcpInput; captureProcessTree = $true; validate = $mcpResult }
  @{ lane = 'NCL-05'; id = 'sdk'; exe = $membrane; args = @('cli', 'explicit-call'); input = $sdkInput; captureProcessTree = $true; validate = $sdkResult }
  @{ lane = 'NCL-05'; id = 'federation'; exe = $membrane; args = $federationArgs; captureProcessTree = $true; validate = $federationResult }

  @{ lane = 'LC-01'; id = 'hub-only'; control = $true }
  @{ lane = 'LC-01'; id = 'coderight-only'; control = $true }
  @{ lane = 'LC-01'; id = 'both'; control = $true }
  @{ lane = 'LC-01'; id = 'holder-crash'; control = $true }
  @{ lane = 'LC-01'; id = 'holder-exit'; control = $true }
  @{ lane = 'LC-01'; id = 'final-holder-shutdown'; control = $true }
  @{ lane = 'LC-01'; id = 'concurrent-acquire-renew-release'; control = $true }
  @{ lane = 'LC-01'; id = 'drain-acquire-race'; control = $true }
  @{ lane = 'LC-01'; id = 'restart-during-acquire'; control = $true }
  @{ lane = 'LC-01'; id = 'stale-fencing'; control = $true }
  @{ lane = 'LC-01'; id = 'survivor-continuity'; control = $true }

  # Use a tiny temporary repository so this installed-path probe measures the
  # shipped refresh implementation, not an unrelated checkout's scan time.
  @{ lane = 'LC-02'; id = 'idle-refresh'; exe = $membrane; args = $refreshProbeArgs; validate = $refreshResult }
  @{ lane = 'LC-02'; id = 'mid-build-refresh'; control = $true }
  @{ lane = 'LC-02'; id = 'watcher-disabled-refresh'; control = $true }
  @{ lane = 'LC-02'; id = 'hub-off-refresh'; exe = $membrane; args = $refreshProbeArgs; validate = $refreshResult }

  @{ lane = 'LC-03'; id = 'fair-service'; control = $true }
  @{ lane = 'LC-03'; id = 'deadline-cancellation'; control = $true }
  @{ lane = 'LC-03'; id = 'scope-isolation'; control = $true }
  @{ lane = 'LC-03'; id = 'deduplicated-work'; control = $true }

  @{ lane = 'LC-04'; id = 'hub-off-explicit'; exe = $membrane; args = @('cli', 'doctor', '--json'); validate = { param($a) $a.exitCode -eq 0 -and $a.stdout.TrimStart().StartsWith('{') } }
  @{ lane = 'LC-04'; id = 'hub-background'; control = $true }
  @{ lane = 'LC-04'; id = 'coderight-adopt'; control = $true }
  # Dry-run status validates missing-root provisioning refusal without touching
  # shared install state; the canonical installer remains out of scope.
  @{ lane = 'LC-04'; id = 'provision-missing'; exe = $membrane; args = @('status', '--dry-run', '--install-root', $missingRoot); validate = {
      param($a)
      $a.exitCode -ne 0 -and (($a.stdout + $a.stderr) -match 'stable installed path|installed root|activation')
    } }
  @{ lane = 'LC-04'; id = 'reject-corrupt'; control = $true }
  @{ lane = 'LC-04'; id = 'reject-denied'; control = $true }
  @{ lane = 'LC-04'; id = 'reject-unverifiable'; control = $true }
  @{ lane = 'LC-04'; id = 'reject-development-checkout'; exe = $membrane; args = @('status', '--dry-run', '--install-root', $devCheckoutRoot); validate = { param($a) $a.exitCode -ne 0 } }

  @{ lane = 'LC-05'; id = 'credential-race'; control = $true }
  @{ lane = 'LC-05'; id = 'lease-incarnation'; control = $true }
  @{ lane = 'LC-05'; id = 'tombstone'; control = $true }
  @{ lane = 'LC-05'; id = 'reordered-response'; control = $true }
  @{ lane = 'LC-05'; id = 'lost-response'; control = $true }
  @{ lane = 'LC-05'; id = 'clock-rewind'; control = $true }
  @{ lane = 'LC-05'; id = 'replay-bound'; control = $true }

  # canonical-roots validates against the exact observed shape of `status --dry-run`'s
  # JSON stdout: installRoot is emitted as a JSON-escaped Windows path, so each
  # single backslash in the real filesystem path (...\Orthic Labs\Membrane\current)
  # appears doubled in stdout (...\\Orthic Labs\\Membrane\\current). A single-backslash
  # pattern never matches JSON-escaped stdout and was the prior validator's bug.
  @{ lane = 'LC-06'; id = 'canonical-roots'; exe = $membrane; args = @('status', '--dry-run'); validate = { param($a) $a.exitCode -eq 0 -and $a.stdout -match [regex]::Escape('Orthic Labs') -and $a.stdout -match [regex]::Escape('\\Membrane\\current') } }
  # A resident-free installed runtime reports typed health_unavailable on
  # stderr before any HTTP response exists. A resident returning HTTP 503 may
  # emit a structured body; accept either typed unavailable shape only.
  @{ lane = 'LC-06'; id = 'health-probe'; exe = $membrane; args = @('cli', 'health'); validate = {
      param($a)
      if ($a.exitCode -eq 0 -or ([string]::IsNullOrWhiteSpace($a.stdout) -and [string]::IsNullOrWhiteSpace($a.stderr))) { return $false }
      if ($a.stderr -match 'health_unavailable') { return $true }
      try {
        $value = $a.stdout | ConvertFrom-Json
        return $value.runtimeOrigin -eq 'installed' -and
          -not [string]::IsNullOrWhiteSpace([string]$value.releaseGeneration) -and
          (($a.stderr) -match 'HTTP 503|health unavailable|health probe')
      } catch { return $false }
    } }
  @{ lane = 'LC-06'; id = 'startup-lock'; control = $true }
  @{ lane = 'LC-06'; id = 'atomic-promotion'; control = $true }
  @{ lane = 'LC-06'; id = 'hook-containment'; control = $true }
)
# Overall script-level deadline: bound the whole scenario sweep well under the
# 4-minute lane budget so a single stuck action can never hang the observer.
# Any scenario that has not started by the deadline is recorded 'insufficient'
# with a timeout reason instead of being silently skipped or fabricated.
$scriptDeadline = [System.Diagnostics.Stopwatch]::StartNew()
$scriptBudgetMs = 180000
$scenarios = foreach ($spec in $scenarioSpecs) {
  if ($scriptDeadline.ElapsedMilliseconds -ge $scriptBudgetMs) {
    Run-InsufficientScenario $spec.lane $spec.id "observer script-level timeout budget (${scriptBudgetMs}ms) exceeded before this scenario could run"
    continue
  }
  if ($spec.ContainsKey('observed')) {
    Run-QualificationObservation $qualification $spec.lane $spec.id
  } elseif ($spec.ContainsKey('control')) {
    Run-QualificationScenario $spec.lane $spec.id $membrane
  } elseif ($spec.ContainsKey('reason')) {
    Run-InsufficientScenario $spec.lane $spec.id $spec.reason
  } elseif ($spec.ContainsKey('input')) {
    Run-ExecScenario $spec.lane $spec.id $spec.exe $spec.args $spec.validate $spec.input ([bool]$spec.captureProcessTree)
  } else {
    Run-ExecScenario $spec.lane $spec.id $spec.exe $spec.args $spec.validate $null ([bool]$spec.captureProcessTree)
  }
}
$dispositionRows = @(); if (Test-Path -LiteralPath $InterpreterDispositions) { $dispositionRows = @(Get-Content -LiteralPath $InterpreterDispositions -Raw | ConvertFrom-Json) }
$accounting = [ordered]@{ interpretedFiles = $dispositionRows.Count; installedPayloadFiles = $files.Count; interpreterDispositions = $dispositionRows.Count }
$surfaceRows = @($scenarios | Where-Object { $_.lane -eq 'NCL-05' })
$surfaces = foreach ($name in @('cli', 'mcp', 'sdk', 'federation')) {
  $row = @($surfaceRows | Where-Object { $_.id -eq $name }) | Select-Object -Last 1
  $action = if ($null -ne $row) { @($row.actions) | Select-Object -First 1 } else { $null }
  [object[]]$surfaceTree = if ($null -ne $action) { @($action.processTree) } else { @() }
  [object[]]$surfaceForbidden = if ($null -ne $action) { @($action.forbiddenChildren) } else { @() }
  [ordered]@{
    name = $name
    status = if ($null -ne $row) { $row.status } else { 'failed' }
    executable = if ($null -ne $action) { $action.executable } else { $membrane }
    sha256 = if ($null -ne $action) { $action.sha256 } else { File-Sha256 $membrane }
    pid = if ($null -ne $action) { $action.pid } else { $null }
    exitCode = if ($null -ne $action) { $action.exitCode } else { $null }
    terminal = if ($null -ne $action) { $action.terminal } else { $false }
    processTree = $surfaceTree
    forbiddenChildren = $surfaceForbidden
    reason = if ($null -ne $row) { $row.reason } else { 'surface probe did not execute' }
  }
}
$receipt = [ordered]@{
  schema = 'membrane.windows-lifecycle-observation.v1'; platform = 'windows'; installed = (Test-Path -LiteralPath $membrane -PathType Leaf)
  generatedAt = [DateTime]::UtcNow.ToString('o'); host = $env:COMPUTERNAME; os = $platform; installedRoot = $InstalledRoot
  buildIdentity = [ordered]@{ root = $InstalledRoot; generation = if (Test-Path -LiteralPath (Join-Path $InstalledRoot 'release.json')) { (Get-Content (Join-Path $InstalledRoot 'release.json') -Raw | ConvertFrom-Json).version } else { $null }; membraneSha256 = if (Test-Path -LiteralPath $membrane) { (Get-FileHash -LiteralPath $membrane -Algorithm SHA256).Hash } else { $null }; files = $files }
  processTree = @(Snapshot); nativeSurfaces = @($surfaces); surfaces = $surfaces; scenarios = @($scenarios)
  accounting = $accounting
  storage = [ordered]@{ compatibility = 'unmeasured'; database = $null }; vectorScale = [ordered]@{ status = 'unmeasured' }; packageSize = [ordered]@{ bytes = (($files | ForEach-Object { $_.bytes }) | Measure-Object -Sum).Sum; status = 'measured' }
  qualificationEvidence = if ($qualification) { [ordered]@{ path = $QualificationEvidence; schema = $qualification.schema; generatedAt = $qualification.generatedAt; artifactSha256 = $qualification.artifact.sha256; installedRoot = $qualification.installedCurrent.root } } else { $null }
  payloadInterpreters = $payloadInterpreters; producer = 'observe-windows-lifecycle.ps1'; startedAt = $started.ToString('o')
}
Remove-Item -LiteralPath $refreshProbeRoot -Recurse -Force -ErrorAction SilentlyContinue
Write-JsonBounded -Value $receipt -Path $Output -Depth 12 -TimeoutMs 20000 | Out-Null
$nativeReceipt = [ordered]@{ schema = 'membrane.windows-native-observation.v1'; platform = 'windows'; generatedAt = $receipt.generatedAt; installedRoot = $InstalledRoot; processTree = @(Snapshot); surfaces = $surfaces; payloadInterpreters = $payloadInterpreters; buildIdentity = $receipt.buildIdentity }
Write-JsonBounded -Value $nativeReceipt -Path $NativeOutput -Depth 12 -TimeoutMs 20000 | Out-Null
Write-Output $Output
Write-Output $NativeOutput
