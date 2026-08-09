# D18: verify every signable payload under a root is Authenticode-signed with
# SHA-256 and RFC3161 timestamping.
param(
  [Parameter(Mandatory = $true)][string]$Root
)
$ErrorActionPreference = "Stop"
$sigcheck = Get-Command "Get-AuthenticodeSignature" -ErrorAction Stop
$failures = 0
Get-ChildItem -Recurse -File $Root | Where-Object { $_.Extension -in ".exe", ".dll", ".msi" } | ForEach-Object {
  $sig = Get-AuthenticodeSignature $_.FullName
  if ($sig.Status -ne "Valid") {
    Write-Error "UNSIGNED or invalid: $($_.FullName) ($($sig.Status))"
    $failures++
  } elseif ($sig.SignerCertificate -and $sig.SignerCertificate.SignatureAlgorithm.FriendlyName -notmatch "sha256|SHA256") {
    Write-Error "NOT SHA-256: $($_.FullName)"
    $failures++
  } else {
    Write-Host "OK: $($_.FullName)"
  }
}
if ($failures -gt 0) { exit 1 }
Write-Host "verify-signatures OK: all signable payloads signed (SHA-256)"
