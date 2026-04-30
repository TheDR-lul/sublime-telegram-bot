-- Slot: 'manual' = Pidor of the Day (manual, 1/day); 'morning'/'day'/'evening' = autorun (3/day).
ALTER TABLE gameresult ADD COLUMN IF NOT EXISTS slot TEXT;
UPDATE gameresult SET slot = 'manual' WHERE slot IS NULL;
ALTER TABLE gameresult ALTER COLUMN slot SET NOT NULL;
ALTER TABLE gameresult ALTER COLUMN slot SET DEFAULT 'manual';

DROP INDEX IF EXISTS idx_gameresult_game_year_day;
CREATE UNIQUE INDEX IF NOT EXISTS idx_gameresult_game_year_day_slot ON gameresult (game_id, year, day, slot);
