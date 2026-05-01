#!/usr/bin/env bash
# Build an offline migration bundle under /root/server-export-<timestamp>/ on the VPS.
# Use for copying to local disk D:\ (scp) — see docs/RESTORE_FROM_LOCAL_SSD_BACKUP.md
#
# Requirements: Docker running; /root/sublime-deploy/docker-compose.deploy.yml exists;
# optional stacks: HA volumes ha_config, caddy_config, caddy_data;
# bind tree /opt/home-assistant.
#
# Usage (on server, as root):
#   chmod +x scripts/export-server-bundle.sh
#   ./scripts/export-server-bundle.sh
#
# Env:
#   SUBLIME_DEPLOY_DIR=/root/sublime-deploy   (default)
#   HA_OPT_DIR=/opt/home-assistant            (default)
#   SKIP_HA_STACK=1                           skip HA/Caddy archives if volumes missing
#   SKIP_SUBLIME=1                            skip bot deploy tarball + pg_dump + postgres volume
#   COPY_BOT_IMAGE_TAR=1                    copy sublime-bot.tar into bundle if present (large)

set -euo pipefail

SUBLIME_DEPLOY_DIR="${SUBLIME_DEPLOY_DIR:-/root/sublime-deploy}"
HA_OPT_DIR="${HA_OPT_DIR:-/opt/home-assistant}"
COPY_BOT_IMAGE_TAR="${COPY_BOT_IMAGE_TAR:-}"

STAMP=$(date +%Y%m%d_%H%M%S)
DEST="/root/server-export-${STAMP}"
COMPOSE_FILE="${SUBLIME_DEPLOY_DIR}/docker-compose.deploy.yml"

mkdir -p "$DEST"
echo "Export directory: $DEST"

need_alpine() {
  docker image inspect alpine:latest >/dev/null 2>&1 || docker pull alpine:latest
}

backup_volume() {
  local vol_name="$1"
  local out_file="$2"
  if ! docker volume inspect "$vol_name" >/dev/null 2>&1; then
    echo "  (skip) volume not found: $vol_name"
    return 0
  fi
  echo "  archiving volume: $vol_name -> $(basename "$out_file")"
  docker run --rm \
    -v "${vol_name}:/from:ro" \
    -v "${DEST}:/to" \
    alpine:latest \
    tar czf "/to/${out_file}" -C /from .
}

if [[ "${SKIP_HA_STACK:-}" != "1" ]]; then
  need_alpine
  echo "== Home Assistant / Caddy Docker volumes =="
  backup_volume ha_config "home-assistant_config_volume.tgz"
  backup_volume caddy_config "caddy_config_volume.tgz"
  backup_volume caddy_data "caddy_data_volume.tgz"

  if [[ -d "$HA_OPT_DIR" ]]; then
    echo "== Bind mounts: $HA_OPT_DIR =="
    tar czf "${DEST}/opt_home-assistant_bindmounts.tgz" -C "$(dirname "$HA_OPT_DIR")" "$(basename "$HA_OPT_DIR")"
  else
    echo "  (skip) directory missing: $HA_OPT_DIR"
  fi

  if [[ -d /root/smarthome ]]; then
    echo "== /root/smarthome =="
    tar czf "${DEST}/root_smarthome.tgz" -C /root smarthome
  fi
else
  echo "== SKIP_HA_STACK=1: skipping HA/Caddy/smarthome =="
fi

if [[ "${SKIP_SUBLIME:-}" != "1" ]]; then
  if [[ ! -f "$COMPOSE_FILE" ]]; then
    echo "ERROR: compose file not found: $COMPOSE_FILE" >&2
    exit 1
  fi

  echo "== Sublime deploy tree (no image tar) =="
  tar czf "${DEST}/sublime-deploy_nobotimage.tgz" \
    --exclude='sublime-bot.tar' \
    -C "$(dirname "$SUBLIME_DEPLOY_DIR")" \
    "$(basename "$SUBLIME_DEPLOY_DIR")"

  echo "== Postgres logical dump =="
  need_alpine
  if [[ -z "$(docker compose -f "$COMPOSE_FILE" ps -q db 2>/dev/null)" ]]; then
    echo "  starting db container for pg_dump..."
    docker compose -f "$COMPOSE_FILE" up -d db
    for _ in $(seq 1 30); do
      if docker compose -f "$COMPOSE_FILE" exec -T db pg_isready -U postgres -d sublime_bot >/dev/null 2>&1; then
        break
      fi
      sleep 1
    done
  fi
  (
    cd "$SUBLIME_DEPLOY_DIR"
    docker compose -f docker-compose.deploy.yml exec -T db pg_dump -U postgres sublime_bot
  ) > "${DEST}/sublime_bot_pgdump_${STAMP}.sql"
  echo "  wrote sublime_bot_pgdump_${STAMP}.sql"

  echo "== Postgres data volume (raw) =="
  backup_volume sublime-deploy_postgres-data "postgres_sublime_data_volume.tgz"

  if [[ "$COPY_BOT_IMAGE_TAR" == "1" ]] && [[ -f "${SUBLIME_DEPLOY_DIR}/sublime-bot.tar" ]]; then
    echo "== Copying sublime-bot.tar (large) =="
    cp -f "${SUBLIME_DEPLOY_DIR}/sublime-bot.tar" "${DEST}/"
  fi
else
  echo "== SKIP_SUBLIME=1: skipping bot bundle =="
fi

echo ""
echo "Done. Total size:"
du -sh "$DEST"
ls -lah "$DEST"
echo ""
echo "Copy to your PC (example):"
echo "  scp -r root@THIS_HOST:${DEST}/* D:\\\\server-export-${STAMP}\\\\"
