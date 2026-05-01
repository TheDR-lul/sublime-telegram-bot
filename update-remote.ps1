# Update when Docker is not available locally: copy source to server, backup DB, build on server, migrate, restart.
# Run: .\update-remote.ps1
param(
    [Parameter(Mandatory = $false)]
    [string]$TelegramBotToken,
    [Parameter(Mandatory = $false)]
    [string]$NotificationBotToken,
    [Parameter(Mandatory = $false)]
    [string]$NotificationChatId
)

$ErrorActionPreference = "Stop"
$RemoteDir    = "/root/sublime-deploy"
. "$PSScriptRoot\scripts\DeploySshTarget.ps1"
$Target       = Get-SublimeSshTarget
$SshOpts      = "-o StrictHostKeyChecking=accept-new"
$LocalBackups = "backups"

if (-not $TelegramBotToken -and $env:TELEGRAM_BOT_TOKEN) { $TelegramBotToken = $env:TELEGRAM_BOT_TOKEN }
if (-not $NotificationBotToken -and $env:NOTIFICATION_BOT_TOKEN) { $NotificationBotToken = $env:NOTIFICATION_BOT_TOKEN }
if (-not $NotificationChatId -and $env:NOTIFICATION_CHAT_ID) { $NotificationChatId = $env:NOTIFICATION_CHAT_ID }
if (Test-Path "config.toml") {
    $config = Get-Content "config.toml" -Raw
    if (-not $TelegramBotToken -and $config -match 'telegram_token\s*=\s*"([^"]+)"') { $TelegramBotToken = $Matches[1] }
    if (-not $NotificationBotToken -and $config -match 'notification_bot_token\s*=\s*"([^"]+)"') { $NotificationBotToken = $Matches[1] }
    if (-not $NotificationChatId -and $config -match 'notification_chat_id\s*=\s*"([^"]+)"') { $NotificationChatId = $Matches[1] }
}
if (-not $TelegramBotToken) {
    Write-Error "Set TelegramBotToken (param, env TELEGRAM_BOT_TOKEN, or config.toml)"
    exit 1
}

function Send-TelegramNotification {
    param(
        [string]$Text
    )
    try {
        $botToken = if ($NotificationBotToken) { $NotificationBotToken } else { $TelegramBotToken }
        if (-not $botToken -or -not $NotificationChatId) {
            return
        }
        $body = @{
            chat_id = $NotificationChatId
            text    = $Text
        }
        Invoke-RestMethod -Method Post -Uri "https://api.telegram.org/bot$botToken/sendMessage" -Body $body -ErrorAction SilentlyContinue | Out-Null
    } catch {
        Write-Host "Warning: failed to send Telegram notification: $($_.Exception.Message)" -ForegroundColor DarkYellow
    }
}

Write-Host "=== REMOTE UPDATE (no local Docker: backup -> copy source -> build on server -> migrate -> verify) ===" -ForegroundColor Green
Write-Host ""

Send-TelegramNotification "sublime: remote update started at $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"

# 0. Sync scripts
Write-Host "== 0. Sync scripts to server ==" -ForegroundColor Cyan
ssh $SshOpts $Target "mkdir -p $RemoteDir $RemoteDir/backups"
scp $SshOpts "scripts/remote-backup-db.sh" "scripts/remote-update.sh" "scripts/notify-update-success.sh" "scripts/fix_checksum.sql" "${Target}:${RemoteDir}/"
ssh $SshOpts $Target "chmod +x $RemoteDir/remote-backup-db.sh $RemoteDir/remote-update.sh $RemoteDir/notify-update-success.sh"
if (Test-Path "scripts/watchdog-telegram.sh") { scp $SshOpts "scripts/watchdog-telegram.sh" "${Target}:${RemoteDir}/" }

