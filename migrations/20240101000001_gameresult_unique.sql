-- Ensure one result per game per calendar day (required for INSERT ... ON CONFLICT (game_id, year, day) DO NOTHING).
CREATE UNIQUE INDEX IF NOT EXISTS idx_gameresult_game_year_day ON gameresult (game_id, year, day);
