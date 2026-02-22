-- Pidor duel: tic-tac-toe with cell TTL, open or tagged challenge, 1 min accept timeout.

CREATE TABLE IF NOT EXISTS duel_game (
    id                SERIAL PRIMARY KEY,
    chat_id           BIGINT NOT NULL,
    invite_message_id  BIGINT,
    message_id         BIGINT,
    challenger_tg_id   BIGINT NOT NULL,
    invited_tg_id      BIGINT,
    player1_tg_id      BIGINT,
    player2_tg_id      BIGINT,
    board              CHAR(9) NOT NULL DEFAULT '         ',
    cell_filled_at     VARCHAR(90) NOT NULL DEFAULT '0,0,0,0,0,0,0,0,0',
    turn               SMALLINT NOT NULL DEFAULT 1,
    status             VARCHAR(32) NOT NULL DEFAULT 'pending_accept',
    winner_tg_id       BIGINT,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_duel_game_chat_status ON duel_game(chat_id, status);
CREATE INDEX IF NOT EXISTS idx_duel_game_created_at ON duel_game(created_at) WHERE status = 'pending_accept';
