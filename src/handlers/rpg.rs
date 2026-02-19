//! Handlers for Pidor-Royale RPG core: `/rpg` main menu.

use sqlx::{PgPool, Row};
use teloxide::prelude::*;
use teloxide::types::{
    CallbackQuery, InlineKeyboardButton, InlineKeyboardMarkup, Message, MaybeInaccessibleMessage,
    ParseMode,
};
use teloxide::utils::html::escape as escape_html;

use crate::db;
use crate::error::AppError;

const RPG_CB_PREFIX: &str = "rpg";

pub async fn rpg_menu_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    let chat_id = msg.chat.id;

    let from_user = match msg.from.as_ref() {
        Some(u) => u,
        None => {
            bot.send_message(chat_id, "Cannot start RPG for anonymous message.")
                .await?;
            return Ok(());
        }
    };

    let tg_user = db::user::upsert_tg_user(&pool, from_user).await?;
    let player = db::rpg::get_or_create_player(&pool, tg_user.id).await?;

    let (text, keyboard) = build_main_menu(&tg_user.full_username(false), &player);

    let m = bot
        .send_message(chat_id, text)
        .reply_markup(keyboard)
        .parse_mode(ParseMode::Html)
        .await?;
    let sent = m.id.0 as i64;

    db::rpg::upsert_ui_state(&pool, tg_user.id, chat_id.0, sent, "main", Some("menu")).await?;

    Ok(())
}

pub async fn rpg_callback_handler(
    bot: Bot,
    q: CallbackQuery,
    pool: PgPool,
) -> Result<(), AppError> {
    let data = match q.data.as_deref() {
        Some(d) => d,
        None => return Ok(()),
    };

    let parts: Vec<&str> = data.split(':').collect();
    if parts.len() < 2 || parts[0] != RPG_CB_PREFIX {
        return Ok(());
    }

    let mode = parts.get(1).copied().unwrap_or_default();
    let action = parts.get(2).copied().unwrap_or_default();
    let extra = parts.get(3).copied().unwrap_or_default();

    let message = match &q.message {
        Some(MaybeInaccessibleMessage::Regular(m)) => m.as_ref(),
        _ => return Ok(()),
    };

    let chat_id = message.chat.id;
    let from_user = &q.from;

    let tg_user = db::user::upsert_tg_user(&pool, from_user).await?;
    let player = db::rpg::get_or_create_player(&pool, tg_user.id).await?;

    match mode {
        "profile" => {
            handle_profile_mode(&bot, chat_id, message, &tg_user.full_username(false), &player)
                .await?;
        }
        "world" => {
            handle_world_mode(&bot, chat_id, message, &player, action, extra, &pool).await?;
        }
        "inventory" => {
            handle_inventory_mode(&bot, chat_id, message, &player, &pool).await?;
        }
        "guild" => {
            handle_guild_placeholder(&bot, chat_id, message).await?;
        }
        "battle" => {
            handle_battle_mode(&bot, chat_id, message, &player, action, &pool).await?;
        }
        "building" => {
            handle_building_mode(&bot, chat_id, message, action, extra).await?;
        }
        "main" => {
            let (text, keyboard) = build_main_menu(&tg_user.full_username(false), &player);
            bot.edit_message_text(chat_id, message.id, text)
                .reply_markup(keyboard)
                .parse_mode(ParseMode::Html)
                .await?;
        }
        "onboarding" => {
            handle_onboarding_callback(&bot, chat_id, message, &tg_user, &player, action, extra, &pool).await?;
        }
        _ => {}
    }

    Ok(())
}

fn build_main_menu(username: &str, player: &crate::db::rpg::RpgPlayer) -> (String, InlineKeyboardMarkup) {
    let username_esc = escape_html(username);
    let text = format!(
        "📜 <b>RPG Menu</b>\n\
        \n\
        You: <b>{}</b>\n\
        Level: <b>{}</b>\n\
        HP: <b>{}/{}</b>\n\
        Stats: STR {} / AGI {} / INT {} / VIT {} / LUCK {}\n\
        \n\
        Use buttons below to open profile, world map, inventory and other RPG features.",
        username_esc,
        player.level,
        player.hp_current,
        player.hp_max,
        player.strength,
        player.agility,
        player.intellect,
        player.vitality,
        player.luck
    );

    let keyboard = InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback("🧍 Profile", "rpg:profile:open"),
            InlineKeyboardButton::callback("🗺 World", "rpg:world:open"),
        ],
        vec![
            InlineKeyboardButton::callback("🎒 Inventory", "rpg:inventory:open"),
            InlineKeyboardButton::callback("👥 Guild", "rpg:guild:open"),
        ],
        vec![InlineKeyboardButton::callback(
            "❌ Close",
            "rpg:main:close",
        )],
    ]);

    (text, keyboard)
}

