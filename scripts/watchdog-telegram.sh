#!/bin/bash
# Watchdog: if sublime-bot container is not running, send Telegram alert.
# Use a separate notification bot so alerts don't depend on the main bot.
# Usage: ALERT_CHAT_ID=123456789 NOTIFICATION_BOT_TOKEN=xxx ./watchdog-telegram.sh
# Or: ALERT_CHAT_ID=... TELEGRAM_BOT_TOKEN=... (fallback to main bot token)

set -e
CONTAINER="${WATCHDOG_CONTAINER:-sublime-bot}"
STATUS=$(docker ps --filter "name=${CONTAINER}" --format '{{.Status}}' 2>/dev/null || true)

if [[ -z "$STATUS" || "$STATUS" != *"Up"* ]]; then
  BOT_TOKEN="${NOTIFICATION_BOT_TOKEN:-$TELEGRAM_BOT_TOKEN}"
  if [[ -z "$BOT_TOKEN" || -z "$ALERT_CHAT_ID" ]]; then
    echo "Watchdog: ${CONTAINER} not running. Set NOTIFICATION_BOT_TOKEN (or TELEGRAM_BOT_TOKEN) and ALERT_CHAT_ID to send alert." >&2
    exit 1
  fi
  TEXT="Sublime bot is down on $(hostname). Container: ${CONTAINER}"
  curl -sf -X POST "https://api.telegram.org/bot${BOT_TOKEN}/sendMessage" \
    -d "chat_id=${ALERT_CHAT_ID}" -d "text=${TEXT}" >/dev/null || echo "Watchdog: failed to send Telegram alert" >&2
  exit 1
fi
exit 0
