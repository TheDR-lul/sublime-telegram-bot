-- Ensure one result per game per calendar day (required for INSERT ... ON CONFLICT (game_id, year, day) DO NOTHING).
DELETE FROM gameresult
WHERE id IN (
    SELECT id
    FROM (
        SELECT id,
               ROW_NUMBER() OVER (PARTITION BY game_id, year, day ORDER BY id DESC) as row_num
        FROM gameresult
    ) t
    WHERE t.row_num > 1
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_gameresult_game_year_day ON gameresult (game_id, year, day);
