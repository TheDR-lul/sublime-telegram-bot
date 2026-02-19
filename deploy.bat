@echo off
cd /d "%~dp0"
if "%~1"=="ssh-key" (
    powershell -ExecutionPolicy Bypass -File "%~dp0setup-ssh-key.ps1"
) else (
    powershell -ExecutionPolicy Bypass -File "%~dp0deploy.ps1" %*
)
if errorlevel 1 pause
