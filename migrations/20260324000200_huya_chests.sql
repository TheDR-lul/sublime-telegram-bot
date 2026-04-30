-- Gacha chests, daily claim, and richer inventory item metadata.

ALTER TABLE huya
  ADD COLUMN IF NOT EXISTS steal_boost INT NOT NULL DEFAULT 0;

ALTER TABLE huya_inventory
  ADD COLUMN IF NOT EXISTS rarity TEXT NOT NULL DEFAULT 'common',
  ADD COLUMN IF NOT EXISTS item_kind TEXT NOT NULL DEFAULT 'equipment',
  ADD COLUMN IF NOT EXISTS slot TEXT,
  ADD COLUMN IF NOT EXISTS trait TEXT,
  ADD COLUMN IF NOT EXISTS roll INT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS charges INT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS sell_price_mm INT NOT NULL DEFAULT 5,
  ADD COLUMN IF NOT EXISTS booster_effect TEXT,
  ADD COLUMN IF NOT EXISTS booster_value INT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS booster_scope TEXT;

CREATE TABLE IF NOT EXISTS huya_chest_def (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  price_mm INT NOT NULL,
  daily_free BOOLEAN NOT NULL DEFAULT FALSE,
  enabled BOOLEAN NOT NULL DEFAULT TRUE
);

CREATE TABLE IF NOT EXISTS huya_drop_table (
  id TEXT PRIMARY KEY
);

CREATE TABLE IF NOT EXISTS huya_drop_entry (
  id SERIAL PRIMARY KEY,
  table_id TEXT NOT NULL REFERENCES huya_drop_table(id) ON DELETE CASCADE,
  rarity TEXT NOT NULL,
  weight INT NOT NULL CHECK (weight > 0)
);

CREATE TABLE IF NOT EXISTS huya_daily_chest_claim (
  chat_id BIGINT NOT NULL,
  tg_id BIGINT NOT NULL,
  chest_id TEXT NOT NULL REFERENCES huya_chest_def(id) ON DELETE CASCADE,
  claimed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (chat_id, tg_id, chest_id)
);

CREATE TABLE IF NOT EXISTS huya_loot_log (
  id SERIAL PRIMARY KEY,
  chat_id BIGINT NOT NULL,
  tg_id BIGINT NOT NULL,
  chest_id TEXT NOT NULL,
  item_id TEXT NOT NULL,
  rarity TEXT NOT NULL,
  item_kind TEXT NOT NULL,
  rolled_trait TEXT,
  roll INT NOT NULL DEFAULT 0,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_huya_loot_log_owner ON huya_loot_log (chat_id, tg_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_huya_inventory_rarity ON huya_inventory (chat_id, tg_id, rarity, acquired_at DESC);

INSERT INTO huya_chest_def (id, name, price_mm, daily_free, enabled) VALUES
  ('cheap_crate', 'Дешман-ящик', 35, FALSE, TRUE),
  ('fighter_crate', 'Бойцовский ларец', 90, FALSE, TRUE),
  ('royal_crate', 'Царский сундук', 220, FALSE, TRUE),
  ('daily_free_crate', 'Халявный сундук', 0, TRUE, TRUE)
ON CONFLICT (id) DO NOTHING;

INSERT INTO huya_drop_table (id) VALUES
  ('cheap_crate'),
  ('fighter_crate'),
  ('royal_crate'),
  ('daily_free_crate')
ON CONFLICT (id) DO NOTHING;

-- Weight by rarity; exact item roll happens in app logic.
INSERT INTO huya_drop_entry (table_id, rarity, weight) VALUES
  ('cheap_crate', 'trash', 50),
  ('cheap_crate', 'common', 40),
  ('cheap_crate', 'rare', 10),
  ('fighter_crate', 'common', 45),
  ('fighter_crate', 'rare', 38),
  ('fighter_crate', 'epic', 15),
  ('fighter_crate', 'legendary', 2),
  ('royal_crate', 'rare', 45),
  ('royal_crate', 'epic', 40),
  ('royal_crate', 'legendary', 15),
  ('daily_free_crate', 'trash', 55),
  ('daily_free_crate', 'common', 40),
  ('daily_free_crate', 'rare', 5)
ON CONFLICT DO NOTHING;
