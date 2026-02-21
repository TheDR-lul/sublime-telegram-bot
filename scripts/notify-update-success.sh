#!/bin/bash
# Send a Telegram message (via notification bot) when deploy/update completed successfully.
# Requires: .env.watchdog with NOTIFICATION_BOT_TOKEN and ALERT_CHAT_ID.
# Usage: run from deploy dir, e.g. cd /root/sublime-deploy && ./notify-update-success.sh

set -e
# Script is copied to deploy dir (e.g. /root/sublime-deploy); run from there.
DEPLOY_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$DEPLOY_DIR"

if [ ! -f .env.watchdog ]; then
  exit 0
fi
. .env.watchdog
if [ -z "$NOTIFICATION_BOT_TOKEN" ] || [ -z "$ALERT_CHAT_ID" ]; then
  exit 0
fi

VERSION="?"
if docker ps --filter name=sublime-bot --format '{{.Names}}' 2>/dev/null | grep -q sublime-bot; then
  VERSION=$(docker exec sublime-bot /app/sublime --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' || echo "?")
fi
BOT_STATUS=$(docker ps --filter name=sublime-bot --format '{{.Status}}' 2>/dev/null || echo "?")
WATCHDOG_STATUS=$(docker ps --filter name=sublime-watchdog --format '{{.Status}}' 2>/dev/null || echo "not running")
TIME=$(date +%Y-%m-%d\ %H:%M)
HOST=$(hostname -s 2>/dev/null || echo "server")

# URL-encode newlines and build message
TEXT="✅ Обновление прошло успешно%0AВерсия: ${VERSION}%0AВремя: ${TIME}%0AСервер: ${HOST}%0AБот: ${BOT_STATUS}%0AWatchdog: ${WATCHDOG_STATUS}"
curl -sf -X POST "https://api.telegram.org/bot${NOTIFICATION_BOT_TOKEN}/sendMessage" \
  -d "chat_id=${ALERT_CHAT_ID}" -d "text=${TEXT}" >/dev/null || true
