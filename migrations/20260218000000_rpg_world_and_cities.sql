-- Initial world and city data for RPG module.

-- Global map: simple 50x50 grid with a few biomes and two settlements.
DO $$
DECLARE
    vx INT;
    vy INT;
BEGIN
    FOR vx IN 0..49 LOOP
        FOR vy IN 0..49 LOOP
            INSERT INTO rpg_map_tile (x, y, biome, min_level, max_level)
            VALUES (
                vx,
                vy,
                CASE
                    WHEN vx BETWEEN 20 AND 29 AND vy BETWEEN 20 AND 29 THEN 'forest'
                    WHEN vx BETWEEN 35 AND 49 AND vy BETWEEN 0 AND 9 THEN 'mountain'
                    WHEN vx BETWEEN 0 AND 9 AND vy BETWEEN 35 AND 49 THEN 'desert'
                    ELSE 'plain'
                END,
                1,
                10
            )
            ON CONFLICT (x, y) DO NOTHING;
        END LOOP;
    END LOOP;
END$$;

-- Small village near the center.
INSERT INTO rpg_map_tile (x, y, biome, min_level, max_level, object_code)
VALUES (10, 10, 'village_plains', 1, 5, 'village_start')
ON CONFLICT (x, y) DO UPDATE
SET biome = EXCLUDED.biome,
    min_level = EXCLUDED.min_level,
    max_level = EXCLUDED.max_level,
    object_code = EXCLUDED.object_code;

-- Small city near the center.
INSERT INTO rpg_map_tile (x, y, biome, min_level, max_level, object_code)
VALUES (25, 25, 'city_small', 3, 10, 'city_start')
ON CONFLICT (x, y) DO UPDATE
SET biome = EXCLUDED.biome,
    min_level = EXCLUDED.min_level,
    max_level = EXCLUDED.max_level,
    object_code = EXCLUDED.object_code;

-- Capital in the north.
INSERT INTO rpg_map_tile (x, y, biome, min_level, max_level, object_code)
VALUES (40, 5, 'capital', 10, 30, 'city_capital')
ON CONFLICT (x, y) DO UPDATE
SET biome = EXCLUDED.biome,
    min_level = EXCLUDED.min_level,
    max_level = EXCLUDED.max_level,
    object_code = EXCLUDED.object_code;

-- Basic city definitions.
CREATE TABLE IF NOT EXISTS rpg_city (
    id          SERIAL PRIMARY KEY,
    code        TEXT NOT NULL UNIQUE,
    name        TEXT NOT NULL,
    tier        INT  NOT NULL,
    description TEXT NOT NULL,
    map_width   INT  NOT NULL,
    map_height  INT  NOT NULL,
    entry_x     INT  NOT NULL,
    entry_y     INT  NOT NULL
);

CREATE TABLE IF NOT EXISTS rpg_city_tile (
    city_id      INT NOT NULL REFERENCES rpg_city(id) ON DELETE CASCADE,
    x            INT NOT NULL,
    y            INT NOT NULL,
    tile_type    TEXT NOT NULL,
    building_code TEXT,
    PRIMARY KEY (city_id, x, y)
);

-- Helper to insert a simple 5x5 city map.
DO $$
DECLARE
    v_city_id INT;
BEGIN
    -- Village.
    INSERT INTO rpg_city (code, name, tier, description, map_width, map_height, entry_x, entry_y)
    VALUES (
        'village_start',
        'Starter Village',
        1,
        'A small village with basic services.',
        5,
        5,
        2,
        4
    )
    ON CONFLICT (code) DO UPDATE
    SET name = EXCLUDED.name
    RETURNING id INTO v_city_id;

    INSERT INTO rpg_city_tile (city_id, x, y, tile_type, building_code) VALUES
        (v_city_id, 0, 0, 'house', NULL),
        (v_city_id, 1, 0, 'house', NULL),
        (v_city_id, 2, 0, 'square', NULL),
        (v_city_id, 3, 0, 'house', NULL),
        (v_city_id, 4, 0, 'house', NULL),
        (v_city_id, 1, 1, 'shop', 'shop_general'),
        (v_city_id, 3, 1, 'healer', 'healer_basic'),
        (v_city_id, 2, 2, 'square', NULL),
        (v_city_id, 2, 4, 'gate', 'gate_world');

    -- Small city.
    INSERT INTO rpg_city (code, name, tier, description, map_width, map_height, entry_x, entry_y)
    VALUES (
        'city_start',
        'First City',
        2,
        'A small city with guild hall and basic services.',
        5,
        5,
        2,
        4
    )
    ON CONFLICT (code) DO UPDATE
    SET name = EXCLUDED.name
    RETURNING id INTO v_city_id;

    INSERT INTO rpg_city_tile (city_id, x, y, tile_type, building_code) VALUES
        (v_city_id, 0, 0, 'house', NULL),
        (v_city_id, 1, 0, 'shop', 'shop_general'),
        (v_city_id, 2, 0, 'square', NULL),
        (v_city_id, 3, 0, 'forge', 'forge_main'),
        (v_city_id, 4, 0, 'house', NULL),
        (v_city_id, 1, 1, 'tavern', 'tavern_main'),
        (v_city_id, 2, 1, 'guild_house', 'guild_house_main'),
        (v_city_id, 3, 1, 'arena', 'arena_local'),
        (v_city_id, 2, 4, 'gate', 'gate_world');
END$$;

