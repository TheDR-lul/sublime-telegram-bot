# Deploy: build image locally, copy only image tar + compose to server, run there.
param(
    [Parameter(Mandatory = $false)]
    [string]$TelegramBotToken
)

$Server       = "5.189.154.71"
$User         = "root"
$RemoteDir    = "/root/sublime-deploy"
$Target       = "$User@$Server"
$SshOpts      = "-o StrictHostKeyChecking=accept-new"
$ImageName    = "sublime-bot:latest"
$ImageTar     = "sublime-bot.tar"

# Resolve token
if (-not $TelegramBotToken -and $env:TELEGRAM_BOT_TOKEN) { $TelegramBotToken = $env:TELEGRAM_BOT_TOKEN }
if (-not $TelegramBotToken -and (Test-Path "config.toml")) {
    $line = Get-Content "config.toml" | Where-Object { $_ -match 'telegram_token\s*=\s*"([^"]+)"' } | Select-Object -First 1
    if ($line -match 'telegram_token\s*=\s*"([^"]+)"') { $TelegramBotToken = $Matches[1] }
}
if (-not $TelegramBotToken) {
    Write-Error "Set TelegramBotToken (param, env TELEGRAM_BOT_TOKEN, or config.toml)"
    exit 1
}

Write-Host "Enter SSH password when prompted (input is hidden)." -ForegroundColor Yellow
Write-Host "Tip: run once 'ssh-copy-id $Target' to skip password next time." -ForegroundColor DarkGray
Write-Host ""

Write-Host "== 1/4. Building Docker image locally ==" -ForegroundColor Cyan
docker build --no-cache -t $ImageName .
if ($LASTEXITCODE -ne 0) { Write-Error "Docker build failed"; exit 1 }

Write-Host "== 2/4. Saving image to $ImageTar ==" -ForegroundColor Cyan
docker save $ImageName -o $ImageTar
if ($LASTEXITCODE -ne 0) { Write-Error "Docker save failed"; exit 1 }
$sizeMb = [math]::Round((Get-Item $ImageTar).Length / 1MB, 1)
Write-Host "  Image size: $sizeMb MB" -ForegroundColor DarkGray

Write-Host "== 3/4. Copying to server (1 archive + 1 file) ==" -ForegroundColor Cyan
ssh $SshOpts $Target "mkdir -p $RemoteDir"
scp $SshOpts $ImageTar "${Target}:${RemoteDir}/"
scp $SshOpts "docker-compose.deploy.yml" "${Target}:${RemoteDir}/"
if ($LASTEXITCODE -ne 0) { Write-Error "scp failed"; exit 1 }

Write-Host "== 4/4. Loading image and starting on server ==" -ForegroundColor Cyan
$remoteCmd = @"
set -e
if ! command -v docker &>/dev/null; then echo 'Installing Docker...'; apt-get update && apt-get install -y docker.io docker-compose && systemctl enable --now docker; fi
cd $RemoteDir
docker load -i $ImageTar
export TELEGRAM_BOT_TOKEN='$($TelegramBotToken -replace "'","'\"'\"'")'
COMPOSE='docker compose -f docker-compose.deploy.yml'; command -v docker-compose &>/dev/null && COMPOSE='docker-compose -f docker-compose.deploy.yml'
$COMPOSE up -d
$COMPOSE run --rm bot /app/sublime migrate
$COMPOSE run --rm bot /app/sublime commands set
echo Done.
"@
ssh $SshOpts $Target $remoteCmd
if ($LASTEXITCODE -ne 0) { Write-Error "Remote run failed"; exit 1 }

# Optional: remove local tar to free space
Remove-Item $ImageTar -ErrorAction SilentlyContinue

Write-Host "`nDeploy finished. Check: ssh $Target 'docker ps'" -ForegroundColor Green