async fn handle_profile_mode(
    bot: &Bot,
    chat_id: ChatId,
    message: &Message,
    username: &str,
    player: &crate::db::rpg::RpgPlayer,
) -> Result<(), AppError> {
    let username_esc = escape_html(username);
    let text = format!(
        "<b>RPG profile</b>\n\
        \n\
        Player: <b>{}</b>\n\
        Level: <b>{}</b>\n\
        XP: <b>{} / {}</b>\n\
        HP: <b>{}/{}</b>\n\
        Position: ({}, {})\n\
        \n\
        Stats:\n\
        STR {} / AGI {} / INT {} / VIT {} / LUCK {}",
        username_esc,
        player.level,
        player.xp,
        player.xp_to_next,
        player.hp_current,
        player.hp_max,
        player.pos_x,
        player.pos_y,
        player.strength,
        player.agility,
        player.intellect,
        player.vitality,
        player.luck
    );

    let keyboard = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
        "⬅ Back",
        "rpg:main:open",
    )]]);

    bot.edit_message_text(chat_id, message.id, text)
        .reply_markup(keyboard)
        .parse_mode(ParseMode::Html)
        .await?;

    Ok(())
}

async fn handle_world_mode(
    bot: &Bot,
    chat_id: ChatId,
    message: &Message,
    player: &crate::db::rpg::RpgPlayer,
    action: &str,
    extra: &str,
    pool: &PgPool,
) -> Result<(), AppError> {
    let mut player = player.clone();

    if action == "move" {
        let (dx, dy) = match extra {
            "N" => (0, -1),
            "S" => (0, 1),
            "W" => (-1, 0),
            "E" => (1, 0),
            "NW" => (-1, -1),
            "NE" => (1, -1),
            "SW" => (-1, 1),
            "SE" => (1, 1),
            _ => (0, 0),
        };
        player.pos_x += dx;
        player.pos_y += dy;

        sqlx::query(
            r#"
            UPDATE rpg_player
            SET pos_x = $1, pos_y = $2
            WHERE id = $3
            "#,
        )
        .bind(player.pos_x)
        .bind(player.pos_y)
        .bind(player.id)
        .execute(pool)
        .await?;
    }

    if action == "action" {
        // Contextual action on current tile (MVP: simple camp screen).
        let tiles = db::rpg::load_map_window(pool, player.pos_x, player.pos_y, 0).await?;
        let biome = tiles
            .into_iter()
            .find(|t| t.x == player.pos_x && t.y == player.pos_y)
            .map(|t| t.biome)
            .unwrap_or_else(|| "unknown".to_string());

        let title = match biome.as_str() {
            "city_small" | "city_big" | "capital" => "City outskirts",
            "village" | "village_plains" => "Village outskirts",
            "raid_zone" => "Raid camp",
            "world_boss" => "Boss approach",
            _ => "Camp",
        };

        let text = format!(
            "<b>{}</b>\n\nYou set up a temporary camp at ({}, {}).\nFuture actions will live here: rest, change equipment, fast travel, city/raid entry.",
            title, player.pos_x, player.pos_y
        );

        let keyboard = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
            "⬅ Back to map",
            "rpg:world:open",
        )]]);

        bot.edit_message_text(chat_id, message.id, text)
            .reply_markup(keyboard)
            .parse_mode(ParseMode::Html)
            .await?;
        return Ok(());
    }

    if action == "legend" {
        let text = "\
<b>World map legend</b>\n\n\
🌊 water / ocean\n\
🌾 plains\n\
🌲 forest\n\
🏜 desert\n\
❄ snow\n\
⛰ mountains\n\
🐸 swamp / corrupted lands\n\
🏘 village\n\
🏙 city\n\
🏰 capital\n\
🕳 dungeon entrance\n\
🗼 tower\n\
💀 raid zone\n\
🐉 world boss area";

        let keyboard = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
            "⬅ Back to map",
            "rpg:world:open",
        )]]);

        bot.edit_message_text(chat_id, message.id, text)
            .reply_markup(keyboard)
            .parse_mode(ParseMode::Html)
            .await?;
        return Ok(());
    }

    // Check current tile for special biomes (city / village / raid zones).
    let tiles = db::rpg::load_map_window(pool, player.pos_x, player.pos_y, 0).await?;
    let current_tile = tiles.into_iter().find(|t| t.x == player.pos_x && t.y == player.pos_y);

    if let Some(tile) = current_tile {
        match tile.biome.as_str() {
            "city_small" | "city_big" | "capital" | "village_plains" => {
                handle_city_mode(bot, chat_id, message, &player, &tile, pool).await?;
                return Ok(());
            }
            "raid_zone" | "world_boss" => {
                handle_special_zone_mode(bot, chat_id, message, &player, &tile).await?;
                return Ok(());
            }
            _ => {}
        }
    }

    // Default: render world map window around the player.
    // Use smaller radius so emoji grid fits nicely on mobile.
    let radius = 3;
    let tiles_window =
        db::rpg::load_map_window(pool, player.pos_x, player.pos_y, radius).await?;
    let map_text = render_map_window(&tiles_window, player.pos_x, player.pos_y, radius);

    // Small one-line description of current tile under the map.
    let current_for_text = tiles_window
        .iter()
        .find(|t| t.x == player.pos_x && t.y == player.pos_y);
    let tile_desc = if let Some(tile) = current_for_text {
        let biome_desc = match tile.biome.as_str() {
            "plain" => "Plains",
            "forest" => "Forest",
            "desert" => "Desert",
            "snow" => "Snowy area",
            "mountain" => "Mountains",
            "swamp" => "Swamp",
            "water" | "ocean" => "Water",
            "city_small" | "city_big" => "City outskirts",
            "capital" => "Capital outskirts",
            "village" | "village_plains" => "Village outskirts",
            "dungeon_entrance" => "Dungeon entrance",
            "tower" => "Tower",
            "raid_zone" => "Raid zone",
            "world_boss" => "World boss area",
            _ => "Unknown area",
        };
        let level_info = if let (Some(min), Some(max)) = (tile.min_level, tile.max_level) {
            format!(" (recommended level {}–{})", min, max)
        } else {
            String::new()
        };
        format!("Area: <b>{}</b>{}", biome_desc, level_info)
    } else {
        "Area: <b>Unknown</b>".to_string()
    };

    let map_esc = escape_html(&map_text);
    let text = format!(
        "<b>World map</b>\n\n{}\n\n{}\nPosition: ({}, {})",
        map_esc, tile_desc, player.pos_x, player.pos_y
    );

    // Circular navigation layout:
    //     ↖ ⬆ ↗
    //     ⬅ ⛺ ➡
    //        📜
    //        ⬅ Back
    let keyboard = InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback("↖", "rpg:world:move:NW"),
            InlineKeyboardButton::callback("⬆", "rpg:world:move:N"),
            InlineKeyboardButton::callback("↗", "rpg:world:move:NE"),
        ],
        vec![
            InlineKeyboardButton::callback("⬅", "rpg:world:move:W"),
            InlineKeyboardButton::callback("⛺", "rpg:world:action"),
            InlineKeyboardButton::callback("➡", "rpg:world:move:E"),
        ],
        vec![
            InlineKeyboardButton::callback("↙", "rpg:world:move:SW"),
            InlineKeyboardButton::callback("⬇", "rpg:world:move:S"),
            InlineKeyboardButton::callback("↘", "rpg:world:move:SE"),
        ],
        vec![InlineKeyboardButton::callback("📜 Legend", "rpg:world:legend")],
        vec![InlineKeyboardButton::callback(
            "⬅ Back",
            "rpg:main:open",
        )],
    ]);

    bot.edit_message_text(chat_id, message.id, text)
        .reply_markup(keyboard)
        .parse_mode(ParseMode::Html)
        .await?;

    Ok(())
}

