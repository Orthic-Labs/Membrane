import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import test from "node:test";

const source = readFileSync(new URL("./install-release.ps1", import.meta.url), "utf8");
const lower = source.toLowerCase();
const nsi = readFileSync(new URL("../../apps/membrane-hub/src-tauri/windows/installer.nsi", import.meta.url), "utf8");

test("native LOCALAPPDATA preflight rejects packaged path virtualization", { skip: process.platform !== "win32" }, () => {
  const run = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", fileURLToPath(new URL("./assert-native-localappdata.test.ps1", import.meta.url))], {
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(run.status, 0, `${run.stdout}\n${run.stderr}`);
  assert.match(run.stdout, /assert-native-localappdata tests passed/);
});

test("qualification receipt writes a file & rejects directory destinations", { skip: process.platform !== "win32" }, () => {
  const powershell = String.raw`
$ErrorActionPreference = 'Stop'
foreach ($module in @('Microsoft.PowerShell.Utility', 'Microsoft.PowerShell.Management')) {
  Import-Module (Join-Path $PSHOME "Modules\$module\$module.psd1") -Force
}
$tokens = $null; $parseErrors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($env:MEMBRANE_QUALIFICATION_SOURCE, [ref]$tokens, [ref]$parseErrors)
$fn = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Write-JsonAtomic' }, $true)
function Require([bool]$Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
. ([scriptblock]::Create($fn.Extent.Text))
$caseDir = Join-Path ([IO.Path]::GetTempPath()) ('membrane-evidence-test-' + [guid]::NewGuid().ToString('N'))
try {
  $receipt = Join-Path $caseDir 'evidence.json'
  Write-JsonAtomic $receipt @{ status = 'pass' }
  if ((Get-Content -Raw -LiteralPath $receipt | ConvertFrom-Json).status -ne 'pass') { throw 'receipt not readable at exact path' }
  Write-JsonAtomic $receipt @{ status = 'replaced' }
  if ((Get-Content -Raw -LiteralPath $receipt | ConvertFrom-Json).status -ne 'replaced') { throw 'receipt replacement failed' }
  try { Write-JsonAtomic $caseDir @{ status = 'wrong' }; throw 'directory accepted' }
  catch { if ($_.Exception.Message -notmatch 'JSON evidence destination is a directory') { throw } }
  if (@(Get-ChildItem -LiteralPath $caseDir).Count -ne 1) { throw 'stranded temporary receipt' }
  Write-Output 'PASS'
} finally {
  if (Test-Path -LiteralPath $caseDir) {
    $resolved = (Resolve-Path -LiteralPath $caseDir).Path
    if (-not $resolved.StartsWith([IO.Path]::GetTempPath(), [StringComparison]::OrdinalIgnoreCase)) { throw 'test cleanup escaped temp root' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
  }
}
`;
  const run = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", powershell], {
    encoding: "utf8",
    env: { ...process.env, MEMBRANE_QUALIFICATION_SOURCE: fileURLToPath(new URL("./install-release.ps1", import.meta.url)) },
    windowsHide: true,
  });
  assert.equal(run.status, 0, `${run.stdout}\n${run.stderr}`);
  assert.match(run.stdout, /PASS/);
});

test("installer releases the install lock before binding & suppresses the supervisor task during extract", () => {
  // Binding-only reconciliation runs after cutover and lock release.
  const cutover = nsi.indexOf('"cutover-current ok"');
  const bind = nsi.indexOf('"bind-installed-clients"');
  const lockRelease = nsi.indexOf('RMDir /r "$INSTDIR\\.install-lock"', cutover);
  assert.ok(cutover > -1, "cutover marker missing");
  assert.ok(bind > -1, "bind step missing");
  assert.ok(lockRelease > cutover, "post-cutover lock release missing");
  assert.ok(bind > lockRelease, "bind must run after the install lock is released");
  // Pre-cutover installs may still carry a per-minute engine task; it must
  // be disabled before extract, and the installer deletes it — activation
  // never re-creates it (decisions 21/24: no OS-scheduler lifetime lane).
  const extract = nsi.indexOf('"extract-version-tree"');
  const disable = nsi.indexOf('schtasks.exe /Change /TN "Membrane Engine" /Disable');
  assert.ok(extract > -1, "extract step missing");
  assert.ok(disable > -1 && disable < extract, "supervisor task must be disabled before extract begins");
  const taskDelete = nsi.indexOf('schtasks.exe /Delete /TN "Membrane Engine" /F');
  assert.ok(taskDelete > -1, "legacy supervisor task must be deleted during install");
  assert.ok(!nsi.includes('schtasks.exe /Create'), "installer must never create an engine task");
  assert.ok(!nsi.includes('schtasks.exe /Run'), "installer must never run an engine task");
  // Login startup targets the tray (which starts and holds the engine),
  // never the engine binary itself.
  assert.ok(nsi.includes('membrane-tray.exe" --login-launch'), "login startup must launch the tray");
  assert.match(nsi, /ReadRegStr \$R3[\s\S]*membrane\.exe" activate --install-root[\s\S]*WriteRegStr[\s\S]*membrane-tray\.exe" --login-launch/, "legacy engine login must migrate to tray");
  // Bind is registration-only: it must not start the engine.
  assert.ok(nsi.includes('activate --bindings-only --install-root'), "bind step must be bindings-only");
});

