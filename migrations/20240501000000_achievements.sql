-- Achievements table: per-user flags for fun achievements.

CREATE TABLE IF NOT EXISTS achievement (
    id         SERIAL PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES tguser(id) ON DELETE CASCADE,
    code       TEXT NOT NULL,
    earned_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT achievement_user_code_unique UNIQUE (user_id, code)
);

CREATE INDEX IF NOT EXISTS idx_achievement_user_id ON achievement(user_id);