async fn handle_city_mode(
    bot: &Bot,
    chat_id: ChatId,
    message: &Message,
    player: &crate::db::rpg::RpgPlayer,
    tile: &crate::db::rpg::RpgMapTile,
    pool: &PgPool,
) -> Result<(), AppError> {
    let city_code = match &tile.object_code {
        Some(c) => c.as_str(),
        None => {
            // Fallback: show simple text if city has no detailed data.
            let text = format!(
                "<b>City</b>\n\nYou are in a settlement at ({}, {}).\nNo detailed layout is defined yet.",
                player.pos_x, player.pos_y
            );
            let keyboard = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                "⬅ Back to world",
                "rpg:world:open",
            )]]);
            bot.edit_message_text(chat_id, message.id, text)
                .reply_markup(keyboard)
                .parse_mode(ParseMode::Html)
                .await?;
            return Ok(());
        }
    };

    let city_row = sqlx::query(
        r#"
        SELECT name, description
        FROM rpg_city
        WHERE code = $1
        "#,
    )
    .bind(city_code)
    .fetch_optional(pool)
    .await?;

    if let Some(city) = city_row {
        let name: String = city.get("name");
        let description: String = city.get("description");
        let text = format!(
            "<b>{}</b>\n\n{}\n\nAvailable buildings:",
            escape_html(&name), escape_html(&description)
        );

        let rows = vec![
            vec![
                InlineKeyboardButton::callback("🏪 Shop", "rpg:building:open:shop_general"),
                InlineKeyboardButton::callback("🏰 Guild hall", "rpg:building:open:guild_house_main"),
            ],
            vec![
                InlineKeyboardButton::callback("⚔ Arena", "rpg:building:open:arena_local"),
                InlineKeyboardButton::callback("🍺 Tavern", "rpg:building:open:tavern_main"),
            ],
            vec![InlineKeyboardButton::callback(
                "⬅ Back to world",
                "rpg:world:open",
            )],
        ];

        let keyboard = InlineKeyboardMarkup::new(rows);

        bot.edit_message_text(chat_id, message.id, text)
            .reply_markup(keyboard)
            .parse_mode(ParseMode::Html)
            .await?;
    } else {
        let text = format!(
            "<b>Settlement</b>\n\nYou are in a settlement at ({}, {}).",
            player.pos_x, player.pos_y
        );
        let keyboard = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
            "⬅ Back to world",
            "rpg:world:open",
        )]]);
        bot.edit_message_text(chat_id, message.id, text)
            .reply_markup(keyboard)
            .parse_mode(ParseMode::Html)
            .await?;
    }

    Ok(())
}

