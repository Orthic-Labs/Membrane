@echo off
setlocal
set "BLUEPRINT_ROOT=%~dp0.."
"%BLUEPRINT_ROOT%\lib\node.exe" "%BLUEPRINT_ROOT%\app\package\scripts\blueprint.mjs" %*
exit /b %ERRORLEVEL%
