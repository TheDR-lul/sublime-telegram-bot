-- HP, skills, shop boosts for the HuyActa game.
-- hp        : current HP (persists between fights; replenishes via consume_action)
-- skill_*   : skill levels (0-5), each level grants bonuses in fights/steal
-- *_boost   : temporary combat/grow bonuses purchased from the shop; reset after use
-- skill_points: unspent points earned on level-up (1 per level)
ALTER TABLE huya
  ADD COLUMN IF NOT EXISTS hp           INT NOT NULL DEFAULT 100,
  ADD COLUMN IF NOT EXISTS skill_points INT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS skill_atk    INT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS skill_def    INT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS skill_hp     INT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS skill_luck   INT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS skill_regen  INT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS atk_boost    INT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS def_boost    INT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS grow_boost   INT NOT NULL DEFAULT 0;

-- ch_hp / tg_hp: HP snapshot at fight start, decremented each round.
-- round: current round number (1-based); fight ends when either HP ≤ 0 or round > 5.
ALTER TABLE huya_fight
  ADD COLUMN IF NOT EXISTS ch_hp INT NOT NULL DEFAULT 100,
  ADD COLUMN IF NOT EXISTS tg_hp INT NOT NULL DEFAULT 100,
  ADD COLUMN IF NOT EXISTS round INT NOT NULL DEFAULT 1;
