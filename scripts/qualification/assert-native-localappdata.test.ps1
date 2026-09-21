$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'assert-native-localappdata.ps1')

if (-not (Test-NativeLocalAppDataPath 'C:\Users\test\AppData\Local\Orthic Labs' 'c:\users\TEST\appdata\local\orthic labs')) { throw 'matching native path rejected' }
if (Test-NativeLocalAppDataPath 'C:\Users\test\AppData\Local\Orthic Labs' 'C:\Users\test\AppData\Local\Packages\OpenAI.Codex\LocalCache\Local\Orthic Labs') { throw 'redirected path accepted' }

$probe = Join-Path ([IO.Path]::GetTempPath()) ('.membrane-native-probe-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $probe | Out-Null
try {
  $resolved = [Membrane.NativePathProbe]::ResolveDirectory($probe)
  if (-not (Test-NativeLocalAppDataPath $probe $resolved)) { throw "native handle resolution mismatch: $resolved" }
} finally {
  if (Test-Path -LiteralPath $probe -PathType Container) {
    Remove-Item -LiteralPath $probe -Force
    if (Test-Path -LiteralPath $probe) { throw 'probe cleanup failed' }
  }
}

$release = Get-Content -Raw (Join-Path $PSScriptRoot '..\release\local-windows-development.ps1')
$qualification = Get-Content -Raw (Join-Path $PSScriptRoot 'install-release.ps1')
$releaseGuard = $release.IndexOf('Assert-NativeLocalAppData', [StringComparison]::Ordinal)
$qualificationGuard = $qualification.IndexOf('Assert-NativeLocalAppData', [StringComparison]::Ordinal)
if ($releaseGuard -lt 0 -or $releaseGuard -gt $release.IndexOf('& pnpm.cmd', [StringComparison]::Ordinal)) { throw 'release guard missing or runs after build' }
if ($qualificationGuard -lt 0 -or $qualificationGuard -gt $qualification.IndexOf('Add-Type -AssemblyName System.Net.Http', [StringComparison]::Ordinal)) { throw 'qualification guard missing or runs after setup' }
'assert-native-localappdata tests passed'
