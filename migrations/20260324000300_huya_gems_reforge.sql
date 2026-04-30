-- Gems, sockets, and reforging metadata for huya inventory.

ALTER TABLE huya_inventory
  ADD COLUMN IF NOT EXISTS socket_capacity INT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS reforge_level INT NOT NULL DEFAULT 0;

CREATE TABLE IF NOT EXISTS huya_item_socket (
  id                SERIAL PRIMARY KEY,
  chat_id           BIGINT NOT NULL,
  tg_id             BIGINT NOT NULL,
  item_inventory_id INT NOT NULL REFERENCES huya_inventory(id) ON DELETE CASCADE,
  socket_index      INT NOT NULL CHECK (socket_index >= 1),
  gem_item_id       TEXT NOT NULL,
  gem_trait         TEXT,
  gem_roll          INT NOT NULL DEFAULT 0,
  gem_rarity        TEXT NOT NULL DEFAULT 'common',
  created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE (item_inventory_id, socket_index)
);

CREATE INDEX IF NOT EXISTS idx_huya_item_socket_owner
  ON huya_item_socket (chat_id, tg_id, item_inventory_id);

CREATE TABLE IF NOT EXISTS huya_reforge_log (
  id                SERIAL PRIMARY KEY,
  chat_id           BIGINT NOT NULL,
  tg_id             BIGINT NOT NULL,
  item_inventory_id INT NOT NULL,
  catalyst_item_id  TEXT,
  old_roll          INT NOT NULL DEFAULT 0,
  new_roll          INT NOT NULL DEFAULT 0,
  old_reforge_level INT NOT NULL DEFAULT 0,
  new_reforge_level INT NOT NULL DEFAULT 0,
  outcome           TEXT NOT NULL,
  created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_huya_reforge_log_owner
  ON huya_reforge_log (chat_id, tg_id, created_at DESC);
