-- RPG MVP schema for Pidor-Royale.

-- Players in RPG (one per tguser).
CREATE TABLE IF NOT EXISTS rpg_player (
    id                  SERIAL PRIMARY KEY,
    user_id             INTEGER NOT NULL REFERENCES tguser(id) ON DELETE CASCADE,
    level               INTEGER NOT NULL DEFAULT 1,
    xp                  BIGINT  NOT NULL DEFAULT 0,
    xp_to_next          BIGINT  NOT NULL DEFAULT 100,

    hp_max              INTEGER NOT NULL DEFAULT 100,
    hp_current          INTEGER NOT NULL DEFAULT 100,

    stamina_max         INTEGER NOT NULL DEFAULT 100,
    stamina_current     INTEGER NOT NULL DEFAULT 100,
    stamina_updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    strength            INTEGER NOT NULL DEFAULT 1,
    agility             INTEGER NOT NULL DEFAULT 1,
    intellect           INTEGER NOT NULL DEFAULT 1,
    vitality            INTEGER NOT NULL DEFAULT 1,
    luck                INTEGER NOT NULL DEFAULT 1,

    -- Position on global map.
    pos_x               INTEGER NOT NULL DEFAULT 0,
    pos_y               INTEGER NOT NULL DEFAULT 0,

    class_code          TEXT,

    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_rpg_player_user_id ON rpg_player(user_id);

-- Map tiles for RPG world.
CREATE TABLE IF NOT EXISTS rpg_map_tile (
    x           INTEGER NOT NULL,
    y           INTEGER NOT NULL,
    biome       TEXT    NOT NULL,
    object_code TEXT,
    min_level   INTEGER,
    max_level   INTEGER,
    flags       INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (x, y)
);

-- UI state per user/chat/message for RPG menus.
CREATE TABLE IF NOT EXISTS rpg_ui_state (
    id           SERIAL PRIMARY KEY,
    user_id      INTEGER NOT NULL REFERENCES tguser(id) ON DELETE CASCADE,
    chat_id      BIGINT  NOT NULL,
    message_id   BIGINT  NOT NULL,
    mode         TEXT    NOT NULL,
    submode      TEXT,
    payload_json JSONB   NOT NULL DEFAULT '{}'::jsonb,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_rpg_ui_state_user_chat ON rpg_ui_state(user_id, chat_id);

-- Battles (PvE / PvP).
CREATE TABLE IF NOT EXISTS rpg_battle (
    id              SERIAL PRIMARY KEY,
    type            TEXT    NOT NULL, -- 'pvp' or 'pve'
    player1_id      INTEGER NOT NULL REFERENCES rpg_player(id) ON DELETE CASCADE,
    player2_id      INTEGER REFERENCES rpg_player(id) ON DELETE SET NULL,
    mob_group_code  TEXT,
    turn_player_id  INTEGER REFERENCES rpg_player(id) ON DELETE SET NULL,
    status          TEXT    NOT NULL, -- 'pending', 'active', 'finished', 'timeout'
    state_json      JSONB   NOT NULL DEFAULT '{}'::jsonb,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    finished_at     TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_rpg_battle_status ON rpg_battle(status);
CREATE INDEX IF NOT EXISTS idx_rpg_battle_players ON rpg_battle(player1_id, player2_id);

-- Items and inventory.
CREATE TABLE IF NOT EXISTS rpg_item (
    id          SERIAL PRIMARY KEY,
    code        TEXT    NOT NULL UNIQUE,
    name        TEXT    NOT NULL,
    description TEXT    NOT NULL,
    item_type   TEXT    NOT NULL, -- 'consumable', 'weapon', 'armor', 'accessory', 'key', 'quest'
    slot        TEXT,
    rarity      TEXT    NOT NULL, -- 'common', 'uncommon', 'rare', 'epic', 'legendary'
    base_stats  JSONB   NOT NULL DEFAULT '{}'::jsonb,
    effects     JSONB   NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE IF NOT EXISTS rpg_inventory (
    id          SERIAL PRIMARY KEY,
    player_id   INTEGER NOT NULL REFERENCES rpg_player(id) ON DELETE CASCADE,
    item_id     INTEGER NOT NULL REFERENCES rpg_item(id) ON DELETE CASCADE,
    quantity    INTEGER NOT NULL DEFAULT 1,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_rpg_inventory_player ON rpg_inventory(player_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_rpg_inventory_player_item_unique
    ON rpg_inventory(player_id, item_id);

CREATE TABLE IF NOT EXISTS rpg_equipment (
    player_id   INTEGER NOT NULL REFERENCES rpg_player(id) ON DELETE CASCADE,
    slot        TEXT    NOT NULL,
    item_id     INTEGER NOT NULL REFERENCES rpg_item(id) ON DELETE CASCADE,
    equipped_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (player_id, slot)
);

-- Guilds and members (phase 1).
CREATE TABLE IF NOT EXISTS guild (
    id               SERIAL PRIMARY KEY,
    name             TEXT    NOT NULL UNIQUE,
    tag              TEXT    NOT NULL UNIQUE,
    leader_player_id INTEGER NOT NULL REFERENCES rpg_player(id) ON DELETE RESTRICT,
    level            INTEGER NOT NULL DEFAULT 1,
    xp               BIGINT  NOT NULL DEFAULT 0,
    description      TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS guild_member (
    guild_id   INTEGER NOT NULL REFERENCES guild(id) ON DELETE CASCADE,
    player_id  INTEGER NOT NULL REFERENCES rpg_player(id) ON DELETE CASCADE,
    role       TEXT    NOT NULL, -- 'leader', 'officer', 'member', 'recruit'
    joined_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    contribution BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (guild_id, player_id)
);

CREATE INDEX IF NOT EXISTS idx_guild_member_player ON guild_member(player_id);