async fn handle_special_zone_mode(
    bot: &Bot,
    chat_id: ChatId,
    message: &Message,
    _player: &crate::db::rpg::RpgPlayer,
    tile: &crate::db::rpg::RpgMapTile,
) -> Result<(), AppError> {
    let text = match tile.biome.as_str() {
        "raid_zone" => "<b>Raid zone</b>\n\nThis area is reserved for future guild raids.",
        "world_boss" => "<b>World boss area</b>\n\nThis area will host world bosses for multiple guilds.",
        _ => "<b>Special zone</b>",
    };

    let keyboard = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
        "⬅ Back to world",
        "rpg:world:open",
    )]]);

    bot.edit_message_text(chat_id, message.id, text)
        .reply_markup(keyboard)
        .parse_mode(ParseMode::Html)
        .await?;

    Ok(())
}

async fn handle_building_mode(
    bot: &Bot,
    chat_id: ChatId,
    message: &Message,
    _action: &str,
    building_code: &str,
) -> Result<(), AppError> {
    let title = match building_code {
        "shop_general" => "Shop",
        "guild_house_main" => "Guild hall",
        "arena_local" => "Arena",
        "tavern_main" => "Tavern",
        _ => "Building",
    };

    let description = match building_code {
        "shop_general" => "Here you will be able to buy and sell basic items.",
        "guild_house_main" => "This is the guild hall. Here you can manage guilds and raids.",
        "arena_local" => "Local arena for duels and future ranked fights.",
        "tavern_main" => "Tavern where you will find quests and social features.",
        _ => "This building does not have detailed logic yet.",
    };

    let text = format!("<b>{}</b>\n\n{}", escape_html(title), escape_html(description));

    let back_target = if building_code.starts_with("guild_house") {
        "rpg:guild:open"
    } else {
        "rpg:world:open"
    };

    let keyboard = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
        "⬅ Back",
        back_target,
    )]]);

    bot.edit_message_text(chat_id, message.id, text)
        .reply_markup(keyboard)
        .parse_mode(ParseMode::Html)
        .await?;

    Ok(())
}

