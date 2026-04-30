-- Add mini-game type and state to duel_game.
-- game_type: 'tictactoe' | 'dice' | 'coin' | 'rps'
-- game_state: JSONB for per-game state (dice rolls, coin pick, rps choices)
ALTER TABLE duel_game
  ADD COLUMN IF NOT EXISTS game_type  VARCHAR(20) NOT NULL DEFAULT 'tictactoe',
  ADD COLUMN IF NOT EXISTS game_state JSONB;
