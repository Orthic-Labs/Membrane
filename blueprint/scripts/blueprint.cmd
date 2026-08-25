@echo off
setlocal EnableExtensions
rem Normalize the script directory (strip the trailing backslash %~dp0 adds)
rem so quoted invocation survives paths with spaces.
set "SCRIPT_DIR=%~dp0"
if "%SCRIPT_DIR:~-1%"=="\" set "SCRIPT_DIR=%SCRIPT_DIR:~0,-1%"
node "%SCRIPT_DIR%\blueprint.mjs" %*
endlocal & exit /b %ERRORLEVEL%
