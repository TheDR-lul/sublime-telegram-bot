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
    let existing_state = db::rpg::get_ui_state(&pool, tg_user.id, chat_id.0).await?;

    let (text, keyboard) = build_main_menu(&tg_user.full_username(false), &player);

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
        _ => {}
    }

    Ok(())
}

fn build_main_menu(username: &str, player: &crate::db::rpg::RpgPlayer) -> (String, InlineKeyboardMarkup) {
    let username_esc = escape_html(username);
    let text = format!(
        "📜 <b>Pidor Royale</b>\n\
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
    let radius = 2;
    let tiles =
        db::rpg::load_map_window(pool, player.pos_x, player.pos_y, radius).await?;
    let map_text = render_map_window(&tiles, player.pos_x, player.pos_y, radius);

    let map_esc = escape_html(&map_text);
    let text = format!(
        "<b>World map</b>\n\n{}\n\nPosition: ({}, {})",
        map_esc, player.pos_x, player.pos_y
    );

    let keyboard = InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback("⬆", "rpg:world:move:N"),
            InlineKeyboardButton::callback("⬇", "rpg:world:move:S"),
            InlineKeyboardButton::callback("⬅", "rpg:world:move:W"),
            InlineKeyboardButton::callback("➡", "rpg:world:move:E"),
        ],
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
            let ch = match tile.map(|t| t.biome.as_str()) {
                Some("plain") => '⬜',
                Some("forest") => '🌲',
                Some("desert") => '🏜',
                Some("snow") => '❄',
                Some("city_small") | Some("city_big") | Some("capital") => '🏙',
                Some("village") => '🏘',
                Some("dungeon_entrance") => '🕳',
                Some("tower") => '🗼',
                Some("raid_zone") => '💀',
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
    use crate::db::rpg::{create_pve_battle, get_active_battle_for_player, RpgBattleState};

    let active = get_active_battle_for_player(pool, player.id).await?;
    let battle = if let Some(b) = active {
        b
    } else {
        create_pve_battle(pool, player, "training_dummy", 50).await?
    };

    let mut state: RpgBattleState =
        serde_json::from_value(battle.state_json.clone()).unwrap_or_else(|_| RpgBattleState {
            player_id: player.id,
            enemy_code: "training_dummy".to_string(),
            player: crate::db::rpg::BattleParticipantState { hp: player.hp_current },
            enemy: crate::db::rpg::BattleParticipantState { hp: 50 },
            turn_player_id: player.id,
            log: Vec::new(),
        });

    if action == "attack" && state.turn_player_id == player.id {
        let damage = 5.max(player.strength);
        state.enemy.hp -= damage;
        state
            .log
            .push(format!("You hit the dummy for {} damage.", damage));
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
