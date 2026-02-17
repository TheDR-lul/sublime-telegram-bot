
-- Users table (matches TgUser / tguser in queries)
CREATE TABLE IF NOT EXISTS tguser (
    id              SERIAL PRIMARY KEY,
    tg_id           BIGINT NOT NULL UNIQUE,
    username        TEXT,
    first_name      TEXT NOT NULL,
    last_name       TEXT,
    lang_code       TEXT NOT NULL,
    is_blocked      BOOLEAN NOT NULL DEFAULT FALSE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Game per chat (matches Game / game)
CREATE TABLE IF NOT EXISTS game (
    id       SERIAL PRIMARY KEY,
    chat_id  BIGINT NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS gameplayer (
    game_id  INTEGER NOT NULL REFERENCES game(id) ON DELETE CASCADE,
    user_id  INTEGER NOT NULL REFERENCES tguser(id) ON DELETE CASCADE,
    PRIMARY KEY (game_id, user_id)
);

-- Pidor of the day results (matches gameresult)
CREATE TABLE IF NOT EXISTS gameresult (
    id         SERIAL PRIMARY KEY,
    game_id    INTEGER NOT NULL REFERENCES game(id) ON DELETE CASCADE,
    winner_id  INTEGER NOT NULL REFERENCES tguser(id),
    year       INTEGER NOT NULL,
    day        INTEGER NOT NULL
);

-- KV store per chat (matches kvitem)
CREATE TABLE IF NOT EXISTS kvitem (
    id         SERIAL PRIMARY KEY,
    chat_id    BIGINT NOT NULL,
    key        TEXT NOT NULL,
    value      TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT kv_item_chat_key_unique UNIQUE (chat_id, key)
);

-- TikTok links cache (matches tiktoklink)
CREATE TABLE IF NOT EXISTS tiktoklink (
    id                   SERIAL PRIMARY KEY,
    link                 TEXT NOT NULL,
    share_link           TEXT,
    telegram_message_id  TEXT NOT NULL,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