test("Windows installed qualification is package-only & signature-bound", () => {
  for (const term of [
    "Get-AuthenticodeSignature",
    "Assert-SignedFile $HubExecutable",
    "Assert-BoundEvidence",
    "Get-FileHash",
    "EvidencePath",
    "membrane.windows-installed-qualification.v1",
    "PreviousInstaller",
    "uninstall.exe",
  ]) assert.ok(lower.includes(term.toLowerCase()), term);
  assert.doesNotMatch(lower, /pnpm|cargo|tauri\s+build|right-release\s+(build|sign)/);
  assert.match(lower, /publisher does not match installer publisher/);
  assert.doesNotMatch(lower, /expectedthumbprint|signer does not match installer signer/);
  assert.match(source, /foreach \(\$module in @\('Microsoft\.PowerShell\.Security', 'Microsoft\.PowerShell\.Utility', 'Microsoft\.PowerShell\.Management'\)\)/);
  assert.match(source, /Import-Module \(Join-Path \$PSHOME "Modules\\\$module\\\$module\.psd1"\) -Force -ErrorAction Stop/);
});

test("qualification exercises native stdio MCP discovery & every registry tool", () => {
  assert.match(source, /Invoke-NativeMcp \$native\.Client -ExerciseAll/);
  assert.match(source, /Daemon = \$membrane;/);
  assert.match(source, /\$health\.backgroundAuthority\.active -eq \$true/);
  assert.match(lower, /stdio-mcp/);
  assert.match(lower, /tools\/list/);
  assert.match(lower, /tools\.count -eq \$publictools\.count/);
  assert.match(source, /\$publicTools = @\('pull', 'push'\)/);
  assert.match(lower, /membrane\.toolsets\.v1/);
  for (const name of [
    "membrane_context", "membrane_source_read", "membrane_blueprint",
    "membrane_knowledge_propose", "membrane_checkpoint_save", "membrane_checkpoint_load",
    "membrane_working_context", "membrane_temporal_fact", "membrane_scratchpad",
    "membrane_feedback", "membrane_memory", "membrane_memory_read", "membrane_ledger",
    "membrane_diagnostic_workspace", "membrane_diagnostic_mutation",
    "membrane_diagnostic_snapshot", "membrane_diagnostic_fence",
    "membrane_diagnostic_capabilities", "membrane_diagnostic_baseline",
    "membrane_diagnostic_provider",
  ]) assert.ok(lower.includes(name), name);
});

test("qualification covers tray, popup, renderer, native cutover & forbidden descendants", () => {
  for (const term of [
    "Shell_TrayWnd", "Find-TrayElement", "Assert-TrayAndPopup", "Assert-RendererWindows",
    "Assert-Dashboard", "exactly one visible on-demand dashboard renderer window", "Assert-NativeHostCutover", "TrayProcessId", "DaemonProcessId",
    "native-only steady-state", "retired interpreter process", "installed engine is not a singleton",
    "--open-dashboard", "bootstrapped Hub", "unexpected",
  ]) assert.ok(lower.includes(term.toLowerCase()), term);
  assert.match(lower, /windows notification area is unavailable/);
  for (const field of ["serviceId", "installationId", "cortexStoreId", "releaseGeneration", "protocolVersion", "schemaVersion", "nativeOnly", "subsystems", "capabilities"]) assert.ok(source.includes(field), field);
  assert.match(lower, /executableSha256/i);
  assert.match(lower, /Get-InstalledContentEvidence/i);
});