fn render_map_window(
    tiles: &[crate::db::rpg::RpgMapTile],
    center_x: i32,
    center_y: i32,
    radius: i32,
) -> String {
    let mut result = String::new();
    for y in (center_y - radius)..=(center_y + radius) {
        for x in (center_x - radius)..=(center_x + radius) {
            if x == center_x && y == center_y {
                result.push('🧍');
                continue;
            }
            let tile = tiles.iter().find(|t| t.x == x && t.y == y);
            let noise = (x as i64 * 31 + y as i64 * 17).abs();
            let ch = match tile.map(|t| t.biome.as_str()) {
                Some("plain") => match noise % 4 {
                    0 => '🌾',
                    1 => '🌿',
                    2 => '🌻',
                    _ => '🍀',
                },
                Some("forest") => match noise % 3 {
                    0 => '🌲',
                    1 => '🌳',
                    _ => '🌲',
                },
                Some("desert") => match noise % 3 {
                    0 => '🏜',
                    1 => '🌵',
                    _ => '🏜',
                },
                Some("snow") => match noise % 2 {
                    0 => '❄',
                    _ => '⛄',
                },
                Some("mountain") => match noise % 3 {
                    0 => '⛰',
                    1 => '🏔',
                    _ => '🪨',
                },
                Some("swamp") => match noise % 3 {
                    0 => '🐸',
                    1 => '🌫',
                    _ => '🪵',
                },
                Some("water") | Some("ocean") => match noise % 3 {
                    0 => '🌊',
                    1 => '💧',
                    _ => '🌊',
                },
                Some("road") => '🛣',
                Some("city_small") | Some("city_big") => '🏙',
                Some("capital") => '🏰',
                Some("village") | Some("village_plains") => '🏘',
                Some("dungeon_entrance") => '🕳',
                Some("tower") => '🗼',
                Some("raid_zone") => '💀',
                Some("world_boss") => '🐉',
                None => '🌊', // unexplored / outside map as water instead of black
                _ => '⬛',
            };
            result.push(ch);
        }
        result.push('\n');
    }
    result
}

async fn handle_inventory_mode(
    bot: &Bot,
    chat_id: ChatId,
    message: &Message,
    player: &crate::db::rpg::RpgPlayer,
    pool: &PgPool,
) -> Result<(), AppError> {
    let entries = db::rpg::list_inventory(pool, player.id).await?;

    if entries.is_empty() {
        let text = "<b>Inventory is empty</b>".to_string();
        let keyboard = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
            "⬅ Back",
            "rpg:main:open",
        )]]);
        bot.edit_message_text(chat_id, message.id, text)
            .reply_markup(keyboard)
            .parse_mode(ParseMode::Html)
            .await?;
        return Ok(());
    }

    let mut text = String::from("<b>Inventory:</b>\n\n");
    for entry in entries {
        text.push_str(&format!(
            "• {} x{} ({})\n",
            escape_html(&entry.name), entry.quantity, escape_html(&entry.rarity)
        ));
    }

    let keyboard = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
        "⬅ Back",
        "rpg:main:open",
    )]]);

    bot.edit_message_text(chat_id, message.id, text)
        .reply_markup(keyboard)
        .parse_mode(ParseMode::Html)
        .await?;

    Ok(())
}

async fn handle_guild_placeholder(
    bot: &Bot,
    chat_id: ChatId,
    message: &Message,
) -> Result<(), AppError> {
    let text = format!(
        "<b>{}</b>",
        escape_html("Guilds are not implemented yet in this MVP. Stay tuned!")
    );

    let keyboard = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
        "⬅ Back",
        "rpg:main:open",
    )]]);

    bot.edit_message_text(chat_id, message.id, text)
        .reply_markup(keyboard)
        .parse_mode(ParseMode::Html)
        .await?;

    Ok(())
}

