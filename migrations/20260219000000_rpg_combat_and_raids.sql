-- RPG combat expansion: mobs, skills, loot tables.

-- Mob templates for PvE encounters.
CREATE TABLE IF NOT EXISTS rpg_mob_template (
    id                SERIAL PRIMARY KEY,
    code              TEXT    NOT NULL UNIQUE,
    name              TEXT    NOT NULL,
    description       TEXT    NOT NULL,
    base_stats        JSONB   NOT NULL, -- e.g. {"hp": 50, "attack_phys": 10, "defense_phys": 5}
    loot_table_code   TEXT,
    recommended_level INT     NOT NULL DEFAULT 1
);

-- Optional encounter table for biome-based mob groups.
CREATE TABLE IF NOT EXISTS rpg_encounter (
    id          SERIAL PRIMARY KEY,
    code        TEXT    NOT NULL UNIQUE,
    biome       TEXT    NOT NULL,
    min_level   INT     NOT NULL,
    max_level   INT     NOT NULL,
    mob_codes   JSONB   NOT NULL -- e.g. ["mob_wolf", "mob_wolf_elite"]
);

-- Skills used in combat.
CREATE TABLE IF NOT EXISTS rpg_skill (
    id             SERIAL PRIMARY KEY,
    code           TEXT    NOT NULL UNIQUE,
    name           TEXT    NOT NULL,
    description    TEXT    NOT NULL,
    cost_stamina   INT     NOT NULL DEFAULT 0,
    cooldown_turns INT     NOT NULL DEFAULT 0,
    target_type    TEXT    NOT NULL, -- 'self', 'enemy', 'all_enemies', 'ally', 'all_allies'
    effect_type    TEXT    NOT NULL, -- 'damage_phys', 'damage_magic', 'heal', 'buff', 'debuff', etc.
    scaling        JSONB   NOT NULL  -- e.g. {"strength": 1.1, "attack_phys": 0.3}
);

-- Loot table linking sources (mobs/biomes/bosses) to items.
CREATE TABLE IF NOT EXISTS rpg_loot_table (
    id           SERIAL PRIMARY KEY,
    source_code  TEXT    NOT NULL, -- mob code, biome code or boss code
    biome        TEXT,
    min_level    INT,
    max_level    INT,
    item_id      INT     NOT NULL REFERENCES rpg_item(id) ON DELETE CASCADE,
    weight       INT     NOT NULL DEFAULT 1
);

CREATE INDEX IF NOT EXISTS idx_rpg_loot_table_source
    ON rpg_loot_table(source_code);

-- Guild extension: optional emblem/settings for UX.
ALTER TABLE guild
    ADD COLUMN IF NOT EXISTS emblem TEXT,
    ADD COLUMN IF NOT EXISTS settings JSONB NOT NULL DEFAULT '{}'::jsonb;

CREATE INDEX IF NOT EXISTS idx_guild_name_tag
    ON guild(name, tag);

-- Raid bosses and guild raids.
CREATE TABLE IF NOT EXISTS guild_boss_template (
    id                SERIAL PRIMARY KEY,
    code              TEXT    NOT NULL UNIQUE,
    name              TEXT    NOT NULL,
    description       TEXT    NOT NULL,
    recommended_level INT     NOT NULL,
    max_hp            BIGINT  NOT NULL,
    attack_pattern    JSONB   NOT NULL, -- phases/skills pattern
    loot_table_code   TEXT
);

CREATE TABLE IF NOT EXISTS guild_raid (
    id          SERIAL PRIMARY KEY,
    guild_id    INT     NOT NULL REFERENCES guild(id) ON DELETE CASCADE,
    boss_id     INT     NOT NULL REFERENCES guild_boss_template(id) ON DELETE RESTRICT,
    status      TEXT    NOT NULL, -- 'active', 'finished', 'failed'
    hp_left     BIGINT  NOT NULL,
    phase_state JSONB   NOT NULL DEFAULT '{}'::jsonb,
    started_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    finished_at TIMESTAMPTZ
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_guild_raid_active
    ON guild_raid(guild_id)
    WHERE status = 'active';

CREATE TABLE IF NOT EXISTS guild_raid_participation (
    raid_id        INT     NOT NULL REFERENCES guild_raid(id) ON DELETE CASCADE,
    player_id      INT     NOT NULL REFERENCES rpg_player(id) ON DELETE CASCADE,
    damage_done    BIGINT  NOT NULL DEFAULT 0,
    healing_done   BIGINT  NOT NULL DEFAULT 0,
    hits           INT     NOT NULL DEFAULT 0,
    deaths         INT     NOT NULL DEFAULT 0,
    PRIMARY KEY (raid_id, player_id)
);