test("qualification proves current -> transition -> upgrade or repair state continuity & uninstall residue", () => {
  assert.match(lower, /invoke-installer \$installerpath[\s\S]*(?:invoke-installer \$previouspath[\s\S]*invoke-installer \$installerpath|invoke-installer \$installerpath)/);
  assert.match(lower, /start-andverifyprevioushub \$previousversion/);
  assert.match(lower, /tray-owned installed daemon did not become resident during \$phase/);
  assert.match(lower, /downgrade\s*=\s*\$rollback/);
  assert.match(lower, /transitioncontract\s*=\s*'signed-version-liveness-durable-state-v1'/);
  assert.match(lower, /transitioncontract\s*=\s*'first-stable-layout-repair-v1'/);
  assert.match(lower, /same-version repair did not reuse the version root/);
  assert.match(lower, /invoke-activation \$installroot/);
  assert.match(lower, /invoke-activationdryrun \$installroot/);
  assert.match(lower, /durablestate.*preserved/);
  assert.match(lower, /upgradecontract\s*=\s*'full-native-upgrade-uninstall-v1'/);
  for (const field of ["installRootRemoved", "processesRemoved", "shortcutsRemoved", "registryRemoved", "durableStatePreserved"]) assert.ok(source.includes(field), field);
  for (const term of [
    "Save-State", "Assert-State", "native-upgrade-continuity", "roots.data",
    "durable data changed during downgrade", "durable data changed during upgrade",
    "Assert-UninstallResidue", "receipt-owned residue", "shortcut targeting install root",
    "registry install entry",
    "uninstall left current junction",
    "uninstall left versioned payloads",
  ]) assert.ok(lower.includes(term.toLowerCase()), term);
  assert.match(lower, /get-artifactversion/);
  assert.match(lower, /current & previous installers are the same version/);
  assert.match(lower, /previous installer version .* is not older than current/);
  assert.match(lower, /expectedgeneration/);
  assert.match(lower, /forbiddengeneration/);
  assert.match(lower, /tray dashboard signal did not exit/);
  assert.match(source, /Start-HiddenProcess \$script:TrayPath @\('--replace'\) \$InstallRoot/);
  assert.match(lower, /graceful tray exit fell back to \$trayexitmode/);
  assert.match(lower, /final-holder daemon drain did not complete/);
  assert.match(lower, /assert-qualificationprocesstreegone/);
  assert.doesNotMatch(lower, /membrane-blueprint-|namedpipeclientstream|named pipe remained open/);
});

test("qualification binds exact installed renderer, sidecar, & Blueprint process paths", () => {
  assert.match(lower, /count -eq 1/);
  assert.match(lower, /installerpath/);
  assert.match(lower, /sidecar is missing at inventory path/);
  assert.match(lower, /bounded native blueprint one-shot/);
  assert.match(lower, /cli blueprint status --repo-root/);
  assert.match(lower, /tray-owned installed daemon/);
  assert.match(lower, /hub dashboard did not become visible through tray bootstrap/);
  assert.match(lower, /findings\.get/);
  assert.match(lower, /blueprint recall/);
  assert.doesNotMatch(lower, /invoke-blueprintpipe|get-blueprintendpoint/);
  assert.match(lower, /membrane\.exe cli blueprint/);
});

test("Blueprint qualification uses supported native CLI & proves typed negative seam", () => {
  assert.match(lower, /invoke-blueprintoneshot/);
  assert.match(lower, /hub health omitted blueprint watcher subsystem/);
  for (const term of [
    "cli blueprint recall --repo-root",
    "cli blueprint findings.get --repo-root",
    "generationMismatch = 'pass'",
    "generation_mismatch|stale_blocked",
    "watcher = 'hub-health-and-freshness'",
    "watcher-qualification.mjs",
    "newer fresh generation after isolated file mutation",
    "watcherMutation = 'pass'",
    "watcherQuery = 'pass'",
  ]) assert.ok(lower.includes(term.toLowerCase()), term);
});

