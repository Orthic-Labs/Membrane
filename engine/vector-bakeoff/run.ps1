param(
    [ValidateSet("smoke", "round1", "compare")]
    [string]$Mode = "smoke",
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$HarnessArgs
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$launchDir = Join-Path $root ".results\windows-launch"
New-Item -ItemType Directory -Force -Path $launchDir | Out-Null
$stdout = Join-Path $launchDir "stdout.log"
$stderr = Join-Path $launchDir "stderr.log"
$arguments = @("-3.11", (Join-Path $root "harness.py"), $Mode) + $HarnessArgs
$process = Start-Process -FilePath "py.exe" -ArgumentList $arguments -WorkingDirectory $root -WindowStyle Hidden -Wait -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
if (Test-Path $stdout) { Get-Content $stdout }
if ($process.ExitCode -ne 0) {
    if (Test-Path $stderr) { Get-Content $stderr }
    throw "Vector bake-off failed with exit code $($process.ExitCode)"
}
