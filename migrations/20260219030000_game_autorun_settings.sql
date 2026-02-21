-- Per-chat settings for "внезапный пидор" (autorun). Admins can disable entirely or per slot (morning/day/evening).
ALTER TABLE game
  ADD COLUMN IF NOT EXISTS autorun_enabled BOOLEAN NOT NULL DEFAULT true,
  ADD COLUMN IF NOT EXISTS autorun_morning BOOLEAN NOT NULL DEFAULT true,
  ADD COLUMN IF NOT EXISTS autorun_day BOOLEAN NOT NULL DEFAULT true,
  ADD COLUMN IF NOT EXISTS autorun_evening BOOLEAN NOT NULL DEFAULT true;
