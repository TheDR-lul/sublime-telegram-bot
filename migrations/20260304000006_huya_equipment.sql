-- Inventory and equipment tables for the Huya tamagotchi game.
-- huya_inventory: individual items owned by a player.
-- huya_equipment: which inventory item is equipped into which slot.

CREATE TABLE IF NOT EXISTS huya_inventory (
  id          SERIAL PRIMARY KEY,
  chat_id     BIGINT NOT NULL,
  tg_id       BIGINT NOT NULL,
  item_id     TEXT   NOT NULL,
  acquired_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_huya_inventory_owner
  ON huya_inventory (chat_id, tg_id);

-- slot: 'ring_1'..'ring_6', 'tip', 'base', 'balls'
CREATE TABLE IF NOT EXISTS huya_equipment (
  chat_id      BIGINT NOT NULL,
  tg_id        BIGINT NOT NULL,
  slot         TEXT   NOT NULL,
  inventory_id INT    NOT NULL REFERENCES huya_inventory(id) ON DELETE CASCADE,
  PRIMARY KEY (chat_id, tg_id, slot)
);

CREATE INDEX IF NOT EXISTS idx_huya_equipment_owner
  ON huya_equipment (chat_id, tg_id);

