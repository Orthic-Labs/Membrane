# Fail closed when a packaged desktop host virtualizes LOCALAPPDATA writes.
function Test-NativeLocalAppDataPath {
  param([Parameter(Mandatory = $true)][string]$Expected, [Parameter(Mandatory = $true)][string]$Actual)
  $expectedFull = [IO.Path]::GetFullPath($Expected)
  $actualFull = [IO.Path]::GetFullPath($Actual)
  if ($expectedFull.StartsWith('\\?\UNC\', [StringComparison]::OrdinalIgnoreCase)) { $expectedFull = '\\' + $expectedFull.Substring(8) }
  elseif ($expectedFull.StartsWith('\\?\', [StringComparison]::OrdinalIgnoreCase)) { $expectedFull = $expectedFull.Substring(4) }
  if ($actualFull.StartsWith('\\?\UNC\', [StringComparison]::OrdinalIgnoreCase)) { $actualFull = '\\' + $actualFull.Substring(8) }
  elseif ($actualFull.StartsWith('\\?\', [StringComparison]::OrdinalIgnoreCase)) { $actualFull = $actualFull.Substring(4) }
  $expectedFull = $expectedFull.TrimEnd('\')
  $actualFull = $actualFull.TrimEnd('\')
  return $expectedFull.Equals($actualFull, [StringComparison]::OrdinalIgnoreCase)
}

if (-not ('Membrane.NativePathProbe' -as [type])) {
  Add-Type @"
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
namespace Membrane {
  public static class NativePathProbe {
    const uint OPEN_EXISTING = 3;
    const uint FILE_FLAG_BACKUP_SEMANTICS = 0x02000000;
    static readonly IntPtr InvalidHandle = new IntPtr(-1);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern IntPtr CreateFile(string name, uint access, uint share, IntPtr security, uint creation, uint flags, IntPtr template);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern uint GetFinalPathNameByHandle(IntPtr handle, System.Text.StringBuilder path, uint length, uint flags);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool CloseHandle(IntPtr handle);
    public static string ResolveDirectory(string path) {
      var handle = CreateFile(path, 0, 7, IntPtr.Zero, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS, IntPtr.Zero);
      if (handle == InvalidHandle) throw new Win32Exception(Marshal.GetLastWin32Error());
      try {
        var buffer = new System.Text.StringBuilder(512);
        var length = GetFinalPathNameByHandle(handle, buffer, (uint)buffer.Capacity, 0);
        if (length == 0) throw new Win32Exception(Marshal.GetLastWin32Error());
        if (length >= buffer.Capacity) {
          buffer = new System.Text.StringBuilder((int)length + 1);
          length = GetFinalPathNameByHandle(handle, buffer, (uint)buffer.Capacity, 0);
        }
        if (length == 0 || length >= buffer.Capacity) throw new Win32Exception(Marshal.GetLastWin32Error());
        return buffer.ToString();
      } finally { CloseHandle(handle); }
    }
  }
}
"@
}

function Assert-NativeLocalAppData {
  param([string]$LocalAppData = $env:LOCALAPPDATA)
  if ([string]::IsNullOrWhiteSpace($LocalAppData)) { throw [InvalidOperationException]::new('LOCALAPPDATA is unavailable; run from native Windows desktop PowerShell.') }
  $expectedParent = [IO.Path]::GetFullPath((Join-Path $LocalAppData 'Orthic Labs'))
  $probe = Join-Path $expectedParent ('.membrane-native-path-probe-' + [guid]::NewGuid().ToString('N'))
  $created = $false
  try {
    New-Item -ItemType Directory -Path $probe | Out-Null
    $created = $true
    $actualProbe = [Membrane.NativePathProbe]::ResolveDirectory($probe)
    $actualParent = [IO.Directory]::GetParent($actualProbe).FullName
    if (-not (Test-NativeLocalAppDataPath $expectedParent $actualParent)) {
      throw [InvalidOperationException]::new("Membrane requires native LOCALAPPDATA access. Path resolves to '$actualParent' instead of '$expectedParent'. Run this command from native Windows desktop PowerShell, outside packaged Codex.")
    }
  } finally {
    if ($created -and (Test-Path -LiteralPath $probe -PathType Container)) {
      Remove-Item -LiteralPath $probe -Force
      if (Test-Path -LiteralPath $probe) { throw [IOException]::new("native LOCALAPPDATA probe cleanup failed: $probe") }
    }
  }
}
