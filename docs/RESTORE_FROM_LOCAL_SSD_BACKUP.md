# Восстановление стека с локального бэкапа (диск D / внешний SSD на твоём ПК)

На **Windows** буква **`D:`** — это твой локальный или внешний диск, **не VPS**. Выгрузка с сервера делается командами `scp`/`rsync` **на этот диск**. На новом Linux-сервере буквы `D:` нет: сначала копируешь архивы с ПК на сервер (`scp`), затем восстанавливаешь по шагам ниже.

Общая схема: **архивная папка** вида `D:\server-export-YYYYMMDD_HHMMSS\` содержит слепки Docker-томов, `docker-compose` умного дома и артефакты бота. Имена ниже совпадают с теми, что создавались при полном офлайн-экспорте (см. раздел «Состав папки»).

См. также: [DEPLOY.md](DEPLOY.md) (деплой бота после восстановления), [NEW_MACHINE_AND_SERVER.md](NEW_MACHINE_AND_SERVER.md) (ПК и SSH).

---

## Состав типичной папки экспорта

| Файл | Назначение |
|------|------------|
| `home-assistant_config_volume.tgz` | Том Docker **`ha_config`** — каталог `/config` Home Assistant (сущности, интеграции, `configuration.yaml` и т.д.). |
| `caddy_config_volume.tgz`, `caddy_data_volume.tgz` | Тома Caddy (конфиг внутри контейнера и данные, в т.ч. TLS). |
| `opt_home-assistant_bindmounts.tgz` | Содержимое **`/opt/home-assistant`**: `docker-compose.yml`, Mosquitto (`config/`/`data/`/`log/`), `caddy/Caddyfile`, локальные `.env`. |
| `root_smarthome.tgz` | Опциональная копия **`/root/smarthome`** (если был дубль репозитория на сервере). |
| `sublime-deploy_nobotimage.tgz` | **`/root/sublime-deploy`** без `sublime-bot.tar` — compose, скрипты деплоя, часто **`.env`** (секреты). Не светить публично. |
| `sublime_bot_pgdump_*.sql` | Логический дамп Postgres (`pg_dump`), предпочтительный способ поднять БД бота. |
| `postgres_sublime_data_volume.tgz` | Сырой том **`sublime-deploy_postgres-data`** — только если нужно точное клонирование data directory (обычно хватает `pg_dump`). |
| `sublime-bot.tar` | Образ **`docker save`** основного бота + watchdog; на сервере: `docker load -i sublime-bot.tar`. |

Дополнительно на том же **`D:`** может лежать отдельно свежий дамп только БД или полный тар образа — ориентируйся по датам в имени файла.

---

## 0. Подготовка нового VPS

1. **Ubuntu/Debian** (или свой дистрибутив), открытые порты под твои сервисы (часто 80/443 для Caddy, 8123 для HA за прокси или напрямую — как было в `Caddyfile`).
2. Установить **Docker** и **Compose plugin**.
3. Скопировать папку с **`D:`** на сервер, например в `/root/restore-bundle`:

```bash
# На твоём ПК (PowerShell), пример:
scp -r D:\server-export-20260501_221417 root@НОВЫЙ_IP:/root/restore-bundle
```

Дальше все команды **на новом сервере** от `root` (или с `sudo`).

---

## 1. Умный дом: Home Assistant + Mosquitto + Caddy

Исходная раскладка на старом сервере: compose в **`/opt/home-assistant/docker-compose.yml`**, том **`ha_config`**, бинды Mosquitto из подкаталогов `opt`, Caddy с монтированием `Caddyfile` и томами `caddy_config` / `caddy_data`.

### 1.1 Распаковать bind-mount дерево

```bash
sudo mkdir -p /opt/home-assistant
sudo tar xzf /root/restore-bundle/opt_home-assistant_bindmounts.tgz -C /opt
# Должно появиться /opt/home-assistant/docker-compose.yml, mosquitto/, caddy/
```

Проверь **`/opt/home-assistant/.env`** и **`caddy/Caddyfile`** — домены, порты, пути после смены IP/DNS.

### 1.2 Восстановить Docker-том Home Assistant (`ha_config`)

Имя тома должно совпасть с тем, что указано в `docker-compose.yml` (обычно `ha_config`).

```bash
docker volume create ha_config
docker run --rm \
  -v ha_config:/to \
  -v /root/restore-bundle:/from \
  alpine sh -c "cd /to && rm -rf ./* ; tar xzf /from/home-assistant_config_volume.tgz"
```

### 1.3 Восстановить тома Caddy

```bash
docker volume create caddy_config
docker volume create caddy_data
docker run --rm -v caddy_config:/to -v /root/restore-bundle:/from alpine \
  sh -c "cd /to && rm -rf ./* 2>/dev/null; tar xzf /from/caddy_config_volume.tgz"
