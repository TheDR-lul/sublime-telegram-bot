#!/bin/bash
set -e
cd /root/sublime-deploy
source .env.watchdog 2>/dev/null || true

if [ -z "$ALERT_CHAT_ID" ] && [ -n "$NOTIFICATION_BOT_TOKEN" ]; then
  UPDATES=$(curl -sf "https://api.telegram.org/bot${NOTIFICATION_BOT_TOKEN}/getUpdates?limit=1" 2>/dev/null || echo '{}')
  CHAT_ID=$(echo "$UPDATES" | grep -o '"chat":{"id":[0-9]*' | head -1 | grep -o '[0-9]*$' || true)
  if [ -n "$CHAT_ID" ]; then
    echo "export ALERT_CHAT_ID=$CHAT_ID" >> .env.watchdog
    echo "Added ALERT_CHAT_ID=$CHAT_ID to .env.watchdog"
  else
    echo "No updates from notification bot. Message the bot in Telegram, then run this script again or add ALERT_CHAT_ID to .env.watchdog manually."
  fi
fi

if ! crontab -l 2>/dev/null | grep -q watchdog-telegram; then
  (crontab -l 2>/dev/null; echo '*/5 * * * * . /root/sublime-deploy/.env.watchdog 2>/dev/null; /root/sublime-deploy/watchdog-telegram.sh') | crontab -
  echo "Cron added for watchdog (every 5 min)"
else
  echo "Cron already has watchdog entry"
fi

chmod +x /root/sublime-deploy/watchdog-telegram.sh 2>/dev/null || true
echo "Done."
