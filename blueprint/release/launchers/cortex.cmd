@echo off
setlocal
set "ROOT=%~dp0.."
"%ROOT%\lib\node.exe" "%ROOT%\app\package\scripts\cortex.mjs" %*
exit /b %ERRORLEVEL%
