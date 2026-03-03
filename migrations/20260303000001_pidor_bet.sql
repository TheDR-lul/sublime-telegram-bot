CREATE TABLE IF NOT EXISTS pidor_bet (
    id            SERIAL PRIMARY KEY,
    chat_id       BIGINT NOT NULL,
    bettor_tg_id  BIGINT NOT NULL,
    target_tg_id  BIGINT NOT NULL,
    year          INT NOT NULL,
    day           INT NOT NULL,
    slot          VARCHAR(16) NOT NULL DEFAULT 'manual',
    correct       BOOLEAN,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (chat_id, bettor_tg_id, year, day, slot)
);
