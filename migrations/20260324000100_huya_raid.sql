-- Party raid mode for huya: up to 5 attackers vs one target.
CREATE TABLE IF NOT EXISTS huya_raid (
  id                  SERIAL PRIMARY KEY,
  chat_id             BIGINT NOT NULL,
  leader_tg_id        BIGINT NOT NULL,
  target_tg_id        BIGINT NOT NULL,
  status              VARCHAR(12) NOT NULL DEFAULT 'pending',
  target_accepted     BOOLEAN NOT NULL DEFAULT FALSE,
  message_id          INT NOT NULL DEFAULT 0,
  round               INT NOT NULL DEFAULT 1,
  turn_index          INT NOT NULL DEFAULT 0,
  focus_round         INT NOT NULL DEFAULT 0,
  initiative_order    TEXT NOT NULL DEFAULT '',
  max_rounds          INT NOT NULL DEFAULT 8,
  created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  expires_at          TIMESTAMPTZ NOT NULL DEFAULT (NOW() + INTERVAL '20 minutes')
);

CREATE TABLE IF NOT EXISTS huya_raid_member (
  id                  SERIAL PRIMARY KEY,
  raid_id             INT NOT NULL REFERENCES huya_raid(id) ON DELETE CASCADE,
  tg_id               BIGINT NOT NULL,
  side                VARCHAR(10) NOT NULL,
  slot                INT NOT NULL DEFAULT 0,
  hp_snapshot         INT NOT NULL DEFAULT 100,
  is_alive            BOOLEAN NOT NULL DEFAULT TRUE,
  acted_in_round      BOOLEAN NOT NULL DEFAULT FALSE,
  guard_until_round   INT NOT NULL DEFAULT 0,
  damage_done         INT NOT NULL DEFAULT 0,
  created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(raid_id, tg_id)
);

CREATE INDEX IF NOT EXISTS huya_raid_status_idx ON huya_raid(chat_id, status, created_at);
CREATE INDEX IF NOT EXISTS huya_raid_member_raid_idx ON huya_raid_member(raid_id, side, slot);
CREATE UNIQUE INDEX IF NOT EXISTS huya_raid_chat_one_live
  ON huya_raid(chat_id)
  WHERE status IN ('pending', 'active');
