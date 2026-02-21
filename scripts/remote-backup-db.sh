#!/bin/bash
# Run on server: dump PostgreSQL DB to a timestamped file.
# Usage: REMOTE_DIR=/root/sublime-deploy BACKUP_DIR=/root/sublime-deploy/backups ./remote-backup-db.sh
# Or: ./remote-backup-db.sh  (uses defaults)
set -e
REMOTE_DIR="${REMOTE_DIR:-/root/sublime-deploy}"
BACKUP_DIR="${BACKUP_DIR:-$REMOTE_DIR/backups}"
COMPOSE_FILE="${REMOTE_DIR}/docker-compose.deploy.yml"
mkdir -p "$BACKUP_DIR"
STAMP=$(date +%Y%m%d_%H%M%S)
BACKUP_FILE="${BACKUP_DIR}/sublime_db_backup_${STAMP}.sql"
cd "$REMOTE_DIR"
docker compose -f "$COMPOSE_FILE" exec -T db pg_dump -U postgres sublime_bot > "$BACKUP_FILE"
echo "Backup: $BACKUP_FILE ($(wc -c < "$BACKUP_FILE") bytes)"
