-- Add position columns to rpg_player for existing databases.

ALTER TABLE rpg_player
    ADD COLUMN IF NOT EXISTS pos_x INTEGER NOT NULL DEFAULT 0;

ALTER TABLE rpg_player
    ADD COLUMN IF NOT EXISTS pos_y INTEGER NOT NULL DEFAULT 0;

