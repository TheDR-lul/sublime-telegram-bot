# Update: same as deploy but for app updates. Database is NOT wiped; only new migrations are applied.
param(
    [Parameter(Mandatory = $false)]
    [string]$TelegramBotToken,
    [Parameter(Mandatory = $false)]
    [string]$NotificationBotToken
)

$Server       = "5.189.154.71"
$User         = "root"
$RemoteDir    = "/root/sublime-deploy"
$Target       = "$User@$Server"
$SshOpts      = "-o StrictHostKeyChecking=accept-new"
$ImageName    = "sublime-bot:latest"
$ImageTar     = "sublime-bot.tar"

# Resolve tokens
if (-not $TelegramBotToken -and $env:TELEGRAM_BOT_TOKEN) { $TelegramBotToken = $env:TELEGRAM_BOT_TOKEN }
if (-not $NotificationBotToken -and $env:NOTIFICATION_BOT_TOKEN) { $NotificationBotToken = $env:NOTIFICATION_BOT_TOKEN }
if (Test-Path "config.toml") {
    $config = Get-Content "config.toml" -Raw
    if (-not $TelegramBotToken -and $config -match 'telegram_token\s*=\s*"([^"]+)"') { $TelegramBotToken = $Matches[1] }
    if (-not $NotificationBotToken -and $config -match 'notification_bot_token\s*=\s*"([^"]+)"') { $NotificationBotToken = $Matches[1] }
}
if (-not $TelegramBotToken) {
    Write-Error "Set TelegramBotToken (param, env TELEGRAM_BOT_TOKEN, or config.toml)"
    exit 1
}

Write-Host "=== UPDATE (database preserved; only new migrations applied) ===" -ForegroundColor Green
Write-Host "Enter SSH password when prompted (input is hidden)." -ForegroundColor Yellow
Write-Host "Tip: run once '.\deploy.bat ssh-key' to skip password next time." -ForegroundColor DarkGray
Write-Host ""

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
if ($LASTEXITCODE -ne 0) { Write-Error "scp failed"; exit 1 }

if ($NotificationBotToken) {
    Write-Host "Writing NOTIFICATION_BOT_TOKEN to server .env.watchdog" -ForegroundColor Cyan
    $watchdogEnv = "export NOTIFICATION_BOT_TOKEN='$($NotificationBotToken -replace "'", "'\''")'"
    $tempFile = [System.IO.Path]::GetTempFileName()
    [System.IO.File]::WriteAllText($tempFile, $watchdogEnv)
    scp $SshOpts $tempFile "${Target}:${RemoteDir}/.env.watchdog"
    ssh $SshOpts $Target "chmod 600 $RemoteDir/.env.watchdog"
    Remove-Item $tempFile -ErrorAction SilentlyContinue
}

Write-Host "== 4/4. Loading image, restarting bot, applying new migrations ==" -ForegroundColor Cyan
$tokenForBash = $TelegramBotToken -replace "'", "'\''"
$remoteCmd = @"
set -e
cd $RemoteDir
docker load -i $ImageTar
export TELEGRAM_BOT_TOKEN='$tokenForBash'
COMPOSE='docker compose -f docker-compose.deploy.yml'; command -v docker-compose &>/dev/null && COMPOSE='docker-compose -f docker-compose.deploy.yml'
`$COMPOSE up -d
`$COMPOSE run --rm bot /app/sublime migrate
`$COMPOSE run --rm bot /app/sublime commands set
if [ -f .env.watchdog ] && . .env.watchdog 2>/dev/null && [ -n "`$NOTIFICATION_BOT_TOKEN" ]; then
  `$COMPOSE run --rm -e NOTIFICATION_BOT_TOKEN bot /app/sublime watchdog commands
  `$COMPOSE --profile watchdog up -d
fi
echo Done.
"@
ssh $SshOpts $Target $remoteCmd
if ($LASTEXITCODE -ne 0) { Write-Error "Remote run failed"; exit 1 }

Remove-Item $ImageTar -ErrorAction SilentlyContinue

Write-Host "Verify:" -ForegroundColor Cyan
$apiCheck = ssh $SshOpts $Target "cd $RemoteDir && . .env 2>/dev/null; curl -sf \`"https://api.telegram.org/bot`$TELEGRAM_BOT_TOKEN/getMe\`" 2>/dev/null | grep -q '\`"ok\`":true' && echo 'Bot API: OK' || echo 'Bot API: check token'"
Write-Host "  $apiCheck" -ForegroundColor DarkGray

Write-Host "`nUpdate finished. Database unchanged except for new migrations." -ForegroundColor Green
Write-Host "Check: ssh $Target 'docker ps'" -ForegroundColor DarkGray
