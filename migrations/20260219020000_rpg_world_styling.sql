-- World styling: oceans, roads between settlements, and basic biome tweaks.

-- Add ocean border around the 50x50 world.
UPDATE rpg_map_tile
SET biome = 'water', min_level = NULL, max_level = NULL
WHERE (x BETWEEN 0 AND 49 AND (y <= 1 OR y >= 48))
   OR (y BETWEEN 0 AND 49 AND (x <= 1 OR x >= 48));

-- Add simple roads between starter village (10,10), city (25,25) and capital (40,5).

-- Road from village_start (10,10) to city_start (25,25): go east then south.
UPDATE rpg_map_tile
SET biome = 'road', min_level = 1, max_level = 3
WHERE (y = 10 AND x BETWEEN 10 AND 25)
   OR (x = 25 AND y BETWEEN 10 AND 25);

-- Road from city_start (25,25) to capital (40,5): go north then east.
UPDATE rpg_map_tile
SET biome = 'road', min_level = 5, max_level = 15
WHERE (x = 25 AND y BETWEEN 5 AND 25)
   OR (y = 5 AND x BETWEEN 25 AND 40);

-- Soften biome borders a bit near village and city (more plains around them).
UPDATE rpg_map_tile
SET biome = 'plain'
WHERE (x BETWEEN 7 AND 13 AND y BETWEEN 7 AND 13)
  AND biome NOT IN ('village_plains', 'city_small', 'capital');

UPDATE rpg_map_tile
SET biome = 'plain'
WHERE (x BETWEEN 22 AND 28 AND y BETWEEN 22 AND 28)
  AND biome NOT IN ('village_plains', 'city_small', 'capital');

