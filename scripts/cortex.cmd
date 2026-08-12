@echo off
setlocal
node "%~dp0cortex.mjs" %*
exit /b %ERRORLEVEL%
