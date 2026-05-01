# Deploy and update pipeline

New PC or new VPS: see **[NEW_MACHINE_AND_SERVER.md](NEW_MACHINE_AND_SERVER.md)** (SSH, `SUBLIME_SSH_TARGET`, DB move).

Restore from offline bundle copied to your PC disk (e.g. `D:\server-export-*`): **[RESTORE_FROM_LOCAL_SSD_BACKUP.md](RESTORE_FROM_LOCAL_SSD_BACKUP.md)** (Home Assistant + Mosquitto + Caddy + Sublime/Postgres).

## Quick reference

| Script | When to use |
|--------|-------------|
| **`.\update-full.ps1`** | Normal update: **backup DB → build locally (Docker) → deploy → verify**. Use when Docker is available on your machine. |
| **`.\update-remote.ps1`** | Update **without local Docker**: backup → copy source to server → build on server → migrate → verify. |
| **`.\update.ps1`** | Minimal update (no backup, no verify): build → copy image → migrate → restart. |
| **`.\deploy.ps1`** | First-time deploy (same server, from scratch). |

All update scripts need **Telegram token** (param `-TelegramBotToken`, env `TELEGRAM_BOT_TOKEN`, or `config.toml`).  
Full pipeline scripts also **download the DB backup** into the local `backups/` folder.  
Use `config.toml.example` as a template; copy to `config.toml` and fill in (do not commit `config.toml`).

**SSH target:** переменная `SUBLIME_SSH_TARGET` = `user@host` переопределяет сервер для всех `.ps1`. Если не задана — дефолт **`root@5.189.154.71`** (см. `scripts/DeploySshTarget.ps1`).

---

## Full update pipeline (recommended)

Run from the project root:

```powershell
.\update-full.ps1
```

What it does:

1. **Sync scripts** – copies `remote-backup-db.sh`, `remote-update.sh`, `notify-update-success.sh`, `watchdog-telegram.sh` to the server.
2. **Backup DB** – on server runs `pg_dump`, then downloads the latest backup to `backups/sublime_db_backup_YYYYMMDD_HHMMSS.sql`.
3. **Build** – `docker build` locally (requires Docker Desktop / Docker running).
4. **Copy** – saves image to tar, copies tar + `docker-compose.deploy.yml` to server.
5. **Deploy** – on server: `docker load`, `compose up -d`, `sublime migrate`, `sublime commands set`. If `.env.watchdog` exists with `NOTIFICATION_BOT_TOKEN`, also runs `sublime watchdog commands` and starts the **watchdog** container (`compose --profile watchdog up -d`). If `ALERT_CHAT_ID` is set in `.env.watchdog`, the **notification bot** sends a success message to that chat (version, time, container status).
6. **Verify** – prints main bot and (if running) watchdog container status and last 5 bot log lines.

If you use a notification bot for alerts and /status, /stats, pass its token so `.env.watchdog` is written and the watchdog container is started (existing `ALERT_CHAT_ID` in `.env.watchdog` is kept):

```powershell
.\update-full.ps1 -NotificationBotToken "your_notification_bot_token"
```

---

## Update without local Docker

When Docker is not installed or not running locally:

```powershell
.\update-remote.ps1
```

Steps:

1. Sync scripts to server.
2. Backup DB on server and download to `backups/`.
3. Copy full source to server (`Cargo.toml`, `Cargo.lock`, `Dockerfile`, `config.toml`, `src/`, `migrations/`, compose).
4. On server: run `remote-update.sh` with `BUILD_ON_SERVER=1` (backup again, then `docker build`, up, migrate, commands set).
5. Verify.

**Requirement:** server must have `TELEGRAM_BOT_TOKEN` in `/root/sublime-deploy/.env` (create it once, e.g. from `config.toml`).

---

## Server-side scripts (used by the pipeline)

These live in `scripts/` and are copied to the server by the update scripts.

