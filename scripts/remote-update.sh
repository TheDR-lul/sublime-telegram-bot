#!/bin/bash
# Run on server: backup DB, then load new image (or build), migrate, restart, set commands.
# Expects: TELEGRAM_BOT_TOKEN in env or .env in REMOTE_DIR. Optionally IMAGE_TAR path if updating from tar.
# Usage (after copying image tar to server):
#   cd /root/sublime-deploy && . .env && ./remote-update.sh
# Or build on server (no tar):
#   cd /root/sublime-deploy && . .env && BUILD_ON_SERVER=1 ./remote-update.sh
set -e
REMOTE_DIR="${REMOTE_DIR:-/root/sublime-deploy}"
BACKUP_DIR="${BACKUP_DIR:-$REMOTE_DIR/backups}"
COMPOSE_FILE="${REMOTE_DIR}/docker-compose.deploy.yml"
IMAGE_TAR="${IMAGE_TAR:-$REMOTE_DIR/sublime-bot.tar}"
cd "$REMOTE_DIR"

echo "== 1. Backup DB =="
mkdir -p "$BACKUP_DIR"
STAMP=$(date +%Y%m%d_%H%M%S)
BACKUP_FILE="${BACKUP_DIR}/sublime_db_backup_${STAMP}.sql"
docker compose -f "$COMPOSE_FILE" exec -T db pg_dump -U postgres sublime_bot > "$BACKUP_FILE"
echo "  OK: $BACKUP_FILE ($(wc -c < "$BACKUP_FILE") bytes)"

echo "== 2. Load or build image =="
if [ -n "$BUILD_ON_SERVER" ] && [ "$BUILD_ON_SERVER" != "0" ]; then
  docker build -t sublime-bot:latest .
else
  [ -f "$IMAGE_TAR" ] || { echo "Missing $IMAGE_TAR and BUILD_ON_SERVER not set"; exit 1; }
  docker load -i "$IMAGE_TAR"
fi

echo "== 3. Restart bot =="
export TELEGRAM_BOT_TOKEN
COMPOSE='docker compose -f docker-compose.deploy.yml'
command -v docker-compose &>/dev/null && COMPOSE='docker-compose -f docker-compose.deploy.yml'
$COMPOSE up -d

echo "== 4. Migrate =="
$COMPOSE run --rm bot /app/sublime migrate || { [ -f fix_checksum.sql ] && cat fix_checksum.sql | docker exec -i sublime-postgres psql -U postgres -d sublime_bot -f - 2>/dev/null; $COMPOSE run --rm bot /app/sublime migrate; } || { echo "Migrate failed. Restore from $BACKUP_FILE if needed."; exit 1; }

echo "== 5. Set commands =="
$COMPOSE run --rm bot /app/sublime commands set

if [ -f .env.watchdog ] && . .env.watchdog 2>/dev/null && [ -n "$NOTIFICATION_BOT_TOKEN" ]; then
  echo "== 6. Watchdog: set commands and start =="
  $COMPOSE run --rm -e NOTIFICATION_BOT_TOKEN bot /app/sublime watchdog commands
  $COMPOSE --profile watchdog up -d
fi

[ -x ./notify-update-success.sh ] && ./notify-update-success.sh || true
echo "Done. Bot restarted."
