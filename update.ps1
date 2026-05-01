# Update: same as deploy but for app updates. Database is NOT wiped; only new migrations are applied.
param(
    [Parameter(Mandatory = $false)]
    [string]$TelegramBotToken,
    [Parameter(Mandatory = $false)]
    [string]$NotificationBotToken,
    [Parameter(Mandatory = $false)]
    [string]$NotificationChatId
)

$RemoteDir    = "/root/sublime-deploy"
. "$PSScriptRoot\scripts\DeploySshTarget.ps1"
$Target       = Get-SublimeSshTarget
$SshOpts      = "-o StrictHostKeyChecking=accept-new"
$ImageName    = "sublime-bot:latest"
$ImageTar     = "sublime-bot.tar"

# Resolve tokens and notification chat
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

Write-Host "=== UPDATE (database preserved; only new migrations applied) ===" -ForegroundColor Green
Write-Host "Enter SSH password when prompted (input is hidden)." -ForegroundColor Yellow
Write-Host "Tip: run once '.\deploy.bat ssh-key' to skip password next time." -ForegroundColor DarkGray
Write-Host ""

Send-TelegramNotification "sublime: update started at $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"

Write-Host "== 1/4. Building Docker image locally ==" -ForegroundColor Cyan
docker build --no-cache -t $ImageName .
if ($LASTEXITCODE -ne 0) { Write-Error "Docker build failed"; exit 1 }

Write-Host "== 2/4. Saving image to $ImageTar ==" -ForegroundColor Cyan
docker save $ImageName -o $ImageTar
if ($LASTEXITCODE -ne 0) { Write-Error "Docker save failed"; exit 1 }
$sizeMb = [math]::Round((Get-Item $ImageTar).Length / 1MB, 1)
Write-Host "  Image size: $sizeMb MB" -ForegroundColor DarkGray

Write-Host "== 3/4. Copying to server (image + compose + watchdog script) ==" -ForegroundColor Cyan
ssh $SshOpts $Target "mkdir -p $RemoteDir"
scp $SshOpts $ImageTar "${Target}:${RemoteDir}/"
scp $SshOpts "docker-compose.deploy.yml" "${Target}:${RemoteDir}/"
if (Test-Path "scripts/watchdog-telegram.sh") { scp $SshOpts "scripts/watchdog-telegram.sh" "${Target}:${RemoteDir}/" }
if (Test-Path "scripts/fix_checksum.sql") { scp $SshOpts "scripts/fix_checksum.sql" "${Target}:${RemoteDir}/" }
if ($LASTEXITCODE -ne 0) { Write-Error "scp failed"; exit 1 }

if ($NotificationBotToken -or $NotificationChatId) {
    Write-Host "Updating .env.watchdog on server" -ForegroundColor Cyan
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

Write-Host "== 4/4. Loading image, restarting bot, applying new migrations ==" -ForegroundColor Cyan
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
fi
echo Done.
"@
ssh $SshOpts $Target $remoteCmd
if ($LASTEXITCODE -ne 0) { Write-Error "Remote run failed"; exit 1 }

Send-TelegramNotification "sublime: update finished successfully at $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"

Remove-Item $ImageTar -ErrorAction SilentlyContinue

Write-Host "Verify:" -ForegroundColor Cyan
$apiCheck = ssh $SshOpts $Target "cd $RemoteDir && . .env 2>/dev/null; curl -sf \`"https://api.telegram.org/bot`$TELEGRAM_BOT_TOKEN/getMe\`" 2>/dev/null | grep -q '\`"ok\`":true' && echo 'Bot API: OK' || echo 'Bot API: check token'"
Write-Host "  $apiCheck" -ForegroundColor DarkGray

Write-Host "`nUpdate finished. Database unchanged except for new migrations." -ForegroundColor Green
Write-Host "Check: ssh $Target 'docker ps'" -ForegroundColor DarkGray
