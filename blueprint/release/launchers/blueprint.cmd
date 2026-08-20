@echo off
setlocal
set "ROOT=%~dp0.."
"%ROOT%\lib\node.exe" "%ROOT%\app\package\scripts\blueprint.mjs" %*
exit /b %ERRORLEVEL%
