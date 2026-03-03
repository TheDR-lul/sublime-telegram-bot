-- HuyActa tamagotchi: each user per chat grows a dick (or ass if negative).
-- length_mm stored as integer millimetres; negative = ass mode.
-- actions_left resets to 4 each day (Kyiv midnight), tracked by actions_reset_at.
CREATE TABLE IF NOT EXISTS huya (
  id               SERIAL PRIMARY KEY,
  chat_id          BIGINT       NOT NULL,
  tg_id            BIGINT       NOT NULL,
  length_mm        INT          NOT NULL DEFAULT 10,
  level            INT          NOT NULL DEFAULT 1,
  xp               INT          NOT NULL DEFAULT 0,
  actions_left     INT          NOT NULL DEFAULT 4,
  actions_reset_at DATE         NOT NULL DEFAULT CURRENT_DATE,
  created_at       TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
  UNIQUE(chat_id, tg_id)
);

CREATE INDEX IF NOT EXISTS huya_chat_length ON huya(chat_id, length_mm DESC);