test("qualification binds all four native sidecars", () => {
  for (const sidecar of ["membrane-tray", "membrane-client", "membrane-engine", "cortex-cli"]) {
    assert.match(lower, new RegExp(sidecar.replace('.', '\\.'), 'i'), sidecar);
  }
});

test("qualification proves installed native Adapt selected-transcript lifecycle", () => {
  for (const term of [
    "Invoke-InstalledAdaptQualification",
    "adapt.user-taste-review.v1",
    "review-contract.json",
    "local-user-review",
    "pending_manifest_sha256",
    "selected-transcript.jsonl",
    "adapt mine --host pi --scope workspace",
    "adapt review --input",
    "review-taste --input",
    "adapt adjudicate-taste --manifest",
    "apply --manifest",
    "recall npm --scope workspace",
    "candidate_set_sha256",
    "sourceBindings",
    "nativeOnly = $true",
    "Python, Pi, OpenCode, and Node are absent",
    "python = $false",
    "pi = $false",
    "openCode = $false",
    "node = $false",
    "checkout = $false",
  ]) assert.ok(source.includes(term), term);
  assert.doesNotMatch(lower, /adapt\s+--db/);
  assert.doesNotMatch(lower, /adapt-installed-qualification/);
  assert.match(lower, /qualificationworkspace[\s\S]*tools\\.cache\\memory\\cortex-engine.db/);
  assert.match(lower, /adapt\s*=\s*\$script:adaptevidence/);
  assert.match(lower, /lifecycle[\s\S]*adapt\s*=\s*'pass'/);
  assert.match(lower, /caller-selected/);
  assert.match(lower, /source.*bindings/);
});

