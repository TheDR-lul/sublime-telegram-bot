-- Raid timeout/override and energy purchase scaling.

ALTER TABLE huya_raid
  ADD COLUMN IF NOT EXISTS power_override_by_target BOOLEAN NOT NULL DEFAULT FALSE,
  ADD COLUMN IF NOT EXISTS turn_deadline_at TIMESTAMPTZ;

ALTER TABLE huya
  ADD COLUMN IF NOT EXISTS energy_buys_today INT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS energy_buys_reset_at DATE NOT NULL DEFAULT CURRENT_DATE;
