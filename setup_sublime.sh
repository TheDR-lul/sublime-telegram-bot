#!/usr/bin/env bash
set -euo pipefail

BOT_DIR="/root/sublime"

echo "[1/6] Updating system and installing packages..."
apt update && apt upgrade -y
apt install -y docker.io docker-compose git

echo "[2/6] Enabling Docker service..."
systemctl enable --now docker

echo "[3/6] Using project in $BOT_DIR..."
cd "$BOT_DIR"

echo "[4/6] Writing config.toml if TELEGRAM_BOT_TOKEN is provided..."
if [ -n "${TELEGRAM_BOT_TOKEN:-}" ]; then
  cat > config.toml <<EOF
telegram_token = "${TELEGRAM_BOT_TOKEN}"
database_url = "postgresql://postgres:postgres@localhost:5432/sublime_bot"
EOF
else
  echo "TELEGRAM_BOT_TOKEN is not set. Using existing config.toml if present."
fi

echo "[5/6] Building and starting Docker containers..."
if command -v docker compose >/dev/null 2>&1; then
  docker compose build
  docker compose up -d
else
  docker-compose build
  docker-compose up -d
fi

echo "[6/6] Running database migrations and setting commands..."
if command -v docker compose >/dev/null 2>&1; then
  docker compose run --rm bot /app/sublime migrate
  docker compose run --rm bot /app/sublime commands set
else
  docker-compose run --rm bot /app/sublime migrate
  docker-compose run --rm bot /app/sublime commands set
fi

echo "All done. Check containers with: docker ps"
echo "View logs with: docker logs -f sublime-bot"