# 1. Backup DB on server and download
Write-Host "== 1/4. Backup DB on server ==" -ForegroundColor Cyan
ssh $SshOpts $Target "cd $RemoteDir && ./remote-backup-db.sh"
$latestBackup = (ssh $SshOpts $Target "ls -t $RemoteDir/backups/sublime_db_backup_*.sql 2>/dev/null | head -1").Trim()
if ($latestBackup) {
    New-Item -ItemType Directory -Force -Path $LocalBackups | Out-Null
    $localName = [System.IO.Path]::GetFileName($latestBackup)
    scp $SshOpts "${Target}:${latestBackup}" "${LocalBackups}/${localName}"
    Write-Host "  Downloaded to $LocalBackups\$localName" -ForegroundColor DarkGray
}

# 2. Copy full source to server
Write-Host "== 2/4. Copy source to server ==" -ForegroundColor Cyan
scp $SshOpts "Cargo.toml" "Cargo.lock" "Dockerfile" "config.toml" "config.toml.example" "docker-compose.deploy.yml" "${Target}:${RemoteDir}/"
scp $SshOpts -r "src" "migrations" "locale" "${Target}:${RemoteDir}/"

# Write .env.watchdog on server before remote-update so watchdog can start
if ($NotificationBotToken -or $NotificationChatId) {
    Write-Host "  Updating .env.watchdog on server" -ForegroundColor DarkGray
    $lines = @()
    if ($NotificationBotToken) {
        $escapedToken = $NotificationBotToken -replace "'", "'\''"
        $lines += "export NOTIFICATION_BOT_TOKEN='$escapedToken'"
    }
    if ($NotificationChatId) {
        $lines += "export ALERT_CHAT_ID='$NotificationChatId'"
    }
    if ($lines.Count -gt 0) {
        $watchdogEnv = ($lines -join "`n") + "`n"
        $tempFile = [System.IO.Path]::GetTempFileName()
        [System.IO.File]::WriteAllText($tempFile, $watchdogEnv)
        scp $SshOpts $tempFile "${Target}:${RemoteDir}/.env.watchdog"
        ssh $SshOpts $Target "chmod 600 $RemoteDir/.env.watchdog"
        Remove-Item $tempFile -ErrorAction SilentlyContinue
    }
}

# 3. On server: build, up, migrate, commands (using .env for token)
Write-Host "== 3/4. On server: build image, restart, migrate, set commands ==" -ForegroundColor Cyan
ssh $SshOpts $Target "cd $RemoteDir && . .env 2>/dev/null; export TELEGRAM_BOT_TOKEN; BUILD_ON_SERVER=1 ./remote-update.sh"
if ($LASTEXITCODE -ne 0) { Write-Error "Remote update failed"; exit 1 }

# 4. Verify
Write-Host "== 4/4. Verify ==" -ForegroundColor Cyan
$psOut = ssh $SshOpts $Target "docker ps --filter name=sublime --format '{{.Names}} {{.Status}}'; echo '---'; docker logs sublime-bot --tail 5 2>&1"
$lines = $psOut -split "`n"
$botLine = $lines | Where-Object { $_ -match "sublime-bot" } | Select-Object -First 1
$watchdogLine = $lines | Where-Object { $_ -match "sublime-watchdog" } | Select-Object -First 1
$logStart = [array]::IndexOf($lines, "---") + 1
$logLines = if ($logStart -gt 0) { $lines[$logStart..($lines.Length - 1)] } else { @() }
Write-Host "  $botLine"
if ($botLine -match "Up") { Write-Host "  Bot: Up" -ForegroundColor Green } else { Write-Host "  Bot: check logs" -ForegroundColor Yellow }
if ($watchdogLine) {
    Write-Host "  $watchdogLine"
    if ($watchdogLine -match "Up") { Write-Host "  Watchdog: Up" -ForegroundColor Green }
}
$apiCheck = ssh $SshOpts $Target "cd $RemoteDir && . .env 2>/dev/null; curl -sf \`"https://api.telegram.org/bot`$TELEGRAM_BOT_TOKEN/getMe\`" 2>/dev/null | grep -q '\`"ok\`":true' && echo 'Bot API: OK' || echo 'Bot API: check token'"
Write-Host "  $apiCheck" -ForegroundColor DarkGray
$logLines | ForEach-Object { Write-Host "    $_" }

Write-Host "`nRemote update finished. Backup in $LocalBackups\" -ForegroundColor Green

Send-TelegramNotification "sublime: remote update finished successfully at $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
