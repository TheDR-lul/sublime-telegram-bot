CREATE TABLE IF NOT EXISTS duel_elo (
    id          SERIAL PRIMARY KEY,
    chat_id     BIGINT NOT NULL,
    tg_id       BIGINT NOT NULL,
    elo         INT NOT NULL DEFAULT 1000,
    wins        INT NOT NULL DEFAULT 0,
    losses      INT NOT NULL DEFAULT 0,
    UNIQUE (chat_id, tg_id)
);
