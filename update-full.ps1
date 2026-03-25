# Full update pipeline: backup DB -> update app -> verify.
# Requires: Docker locally (for build), SSH to server, token in config.toml or env.
# Run: .\update-full.ps1
param(
    [Parameter(Mandatory = $false)]
    [string]$TelegramBotToken,
    [Parameter(Mandatory = $false)]
    [string]$NotificationBotToken,
    [Parameter(Mandatory = $false)]
    [string]$NotificationChatId
)

$ErrorActionPreference = "Stop"
$Server       = "5.189.154.71"
$User         = "root"
$RemoteDir    = "/root/sublime-deploy"
$Target       = "$User@$Server"
$SshOpts      = "-o StrictHostKeyChecking=accept-new"
$ImageName    = "sublime-bot:latest"
$ImageTar     = "sublime-bot.tar"
$LocalBackups = "backups"

# Resolve tokens and notification chat (same as update.ps1)
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

Write-Host "=== FULL UPDATE (backup -> update -> verify) ===" -ForegroundColor Green
Write-Host ""

Send-TelegramNotification "sublime: full update started at $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"

# --- 0. Copy scripts to server so backup/remote-update exist ---
Write-Host "== 0. Sync scripts to server ==" -ForegroundColor Cyan
ssh $SshOpts $Target "mkdir -p $RemoteDir $RemoteDir/backups"
scp $SshOpts "scripts/remote-backup-db.sh" "scripts/remote-update.sh" "scripts/notify-update-success.sh" "scripts/fix_checksum.sql" "${Target}:${RemoteDir}/"
ssh $SshOpts $Target "chmod +x $RemoteDir/remote-backup-db.sh $RemoteDir/remote-update.sh $RemoteDir/notify-update-success.sh"
if (Test-Path "scripts/watchdog-telegram.sh") { scp $SshOpts "scripts/watchdog-telegram.sh" "${Target}:${RemoteDir}/" }

# --- 1. Backup DB on server and download latest ---
Write-Host "== 1/5. Backup DB on server ==" -ForegroundColor Cyan
$backupResult = ssh $SshOpts $Target "cd $RemoteDir && ./remote-backup-db.sh"
Write-Host $backupResult
$latestBackup = (ssh $SshOpts $Target "ls -t $RemoteDir/backups/sublime_db_backup_*.sql 2>/dev/null | head -1").Trim()
if ($latestBackup) {
    New-Item -ItemType Directory -Force -Path $LocalBackups | Out-Null
    $localName = [System.IO.Path]::GetFileName($latestBackup)
    scp $SshOpts "${Target}:${latestBackup}" "${LocalBackups}/${localName}"
    Write-Host "  Downloaded to $LocalBackups\$localName" -ForegroundColor DarkGray
} else {
    Write-Host "  No backup file found (DB may be empty or script path wrong)" -ForegroundColor Yellow
}

# --- 2. Build image locally ---
Write-Host "== 2/5. Build Docker image locally ==" -ForegroundColor Cyan
docker build --no-cache -t $ImageName .
if ($LASTEXITCODE -ne 0) { Write-Error "Docker build failed"; exit 1 }

# --- 3. Save and copy to server ---
Write-Host "== 3/5. Save and copy image to server ==" -ForegroundColor Cyan
docker save $ImageName -o $ImageTar
if ($LASTEXITCODE -ne 0) { Write-Error "Docker save failed"; exit 1 }
scp $SshOpts $ImageTar "${Target}:${RemoteDir}/"
scp $SshOpts "docker-compose.deploy.yml" "${Target}:${RemoteDir}/"

# Write .env.watchdog on server before deploy so watchdog profile can use it
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

# --- 4. On server: load, up, migrate, commands, watchdog (if token set) ---
Write-Host "== 4/5. On server: load image, restart, migrate, set commands ==" -ForegroundColor Cyan
$tokenForBash = $TelegramBotToken -replace "'", "'\''"
$remoteCmd = @"
set -e
cd $RemoteDir
docker load -i $ImageTar
export TELEGRAM_BOT_TOKEN='$tokenForBash'
if [ -f .env.watchdog ]; then . .env.watchdog 2>/dev/null || true; fi
COMPOSE='docker compose -f docker-compose.deploy.yml'; command -v docker-compose &>/dev/null && COMPOSE='docker-compose -f docker-compose.deploy.yml'
`$COMPOSE up -d
`$COMPOSE run --rm bot /app/sublime migrate || { cat fix_checksum.sql | docker exec -i sublime-postgres psql -U postgres -d sublime_bot -f - 2>/dev/null; `$COMPOSE run --rm bot /app/sublime migrate; }
`$COMPOSE run --rm bot /app/sublime commands set
if [ -f .env.watchdog ] && . .env.watchdog 2>/dev/null && [ -n "`$NOTIFICATION_BOT_TOKEN" ]; then
  `$COMPOSE run --rm -e NOTIFICATION_BOT_TOKEN bot /app/sublime watchdog commands
  `$COMPOSE --profile watchdog up -d
  echo Watchdog: commands set and container started.
fi
[ -x ./notify-update-success.sh ] && ./notify-update-success.sh || true
echo Done.
"@
ssh $SshOpts $Target $remoteCmd
if ($LASTEXITCODE -ne 0) { Write-Error "Remote run failed"; exit 1 }

Remove-Item $ImageTar -ErrorAction SilentlyContinue

# --- 5. Verify ---
Write-Host "== 5/5. Verify ==" -ForegroundColor Cyan
$psOut = ssh $SshOpts $Target "docker ps --filter name=sublime --format '{{.Names}} {{.Status}}'; echo '---'; docker logs sublime-bot --tail 5 2>&1"
$lines = $psOut -split "`n"
$botLine = $lines | Where-Object { $_ -match "sublime-bot" } | Select-Object -First 1
$watchdogLine = $lines | Where-Object { $_ -match "sublime-watchdog" } | Select-Object -First 1
$logStart = [array]::IndexOf($lines, "---") + 1
$logLines = if ($logStart -gt 0) { $lines[$logStart..($lines.Length-1)] } else { @() }
Write-Host "  $botLine"
if ($botLine -match "Up") {
    Write-Host "  Bot container: Up" -ForegroundColor Green
} else {
    Write-Host "  Bot container: NOT Up - check logs" -ForegroundColor Red
}
if ($watchdogLine) {
    Write-Host "  $watchdogLine"
    if ($watchdogLine -match "Up") { Write-Host "  Watchdog container: Up" -ForegroundColor Green }
}
Write-Host "  Recent bot logs:" -ForegroundColor DarkGray
$logLines | ForEach-Object { Write-Host "    $_" }
$apiCheck = ssh $SshOpts $Target "cd $RemoteDir && . .env 2>/dev/null; curl -sf \`"https://api.telegram.org/bot`$TELEGRAM_BOT_TOKEN/getMe\`" 2>/dev/null | grep -q '\`"ok\`":true' && echo 'Bot API: OK' || echo 'Bot API: check token'"
Write-Host "  $apiCheck" -ForegroundColor DarkGray

Write-Host "`nFull update finished. Backup in $LocalBackups\" -ForegroundColor Green
Write-Host "Check: ssh $Target 'docker ps'" -ForegroundColor DarkGray

Send-TelegramNotification "sublime: full update finished successfully at $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
