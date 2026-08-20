@echo off
setlocal
node "%~dp0blueprint.mjs" %*
exit /b %ERRORLEVEL%
