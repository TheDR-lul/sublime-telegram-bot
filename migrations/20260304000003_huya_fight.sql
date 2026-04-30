-- Interactive PvP huya fight table.
-- challenger_pick / target_pick: 0=Напор, 1=Финт, 2=В шары, NULL=not chosen yet.
-- status: 'pending' (challenge sent) | 'active' (accepted, picking) | 'done'
CREATE TABLE IF NOT EXISTS huya_fight (
  id               SERIAL PRIMARY KEY,
  chat_id          BIGINT      NOT NULL,
  challenger_tg_id BIGINT      NOT NULL,
  target_tg_id     BIGINT      NOT NULL,
  challenger_pick  INT,
  target_pick      INT,
  status           VARCHAR(10) NOT NULL DEFAULT 'pending',
  message_id       INT         NOT NULL DEFAULT 0,
  created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS huya_fight_active ON huya_fight(chat_id, status, created_at);
