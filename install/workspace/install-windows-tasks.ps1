param(
    [string]$WorkspaceRoot = $(if ($env:WORKSPACE_ROOT) { $env:WORKSPACE_ROOT } else { (Resolve-Path (Join-Path $PSScriptRoot "..\..\..")).Path }),
    [string]$TaskFolder = "\Membrane\Workspace\",
    [string]$ServeTaskName = "crypt-serve",
    [string]$DailyTaskName = "crypt-daily",
    [switch]$EnableDaily,
    [switch]$InstallCryptAliases
)

$ErrorActionPreference = "Stop"

if ($env:OS -ne "Windows_NT") {
    throw "Windows Task Scheduler registration is only supported on Windows."
}

$powerShell = (Get-Command pwsh.exe -ErrorAction SilentlyContinue).Source
if (-not $powerShell) {
    $powerShell = (Get-Command powershell.exe -ErrorAction Stop).Source
}
$gitBash = "C:\Program Files\Git\bin\bash.exe"
$serveScript = Join-Path $WorkspaceRoot "tools\pipelines\memory\crypt-serve.ps1"
$serviceBinary = Join-Path $WorkspaceRoot "tools\bin\crypt-service.exe"
$dailyRunner = Join-Path $WorkspaceRoot "tools\pipelines\memory\run-daily-sync.ps1"
# run-hidden.vbs stays in Adrian's private tree (tools/pipelines/memory), not Membrane, so it is
# addressed relative to -WorkspaceRoot rather than $PSScriptRoot now that this installer lives
# under membrane/install/workspace.
$hiddenLauncher = Join-Path $WorkspaceRoot "tools\pipelines\memory\run-hidden.vbs"
$syncRepo = Join-Path $env:LOCALAPPDATA "Crypt\sync-repo"

if (-not (Test-Path -LiteralPath $gitBash -PathType Leaf)) {
    throw "Git Bash not found: $gitBash"
}
if (-not (Test-Path -LiteralPath $serveScript -PathType Leaf)) {
    throw "Crypt service launcher not found: $serveScript"
}
if (-not (Test-Path -LiteralPath $serviceBinary -PathType Leaf)) {
    throw "Console-free Crypt service binary not found: $serviceBinary"
}
if (-not (Test-Path -LiteralPath $dailyRunner -PathType Leaf)) {
    throw "Crypt daily runner not found: $dailyRunner"
}

$currentUser = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
$servePrincipal = New-ScheduledTaskPrincipal -UserId $currentUser -LogonType Interactive -RunLevel Limited
$dailyPrincipal = New-ScheduledTaskPrincipal -UserId $currentUser -LogonType Interactive -RunLevel Limited