async fn handle_battle_mode(
    bot: &Bot,
    chat_id: ChatId,
    message: &Message,
    player: &crate::db::rpg::RpgPlayer,
    action: &str,
    pool: &PgPool,
) -> Result<(), AppError> {
    use crate::db::rpg::{
        battle_math, create_pve_battle, get_active_battle_for_player, recalc_stamina,
        update_stamina, RpgBattleState,
    };
    use chrono::Utc;

    // Recalculate stamina on each battle entry.
    let now = Utc::now();
    let (stamina_now, stamina_updated_at) = recalc_stamina(player, now);
    if stamina_now <= 0 {
        let text = "<b>Stamina is depleted</b>\n\nYou are too exhausted to fight right now. Please rest a bit before starting a new battle.".to_string();
        let keyboard = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
            "⬅ Back",
            "rpg:main:open",
        )]]);
        bot.edit_message_text(chat_id, message.id, text)
            .reply_markup(keyboard)
            .parse_mode(ParseMode::Html)
            .await?;
        return Ok(());
    }

    // Spend 1 stamina when entering battle (MVP).
    let stamina_after = stamina_now - 1;
    update_stamina(pool, player.id, stamina_after, stamina_updated_at).await?;

    let active = get_active_battle_for_player(pool, player.id).await?;
    let battle = if let Some(b) = active {
        b
    } else {
        create_pve_battle(pool, player, "training_dummy", 50).await?
    };

    let mut state: RpgBattleState = serde_json::from_value(battle.state_json.clone())
        .unwrap_or_else(|_| RpgBattleState {
            player_id: player.id,
            enemy_code: "training_dummy".to_string(),
            player: crate::db::rpg::BattleParticipantState {
                hp: player.hp_current,
            },
            enemy: crate::db::rpg::BattleParticipantState { hp: 50 },
            turn_player_id: player.id,
            log: Vec::new(),
        });

    if action == "attack" && state.turn_player_id == player.id {
        let attack = battle_math::calc_attack_phys(player);
        let defense = 0; // training dummy has no defense in MVP
        let (damage, is_crit) = battle_math::roll_damage(attack, defense, 0.1, 1.5);
        state.enemy.hp -= damage;
        let line = if is_crit {
            format!("You critically hit the dummy for {} damage!", damage)
        } else {
            format!("You hit the dummy for {} damage.", damage)
        };
        state.log.push(line);
        state.turn_player_id = -1;
    }

    let status = if state.enemy.hp <= 0 {
        state.log.push("Enemy defeated!".to_string());
        "finished"
    } else {
        "active"
    };

    db::rpg::update_battle_state(pool, battle.id, &state, status).await?;

    let mut text = String::from("<b>Training battle</b>\n\n");
    text.push_str(&format!("Your HP: {}\nEnemy HP: {}\n\n", state.player.hp, state.enemy.hp));
    if !state.log.is_empty() {
        text.push_str("Log:\n");
        for line in state.log.iter().rev().take(5).rev() {
            text.push_str(&format!("• {}\n", escape_html(line)));
        }
    }

    let keyboard = if status == "finished" {
        InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
            "⬅ Back to main",
            "rpg:main:open",
        )]])
    } else {
        InlineKeyboardMarkup::new(vec![
            vec![InlineKeyboardButton::callback(
                "⚔ Attack",
                "rpg:battle:attack",
            )],
            vec![InlineKeyboardButton::callback(
                "⬅ Back",
                "rpg:main:open",
            )],
        ])
    };

    bot.edit_message_text(chat_id, message.id, text)
        .reply_markup(keyboard)
        .parse_mode(ParseMode::Html)
        .await?;

    Ok(())
}

// Onboarding (character creation) handlers

