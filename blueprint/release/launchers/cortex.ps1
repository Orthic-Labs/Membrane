#!/usr/bin/env pwsh
# D14: Windows PowerShell launcher — computes its own install root and
# invokes the bundled runtime; never uses global Node/npm/cwd.
$ErrorActionPreference = "Stop"
$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
& (Join-Path $Root "lib\node.exe") (Join-Path $Root "app\package\scripts\cortex.mjs") @args
exit $LASTEXITCODE