test("qualification lifecycle output matches platform & native-only seal consumers", () => {
  for (const field of [
    "install", "startup", "hubHealth", "tray", "popup", "renderer", "mcp17",
    "nativeHostCutover", "blueprintHubHosted", "blueprintHubOffOneShot", "downgrade",
    "upgrade", "stateContinuity", "uninstall", "residue", "nativeOnlyProcessTree",
    "runtimeInventory", "currentRootActivation", "doctor", "hubOffManualBlueprint",
    "residentFileChangeRefresh", "zeroInterpreterProcessTree",
  ]) {
    assert.match(source, new RegExp(`${field}\\s*=\\s*(?:if \\(\\$previousPath\\) \\{ )?'pass'`));
  }
  assert.match(source, /blueprint\s*=\s*\[ordered\]@\{[\s\S]*hubOwned\s*=\s*\(\$script:UpgradeEvidence\.Blueprint\.hubOwned\s*-eq\s*\$true\)/);
  assert.match(source, /blueprint\s*=\s*\[ordered\]@\{[\s\S]*nativeOnly\s*=\s*\(\$script:UpgradeEvidence\.Health\.nativeOnly\s*-eq\s*\$true\)/);
});

test("final-holder isolation waits for peers & rejects replaced controller", () => {
  const powershell = String.raw`
$ErrorActionPreference = 'Stop'
$tokens = $null; $parseErrors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($env:MEMBRANE_QUALIFICATION_SOURCE, [ref]$tokens, [ref]$parseErrors)
$fn = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.Ast] -and $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Wait-ResidentHolderIsolation' }, $true)
if ($null -eq $fn) { throw 'Wait-ResidentHolderIsolation AST node missing' }
function Require([bool]$Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
function Read-NativeOutput([string]$Text, [string]$Label) { return ($Text | ConvertFrom-Json) }
function Start-Sleep { param([int]$Milliseconds) }
function Get-Process { param([int]$Id) return [pscustomobject]@{ HasExited = $false } }
function Invoke-NativeProcess {
  param([string]$Executable, [string]$Arguments, [string]$InputText, [string]$WorkingDirectory)
  $next = $script:Responses[[Math]::Min($script:ResponseIndex, $script:Responses.Count - 1)]
  $script:ResponseIndex++
  return [pscustomobject]@{ Stdout = $next; Stderr = ''; ExitCode = 0 }
}
. ([scriptblock]::Create($fn.Extent.Text))
$script:ActiveHubHealth = [pscustomobject]@{ installationId='install'; cortexStoreId='store'; releaseGeneration='sha256:release'; startupGeneration=7; stableInstallRoot='C:\Membrane\current' }
$InstallRoot = 'C:\Membrane\current'; $script:ActiveHubPort = 47851; $TimeoutSeconds = 0
$controller = @{ installationId='install'; cortexStoreId='store'; releaseGeneration='sha256:release'; startupGeneration=7; stableCurrent='C:\Membrane\current' }
function Body([int]$Hub, [hashtable]$ResponseController = $controller, [int]$Harness = 1) {
  return ([ordered]@{ operation='status'; controller=$ResponseController; status=[ordered]@{ controllerActive=$true; servicesReady=$true; hubHolders=$Hub; coderightDaemonHolders=0; harnessHolders=$Harness } } | ConvertTo-Json -Compress -Depth 8)
}
switch ($env:MEMBRANE_ISOLATION_CASE) {
  'peers-then-sole' { $script:Responses=@((Body 2),(Body 1)); $TimeoutSeconds=1; $result=Wait-ResidentHolderIsolation 'membrane.exe' 123; if ([int]$result.status.hubHolders -ne 1) { throw 'sole-holder result missing' } }
  'peer-timeout' { $script:Responses=@((Body 1 $controller 2)); try { Wait-ResidentHolderIsolation 'membrane.exe' 123; throw 'peer timeout unexpectedly passed' } catch { if ($_.Exception.Message -notmatch 'final_holder_isolation_failed') { throw } } }
  'controller-replaced' { $replacement=@{ installationId='install'; cortexStoreId='store'; releaseGeneration='sha256:release'; startupGeneration=8; stableCurrent='C:\Membrane\current' }; $script:Responses=@((Body 1 $replacement)); try { Wait-ResidentHolderIsolation 'membrane.exe' 123; throw 'replacement unexpectedly passed' } catch { if ($_.Exception.Message -notmatch 'final_holder_isolation_failed') { throw } } }
  default { throw 'unknown isolation case' }
}
Write-Output 'PASS'
`;
  for (const isolationCase of ["peers-then-sole", "peer-timeout", "controller-replaced"]) {
    const run = spawnSync("pwsh", ["-NoProfile", "-NonInteractive", "-Command", powershell], {
      encoding: "utf8",
      env: { ...process.env, MEMBRANE_QUALIFICATION_SOURCE: new URL("./install-release.ps1", import.meta.url).pathname.replace(/^\/(?:[A-Za-z]:)/, m => m.slice(1)), MEMBRANE_ISOLATION_CASE: isolationCase },
    });
    assert.equal(run.status, 0, `${isolationCase}: ${run.stdout}\n${run.stderr}`);
    assert.match(run.stdout, /PASS/);
  }
});

test("shutdown process cleanup matches PID plus creation identity", () => {
  const powershell = String.raw`
$ErrorActionPreference = 'Stop'
$tokens = $null; $parseErrors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($env:MEMBRANE_QUALIFICATION_SOURCE, [ref]$tokens, [ref]$parseErrors)
$names = @('Get-ProcessTree','Assert-QualificationProcessTreeGone')
foreach ($name in $names) { $node = $ast.Find({ param($n) $n -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name }, $true); if ($null -eq $node) { throw "$name AST node missing" }; . ([scriptblock]::Create($node.Extent.Text)) }
function Require([bool]$Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
function Start-Sleep { param([int]$Milliseconds) }
function Get-CimInstance {
  param([string]$Class, [string]$Filter)
  if ($script:Mode -eq 'reused') { return [pscustomobject]@{ ProcessId=7; CreationDate=[DateTime]::Parse('2026-09-21T04:32:24.200Z'); Name='svchost.exe' } }
  if ($script:Mode -eq 'same') { return [pscustomobject]@{ ProcessId=7; CreationDate=[DateTime]::Parse('2026-09-21T04:32:24.100Z'); Name='membrane.exe' } }
  return @(
    [pscustomobject]@{ ProcessId=1; ParentProcessId=0; CreationDate=[DateTime]::Parse('2026-09-21T04:32:24Z'); Name='membrane-tray.exe' },
    [pscustomobject]@{ ProcessId=2; ParentProcessId=1; CreationDate=[DateTime]::Parse('2026-09-21T04:32:23Z'); Name='svchost.exe' },
    [pscustomobject]@{ ProcessId=3; ParentProcessId=1; CreationDate=[DateTime]::Parse('2026-09-21T04:32:25Z'); Name='membrane.exe' }
  )
}
$TimeoutSeconds=0
$script:Mode='reused'; Assert-QualificationProcessTreeGone @([pscustomobject]@{ProcessId=7;CreationDate=[DateTime]::Parse('2026-09-21T04:32:24.100Z')})
$script:Mode='same'; try { Assert-QualificationProcessTreeGone @([pscustomobject]@{ProcessId=7;CreationDate=[DateTime]::Parse('2026-09-21T04:32:24.100Z')}); throw 'same identity unexpectedly passed' } catch { if ($_.Exception.Message -notmatch 'Hub process descendants remain') { throw } }
$script:Mode='tree'; $tree=@(Get-ProcessTree 1); if (@($tree.ProcessId) -notcontains 3 -or @($tree.ProcessId) -contains 2) { throw 'process tree creation filtering failed' }
Write-Output 'PASS'
`;
  const run = spawnSync("pwsh", ["-NoProfile", "-NonInteractive", "-Command", powershell], {
    encoding: "utf8",
    env: { ...process.env, MEMBRANE_QUALIFICATION_SOURCE: new URL("./install-release.ps1", import.meta.url).pathname.replace(/^\/(?:[A-Za-z]:)/, m => m.slice(1)) },
  });
  assert.equal(run.status, 0, `${run.stdout}\n${run.stderr}`);
  assert.match(run.stdout, /PASS/);
});

test("qualification proves startup workspace migration is native, strict, atomic, & idempotent", () => {
  for (const term of [
    "PreviousMembraneWorkspaceConfig", "MEMBRANE_WORKSPACE_CONFIG", "Seed-WorkspaceV2Config",
    "schemaVersion = 2", "pythonExecutable", "Assert-WorkspaceConfigMigrated",
    "schemaVersion -eq 3", "pythonExecutable", "workspace config migration left temporary files",
    "workspace-config-v2-to-v3-startup-migration-v1", "upgradeIdempotent", "upgradeSha256",
  ]) assert.ok(source.includes(term), term);
  assert.match(lower, /workspace config hash changed during \$phase/);
  assert.match(lower, /workspaceconfiginitialsha256/);
  assert.match(lower, /tools\\lib\\memory/);
  assert.match(lower, /serviceid = 'membrane-local-v1'/);
  assert.match(lower, /runtime\.json/);
});

test("installer failure collects the NSIS install-step log instead of extracting a payload", () => {
  for (const term of [
    "Save-InstallerFailureEvidence",
    "RIGHT_GIT_QUALIFICATION_EVIDENCE_ROOT",
    "installer-failure.log",
    "installer-failure.json",
    "membrane.installer-failure.v1",
    "install-$($Version.TrimStart('v')).log",
    "nsisExitCode = $ExitCode",
  ]) assert.ok(source.includes(term), term);
  // The evidence is bound to the exact version under test, never the newest log.
  assert.doesNotMatch(source, /Filter 'install-\*\.log'/);
  assert.match(source, /Join-Path \$env:LOCALAPPDATA 'Orthic Labs\\Membrane\\logs'/);
  assert.doesNotMatch(lower, /7z\.exe|7-zip|expand the installer|-releaseroot `"\$releaseroot`"/);
  assert.match(lower, /if \(\$process\.exitcode -ne 0\) \{/);
  assert.match(lower, /payload log: \$logpath/);
});

test("qualification binds Blueprint requests to Hub-enrolled workspace & typed one-shot states", () => {
  assert.match(source, /PreviousMembraneWorkspaceRoot/);
  assert.match(source, /MEMBRANE_WORKSPACE_ROOT\s*=\s*\$script:QualificationWorkspace/);
  assert.ok(source.indexOf("$env:MEMBRANE_WORKSPACE_ROOT = $script:QualificationWorkspace") < source.indexOf("Start-AndVerifyHub 'initial install'"));
  assert.match(lower, /enrollment = 'native'/);
  assert.match(lower, /hubowned = \$true/);
  assert.match(lower, /typedmissing/);
  assert.match(lower, /root_not_enrolled/);
  assert.match(lower, /graph_missing/);
  assert.match(lower, /untyped status/);
  assert.match(lower, /availability\s*=\s*if/);
  assert.match(lower, /state\s*=\s*if \(\$state\)/);
});