async fn handle_onboarding_start(
    bot: &Bot,
    chat_id: ChatId,
    tg_user: &crate::db::models::TgUser,
    _player: &crate::db::rpg::RpgPlayer,
    pool: &PgPool,
    existing_state: Option<crate::db::rpg::RpgUiState>,
) -> Result<(), AppError> {
    // Step 1: Choose nickname
    let default_nickname = tg_user.full_username(false);
    let text = format!(
        "<b>Welcome to Pidor Royale!</b>\n\n\
        Step 1/3: Choose your character nickname\n\n\
        Default: <b>{}</b>\n\n\
        You can accept the default or enter a custom nickname.",
        escape_html(&default_nickname)
    );

    let keyboard = InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback(
            "✓ Accept default",
            "rpg:onboarding:nickname:accept",
        )],
        vec![InlineKeyboardButton::callback(
            "✏ Enter custom",
            "rpg:onboarding:nickname:custom",
        )],
    ]);

    let sent = if let Some(state) = existing_state {
        match bot
            .edit_message_text(
                chat_id,
                teloxide::types::MessageId(state.message_id as i32),
                text.clone(),
            )
            .reply_markup(keyboard.clone())
            .parse_mode(ParseMode::Html)
            .await
        {
            Ok(_) => state.message_id,
            Err(_) => {
                let m = bot
                    .send_message(chat_id, text)
                    .reply_markup(keyboard)
                    .parse_mode(ParseMode::Html)
                    .await?;
                m.id.0 as i64
            }
        }
    } else {
        let m = bot
            .send_message(chat_id, text)
            .reply_markup(keyboard)
            .parse_mode(ParseMode::Html)
            .await?;
        m.id.0 as i64
    };

    db::rpg::upsert_ui_state_with_payload(
        pool,
        tg_user.id,
        chat_id.0,
        sent,
        "onboarding",
        Some("nickname"),
        serde_json::json!({
            "step": 1,
            "nickname": default_nickname
        }),
    )
    .await?;

    Ok(())
}

async fn handle_onboarding_callback(
    bot: &Bot,
    chat_id: ChatId,
    message: &Message,
    tg_user: &crate::db::models::TgUser,
    player: &crate::db::rpg::RpgPlayer,
    action: &str,
    extra: &str,
    pool: &PgPool,
) -> Result<(), AppError> {
    let state = db::rpg::get_ui_state(pool, tg_user.id, chat_id.0).await?;
    let payload = state
        .as_ref()
        .map(|s| s.payload_json.clone())
        .unwrap_or_else(|| serde_json::json!({}));
    let step = payload.get("step").and_then(|v| v.as_i64()).unwrap_or(1);

    match (action, extra, step) {
        ("nickname", "accept", 1) => {
            // Accept default nickname, move to step 2 (gender)
            handle_onboarding_gender(bot, chat_id, message, tg_user, pool, &payload).await?;
        }
        ("nickname", "custom", 1) => {
            // For MVP, just accept default for now (can add text input later)
            handle_onboarding_gender(bot, chat_id, message, tg_user, pool, &payload).await?;
        }
        ("gender", gender, 2) => {
            // Gender selected, move to step 3 (archetype)
            let mut new_payload = payload.clone();
            new_payload["gender"] = serde_json::Value::String(gender.to_string());
            handle_onboarding_archetype(bot, chat_id, message, tg_user, pool, &new_payload).await?;
        }
        ("archetype", archetype, 3) => {
            // Archetype selected, complete onboarding
            let mut new_payload = payload.clone();
            new_payload["archetype"] = serde_json::Value::String(archetype.to_string());
            complete_onboarding(bot, chat_id, message, tg_user, player, pool, &new_payload).await?;
        }
        _ => {
            // Fallback: restart onboarding
            handle_onboarding_start(bot, chat_id, tg_user, player, pool, state).await?;
        }
    }

    Ok(())
}

async fn handle_onboarding_gender(
    bot: &Bot,
    chat_id: ChatId,
    message: &Message,
    tg_user: &crate::db::models::TgUser,
    pool: &PgPool,
    payload: &serde_json::Value,
) -> Result<(), AppError> {
    let text = "<b>Step 2/3: Choose your character</b>\n\n\
        Select your character type:";

    let keyboard = InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback(
            "Male",
            "rpg:onboarding:gender:male",
        )],
        vec![InlineKeyboardButton::callback(
            "Very Male",
            "rpg:onboarding:gender:very_male",
        )],
        vec![InlineKeyboardButton::callback(
            "Female (lost?)",
            "rpg:onboarding:gender:female",
        )],
    ]);

    bot.edit_message_text(chat_id, message.id, text)
        .reply_markup(keyboard)
        .parse_mode(ParseMode::Html)
        .await?;

    let mut new_payload = payload.clone();
    new_payload["step"] = serde_json::Value::Number(2.into());

    db::rpg::upsert_ui_state_with_payload(
        pool,
        tg_user.id,
        chat_id.0,
        message.id.0 as i64,
        "onboarding",
        Some("gender"),
        new_payload,
    )
    .await?;

    Ok(())
}

