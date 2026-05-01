-- Indexes for retention cleanup jobs.

CREATE INDEX IF NOT EXISTS idx_duel_game_status_created_at
  ON duel_game(status, created_at);

CREATE INDEX IF NOT EXISTS idx_huya_fight_status_created_at
  ON huya_fight(status, created_at);

CREATE INDEX IF NOT EXISTS idx_huya_raid_status_created_at
  ON huya_raid(status, created_at);

CREATE INDEX IF NOT EXISTS idx_huya_loot_log_created_at
  ON huya_loot_log(created_at);

CREATE INDEX IF NOT EXISTS idx_huya_reforge_log_created_at
  ON huya_reforge_log(created_at);

CREATE INDEX IF NOT EXISTS idx_rpg_battle_status_updated_at
  ON rpg_battle(status, updated_at);

CREATE INDEX IF NOT EXISTS idx_chat_member_last_seen_at
  ON chat_member(last_seen_at);

CREATE INDEX IF NOT EXISTS idx_tiktoklink_updated_at
  ON tiktoklink(updated_at);