# Prepare the token/ACL once.
& $powerShell -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $serveScript -PrepareTokenOnly | Out-Null
# The federation gateway (engine/federation/gateway.py) writes candidate content that routinely
# contains non-latin1 characters (e.g. the U+2192 arrow). Windows Python defaults stdout to cp1252,
# so an unforced write raises UnicodeEncodeError and kills the gateway worker, opening the /federate
# circuit. Enable Python UTF-8 Mode for the service (and therefore its gateway subprocess) by
# launching it through the hidden wscript wrapper with PYTHONUTF8=1 set in the process environment
# — the same no-console launcher the daily task uses. A user/logon env var does NOT reach a task
# already running in the current session, and this keeps the fix ENTIRELY outside the pinned engine
# source tree, so the release generation is unchanged.
# crypt-service.exe is a GUI-subsystem binary, so PowerShell's call operator (`&`) does NOT block
# on it — the wrapper would exit immediately and orphan the service, flipping the task to Ready.
# Start-Process -Wait blocks even on a GUI-subsystem process, so wscript -> pwsh stays alive for the
# lifetime of the service and Task Scheduler keeps the task Running (which the qualifier requires).
$serveAction = New-ScheduledTaskAction -Execute "$env:SystemRoot\System32\wscript.exe" -Argument (
    '//B //Nologo "{0}" "{1}" "-NoLogo" "-NoProfile" "-NonInteractive" "-WindowStyle" "Hidden" "-Command" "$env:PYTHONUTF8=''1''; Start-Process -FilePath ''{2}'' -WorkingDirectory ''{3}'' -Wait -NoNewWindow"' -f $hiddenLauncher, $powerShell, $serviceBinary, $WorkspaceRoot
) -WorkingDirectory $WorkspaceRoot
$serveTrigger = New-ScheduledTaskTrigger -AtLogOn -User $currentUser
$serveSettings = New-ScheduledTaskSettingsSet `
    -MultipleInstances IgnoreNew `
    -RestartCount 5 `
    -RestartInterval (New-TimeSpan -Minutes 1) `
    -ExecutionTimeLimit ([TimeSpan]::Zero) `
    -StartWhenAvailable `
    -AllowStartIfOnBatteries `
    -DontStopIfGoingOnBatteries

Register-ScheduledTask `
    -TaskName $ServeTaskName `
    -TaskPath $TaskFolder `
    -Action $serveAction `
    -Trigger $serveTrigger `
    -Settings $serveSettings `
    -Principal $servePrincipal `
    -Description "Resident loopback Crypt recall service" `
    -Force | Out-Null

# Register-ScheduledTask -Force can terminate an existing long-running instance. Setup is an
# idempotent wiring operation, so it must also restore the resident daemon before returning.
if ((Get-ScheduledTask -TaskName $ServeTaskName -TaskPath $TaskFolder).State -ne "Running") {
    Start-ScheduledTask -TaskName $ServeTaskName -TaskPath $TaskFolder
}

# CLAUDE.md 4A: launch through the wscript wrapper so no console window is ever allocated.
# powershell -WindowStyle Hidden under an Interactive-logon task still flashes a console.
$dailyAction = New-ScheduledTaskAction -Execute "$env:SystemRoot\System32\wscript.exe" -Argument (
    '//B //Nologo "{0}" "{1}" "-NoLogo" "-NoProfile" "-NonInteractive" "-WindowStyle" "Hidden" "-ExecutionPolicy" "Bypass" "-File" "{2}" "-WorkspaceRoot" "{3}" "-SyncRepo" "{4}"' -f $hiddenLauncher, $powerShell, $dailyRunner, $WorkspaceRoot, $syncRepo
) -WorkingDirectory $WorkspaceRoot
$dailyTrigger = New-ScheduledTaskTrigger -Daily -At 10:00
$dailySettings = New-ScheduledTaskSettingsSet `
    -MultipleInstances IgnoreNew `
    -RestartCount 2 `
    -RestartInterval (New-TimeSpan -Minutes 5) `
    -ExecutionTimeLimit (New-TimeSpan -Minutes 30) `
    -StartWhenAvailable `
    -AllowStartIfOnBatteries `
    -DontStopIfGoingOnBatteries

Register-ScheduledTask `
    -TaskName $DailyTaskName `
    -TaskPath $TaskFolder `
    -Action $dailyAction `
    -Trigger $dailyTrigger `
    -Settings $dailySettings `
    -Principal $dailyPrincipal `
    -Description "Transactional Crypt curation, replication, and metrics refresh" `
    -Force | Out-Null
if (-not $EnableDaily) {
    Disable-ScheduledTask -TaskName $DailyTaskName -TaskPath $TaskFolder | Out-Null
}

if ($InstallCryptAliases) {
    Register-ScheduledTask -TaskName "crypt-serve" -TaskPath $TaskFolder -Action $serveAction -Trigger $serveTrigger -Settings $serveSettings -Principal $servePrincipal -Description "Legacy Crypt compatibility service alias" -Force | Out-Null
    Register-ScheduledTask -TaskName "crypt-daily" -TaskPath $TaskFolder -Action $dailyAction -Trigger $dailyTrigger -Settings $dailySettings -Principal $dailyPrincipal -Description "Legacy Crypt compatibility daily alias" -Force | Out-Null
    Disable-ScheduledTask -TaskName "crypt-serve", "crypt-daily" -TaskPath $TaskFolder | Out-Null
}

$legacyVbs = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\Startup\crypt-serve.vbs"
if (Test-Path -LiteralPath $legacyVbs) {
    Remove-Item -LiteralPath $legacyVbs -Force
}

Get-ScheduledTask -TaskName $ServeTaskName, $DailyTaskName -TaskPath $TaskFolder |
    Select-Object TaskName, State, TaskPath
