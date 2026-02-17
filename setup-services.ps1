# Script to disable auto-start for PostgreSQL and Docker services

Write-Host "Disabling auto-start for PostgreSQL service..." -ForegroundColor Cyan

# Find PostgreSQL service
$postgresService = Get-Service -Name "*postgresql*" -ErrorAction SilentlyContinue | Select-Object -First 1

if ($postgresService) {
    Write-Host "Found PostgreSQL service: $($postgresService.Name)" -ForegroundColor Green
    # Stop the service if running
    if ($postgresService.Status -eq 'Running') {
        Stop-Service -Name $postgresService.Name -Force
        Write-Host "Stopped PostgreSQL service" -ForegroundColor Yellow
    }
    # Set to manual start
    Set-Service -Name $postgresService.Name -StartupType Manual
    Write-Host "Set PostgreSQL service to Manual (no auto-start)" -ForegroundColor Green
} else {
    Write-Host "PostgreSQL service not found yet. Run this script after PostgreSQL installation completes." -ForegroundColor Yellow
}

Write-Host "`nFor Docker Desktop:" -ForegroundColor Cyan
Write-Host "1. Open Docker Desktop" -ForegroundColor White
Write-Host "2. Go to Settings > General" -ForegroundColor White
Write-Host "3. Uncheck 'Start Docker Desktop when you log in'" -ForegroundColor White
