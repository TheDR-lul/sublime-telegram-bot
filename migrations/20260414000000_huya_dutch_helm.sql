CREATE TABLE IF NOT EXISTS huya_dutch_helm_event (
    id SERIAL PRIMARY KEY,
    event_date DATE NOT NULL UNIQUE,
    start_at TIMESTAMPTZ NOT NULL,
    join_deadline_at TIMESTAMPTZ NOT NULL,
    status TEXT NOT NULL DEFAULT 'scheduled',
    seed INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    started_at TIMESTAMPTZ NULL,
    finished_at TIMESTAMPTZ NULL
);

CREATE TABLE IF NOT EXISTS huya_dutch_helm_participant (
    event_id INTEGER NOT NULL REFERENCES huya_dutch_helm_event(id) ON DELETE CASCADE,
    chat_id BIGINT NOT NULL,
    tg_id BIGINT NOT NULL,
    joined_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    reward_mm INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (event_id, chat_id, tg_id)
);

CREATE INDEX IF NOT EXISTS idx_huya_dh_participant_event_chat
    ON huya_dutch_helm_participant(event_id, chat_id);
