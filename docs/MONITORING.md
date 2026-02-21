# Monitoring and crash alerts

## 1. Watchdog: get notified when the bot container is down

On the server, a script checks every N minutes whether the bot container is running and sends you a Telegram message if it is down.

**Recommended:** use a **separate notification bot** (second bot token) for alerts. Then alerts work even when the main bot is down or conflicting with getUpdates, and you can give it a clear name like "Sublime Alerts".

### Setup on server (one-time)

1. **Create a second bot** in Telegram (BotFather → /newbot), e.g. "Sublime Alerts". You will use its token only for sending alerts.

2. Copy the script (already copied by `update.ps1` / `deploy.ps1` to `/root/sublime-deploy/`):
   - `scripts/watchdog-telegram.sh` → `/root/sublime-deploy/watchdog-telegram.sh`

3. Make it executable:
   ```bash
   chmod +x /root/sublime-deploy/watchdog-telegram.sh
   ```

4. Get your **Telegram Chat ID** (where you want to receive alerts):
   - Write any message to your **notification bot** (the second bot) in private.
   - Stop the main Sublime bot temporarily (`docker stop sublime-bot`), then open in browser:  
     `https://api.telegram.org/bot<NOTIFICATION_BOT_TOKEN>/getUpdates`  
     (use the **notification** bot token, not the main bot).
   - In the JSON find `"chat":{"id": 123456789` — that number is `ALERT_CHAT_ID`.
   - Start the main bot again: `docker start sublime-bot`.

5. Add cron (run every 5 minutes):
   ```bash
   crontab -e
   ```
   Add line (use the **notification** bot token and your chat ID):
   ```
   */5 * * * * NOTIFICATION_BOT_TOKEN='your_notification_bot_token' ALERT_CHAT_ID=YOUR_CHAT_ID /root/sublime-deploy/watchdog-telegram.sh
   ```
   Or store in a file and source it:
   ```
   */5 * * * * . /root/sublime-deploy/.env.watchdog 2>/dev/null; /root/sublime-deploy/watchdog-telegram.sh
   ```
   Where `/root/sublime-deploy/.env.watchdog` contains (use notification bot token, not main bot):
   ```
   export NOTIFICATION_BOT_TOKEN='your_notification_bot_token'
   export ALERT_CHAT_ID=123456789
   ```

If `NOTIFICATION_BOT_TOKEN` is not set, the script falls back to `TELEGRAM_BOT_TOKEN` (main bot).

After that, if the container stops, you will get a Telegram message from the notification bot within about 5 minutes. The same chat receives a **success notification** after each successful deploy (version, time, container status) when the pipeline or `remote-update.sh` runs and `.env.watchdog` has both `NOTIFICATION_BOT_TOKEN` and `ALERT_CHAT_ID`.

### Check status on demand: /status and /stats

You can ask the **notification bot** (same token as for alerts):

- **/status** or **status** — replies "Бот работает." or "Бот не запущен." (checks Docker container).
- **/stats** — replies with number of chats and unique users (requires `DATABASE_URL`; in deploy it is set automatically).

**Watchdog in the pipeline:** If you pass `-NotificationBotToken` to `update-full.ps1` or `update.ps1`, the deploy writes `.env.watchdog` and starts the **watchdog** container (`sublime watchdog run`) with the same image. No manual setup needed.

- **Compose:** The `watchdog` service is in `docker-compose.deploy.yml` under profile `watchdog`. It runs `sublime watchdog run`, uses `.env.watchdog` for `NOTIFICATION_BOT_TOKEN`, and mounts `/var/run/docker.sock` so `/status` can run `docker ps`.
- **Manual start** (if not using the pipeline): create `.env.watchdog` with `export NOTIFICATION_BOT_TOKEN=...`, then on the server:
  ```bash
  cd /root/sublime-deploy && . .env.watchdog
  docker compose -f docker-compose.deploy.yml --profile watchdog up -d
  ```
  To only set the notification bot menu commands:  
  `docker compose run --rm -e NOTIFICATION_BOT_TOKEN bot /app/sublime watchdog commands`

---

## 2. Sentry: errors and panics in the app

The bot can send errors and panics to [Sentry](https://sentry.io). You get a dashboard and can set up email/Slack alerts for new issues.

### Setup

1. Create a project at sentry.io and copy the DSN (e.g. `https://xxx@xxx.ingest.sentry.io/123`).

2. Build the bot with the `sentry` feature and set the DSN at runtime:
   - **Docker**: build with `--build-arg SENTRY_FEATURE=1` and pass `SENTRY_DSN` in env (see below).
   - **Local**: `cargo build --release --features sentry` and set env `SENTRY_DSN=...`.

3. On the server, add to the bot container environment (e.g. in `docker-compose.deploy.yml` or `.env`):
   ```
   SENTRY_DSN=https://your-dsn@xxx.ingest.sentry.io/project-id
   ```

To enable Sentry in the Docker image, build with the build-arg:
```bash
docker build --build-arg SENTRY_FEATURE=1 -t sublime-bot:latest .
```
Then in deploy/update use this image and set `SENTRY_DSN` in the bot container environment (e.g. in `.env` or `docker-compose.deploy.yml`).