async fn handle_onboarding_archetype(
    bot: &Bot,
    chat_id: ChatId,
    message: &Message,
    tg_user: &crate::db::models::TgUser,
    pool: &PgPool,
    payload: &serde_json::Value,
) -> Result<(), AppError> {
    let gender = payload.get("gender").and_then(|v| v.as_str()).unwrap_or("male");
    let gender_msg = if gender == "female" {
        "Sorry, but this is Pidor Royale, not Princess Royale. Gender adjusted to standard."
    } else {
        ""
    };

    let text = format!(
        "<b>Step 3/3: Choose your archetype</b>\n\n{}\n\n\
        Select your playstyle:\n\n\
        <b>Tank</b> - High HP and defense\n\
        <b>Crit</b> - High critical strike chance\n\
        <b>Caster</b> - High magic damage",
        if !gender_msg.is_empty() {
            format!("{}\n\n", escape_html(gender_msg))
        } else {
            String::new()
        }
    );

    let keyboard = InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback(
            "🛡 Tank",
            "rpg:onboarding:archetype:tank",
        )],
        vec![InlineKeyboardButton::callback(
            "⚔ Crit",
            "rpg:onboarding:archetype:crit",
        )],
        vec![InlineKeyboardButton::callback(
            "✨ Caster",
            "rpg:onboarding:archetype:caster",
        )],
    ]);

    bot.edit_message_text(chat_id, message.id, text)
        .reply_markup(keyboard)
        .parse_mode(ParseMode::Html)
        .await?;

    let mut new_payload = payload.clone();
    new_payload["step"] = serde_json::Value::Number(3.into());

    db::rpg::upsert_ui_state_with_payload(
        pool,
        tg_user.id,
        chat_id.0,
        message.id.0 as i64,
        "onboarding",
        Some("archetype"),
        new_payload,
    )
    .await?;

    Ok(())
}

async fn complete_onboarding(
    bot: &Bot,
    chat_id: ChatId,
    message: &Message,
    tg_user: &crate::db::models::TgUser,
    player: &crate::db::rpg::RpgPlayer,
    pool: &PgPool,
    payload: &serde_json::Value,
) -> Result<(), AppError> {
    let archetype = payload
        .get("archetype")
        .and_then(|v| v.as_str())
        .unwrap_or("tank");

    // Set class_code and adjust starting stats based on archetype
    let (strength, agility, intellect, vitality, luck, hp_max) = match archetype {
        "tank" => (2, 1, 1, 5, 1, 150),
        "crit" => (3, 5, 1, 2, 3, 100),
        "caster" => (1, 2, 5, 2, 2, 90),
        _ => (2, 2, 2, 2, 2, 100),
    };

    sqlx::query(
        r#"
        UPDATE rpg_player
        SET class_code = $1,
            strength = $2,
            agility = $3,
            intellect = $4,
            vitality = $5,
            luck = $6,
            hp_max = $7,
            hp_current = $7
        WHERE id = $8
        "#,
    )
    .bind(archetype)
    .bind(strength)
    .bind(agility)
    .bind(intellect)
    .bind(vitality)
    .bind(luck)
    .bind(hp_max)
    .bind(player.id)
    .execute(pool)
    .await?;

    // Show completion message and go to main menu
    let text = format!(
        "<b>Character created!</b>\n\n\
        Archetype: <b>{}</b>\n\
        Starting stats:\n\
        STR {} / AGI {} / INT {} / VIT {} / LUCK {}\n\n\
        Welcome to Pidor Royale!",
        archetype, strength, agility, intellect, vitality, luck
    );

    let keyboard = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
        "🎮 Start playing",
        "rpg:main:open",
    )]]);

    bot.edit_message_text(chat_id, message.id, text)
        .reply_markup(keyboard)
        .parse_mode(ParseMode::Html)
        .await?;

    // Update UI state to main menu
    db::rpg::upsert_ui_state(pool, tg_user.id, chat_id.0, message.id.0 as i64, "main", Some("menu")).await?;

    Ok(())
}
