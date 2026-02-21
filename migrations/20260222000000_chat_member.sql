-- Track users seen in a chat (for "call unregistered" admin feature).
-- Filled when users send game-related messages in the chat.
CREATE TABLE IF NOT EXISTS chat_member (
    chat_id   BIGINT NOT NULL,
    user_id   INTEGER NOT NULL REFERENCES tguser(id) ON DELETE CASCADE,
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (chat_id, user_id)
);
