-- Rename Tier 1 skill columns to thematic names.
ALTER TABLE huya RENAME COLUMN skill_atk   TO skill_shaft;
ALTER TABLE huya RENAME COLUMN skill_def   TO skill_skin;
ALTER TABLE huya RENAME COLUMN skill_hp    TO skill_balls;
ALTER TABLE huya RENAME COLUMN skill_luck  TO skill_cunning;
ALTER TABLE huya RENAME COLUMN skill_regen TO skill_stamina;

-- Tier 2: specialisation branches (unlock when T1 parent >= 8).
ALTER TABLE huya
  ADD COLUMN IF NOT EXISTS skill_pierce     INT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS skill_scales     INT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS skill_spirit     INT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS skill_pickpocket INT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS skill_dynamo     INT NOT NULL DEFAULT 0;

-- Tier 3: cross-branch combos (unlock when two T2 parents >= 5).
ALTER TABLE huya
  ADD COLUMN IF NOT EXISTS skill_eggtwist    INT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS skill_bloodsucker INT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS skill_ironballs   INT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS skill_vortex      INT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS skill_phantom     INT NOT NULL DEFAULT 0;

-- Tier 4: hidden skills (unlock when T3 parent >= 7; shown as ??? until then).
ALTER TABLE huya
  ADD COLUMN IF NOT EXISTS skill_berserker INT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS skill_vampire   INT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS skill_fortress  INT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS skill_speedrun  INT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS skill_ghost     INT NOT NULL DEFAULT 0;

-- Tier 5: legendary skills (shown as secret until complex prereqs are met).
ALTER TABLE huya
  ADD COLUMN IF NOT EXISTS skill_eternal  INT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS skill_absolute INT NOT NULL DEFAULT 0;

-- Fight outcome counters (used for hidden skill unlock conditions and /huyatop).
ALTER TABLE huya
  ADD COLUMN IF NOT EXISTS fights_won  INT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS fights_lost INT NOT NULL DEFAULT 0;
