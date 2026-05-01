DO $$
BEGIN
    -- shaft
    IF EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name='huya' AND column_name='skill_atk') THEN
        IF EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name='huya' AND column_name='skill_shaft') THEN
            ALTER TABLE huya DROP COLUMN skill_atk;
        ELSE
            ALTER TABLE huya RENAME COLUMN skill_atk TO skill_shaft;
        END IF;
    END IF;
    -- skin
    IF EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name='huya' AND column_name='skill_def') THEN
        IF EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name='huya' AND column_name='skill_skin') THEN
            ALTER TABLE huya DROP COLUMN skill_def;
        ELSE
            ALTER TABLE huya RENAME COLUMN skill_def TO skill_skin;
        END IF;
    END IF;
    -- balls
    IF EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name='huya' AND column_name='skill_hp') THEN
        IF EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name='huya' AND column_name='skill_balls') THEN
            ALTER TABLE huya DROP COLUMN skill_hp;
        ELSE
            ALTER TABLE huya RENAME COLUMN skill_hp TO skill_balls;
        END IF;
    END IF;
    -- cunning
    IF EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name='huya' AND column_name='skill_luck') THEN
        IF EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name='huya' AND column_name='skill_cunning') THEN
            ALTER TABLE huya DROP COLUMN skill_luck;
        ELSE
            ALTER TABLE huya RENAME COLUMN skill_luck TO skill_cunning;
        END IF;
    END IF;
    -- stamina
    IF EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name='huya' AND column_name='skill_regen') THEN
        IF EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name='huya' AND column_name='skill_stamina') THEN
            ALTER TABLE huya DROP COLUMN skill_regen;
        ELSE
            ALTER TABLE huya RENAME COLUMN skill_regen TO skill_stamina;
        END IF;
    END IF;
END $$;

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
