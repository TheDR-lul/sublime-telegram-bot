-- Expand duel_elo to track pidor_elo (from pidor/bet wins) and huya_elo (from dick fights).
-- total_elo = elo + pidor_elo + huya_elo is computed in Rust.
ALTER TABLE duel_elo
  ADD COLUMN IF NOT EXISTS pidor_elo INT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS huya_elo  INT NOT NULL DEFAULT 0;
