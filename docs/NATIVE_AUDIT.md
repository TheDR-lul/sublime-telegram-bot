# Native Audit Report

**Date:** 2025-02-22  
**Scope:** Secrets, config exposure, .gitignore, Docker, dependencies, deployment.

---

## Executive summary

- **Secrets:** No secrets are committed in the repo. `config.toml` is in `.gitignore` and was never committed. **Critical:** Local file `config.toml` in the project root contains real Telegram tokens and DB URL; it must stay out of version control and must not be baked into Docker images.
- **Fixes applied:** `.env.watchdog` added to `.gitignore`; `config.toml` added to `.dockerignore`; Dockerfile now copies `config.toml.example` as default config instead of `config.toml`; Russian comments in `docker-compose.yml` replaced with English.

---

## 1. Secrets and credentials

| Finding | Severity | Status |
|--------|----------|--------|
| `config.toml` contains real `telegram_token`, `notification_bot_token`, `database_url` | Critical (if committed or in image) | Mitigated: file is gitignored; not in Docker image after fix |
| Tokens only loaded from env or local config; no hardcoded secrets in source | OK | — |
| Tests use placeholder `test_token` and local Postgres URL | OK | — |
| CI uses placeholder `DATABASE_URL` (postgresql://test:test@localhost/test) | OK | — |

**Recommendation:** Rotate both Telegram bot tokens and change DB password if `config.toml` was ever committed in the past (audit shows it was not). Prefer env vars (`TELEGRAM_BOT_TOKEN` / `TELOXIDE_TOKEN`, `DATABASE_URL`) for production and Docker.

---

## 2. .gitignore and sensitive files

| File / pattern | In .gitignore | Notes |
|----------------|---------------|--------|
| `config.toml` | Yes | Contains secrets; must never be committed |
| `.env` | Yes | — |
| `.env.watchdog` | Yes (added in this audit) | Can contain `NOTIFICATION_BOT_TOKEN`, `ALERT_CHAT_ID` |
| `backups/` | Yes | DB backups |
| `target/` | Yes | Build artifacts |

---

## 3. Docker and deployment

| Finding | Severity | Status |
|--------|----------|--------|
| Dockerfile previously copied `config.toml` from build context | Critical | Fixed: image now uses `config.toml.example` as default; runtime uses env |
| `config.toml` not in `.dockerignore` allowed local secrets into image | Critical | Fixed: `config.toml` added to `.dockerignore` |
| Compose uses `POSTGRES_PASSWORD: postgres` and fixed `DATABASE_URL` | Low | Acceptable for local/dev; use env and secrets in production |
| Deploy scripts take tokens from env or `config.toml`, never commit them | OK | — |

**Recommendation:** In production, set `TELEGRAM_BOT_TOKEN`/`TELOXIDE_TOKEN` and `DATABASE_URL` via environment (e.g. `.env` on host or secrets manager); do not rely on `config.toml` inside the image.

---

## 4. Code and config

| Finding | Severity | Status |
|--------|----------|--------|
| Russian comment in `docker-compose.yml` | Low | Fixed: replaced with English |
| `src/config.rs` supports env vars and masks secrets in logs | OK | — |
| `update-remote.ps1` / `deploy.ps1` read tokens from env or config, do not store in repo | OK | — |

---

## 5. Dependencies

- **Cargo.toml:** `edition = "2024"`; dependencies from crates.io with pinned versions.
- **Rust 2024:** Valid; ensure toolchain supports it (e.g. stable with 2024 support).
- No obvious vulnerable or unmaintained crates in the scanned tree.

---

## 6. Actions taken in this audit

1. **.gitignore** – Added `.env.watchdog` so notification/watchdog secrets are not committed.
2. **.dockerignore** – Added `config.toml` so local config with secrets is never sent to the build context.
3. **Dockerfile** – Replaced `COPY --from=builder /app/config.toml` with `COPY config.toml.example /app/config.toml` so the image has a default config and no secrets.
4. **docker-compose.yml** – Replaced Russian comments with English (project rule: no Russian in code).

---

## 7. Recommended next steps

1. Rotate Telegram bot tokens and DB password if there is any chance they were ever committed or shared.
2. In production, use only environment variables (or a secrets manager) for tokens and `DATABASE_URL`.
3. Optionally run `cargo audit` and `cargo outdated` periodically for dependency security and updates.
4. Keep `config.toml` and `.env*` only on developer machines and deploy hosts; never add them to the repo.
