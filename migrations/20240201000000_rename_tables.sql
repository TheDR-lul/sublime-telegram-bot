-- Align table names with Rust sqlx queries

-- Users table
ALTER TABLE IF EXISTS tg_user RENAME TO tguser;

-- Game tables
ALTER TABLE IF EXISTS game_player RENAME TO gameplayer;
ALTER TABLE IF EXISTS game_result RENAME TO gameresult;

-- KV store
ALTER TABLE IF EXISTS kv_item RENAME TO kvitem;

-- TikTok links
ALTER TABLE IF EXISTS tiktok_link RENAME TO tiktoklink;

