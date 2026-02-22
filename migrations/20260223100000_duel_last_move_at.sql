-- Track last move time for active duels; used to cancel duels with no moves for 1+ minute.

ALTER TABLE duel_game ADD COLUMN IF NOT EXISTS last_move_at TIMESTAMPTZ DEFAULT NULL;

UPDATE duel_game SET last_move_at = created_at WHERE status = 'active' AND last_move_at IS NULL;
