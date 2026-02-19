@echo off
cd /d "%~dp0"
powershell -ExecutionPolicy Bypass -File "%~dp0update.ps1" %*
if errorlevel 1 pause
