-- Add per-chat language preference to game table.
-- Defaults to 'ru'. Supported values: 'ru' (more can be added later).
ALTER TABLE game ADD COLUMN IF NOT EXISTS lang VARCHAR(5) NOT NULL DEFAULT 'ru';