docker run --rm -v caddy_data:/to -v /root/restore-bundle:/from alpine \
  sh -c "cd /to && rm -rf ./* 2>/dev/null; tar xzf /from/caddy_data_volume.tgz"
```

### 1.4 Запуск стека умного дома

```bash
cd /opt/home-assistant
docker compose pull   # подтянуть образы home-assistant, mosquitto, caddy с registry
docker compose up -d
docker compose ps
```

Если токены/сертификаты завязаны на старый домен — обновь DNS или выпусти новые certs (часто через Caddy / TLS в конфиге).

**Опционально:** `root_smarthome.tgz` — только если нужен дубль файлов под `/root/smarthome`; рабочий compose у тебя уже в `/opt/home-assistant`.

---

## 2. Бот Sublime (Postgres + образ + compose)

### 2.1 Развернуть каталог деплоя

```bash
sudo mkdir -p /root/sublime-deploy
sudo tar xzf /root/restore-bundle/sublime-deploy_nobotimage.tgz -C /root
```

Проверь **`/root/sublime-deploy/.env`**: `TELEGRAM_BOT_TOKEN`, при необходимости переменные для watchdog. Добавь то, чего не было в архиве.

### 2.2 Загрузить образ бота

```bash
docker load -i /root/restore-bundle/sublime-bot.tar
docker images | grep sublime-bot
```

### 2.3 Поднять только Postgres, залить дамп (рекомендуется)

```bash
cd /root/sublime-deploy
docker compose -f docker-compose.deploy.yml up -d db
# Подождать готовность Postgres (несколько секунд)
docker compose -f docker-compose.deploy.yml exec -T db \
  psql -U postgres -d postgres -c "SELECT 1" 
# Создать БД, если пусто (compose обычно создаёт sublime_bot при первом старте — смотри логи)
docker compose -f docker-compose.deploy.yml exec -T db \
  psql -U postgres -c "CREATE DATABASE sublime_bot;" 2>/dev/null || true
cat /root/restore-bundle/sublime_bot_pgdump_*.sql | \
  docker compose -f docker-compose.deploy.yml exec -T db \
  psql -U postgres -d sublime_bot
```

Если после restore миграции ругаются на checksum — см. раздел *If migrate fails* в [DEPLOY.md](DEPLOY.md).

### 2.4 Поднять бота и при необходимости watchdog

```bash
cd /root/sublime-deploy
docker compose -f docker-compose.deploy.yml up -d bot
# Watchdog — по профилю и .env.watchdog, как в DEPLOY.md
docker compose -f docker-compose.deploy.yml --profile watchdog up -d watchdog
```

Проверка: `docker logs sublime-bot --tail 50`, запрос к Telegram `getMe` с токеном из `.env`.

### 2.5 Альтернатива: восстановить сырой том Postgres (редко нужно)

Только если хочешь **один в один** data directory и понимаешь риски версии Postgres (образ `postgres:17` должен совпадать).

```bash
cd /root/sublime-deploy
docker compose -f docker-compose.deploy.yml down
# ОСТОРОЖНО: удалит текущие данные тома
docker volume rm sublime-deploy_postgres-data 2>/dev/null || true
docker volume create sublime-deploy_postgres-data
docker run --rm \
  -v sublime-deploy_postgres-data:/to \
  -v /root/restore-bundle:/from \
  alpine sh -c "cd /to && rm -rf ./* ; tar xzf /from/postgres_sublime_data_volume.tgz"
docker compose -f docker-compose.deploy.yml up -d
```

---

## 3. Копирование с ПК на сервер (шпаргалка)

**С Windows на Linux:**

```powershell
scp -r D:\server-export-YYYYMMDD_HHMMSS\* root@НОВЫЙ_IP:/root/restore-bundle/
```

Создай каталог заранее: `ssh root@НОВЫЙ_IP "mkdir -p /root/restore-bundle"`.

---

## 4. Безопасность

- В архивах лежат **пароли MQTT** (`mosquitto/passwd`), **токены HA**, **`.env` бота**. Храни папку на `D:` только у себя; в облако без шифрования не заливай.
- После переезда смени пароли/токены, если архив где-то светился.

---

## 5. Чеклист после подъёма

- [ ] `docker ps` — `home-assistant`, `mosquitto`, `caddy`, `sublime-postgres`, `sublime-bot` (и при необходимости `sublime-watchdog`).
- [ ] Home Assistant открывается в браузере (как настроено в Caddy или по порту).
- [ ] MQTT-клиенты коннектятся к новому хосту/сертификату.
- [ ] Telegram-бот отвечает в тестовом чате.
- [ ] Обнови **`SUBLIME_SSH_TARGET`** на ПК — см. [NEW_MACHINE_AND_SERVER.md](NEW_MACHINE_AND_SERVER.md).