- **`remote-backup-db.sh`** – dumps `sublime_bot` DB to `$REMOTE_DIR/backups/sublime_db_backup_YYYYMMDD_HHMMSS.sql`. Run on server: `./remote-backup-db.sh`.
- **`remote-update.sh`** – backup → load image from tar (or build if `BUILD_ON_SERVER=1`) → `compose up -d` → migrate → commands set. If `.env.watchdog` exists with `NOTIFICATION_BOT_TOKEN`, also sets watchdog menu commands and runs `compose --profile watchdog up -d`. At the end, if present, runs **`notify-update-success.sh`** (sends a success message to `ALERT_CHAT_ID` via the notification bot). Expects `TELEGRAM_BOT_TOKEN` in env or `.env`. Run on server: `cd /root/sublime-deploy && . .env && ./remote-update.sh` or with image tar: `./remote-update.sh` (after copying `sublime-bot.tar`).
- **`notify-update-success.sh`** – sends a Telegram message (via notification bot) when deploy finished successfully: version, time, host, bot and watchdog container status. Requires `.env.watchdog` with `NOTIFICATION_BOT_TOKEN` and `ALERT_CHAT_ID`. Called automatically by the pipeline and by `remote-update.sh`.

---

## First-time deploy

1. Build and run once with **`.\deploy.ps1`** (creates containers, applies migrations, sets commands).
2. On the server, create `/root/sublime-deploy/.env` with:
   ```bash
   TELEGRAM_BOT_TOKEN=your_main_bot_token
   ```
   so that `docker compose` and future updates see the token.
3. For crash alerts, configure the watchdog (see [MONITORING.md](MONITORING.md)).

---

## Backup location

- **On server:** `/root/sublime-deploy/backups/sublime_db_backup_*.sql`
- **Local (after full/remote update):** `backups/sublime_db_backup_*.sql`

To restore a backup on the server (only if needed):

```bash
docker compose -f docker-compose.deploy.yml exec -T db psql -U postgres -d sublime_bot < /root/sublime-deploy/backups/sublime_db_backup_YYYYMMDD_HHMMSS.sql
```

---

## If migrate fails (VersionMismatch)

If you see `Error: Migrate(VersionMismatch(20240101000002))` (or another version), the migration file was changed after it was first applied. SQLx stores a SHA384 checksum of each applied migration.

**Fix:** update the checksum in the DB to match the current file in the image.

1. On server, get SHA384 of the migration file **as seen inside the bot container**:
   ```bash
   cd /root/sublime-deploy && . .env
   docker compose -f docker-compose.deploy.yml run --rm bot cat /app/migrations/20240101000002_gameresult_slot.sql | openssl dgst -sha384 -binary | xxd -p -c 999
   ```
2. Update the DB (replace `HEXDIGEST` with the output from step 1):
   ```sql
   UPDATE _sqlx_migrations SET checksum = decode('HEXDIGEST', 'hex') WHERE version = 20240101000002;
   ```
   Run it: `docker exec -i sublime-postgres psql -U postgres -d sublime_bot -f - < fix_checksum.sql`
3. Run migrate again: `docker compose run --rm bot /app/sublime migrate`.

A precomputed fix for one common migration is in `scripts/fix_checksum.sql` (see comments there); use only if the migration file matches.

---

## Checks after update

**Текущий VPS (совпадает с дефолтом в `scripts/DeploySshTarget.ps1`):** `root@5.189.154.71`.

- Containers: `ssh root@5.189.154.71 'docker ps --filter name=sublime'` (ожидай `sublime-bot` и при необходимости `sublime-watchdog`).
- Main bot logs: `ssh root@5.189.154.71 'docker logs sublime-bot --tail 20'`

Если на ПК выставлен `SUBLIME_SSH_TARGET` на другой хост — выполняй те же команды с этим `user@host` вместо `root@5.189.154.71`.
- Watchdog logs (if running): `docker logs sublime-watchdog --tail 20`
- Commands in Telegram: open the main bot and the notification bot menus and confirm command lists are updated.
