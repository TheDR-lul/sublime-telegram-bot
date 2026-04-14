//! HuyActa tamagotchi game handlers.
//! Commands: /huya, /huyagrow, /huyafight @user, /huyasteal @user,
//!           /huyatop, /huyareg, /huyaskills, /huyashop

use sqlx::{self, PgPool};
use rand::RngExt;
use teloxide::prelude::*;
use teloxide::types::{CallbackQuery, InlineKeyboardButton, InlineKeyboardMarkup};
use std::io::Write;
use teloxide::utils::html::escape as escape_html;
use std::time::Duration;

use crate::db::huya as huya_db;
use crate::db::models::Huya;
use crate::db::user;
use crate::error::AppError;
use crate::i18n::LOCALE;
use crate::telegram::topic_routing::{send_text_in_origin_topic, topic_thread_id};
use crate::telegram::target_resolver::{resolve_target as resolve_target_global, ResolvedTarget};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Convert millimetres to a "X.Y" cm string (e.g. 35 → "3.5").
fn mm_to_cm_str(mm: i32) -> String {
    let abs = mm.unsigned_abs();
    format!("{}.{}", abs / 10, abs % 10)
}

fn rarity_label_ru(rarity: &str) -> String {
    LOCALE
        .t_opt("ru", &format!("huya.rarity.{rarity}"))
        .unwrap_or(rarity)
        .to_string()
}

fn trait_label_ru(trait_name: Option<&str>) -> String {
    match trait_name {
        Some(t) => LOCALE
            .t_opt("ru", &format!("huya.trait.{t}"))
            .unwrap_or(t)
            .to_string(),
        None => "—".to_string(),
    }
}

fn item_label_ru(item_id: &str) -> String {
    LOCALE
        .t_opt("ru", &format!("huya.item.{item_id}"))
        .unwrap_or(item_id)
        .to_string()
}

fn chest_label_ru(chest_id: &str) -> String {
    LOCALE
        .t_opt("ru", &format!("huya.chest_name.{chest_id}"))
        .unwrap_or(chest_id)
        .to_string()
}

fn rarity_emoji(rarity: &str) -> &'static str {
    match rarity {
        "trash" => "⚪",
        "common" => "🟢",
        "rare" => "🔵",
        "epic" => "🟣",
        "legendary" => "🟡",
        _ => "⚫",
    }
}

fn kind_label_ru(kind: &str) -> &'static str {
    match kind {
        "equipment" => "экип",
        "booster" => "бустер",
        "gem" => "гем",
        _ => "предмет",
    }
}

fn item_rarity(item_id: &str) -> String {
    huya_db::item_templates()
        .into_iter()
        .find(|t| t.id == item_id)
        .map(|t| t.rarity.to_string())
        .unwrap_or_else(|| "common".to_string())
}

/// HP bar (10 chars) for an arbitrary current/max pair.
fn hp_bar_str(current: i32, max: i32) -> String {
    let max = max.max(1);
    let filled = ((current.max(0) as f64 / max as f64) * 10.0).round() as usize;
    let filled = filled.min(10);
    format!("[{}{}]", "█".repeat(filled), "░".repeat(10 - filled))
}

/// Build equipment summary string for status.
fn equipment_summary(
    h: &Huya,
    equ: &[crate::db::models::HuyaEquipmentSlot],
    inv: &[crate::db::models::HuyaInventoryItem],
) -> String {
    let mut by_slot: std::collections::HashMap<&str, &crate::db::models::HuyaInventoryItem> =
        std::collections::HashMap::new();
    for e in equ {
        if let Some(item) = inv.iter().find(|i| i.id == e.inventory_id) {
            by_slot.insert(e.slot.as_str(), item);
        }
    }
    let slot_line = |name: &str, slot: &str| {
        if !huya_db::slot_unlocked_for_length(slot, h.length_mm) {
            return format!("{}: 🔒", name);
        }
        if let Some(item) = by_slot.get(slot) {
            format!("{}: {}", name, item_label_ru(&item.item_id))
        } else {
            format!("{}: —", name)
        }
    };
    let mut lines = Vec::new();
    lines.push(slot_line("ring_1", "ring_1"));
    lines.push(slot_line("ring_2", "ring_2"));
    lines.push(slot_line("ring_3", "ring_3"));
    lines.push(slot_line("ring_4", "ring_4"));
    lines.push(slot_line("ring_5", "ring_5"));
    lines.push(slot_line("ring_6", "ring_6"));
    lines.push(slot_line("tip", "tip"));
    lines.push(slot_line("base", "base"));
    lines.push(slot_line("balls", "balls"));
    lines.push(slot_line("piercing_tip_1", "piercing_tip_1"));
    lines.push(slot_line("piercing_tip_2", "piercing_tip_2"));
    lines.push(slot_line("piercing_tip_3", "piercing_tip_3"));
    lines.push(slot_line("piercing_shaft_1", "piercing_shaft_1"));
    lines.push(slot_line("piercing_shaft_2", "piercing_shaft_2"));
    lines.push(slot_line("piercing_shaft_3", "piercing_shaft_3"));
    lines.push(slot_line("piercing_base_1", "piercing_base_1"));
    lines.push(slot_line("piercing_base_2", "piercing_base_2"));
    lines.join("\n")
}

/// Build the status text for a player.
fn huya_status_text(h: &Huya, name: &str, equ: &[crate::db::models::HuyaEquipmentSlot], inv: &[crate::db::models::HuyaInventoryItem]) -> String {
    let max_hp = h.max_hp();
    let max_actions = h.max_actions();
    let base = if h.is_pussy() {
        LOCALE.t_fmt("ru", "huya.status_pussy", &[
            ("name",         &escape_html(name)),
            ("size",         &h.display_cm()),
            ("actions_left", &h.actions_left.to_string()),
            ("max_actions",  &max_actions.to_string()),
            ("hp",           &h.hp.to_string()),
            ("max_hp",       &max_hp.to_string()),
            ("hp_bar",       &h.hp_bar()),
        ])
    } else {
        LOCALE.t_fmt("ru", "huya.status", &[
            ("name",         &escape_html(name)),
            ("size",         &h.display_cm()),
            ("level",        &h.level.to_string()),
            ("xp",           &h.xp.to_string()),
            ("actions_left", &h.actions_left.to_string()),
            ("max_actions",  &max_actions.to_string()),
            ("hp",           &h.hp.to_string()),
            ("max_hp",       &max_hp.to_string()),
            ("hp_bar",       &h.hp_bar()),
        ])
    };
    let pet_line = format!("🖐 Поглаживания друзей: {}/3", h.pet_energy_left.max(0).min(3));
    let eq_text = equipment_summary(h, equ, inv);
    format!("{}\n{}\n\n{}\n\n{}", base, pet_line, LOCALE.t("ru", "huya.equipment_header"), eq_text)
}

fn huya_stat_keyboard(tg_id: i64, has_actions: bool) -> InlineKeyboardMarkup {
    let btn = if has_actions {
        LOCALE.t("ru", "huya.stroke_btn").to_string()
    } else {
        LOCALE.t("ru", "huya.no_actions_btn").to_string()
    };
    InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::callback(btn, format!("huya_grow:{}", tg_id)),
    ]])
}

/// Preferred display name for a Telegram user: @username if present, otherwise "first last" or first name.
fn display_name_from_user(user: &teloxide::types::User) -> String {
    user.username
        .as_ref()
        .map(|u| format!("@{}", u))
        .unwrap_or_else(|| {
            user.last_name
                .as_ref()
                .map(|l| format!("{} {}", user.first_name, l))
                .unwrap_or_else(|| user.first_name.clone())
        })
}

fn actions_available(h: &Huya) -> bool {
    h.actions_left > 0 || h.actions_reset_at < chrono::Utc::now().date_naive()
}

// ── Grow helpers ─────────────────────────────────────────────────────────────

/// Pick the correct locale key for a grow result.
fn grow_locale_key(leveled_up: bool, boost_active: bool, is_pussy: bool) -> &'static str {
    match (leveled_up, boost_active, is_pussy) {
        (true,  true, true) => "huya.grow_levelup_boosted_pussy",
        (true,  false, true) => "huya.grow_levelup_pussy",
        (false, true, true) => "huya.grow_boosted_pussy",
        (false, false, true) => "huya.grow_pussy",
        (true,  true, false)  => "huya.grow_levelup_boosted",
        (true,  false, false) => "huya.grow_levelup",
        (false, true, false)  => "huya.grow_boosted",
        (false, false, false) => "huya.grow",
    }
}

// ── Fight helpers ─────────────────────────────────────────────────────────────

fn move_emoji(id: i32) -> &'static str {
    match id {
        0 => "🍆 Напор",
        1 => "🫸 Финт",
        2 => "🎯 В шары",
        _ => "?",
    }
}

fn fight_challenge_keyboard(fight_id: i32) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::callback(
            LOCALE.t("ru", "huya.fight_accept_btn").to_string(),
            format!("huya_fa:{}", fight_id),
        ),
        InlineKeyboardButton::callback(
            LOCALE.t("ru", "huya.fight_decline_btn").to_string(),
            format!("huya_fd:{}", fight_id),
        ),
    ]])
}

fn fight_pick_keyboard(fight_id: i32) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::callback("🍆 Напор",  format!("huya_fm:{}:0", fight_id)),
        InlineKeyboardButton::callback("🫸 Финт",   format!("huya_fm:{}:1", fight_id)),
        InlineKeyboardButton::callback("🎯 В шары", format!("huya_fm:{}:2", fight_id)),
    ]])
}

// #region agent log
fn agent_debug_log(hypothesis_id: &str, location: &str, message: &str, data: serde_json::Value) {
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("debug-3e7364.log")
    {
        let payload = serde_json::json!({
            "sessionId": "3e7364",
            "runId": "pre-fix",
            "hypothesisId": hypothesis_id,
            "location": location,
            "message": message,
            "data": data,
            "timestamp": chrono::Utc::now().timestamp_millis(),
        });
        let _ = writeln!(file, "{}", payload.to_string());
    }
}
// #endregion

// #region agent log 6f3178
fn agent_debug_log_6f3178(
    hypothesis_id: &str,
    location: &str,
    message: &str,
    data: serde_json::Value,
) {
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("debug-6f3178.log")
    {
        let payload = serde_json::json!({
            "sessionId": "6f3178",
            "runId": "pre-fix",
            "hypothesisId": hypothesis_id,
            "location": location,
            "message": message,
            "data": data,
            "timestamp": chrono::Utc::now().timestamp_millis(),
        });
        let _ = writeln!(file, "{}", payload.to_string());
    }
}
// #endregion

// ── Target resolution ─────────────────────────────────────────────────────────

// ── Main dispatcher ───────────────────────────────────────────────────────────

pub async fn huya_handler(
    bot: Bot,
    msg: Message,
    cmd: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    if !msg.chat.is_group() && !msg.chat.is_supergroup() {
        let _ = crate::alerts::notify(
            &pool,
            &format!("drop:huyashop unsupported chat_type chat_id={}", msg.chat.id.0),
        )
        .await;
        let _ = bot
            .send_message(msg.chat.id, "Магазин работает только в группах/супергруппах.")
            .await;
        return Ok(());
    }
    let chat_id = msg.chat.id.0;
    let from = match msg.from.as_ref() { Some(f) => f, None => return Ok(()) };
    let tg_id = from.id.0 as i64;
    let name = display_name_from_user(from);

    user::upsert_tg_user(&pool, from).await?;

    use crate::handlers::commands::Cmd;
    match &cmd {
        Cmd::Huyagrow => {
            handle_grow(&bot, &pool, msg.chat.id, chat_id, tg_id, &name).await
        }
        Cmd::Huyafight(s) => {
            handle_fight(&bot, &pool, &msg, chat_id, tg_id, &name, s.trim()).await
        }
        Cmd::Huyasteal(s) => {
            handle_steal(&bot, &pool, &msg, chat_id, tg_id, &name, s.trim()).await
        }
        Cmd::Huyaraid(s) => {
            handle_raid(&bot, &pool, &msg, chat_id, tg_id, &name, s.trim()).await
        }
        Cmd::Huyachest => {
            huyachest_handler(bot.clone(), msg.clone(), cmd.clone(), pool.clone()).await
        }
        Cmd::Huyainv => {
            huyainv_handler(bot.clone(), msg.clone(), cmd.clone(), pool.clone()).await
        }
        Cmd::Huya(s) => {
            let s = s.trim();
            let mut parts = s.splitn(2, ' ');
            let sub = parts.next().unwrap_or("stat");
            let arg = parts.next().unwrap_or("").trim();
            match sub {
                "grow"  => handle_grow(&bot, &pool, msg.chat.id, chat_id, tg_id, &name).await,
                "fight" => handle_fight(&bot, &pool, &msg, chat_id, tg_id, &name, arg).await,
                "steal" => handle_steal(&bot, &pool, &msg, chat_id, tg_id, &name, arg).await,
                "raid"  => handle_raid(&bot, &pool, &msg, chat_id, tg_id, &name, arg).await,
                "pet"   => handle_pet_friend(&bot, &pool, &msg, chat_id, from, &name, arg).await,
                _       => handle_stat(&bot, &pool, msg.chat.id, chat_id, tg_id, &name).await,
            }
        }
        _ => handle_stat(&bot, &pool, msg.chat.id, chat_id, tg_id, &name).await,
    }
}

// ── Stat ──────────────────────────────────────────────────────────────────────

async fn handle_stat(
    bot: &Bot,
    pool: &PgPool,
    chat_id: ChatId,
    chat_id_raw: i64,
    tg_id: i64,
    name: &str,
) -> Result<(), AppError> {
    let (h, was_created) = huya_db::get_or_create(pool, chat_id_raw, tg_id).await?;
    if was_created {
        let reg = LOCALE.t_rand_fmt("ru", "huya.reg_messages", &[("name", &escape_html(name))]);
        bot.send_message(chat_id, reg)
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;
    }
    let equ = huya_db::get_equipment(pool, chat_id_raw, tg_id).await.unwrap_or_default();
    let inv = huya_db::get_inventory(pool, chat_id_raw, tg_id).await.unwrap_or_default();
    bot.send_message(chat_id, huya_status_text(&h, name, &equ, &inv))
        .parse_mode(teloxide::types::ParseMode::Html)
        .reply_markup(huya_stat_keyboard(tg_id, actions_available(&h)))
        .await?;
    Ok(())
}

// ── Grow ──────────────────────────────────────────────────────────────────────

async fn handle_grow(
    bot: &Bot,
    pool: &PgPool,
    chat_id: ChatId,
    chat_id_raw: i64,
    tg_id: i64,
    name: &str,
) -> Result<(), AppError> {
    let (h, was_created) = huya_db::get_or_create(pool, chat_id_raw, tg_id).await?;
    if was_created {
        let reg = LOCALE.t_rand_fmt("ru", "huya.reg_messages", &[("name", &escape_html(name))]);
        bot.send_message(chat_id, reg)
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;
    }

    if !huya_db::consume_action(pool, &h).await? {
        bot.send_message(chat_id, LOCALE.t("ru", "huya.no_actions")).await?;
        return Ok(());
    }

    let (updated, grow_mm, xp_gain, leveled_up, boost_active) = huya_db::grow(pool, &h).await?;
    let key = grow_locale_key(leveled_up, boost_active, updated.is_pussy());
    let text = LOCALE.t_fmt("ru", key, &[
        ("name",     &escape_html(name)),
        ("grow_cm",  &mm_to_cm_str(grow_mm)),
        ("xp",       &xp_gain.to_string()),
        ("level",    &updated.level.to_string()),
        ("size",     &updated.display_cm()),
    ]);

    bot.send_message(chat_id, text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
    Ok(())
}

/// Callback: huya_grow:{tg_id} — the "Погладить" inline button.
pub async fn huya_grow_callback(
    bot: Bot,
    query: CallbackQuery,
    pool: PgPool,
) -> Result<(), AppError> {
    let data = query.data.as_deref().unwrap_or("");
    let parts: Vec<&str> = data.splitn(2, ':').collect();
    if parts.len() < 2 || parts[0] != "huya_grow" {
        let _ = bot.answer_callback_query(query.id).await;
        return Ok(());
    }
    let owner_tg_id = match parts[1].parse::<i64>() {
        Ok(id) => id,
        Err(_) => { let _ = bot.answer_callback_query(query.id).await; return Ok(()); }
    };
    let clicker = query.from.id.0 as i64;
    if clicker != owner_tg_id {
        let _ = bot.answer_callback_query(query.id).text("Это не твоя хуяка!").await;
        return Ok(());
    }

    let msg_ref = match query.message.as_ref() {
        Some(m) => m,
        None => { let _ = bot.answer_callback_query(query.id).await; return Ok(()); }
    };
    let chat_id = msg_ref.chat().id;

    let from = &query.from;
    let name = display_name_from_user(from);

    let (h, _) = huya_db::get_or_create(&pool, chat_id.0, clicker).await?;

    if !huya_db::consume_action(&pool, &h).await? {
        let _ = bot.answer_callback_query(query.id)
            .text(LOCALE.t("ru", "huya.no_actions"))
            .await;
        return Ok(());
    }

    let (updated, grow_mm, xp_gain, leveled_up, boost_active) = huya_db::grow(&pool, &h).await?;
    let key = grow_locale_key(leveled_up, boost_active, updated.is_pussy());
    let answer_text = LOCALE.t_fmt("ru", key, &[
        ("name",    &escape_html(&name)),
        ("grow_cm", &mm_to_cm_str(grow_mm)),
        ("xp",      &xp_gain.to_string()),
        ("level",   &updated.level.to_string()),
        ("size",    &updated.display_cm()),
    ]);

    let _ = bot.answer_callback_query(query.id)
        .text(format!(
            "+{} см {}",
            mm_to_cm_str(grow_mm),
            if updated.is_pussy() { "глубины" } else { "длины" }
        ))
        .await;

    let equ = huya_db::get_equipment(&pool, chat_id.0, clicker).await.unwrap_or_default();
    let inv = huya_db::get_inventory(&pool, chat_id.0, clicker).await.unwrap_or_default();
    let _ = bot.edit_message_text(msg_ref.chat().id, msg_ref.id(), huya_status_text(&updated, &name, &equ, &inv))
        .parse_mode(teloxide::types::ParseMode::Html)
        .reply_markup(huya_stat_keyboard(clicker, actions_available(&updated)))
        .await;

    bot.send_message(chat_id, answer_text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
    Ok(())
}

// ── Fight ─────────────────────────────────────────────────────────────────────

async fn handle_fight(
    bot: &Bot,
    pool: &PgPool,
    msg: &Message,
    chat_id_raw: i64,
    attacker_tg_id: i64,
    attacker_name: &str,
    sub_arg: &str,
) -> Result<(), AppError> {
    let chat_id = msg.chat.id;

    let target_tg_id = match resolve_target_global(pool, msg, sub_arg).await {
        ResolvedTarget::User(id) => id,
        ResolvedTarget::IsBot => {
            bot.send_message(chat_id, LOCALE.t("ru", "huya.fight_self_bot"))
                .parse_mode(teloxide::types::ParseMode::Html)
                .await?;
            return Ok(());
        }
        ResolvedTarget::NotFound => {
            bot.send_message(chat_id, LOCALE.t("ru", "huya.fight_no_target")).await?;
            return Ok(());
        }
    };

    if target_tg_id == attacker_tg_id {
        let (h, _) = huya_db::get_or_create(pool, chat_id_raw, attacker_tg_id).await?;
        if !huya_db::consume_action(pool, &h).await? {
            bot.send_message(chat_id, LOCALE.t("ru", "huya.no_actions")).await?;
            return Ok(());
        }
        let updated = huya_db::self_fight(pool, chat_id_raw, attacker_tg_id).await?;
        let key = if updated.is_pussy() { "huya.fight_self_pussy" } else { "huya.fight_self" };
        bot.send_message(chat_id, LOCALE.t_fmt("ru", key, &[
            ("name",     &escape_html(attacker_name)),
            ("new_size", &updated.display_cm()),
        ]))
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
        return Ok(());
    }

    let (h, _) = huya_db::get_or_create(pool, chat_id_raw, attacker_tg_id).await?;
    if !huya_db::consume_action(pool, &h).await? {
        bot.send_message(chat_id, LOCALE.t("ru", "huya.no_actions")).await?;
        return Ok(());
    }

    let target_name = user::get_by_tg_id(pool, target_tg_id).await?
        .map(|u| u.full_username(true))
        .unwrap_or_else(|| "???".to_string());

    let fight = huya_db::create_fight(pool, chat_id_raw, attacker_tg_id, target_tg_id).await?;

    let challenge_text = LOCALE.t_fmt("ru", "huya.fight_challenge", &[
        ("challenger", &escape_html(attacker_name)),
        ("target",     &escape_html(&target_name)),
    ]);

    let sent = bot.send_message(chat_id, challenge_text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .reply_markup(fight_challenge_keyboard(fight.id))
        .await?;

    let _ = huya_db::set_fight_message_id(pool, fight.id, sent.id.0).await;
    Ok(())
}

// ── Fight callbacks ───────────────────────────────────────────────────────────

/// huya_fa:{fight_id} — target accepts the challenge.
pub async fn huya_fight_accept_callback(
    bot: Bot,
    query: CallbackQuery,
    pool: PgPool,
) -> Result<(), AppError> {
    let data = query.data.as_deref().unwrap_or("");
    let fight_id: i32 = match data.trim_start_matches("huya_fa:").parse() {
        Ok(id) => id,
        Err(_) => { let _ = bot.answer_callback_query(query.id).await; return Ok(()); }
    };
    let clicker = query.from.id.0 as i64;

    let fight = match huya_db::accept_fight(&pool, fight_id, clicker).await? {
        Some(f) => f,
        None => {
            let _ = bot.answer_callback_query(query.id).text("Бой уже начался или отменён.").await;
            return Ok(());
        }
    };
    let _ = bot.answer_callback_query(query.id).await;

    let ch_name = user::get_by_tg_id(&pool, fight.challenger_tg_id).await?
        .map(|u| u.full_username(true)).unwrap_or_else(|| "???".to_string());
    let tg_name = user::get_by_tg_id(&pool, fight.target_tg_id).await?
        .map(|u| u.full_username(true)).unwrap_or_else(|| "???".to_string());

    // Get max_hp for HP bar display.
    let (ch_huya, _) = huya_db::get_or_create(&pool, fight.chat_id, fight.challenger_tg_id).await?;
    let (tg_huya, _) = huya_db::get_or_create(&pool, fight.chat_id, fight.target_tg_id).await?;

    let text = build_round_status_text(&fight, &ch_name, &tg_name, ch_huya.max_hp(), tg_huya.max_hp());

    if let Some(ref msg) = query.message {
        let _ = bot.edit_message_text(msg.chat().id, msg.id(), text)
            .parse_mode(teloxide::types::ParseMode::Html)
            .reply_markup(fight_pick_keyboard(fight_id))
            .await;
    }
    Ok(())
}

/// Build the round status block shown during active fighting.
fn build_round_status_text(
    fight: &huya_db::HuyaFight,
    ch_name: &str,
    tg_name: &str,
    ch_max: i32,
    tg_max: i32,
) -> String {
    LOCALE.t_fmt("ru", "huya.fight_round_status", &[
        ("challenger", &escape_html(ch_name)),
        ("target",     &escape_html(tg_name)),
        ("ch_hp",      &fight.ch_hp.to_string()),
        ("ch_max_hp",  &ch_max.to_string()),
        ("ch_bar",     &hp_bar_str(fight.ch_hp, ch_max)),
        ("tg_hp",      &fight.tg_hp.to_string()),
        ("tg_max_hp",  &tg_max.to_string()),
        ("tg_bar",     &hp_bar_str(fight.tg_hp, tg_max)),
        ("round",      &fight.round.to_string()),
    ])
}

/// huya_fd:{fight_id} — target declines.
pub async fn huya_fight_decline_callback(
    bot: Bot,
    query: CallbackQuery,
    pool: PgPool,
) -> Result<(), AppError> {
    let data = query.data.as_deref().unwrap_or("");
    let fight_id: i32 = match data.trim_start_matches("huya_fd:").parse() {
        Ok(id) => id,
        Err(_) => { let _ = bot.answer_callback_query(query.id).await; return Ok(()); }
    };
    let clicker = query.from.id.0 as i64;

    let fight = match huya_db::decline_fight(&pool, fight_id, clicker).await? {
        Some(f) => f,
        None => {
            let _ = bot.answer_callback_query(query.id).text("Бой уже не актуален.").await;
            return Ok(());
        }
    };
    let _ = bot.answer_callback_query(query.id).await;

    // Refund challenger's action.
    let (ch_huya, _) = huya_db::get_or_create(&pool, fight.chat_id, fight.challenger_tg_id).await?;
    let _ = sqlx::query("UPDATE huya SET actions_left = LEAST(actions_left + 1, $1) WHERE id = $2")
        .bind(4_i32).bind(ch_huya.id).execute(&pool).await;

    let decliner = display_name_from_user(&query.from);

    let text = LOCALE.t_fmt("ru", "huya.fight_declined", &[("name", &escape_html(&decliner))]);

    if let Some(ref msg) = query.message {
        let _ = bot.edit_message_text(msg.chat().id, msg.id(), text)
            .parse_mode(teloxide::types::ParseMode::Html)
            .reply_markup(InlineKeyboardMarkup::new::<Vec<Vec<InlineKeyboardButton>>>(vec![]))
            .await;
    }
    Ok(())
}

/// huya_fm:{fight_id}:{pick} — a player submits their move.
pub async fn huya_fight_move_callback(
    bot: Bot,
    query: CallbackQuery,
    pool: PgPool,
) -> Result<(), AppError> {
    let data = query.data.as_deref().unwrap_or("");
    let parts: Vec<&str> = data.splitn(3, ':').collect();
    if parts.len() < 3 {
        let _ = bot.answer_callback_query(query.id).await;
        return Ok(());
    }
    let fight_id: i32 = match parts[1].parse() {
        Ok(id) => id,
        Err(_) => { let _ = bot.answer_callback_query(query.id).await; return Ok(()); }
    };
    let pick_id: i32 = match parts[2].parse::<i32>().ok().filter(|&p| p <= 2) {
        Some(p) => p,
        None => { let _ = bot.answer_callback_query(query.id).await; return Ok(()); }
    };

    let clicker = query.from.id.0 as i64;

    let fight = match huya_db::get_fight(&pool, fight_id).await? {
        Some(f) if f.status == "active" => f,
        _ => {
            let _ = bot.answer_callback_query(query.id).text("Бой уже завершён.").await;
            return Ok(());
        }
    };

    if clicker != fight.challenger_tg_id && clicker != fight.target_tg_id {
        let _ = bot.answer_callback_query(query.id).text(LOCALE.t("ru", "huya.fight_not_your_turn")).await;
        return Ok(());
    }

    // Prevent double-picking in the same round.
    let already_picked = if clicker == fight.challenger_tg_id {
        fight.challenger_pick.is_some()
    } else {
        fight.target_pick.is_some()
    };
    if already_picked {
        let _ = bot.answer_callback_query(query.id).text("Ты уже сделал выбор!").await;
        return Ok(());
    }

    // Get player names and max HP before storing (we need them either way).
    let ch_name = user::get_by_tg_id(&pool, fight.challenger_tg_id).await?
        .map(|u| u.full_username(true)).unwrap_or_else(|| "???".to_string());
    let tg_name = user::get_by_tg_id(&pool, fight.target_tg_id).await?
        .map(|u| u.full_username(true)).unwrap_or_else(|| "???".to_string());
    let (ch_huya, _) = huya_db::get_or_create(&pool, fight.chat_id, fight.challenger_tg_id).await?;
    let (tg_huya, _) = huya_db::get_or_create(&pool, fight.chat_id, fight.target_tg_id).await?;
    let ch_max = ch_huya.max_hp();
    let tg_max = tg_huya.max_hp();

    let updated = match huya_db::store_pick(&pool, fight_id, clicker, pick_id).await? {
        Some(f) => f,
        None => {
            let _ = bot.answer_callback_query(query.id).text("Что-то пошло не так.").await;
            return Ok(());
        }
    };

    let _ = bot.answer_callback_query(query.id)
        .text(format!("Ты выбрал {}!", move_emoji(pick_id)))
        .await;

    // One player picked — waiting for the other.
    if updated.challenger_pick.is_none() || updated.target_pick.is_none() {
        let picker_name = display_name_from_user(&query.from);

        let wait_text = format!(
            "{}\n\n{}",
            build_round_status_text(&updated, &ch_name, &tg_name, ch_max, tg_max),
            LOCALE.t_fmt("ru", "huya.fight_waiting", &[("name", &escape_html(&picker_name))]),
        );

        if let Some(ref msg) = query.message {
            let _ = bot.edit_message_text(msg.chat().id, msg.id(), wait_text)
                .parse_mode(teloxide::types::ParseMode::Html)
                .reply_markup(fight_pick_keyboard(fight_id))
                .await;
        }
        return Ok(());
    }

    // Both picked — process this round.
    let ch_pick = updated.challenger_pick.unwrap();
    let tg_pick = updated.target_pick.unwrap();
    let prev_round = updated.round; // round number of the round that just concluded

    let round_result = huya_db::process_round(&pool, &updated, ch_pick, tg_pick).await?;

    // Build round summary line.
    let round_summary = if round_result.round_winner_tg_id.is_none() {
        LOCALE.t_fmt("ru", "huya.fight_round_tie", &[
            ("round",   &prev_round.to_string()),
            ("ch_pick", move_emoji(ch_pick)),
            ("tg_pick", move_emoji(tg_pick)),
        ])
    } else {
        let w_tg_id = round_result.round_winner_tg_id.unwrap();
        let (winner_name, damage) = if w_tg_id == updated.challenger_tg_id {
            (ch_name.as_str(), round_result.tg_damage)
        } else {
            (tg_name.as_str(), round_result.ch_damage)
        };
        LOCALE.t_fmt("ru", "huya.fight_round_win", &[
            ("round",   &prev_round.to_string()),
            ("ch_pick", move_emoji(ch_pick)),
            ("tg_pick", move_emoji(tg_pick)),
            ("winner",  &escape_html(winner_name)),
            ("damage",  &damage.to_string()),
        ])
    };

    // HP status after round.
    let f = &round_result.fight;
    let hp_status = format!(
        "\n\n<b>{}</b>: 💚{}/{} {}\n<b>{}</b>: 💚{}/{} {}",
        escape_html(&ch_name), f.ch_hp, ch_max, hp_bar_str(f.ch_hp, ch_max),
        escape_html(&tg_name), f.tg_hp, tg_max, hp_bar_str(f.tg_hp, tg_max),
    );

    if round_result.fight_over {
        let _ = huya_db::finish_huya_fight(&pool, fight_id).await;

        let result_line = if let Some(winner_tg_id) = round_result.fight_winner_tg_id {
            let loser_tg_id = if winner_tg_id == updated.challenger_tg_id {
                updated.target_tg_id
            } else {
                updated.challenger_tg_id
            };

            let (steal_mm, elo_gain) = huya_db::finalize_fight_result(
                &pool, f, winner_tg_id, loser_tg_id,
            ).await?;

            let _ = crate::db::duel::add_huya_elo(&pool, updated.chat_id, winner_tg_id, elo_gain).await;

            let winner_name = if winner_tg_id == updated.challenger_tg_id { &ch_name } else { &tg_name };
            let loser_name  = if loser_tg_id  == updated.challenger_tg_id { &ch_name } else { &tg_name };

            LOCALE.t_fmt("ru", "huya.fight_result_win", &[
                ("winner",   &escape_html(winner_name)),
                ("loser",    &escape_html(loser_name)),
                ("steal_cm", &mm_to_cm_str(steal_mm)),
                ("elo_gain", &elo_gain.to_string()),
            ])
        } else {
            // Also write HP back (both at draw, no length transfer).
            let (ch_h, _) = huya_db::get_or_create(&pool, f.chat_id, f.challenger_tg_id).await?;
            let (tg_h, _) = huya_db::get_or_create(&pool, f.chat_id, f.target_tg_id).await?;
            let _ = sqlx::query("UPDATE huya SET hp = $1, atk_boost = 0, def_boost = 0 WHERE id = $2")
                .bind(f.ch_hp.max(1)).bind(ch_h.id).execute(&pool).await;
            let _ = sqlx::query("UPDATE huya SET hp = $1, atk_boost = 0, def_boost = 0 WHERE id = $2")
                .bind(f.tg_hp.max(1)).bind(tg_h.id).execute(&pool).await;

            LOCALE.t("ru", "huya.fight_result_draw").to_string()
        };

        let final_text = format!("{}{}\n\n{}", round_summary, hp_status, result_line);

        if let Some(ref msg) = query.message {
            let _ = bot.edit_message_text(msg.chat().id, msg.id(), final_text)
                .parse_mode(teloxide::types::ParseMode::Html)
                .reply_markup(InlineKeyboardMarkup::new::<Vec<Vec<InlineKeyboardButton>>>(vec![]))
                .await;
        }
    } else {
        // Fight continues — show round result + updated HP bars + pick keyboard.
        let next_round_label = format!(
            "\n\nРаунд {}/5 — выбирайте тактику!",
            f.round,
        );
        let cont_text = format!("{}{}{}", round_summary, hp_status, next_round_label);

        if let Some(ref msg) = query.message {
            let _ = bot.edit_message_text(msg.chat().id, msg.id(), cont_text)
                .parse_mode(teloxide::types::ParseMode::Html)
                .reply_markup(fight_pick_keyboard(fight_id))
                .await;
        }
    }

    Ok(())
}

// ── Raid ──────────────────────────────────────────────────────────────────────

fn raid_lobby_keyboard(
    raid_id: i32,
    can_start: bool,
    leader_tg_id: i64,
    target_tg_id: i64,
    members: &[huya_db::HuyaRaidMember],
) -> InlineKeyboardMarkup {
    let mut rows = vec![vec![
        InlineKeyboardButton::callback(
            LOCALE.t("ru", "huya.raid_join_btn").to_string(),
            format!("huya_rj:{raid_id}"),
        ),
    ]];
    if can_start {
        rows.push(vec![InlineKeyboardButton::callback(
            LOCALE.t("ru", "huya.raid_start_btn").to_string(),
            format!("huya_rs:{raid_id}"),
        )]);
        rows.push(vec![InlineKeyboardButton::callback(
            LOCALE.t("ru", "huya.raid_override_btn").to_string(),
            format!("huya_ro:{raid_id}"),
        )]);
        rows.push(vec![InlineKeyboardButton::callback(
            LOCALE.t("ru", "huya.raid_disband_btn").to_string(),
            format!("huya_rx:{raid_id}"),
        )]);
    }
    for m in members.iter().filter(|m| m.side == "party" && m.tg_id != leader_tg_id && m.tg_id != target_tg_id) {
        rows.push(vec![InlineKeyboardButton::callback(
            LOCALE.t_fmt("ru", "huya.raid_kick_btn", &[("tg_id", &m.tg_id.to_string())]),
            format!("huya_rk:{raid_id}:{}", m.tg_id),
        )]);
    }
    InlineKeyboardMarkup::new(rows)
}

fn raid_challenge_keyboard(raid_id: i32) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::callback(
            LOCALE.t("ru", "huya.raid_accept_btn").to_string(),
            format!("huya_ra:{raid_id}"),
        ),
        InlineKeyboardButton::callback(
            LOCALE.t("ru", "huya.raid_decline_btn").to_string(),
            format!("huya_rd:{raid_id}"),
        ),
    ]])
}

fn raid_turn_keyboard(raid_id: i32, actor_side: &str) -> InlineKeyboardMarkup {
    let mut row = vec![
        InlineKeyboardButton::callback("⚔️ Атака", format!("huya_rt:{raid_id}:attack")),
        InlineKeyboardButton::callback("🛡️ Защита", format!("huya_rt:{raid_id}:guard")),
    ];
    if actor_side == "party" {
        row.push(InlineKeyboardButton::callback(
            "🎯 Фокус",
            format!("huya_rt:{raid_id}:focus"),
        ));
    }
    InlineKeyboardMarkup::new(vec![row])
}

async fn raid_member_name(pool: &PgPool, tg_id: i64) -> Result<String, AppError> {
    Ok(user::get_by_tg_id(pool, tg_id)
        .await?
        .map(|u| u.full_username(true))
        .unwrap_or_else(|| format!("id:{tg_id}")))
}

async fn build_raid_lobby_text(pool: &PgPool, raid: &huya_db::HuyaRaid) -> Result<String, AppError> {
    let members = huya_db::get_raid_members(pool, raid.id).await?;
    let mut party_names = Vec::new();
    for m in members.iter().filter(|m| m.side == "party") {
        party_names.push(format!("• {}", escape_html(&raid_member_name(pool, m.tg_id).await?)));
    }
    let target_name = escape_html(&raid_member_name(pool, raid.target_tg_id).await?);
    let accepted = if raid.target_accepted {
        LOCALE.t("ru", "huya.raid_target_yes")
    } else {
        LOCALE.t("ru", "huya.raid_target_no")
    };
    let power = huya_db::raid_power_check(pool, raid.id).await?;
    let party_power = power.as_ref().map(|x| x.party_power).unwrap_or(0);
    let target_power = power.as_ref().map(|x| x.target_power).unwrap_or(0);
    let power_limit = ((target_power as f64) * 1.15_f64).round() as i64;
    Ok(LOCALE.t_fmt(
        "ru",
        "huya.raid_lobby",
        &[
            ("target", &target_name),
            ("accepted", accepted),
            ("party_count", &party_names.len().to_string()),
            ("party_power", &party_power.to_string()),
            ("target_power", &target_power.to_string()),
            ("power_limit", &power_limit.to_string()),
            ("party_list", &party_names.join("\n")),
        ],
    ))
}

async fn build_raid_battle_text(
    pool: &PgPool,
    result: &huya_db::RaidTurnResult,
    steal_cm: Option<String>,
) -> Result<String, AppError> {
    let mut lines = Vec::new();
    let mut actor_line = String::new();
    let actor_name = raid_member_name(pool, result.actor_tg_id).await?;
    actor_line.push_str(&format!(
        "{} {}",
        if result.actor_side == "party" { "🟢" } else { "🔴" },
        escape_html(&actor_name)
    ));

    lines.push(LOCALE.t_fmt(
        "ru",
        "huya.raid_turn_header",
        &[
            ("round", &result.raid.round.to_string()),
            ("actor", &actor_line),
            ("log", &result.log_line),
        ],
    ));

    lines.push(LOCALE.t("ru", "huya.raid_status_header").to_string());
    for m in &result.members {
        let nm = escape_html(&raid_member_name(pool, m.tg_id).await?);
        let side = if m.side == "party" { "🟢" } else { "🔴" };
        lines.push(format!(
            "{} {} — {} HP {}",
            side,
            nm,
            m.hp_snapshot.max(0),
            if m.is_alive { "" } else { "☠️" }
        ));
    }

    if result.finished {
        let winner_text = match result.winner_side.as_deref() {
            Some("party") => LOCALE.t("ru", "huya.raid_winner_party").to_string(),
            Some("target") => LOCALE.t("ru", "huya.raid_winner_target").to_string(),
            _ => LOCALE.t("ru", "huya.raid_draw").to_string(),
        };
        lines.push(format!("\n{}", winner_text));
        if let Some(cm) = steal_cm {
            lines.push(LOCALE.t_fmt("ru", "huya.raid_steal_pool", &[("steal_cm", &cm)]));
        }
    }

    Ok(lines.join("\n"))
}

async fn handle_raid(
    bot: &Bot,
    pool: &PgPool,
    msg: &Message,
    chat_id_raw: i64,
    leader_tg_id: i64,
    leader_name: &str,
    sub_arg: &str,
) -> Result<(), AppError> {
    let chat_id = msg.chat.id;
    let target_tg_id = match resolve_target_global(pool, msg, sub_arg).await {
        ResolvedTarget::User(id) => id,
        ResolvedTarget::IsBot => {
            bot.send_message(chat_id, LOCALE.t("ru", "huya.raid_no_target")).await?;
            return Ok(());
        }
        ResolvedTarget::NotFound => {
            bot.send_message(chat_id, LOCALE.t("ru", "huya.raid_no_target")).await?;
            return Ok(());
        }
    };
    if target_tg_id == leader_tg_id {
        bot.send_message(chat_id, LOCALE.t("ru", "huya.raid_self")).await?;
        return Ok(());
    }

    if huya_db::get_pending_or_active_raid(pool, chat_id_raw).await?.is_some() {
        bot.send_message(chat_id, LOCALE.t("ru", "huya.raid_already_live")).await?;
        return Ok(());
    }
    if huya_db::raid_target_on_cooldown(pool, chat_id_raw, target_tg_id).await? {
        bot.send_message(chat_id, LOCALE.t("ru", "huya.raid_target_cooldown")).await?;
        return Ok(());
    }

    let (leader_huya, _) = huya_db::get_or_create(pool, chat_id_raw, leader_tg_id).await?;
    let (target_huya, _) = huya_db::get_or_create(pool, chat_id_raw, target_tg_id).await?;

    let raid = huya_db::create_raid(
        pool,
        chat_id_raw,
        leader_tg_id,
        target_tg_id,
        leader_huya.hp,
        target_huya.hp,
    )
    .await?;
    let target_name = raid_member_name(pool, target_tg_id).await?;
    let text = LOCALE.t_fmt(
        "ru",
        "huya.raid_challenge",
        &[
            ("leader", &escape_html(leader_name)),
            ("target", &escape_html(&target_name)),
            ("raid_id", &raid.id.to_string()),
        ],
    );
    let sent = bot
        .send_message(chat_id, text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .reply_markup(raid_challenge_keyboard(raid.id))
        .await?;
    let _ = huya_db::set_raid_message_id(pool, raid.id, sent.id.0).await;
    Ok(())
}

pub async fn huya_raid_accept_callback(
    bot: Bot,
    query: CallbackQuery,
    pool: PgPool,
) -> Result<(), AppError> {
    let data = query.data.as_deref().unwrap_or("");
    let raid_id: i32 = match data.trim_start_matches("huya_ra:").parse() {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    let clicker = query.from.id.0 as i64;
    let Some(raid) = huya_db::accept_raid(&pool, raid_id, clicker).await? else {
        let _ = bot.answer_callback_query(query.id).text("Рейд неактуален").await;
        return Ok(());
    };
    let text = build_raid_lobby_text(&pool, &raid).await?;
    let members = huya_db::get_raid_members(&pool, raid.id).await?;
    let _ = bot.answer_callback_query(query.id).text("Цель приняла рейд").await;
    if let Some(msg) = query.message {
        let _ = bot
            .edit_message_text(msg.chat().id, msg.id(), text)
            .parse_mode(teloxide::types::ParseMode::Html)
            .reply_markup(raid_lobby_keyboard(raid.id, true, raid.leader_tg_id, raid.target_tg_id, &members))
            .await;
    }
    Ok(())
}

pub async fn huya_raid_decline_callback(
    bot: Bot,
    query: CallbackQuery,
    pool: PgPool,
) -> Result<(), AppError> {
    let data = query.data.as_deref().unwrap_or("");
    let raid_id: i32 = match data.trim_start_matches("huya_rd:").parse() {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    let clicker = query.from.id.0 as i64;
    let Some(_raid) = huya_db::decline_raid(&pool, raid_id, clicker).await? else {
        let _ = bot.answer_callback_query(query.id).text("Рейд неактуален").await;
        return Ok(());
    };
    let _ = bot.answer_callback_query(query.id).text("Рейд отклонен").await;
    if let Some(msg) = query.message {
        let _ = bot
            .edit_message_text(msg.chat().id, msg.id(), LOCALE.t("ru", "huya.raid_declined"))
            .reply_markup(InlineKeyboardMarkup::new::<Vec<Vec<InlineKeyboardButton>>>(vec![]))
            .await;
    }
    Ok(())
}

pub async fn huya_raid_join_callback(
    bot: Bot,
    query: CallbackQuery,
    pool: PgPool,
) -> Result<(), AppError> {
    let data = query.data.as_deref().unwrap_or("");
    let raid_id: i32 = match data.trim_start_matches("huya_rj:").parse() {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    let clicker = query.from.id.0 as i64;
    let Some(raid) = huya_db::get_raid(&pool, raid_id).await? else {
        return Ok(());
    };
    let (h, _) = huya_db::get_or_create(&pool, raid.chat_id, clicker).await?;
    let joined = huya_db::raid_join_party(&pool, raid_id, clicker, h.hp).await?;
    let answer = if joined {
        LOCALE.t("ru", "huya.raid_joined")
    } else {
        LOCALE.t("ru", "huya.raid_join_failed")
    };
    let _ = bot.answer_callback_query(query.id).text(answer).await;
    let Some(raid_now) = huya_db::get_raid(&pool, raid_id).await? else {
        return Ok(());
    };
    let text = build_raid_lobby_text(&pool, &raid_now).await?;
    let members = huya_db::get_raid_members(&pool, raid_now.id).await?;
    if let Some(msg) = query.message {
        let _ = bot
            .edit_message_text(msg.chat().id, msg.id(), text)
            .parse_mode(teloxide::types::ParseMode::Html)
            .reply_markup(raid_lobby_keyboard(raid_now.id, true, raid_now.leader_tg_id, raid_now.target_tg_id, &members))
            .await;
    }
    Ok(())
}

pub async fn huya_raid_start_callback(
    bot: Bot,
    query: CallbackQuery,
    pool: PgPool,
) -> Result<(), AppError> {
    let data = query.data.as_deref().unwrap_or("");
    let raid_id: i32 = match data.trim_start_matches("huya_rs:").parse() {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    let clicker = query.from.id.0 as i64;
    let Some(raid) = huya_db::get_raid(&pool, raid_id).await? else {
        return Ok(());
    };
    if raid.leader_tg_id != clicker {
        let _ = bot.answer_callback_query(query.id).text(LOCALE.t("ru", "huya.raid_only_leader")).await;
        return Ok(());
    }
    let Some(power) = huya_db::raid_power_check(&pool, raid_id).await? else {
        let _ = bot.answer_callback_query(query.id).text("Ошибка power check").await;
        return Ok(());
    };
    if !power.within_window {
        let _ = bot.answer_callback_query(query.id).text(
            LOCALE.t_fmt(
                "ru",
                "huya.raid_power_fail",
                &[
                    ("party_power", &power.party_power.to_string()),
                    ("target_power", &power.target_power.to_string()),
                ],
            ),
        ).await;
        return Ok(());
    }

    let Some(started) = huya_db::raid_start(&pool, raid_id).await? else {
        let _ = bot.answer_callback_query(query.id).text(LOCALE.t("ru", "huya.raid_start_failed")).await;
        return Ok(());
    };
    let Some(actor_tg_id) = huya_db::raid_current_actor(&pool, started.id).await? else {
        return Ok(());
    };
    let actor_name = raid_member_name(&pool, actor_tg_id).await?;
    let text = LOCALE.t_fmt(
        "ru",
        "huya.raid_started",
        &[("round", &started.round.to_string()), ("actor", &escape_html(&actor_name))],
    );
    let actor_side = huya_db::get_raid_members(&pool, started.id)
        .await?
        .into_iter()
        .find(|m| m.tg_id == actor_tg_id)
        .map(|m| m.side)
        .unwrap_or_else(|| "party".to_string());
    let _ = bot.answer_callback_query(query.id).text("Рейд стартовал").await;
    if let Some(msg) = query.message {
        let _ = bot
            .edit_message_text(msg.chat().id, msg.id(), text)
            .parse_mode(teloxide::types::ParseMode::Html)
            .reply_markup(raid_turn_keyboard(started.id, &actor_side))
            .await;
    }
    Ok(())
}

pub async fn huya_raid_kick_callback(
    bot: Bot,
    query: CallbackQuery,
    pool: PgPool,
) -> Result<(), AppError> {
    let data = query.data.as_deref().unwrap_or("");
    let parts: Vec<&str> = data.splitn(3, ':').collect();
    if parts.len() < 3 {
        return Ok(());
    }
    let raid_id: i32 = match parts[1].parse() {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    let kicked_tg_id: i64 = match parts[2].parse() {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    let clicker = query.from.id.0 as i64;
    let Some(raid) = huya_db::get_raid(&pool, raid_id).await? else {
        return Ok(());
    };
    if raid.leader_tg_id != clicker {
        let _ = bot.answer_callback_query(query.id).text(LOCALE.t("ru", "huya.raid_only_leader")).await;
        return Ok(());
    }
    let ok = huya_db::raid_kick_party_member(&pool, raid_id, clicker, kicked_tg_id).await?;
    let _ = bot.answer_callback_query(query.id)
        .text(if ok { LOCALE.t("ru", "huya.raid_kick_success") } else { LOCALE.t("ru", "huya.raid_kick_fail") })
        .await;
    let Some(raid_now) = huya_db::get_raid(&pool, raid_id).await? else {
        return Ok(());
    };
    let text = build_raid_lobby_text(&pool, &raid_now).await?;
    let members = huya_db::get_raid_members(&pool, raid_now.id).await?;
    if let Some(msg) = query.message {
        let _ = bot
            .edit_message_text(msg.chat().id, msg.id(), text)
            .parse_mode(teloxide::types::ParseMode::Html)
            .reply_markup(raid_lobby_keyboard(raid_now.id, true, raid_now.leader_tg_id, raid_now.target_tg_id, &members))
            .await;
    }
    Ok(())
}

pub async fn huya_raid_disband_callback(
    bot: Bot,
    query: CallbackQuery,
    pool: PgPool,
) -> Result<(), AppError> {
    let data = query.data.as_deref().unwrap_or("");
    let raid_id: i32 = match data.trim_start_matches("huya_rx:").parse() {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    let clicker = query.from.id.0 as i64;
    let disbanded = huya_db::disband_raid_by_leader(&pool, raid_id, clicker).await?;
    let _ = bot.answer_callback_query(query.id)
        .text(if disbanded.is_some() { LOCALE.t("ru", "huya.raid_disband_success") } else { LOCALE.t("ru", "huya.raid_disband_fail") })
        .await;
    if disbanded.is_some() && let Some(msg) = query.message {
        let _ = bot
            .edit_message_text(msg.chat().id, msg.id(), LOCALE.t("ru", "huya.raid_disband_success"))
            .reply_markup(InlineKeyboardMarkup::new::<Vec<Vec<InlineKeyboardButton>>>(vec![]))
            .await;
    }
    Ok(())
}

pub async fn huya_raid_override_callback(
    bot: Bot,
    query: CallbackQuery,
    pool: PgPool,
) -> Result<(), AppError> {
    let data = query.data.as_deref().unwrap_or("");
    let raid_id: i32 = match data.trim_start_matches("huya_ro:").parse() {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    let clicker = query.from.id.0 as i64;
    let Some(raid) = huya_db::get_raid(&pool, raid_id).await? else {
        return Ok(());
    };
    if raid.target_tg_id != clicker {
        let _ = bot.answer_callback_query(query.id).text(LOCALE.t("ru", "huya.raid_override_only_target")).await;
        return Ok(());
    }
    let Some(raid_now) = huya_db::approve_raid_power_override(&pool, raid_id, clicker).await? else {
        let _ = bot.answer_callback_query(query.id).text(LOCALE.t("ru", "huya.raid_override_fail")).await;
        return Ok(());
    };
    let _ = bot.answer_callback_query(query.id).text(LOCALE.t("ru", "huya.raid_override_success")).await;
    let text = build_raid_lobby_text(&pool, &raid_now).await?;
    let members = huya_db::get_raid_members(&pool, raid_now.id).await?;
    if let Some(msg) = query.message {
        let _ = bot
            .edit_message_text(msg.chat().id, msg.id(), text)
            .parse_mode(teloxide::types::ParseMode::Html)
            .reply_markup(raid_lobby_keyboard(raid_now.id, true, raid_now.leader_tg_id, raid_now.target_tg_id, &members))
            .await;
    }
    Ok(())
}

pub async fn huya_raid_turn_callback(
    bot: Bot,
    query: CallbackQuery,
    pool: PgPool,
) -> Result<(), AppError> {
    let data = query.data.as_deref().unwrap_or("");
    let parts: Vec<&str> = data.splitn(3, ':').collect();
    if parts.len() < 3 {
        return Ok(());
    }
    let raid_id: i32 = match parts[1].parse() {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    let action = match parts[2] {
        "attack" => huya_db::RaidTurnAction::Attack,
        "guard" => huya_db::RaidTurnAction::Guard,
        "focus" => huya_db::RaidTurnAction::Focus,
        _ => return Ok(()),
    };
    let clicker = query.from.id.0 as i64;

    let Some(current_actor) = huya_db::raid_current_actor(&pool, raid_id).await? else {
        let _ = bot.answer_callback_query(query.id).text("Рейд не активен").await;
        return Ok(());
    };
    if current_actor != clicker {
        let _ = bot.answer_callback_query(query.id).text(LOCALE.t("ru", "huya.raid_not_your_turn")).await;
        return Ok(());
    }

    let Some(result) = huya_db::raid_take_turn(&pool, raid_id, clicker, action).await? else {
        let _ = bot.answer_callback_query(query.id).text("Ход не применился").await;
        return Ok(());
    };
    let _ = bot.answer_callback_query(query.id).text("Ход принят").await;

    let mut steal_cm = None;
    if result.finished {
        let steal_mm = huya_db::raid_apply_rewards(&pool, &result.raid, result.winner_side.as_deref()).await?;
        if steal_mm > 0 {
            steal_cm = Some(mm_to_cm_str(steal_mm));
        }
    }
    let text = build_raid_battle_text(&pool, &result, steal_cm).await?;
    if let Some(msg) = query.message {
        if result.finished {
            let _ = bot
                .edit_message_text(msg.chat().id, msg.id(), text)
                .parse_mode(teloxide::types::ParseMode::Html)
                .reply_markup(InlineKeyboardMarkup::new::<Vec<Vec<InlineKeyboardButton>>>(vec![]))
                .await;
        } else {
            let Some(next_actor) = huya_db::raid_current_actor(&pool, raid_id).await? else {
                return Ok(());
            };
            let next_side = result
                .members
                .iter()
                .find(|m| m.tg_id == next_actor)
                .map(|m| m.side.clone())
                .unwrap_or_else(|| "party".to_string());
            let _ = bot
                .edit_message_text(msg.chat().id, msg.id(), text)
                .parse_mode(teloxide::types::ParseMode::Html)
                .reply_markup(raid_turn_keyboard(raid_id, &next_side))
                .await;
        }
    }
    Ok(())
}

pub async fn huya_raid_focus_callback(
    bot: Bot,
    query: CallbackQuery,
    _pool: PgPool,
) -> Result<(), AppError> {
    let _ = bot.answer_callback_query(query.id).text("Фокус теперь через кнопку хода").await;
    Ok(())
}

// ── Steal ─────────────────────────────────────────────────────────────────────

async fn handle_steal(
    bot: &Bot,
    pool: &PgPool,
    msg: &Message,
    chat_id_raw: i64,
    attacker_tg_id: i64,
    attacker_name: &str,
    sub_arg: &str,
) -> Result<(), AppError> {
    let chat_id = msg.chat.id;

    let target_tg_id = match resolve_target_global(pool, msg, sub_arg).await {
        ResolvedTarget::User(id) => id,
        ResolvedTarget::IsBot => {
            bot.send_message(chat_id, LOCALE.t("ru", "huya.steal_self_bot"))
                .parse_mode(teloxide::types::ParseMode::Html)
                .await?;
            return Ok(());
        }
        ResolvedTarget::NotFound => {
            bot.send_message(chat_id, LOCALE.t("ru", "huya.steal_no_target")).await?;
            return Ok(());
        }
    };

    if target_tg_id == attacker_tg_id {
        let (h, _) = huya_db::get_or_create(pool, chat_id_raw, attacker_tg_id).await?;
        if !huya_db::consume_action(pool, &h).await? {
            bot.send_message(chat_id, LOCALE.t("ru", "huya.no_actions")).await?;
            return Ok(());
        }
        let updated = huya_db::self_fight(pool, chat_id_raw, attacker_tg_id).await?;
        let key = if updated.is_pussy() { "huya.steal_self_pussy" } else { "huya.steal_self" };
        bot.send_message(chat_id, LOCALE.t_fmt("ru", key, &[
            ("name",     &escape_html(attacker_name)),
            ("new_size", &updated.display_cm()),
        ]))
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
        return Ok(());
    }

    let (h, _) = huya_db::get_or_create(pool, chat_id_raw, attacker_tg_id).await?;
    if !huya_db::consume_action(pool, &h).await? {
        bot.send_message(chat_id, LOCALE.t("ru", "huya.no_actions")).await?;
        return Ok(());
    }

    let result = huya_db::steal_attempt(pool, chat_id_raw, attacker_tg_id, target_tg_id).await?;
    let target_name = user::get_by_tg_id(pool, target_tg_id).await?
        .map(|u| u.full_username(true)).unwrap_or_else(|| "???".to_string());

    let text = if result.success {
        LOCALE.t_fmt("ru", "huya.steal_success", &[
            ("attacker",   &escape_html(attacker_name)),
            ("target",     &escape_html(&target_name)),
            ("steal_cm",   &mm_to_cm_str(result.steal_mm)),
            ("new_size",   &result.attacker.display_cm()),
            ("chance_pct", &result.chance_pct.to_string()),
        ])
    } else if result.backlash_mm > 0 {
        LOCALE.t_rand_fmt("ru", "huya.steal_fail_backlash", &[
            ("attacker",     &escape_html(attacker_name)),
            ("target",       &escape_html(&target_name)),
            ("chance_pct",   &result.chance_pct.to_string()),
            ("backlash_cm",  &mm_to_cm_str(result.backlash_mm)),
            ("new_size",     &result.attacker.display_cm()),
        ])
    } else {
        LOCALE.t_fmt("ru", "huya.steal_fail", &[
            ("attacker",   &escape_html(attacker_name)),
            ("target",     &escape_html(&target_name)),
            ("chance_pct", &result.chance_pct.to_string()),
        ])
    };

    bot.send_message(chat_id, text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
    Ok(())
}

// ── Pet friend (/huyapet, /huya pet) ───────────────────────────────────────────

async fn handle_pet_friend(
    bot: &Bot,
    pool: &PgPool,
    msg: &Message,
    chat_id_raw: i64,
    from: &teloxide::types::User,
    from_name: &str,
    target_arg: &str,
) -> Result<(), AppError> {
    let chat_id = msg.chat.id;
    let from_tg_id = from.id.0 as i64;

    let target_tg_id = match resolve_target_global(pool, msg, target_arg.trim()).await {
        ResolvedTarget::User(id) => id,
        ResolvedTarget::IsBot => {
            bot.send_message(chat_id, LOCALE.t("ru", "huya.pet_bot"))
                .parse_mode(teloxide::types::ParseMode::Html)
                .await?;
            return Ok(());
        }
        ResolvedTarget::NotFound => {
            bot.send_message(chat_id, LOCALE.t("ru", "huya.pet_no_target"))
                .parse_mode(teloxide::types::ParseMode::Html)
                .await?;
            return Ok(());
        }
    };

    if target_tg_id == from_tg_id {
        bot.send_message(
            chat_id,
            LOCALE.t_fmt("ru", "huya.pet_self", &[("name", &escape_html(from_name))]),
        )
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
        return Ok(());
    }

    let result = huya_db::pet_friend(pool, chat_id_raw, from_tg_id, target_tg_id).await?;

    use huya_db::PetFriendState;
    match result.state {
        PetFriendState::TooManyFriends => {
            bot.send_message(chat_id, LOCALE.t("ru", "huya.pet_too_many_friends"))
                .parse_mode(teloxide::types::ParseMode::Html)
                .await?;
        }
        PetFriendState::NoEnergy => {
            bot.send_message(chat_id, LOCALE.t("ru", "huya.pet_no_energy"))
                .parse_mode(teloxide::types::ParseMode::Html)
                .await?;
        }
        PetFriendState::Ok => {
            let target_name = user::get_by_tg_id(pool, target_tg_id)
                .await?
                .map(|u| u.full_username(true))
                .unwrap_or_else(|| "???".to_string());

            let stage1 = LOCALE.t_fmt(
                "ru",
                "huya.pet_friend_stage1",
                &[
                    ("attacker", &escape_html(from_name)),
                    ("target", &escape_html(&target_name)),
                ],
            );
            let stage2 = LOCALE.t("ru", "huya.pet_friend_stage2");
            let stage3 = LOCALE.t("ru", "huya.pet_friend_stage3");

            let text = LOCALE.t_fmt(
                "ru",
                "huya.pet_friend_result",
                &[
                    ("stage1", &stage1),
                    ("stage2", &stage2),
                    ("stage3", &stage3),
                    ("heal", &result.heal.to_string()),
                    ("xp", &result.xp_gain.to_string()),
                    ("target_hp", &result.target.hp.to_string()),
                    ("target_max_hp", &result.target.max_hp().to_string()),
                    ("energy_left", &result.from.pet_energy_left.to_string()),
                ],
            );

            bot.send_message(chat_id, format!("🖐 {}", stage1))
                .parse_mode(teloxide::types::ParseMode::Html)
                .await?;
            tokio::time::sleep(Duration::from_secs(2)).await;

            bot.send_message(chat_id, stage2)
                .parse_mode(teloxide::types::ParseMode::Html)
                .await?;
            tokio::time::sleep(Duration::from_secs(2)).await;

            bot.send_message(chat_id, text)
                .parse_mode(teloxide::types::ParseMode::Html)
                .await?;
        }
    }

    Ok(())
}

pub async fn huyapet_handler(
    bot: Bot,
    msg: Message,
    cmd: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    if !msg.chat.is_group() && !msg.chat.is_supergroup() {
        return Ok(());
    }
    let chat_id_raw = msg.chat.id.0;
    let from = match msg.from.as_ref() {
        Some(f) => f,
        None => return Ok(()),
    };
    let from_name = display_name_from_user(from);

    user::upsert_tg_user(&pool, from).await?;

    let arg = match cmd {
        crate::handlers::commands::Cmd::Huyapet(ref s) => s.as_str(),
        _ => "",
    };

    agent_debug_log_6f3178(
        "H-pet-1",
        "huya.rs:huyapet_handler",
        "enter_huyapet_handler",
        serde_json::json!({
            "raw_text": msg.text(),
            "from_tg_id": from.id.0 as i64,
            "chat_id": chat_id_raw,
            "arg": arg,
            "has_reply": msg.reply_to_message().is_some(),
        }),
    );

    handle_pet_friend(&bot, &pool, &msg, chat_id_raw, from, &from_name, arg).await
}

// ── Registration ─────────────────────────────────────────────────────────────

pub async fn huyareg_handler(
    bot: Bot,
    msg: Message,
    pool: PgPool,
) -> Result<(), AppError> {
    if !msg.chat.is_group() && !msg.chat.is_supergroup() {
        return Ok(());
    }
    let from = match msg.from.as_ref() { Some(f) => f, None => return Ok(()) };
    let tg_id = from.id.0 as i64;
    let chat_id_raw = msg.chat.id.0;
    let name = display_name_from_user(from);

    user::upsert_tg_user(&pool, from).await?;

    let (h, was_created) = huya_db::get_or_create(&pool, chat_id_raw, tg_id).await?;

    let text = if was_created {
        LOCALE.t_rand_fmt("ru", "huya.reg_messages", &[("name", &escape_html(&name))])
    } else {
        let equ = huya_db::get_equipment(&pool, chat_id_raw, tg_id).await.unwrap_or_default();
        let inv = huya_db::get_inventory(&pool, chat_id_raw, tg_id).await.unwrap_or_default();
        format!(
            "{}\n\n{}",
            LOCALE.t("ru", "huya.already_registered"),
            huya_status_text(&h, &name, &equ, &inv),
        )
    };

    let mut req = bot.send_message(msg.chat.id, text)
        .parse_mode(teloxide::types::ParseMode::Html);
    if let Some(thread) = topic_thread_id(&msg) {
        req = req.message_thread_id(thread);
    }
    if !was_created {
        req = req.reply_markup(huya_stat_keyboard(tg_id, actions_available(&h)));
    }
    req.await?;
    Ok(())
}

// ── Leaderboard ───────────────────────────────────────────────────────────────

pub async fn huyatop_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    if !msg.chat.is_group() && !msg.chat.is_supergroup() {
        return Ok(());
    }
    let rows = huya_db::top(&pool, msg.chat.id.0, 10).await?;
    if rows.is_empty() {
        send_text_in_origin_topic(&bot, &msg, LOCALE.t("ru", "huya.top_empty")).await?;
        return Ok(());
    }

    let mut text = LOCALE.t("ru", "huya.top_header").to_string();
    for (i, (h, tg_id)) in rows.iter().enumerate() {
        let name = user::get_by_tg_id(&pool, *tg_id).await?
            .map(|u| u.full_username(false))
            .unwrap_or_else(|| format!("user_{}", tg_id));
        let medal = match i { 0 => "🥇", 1 => "🥈", 2 => "🥉", _ => "•" };
        let line = if h.is_pussy() {
            LOCALE.t_fmt("ru", "huya.top_entry_pussy", &[
                ("medal", medal), ("name", &escape_html(&name)), ("size", &h.display_cm()),
            ])
        } else {
            LOCALE.t_fmt("ru", "huya.top_entry", &[
                ("medal", medal), ("name", &escape_html(&name)),
                ("size", &h.display_cm()), ("level", &h.level.to_string()),
            ])
        };
        text.push_str(&line);
    }

    let mut request = bot
        .send_message(msg.chat.id, text)
        .parse_mode(teloxide::types::ParseMode::Html);
    if let Some(thread) = topic_thread_id(&msg) {
        request = request.message_thread_id(thread);
    }
    request.await?;
    Ok(())
}

// ── Skills (/huyaskills) ──────────────────────────────────────────────────────

const CAP_T1: i32 = 20;
const CAP_T2: i32 = 15;
const CAP_T3: i32 = 10;
const CAP_T4: i32 = 5;
const CAP_T5: i32 = 3;

fn skill_label(skill: &str) -> String {
    let key = match skill {
        "shaft" => "huya.skill_name_shaft",
        "skin" => "huya.skill_name_skin",
        "balls" => "huya.skill_name_balls",
        "cunning" => "huya.skill_name_cunning",
        "stamina" => "huya.skill_name_stamina",
        "pierce" => "huya.skill_name_pierce",
        "scales" => "huya.skill_name_scales",
        "spirit" => "huya.skill_name_spirit",
        "pickpocket" => "huya.skill_name_pickpocket",
        "dynamo" => "huya.skill_name_dynamo",
        "eggtwist" => "huya.skill_name_eggtwist",
        "bloodsucker" => "huya.skill_name_bloodsucker",
        "ironballs" => "huya.skill_name_ironballs",
        "vortex" => "huya.skill_name_vortex",
        "phantom" => "huya.skill_name_phantom",
        "berserker" => "huya.skill_name_berserker",
        "vampire" => "huya.skill_name_vampire",
        "fortress" => "huya.skill_name_fortress",
        "speedrun" => "huya.skill_name_speedrun",
        "ghost" => "huya.skill_name_ghost",
        "eternal" => "huya.skill_name_eternal",
        "absolute" => "huya.skill_name_absolute",
        _ => return "?".to_string(),
    };
    format!("{} +1", LOCALE.t("ru", key))
}

const SKILLS_PER_PAGE: usize = 7;

/// Build keyboard with only skills available to upgrade. Max 7 per page + Prev/Next.
fn skills_keyboard(huya: &Huya, page: u32) -> InlineKeyboardMarkup {
    let available = huya_db::skills_available_to_upgrade(huya);
    if available.is_empty() {
        return InlineKeyboardMarkup::new::<Vec<Vec<InlineKeyboardButton>>>(vec![]);
    }

    let total_pages = ((available.len() as u32) + (SKILLS_PER_PAGE as u32) - 1) / (SKILLS_PER_PAGE as u32);
    let page = page.min(total_pages.saturating_sub(1));
    let start = (page as usize) * SKILLS_PER_PAGE;
    let end = (start + SKILLS_PER_PAGE).min(available.len());
    let page_skills = &available[start..end];

    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();
    let mut current_row: Vec<InlineKeyboardButton> = Vec::new();
    for skill in page_skills {
        let label = skill_label(skill);
        current_row.push(InlineKeyboardButton::callback(
            label,
            format!("huya_skill:{}:{}", skill, page),
        ));
        if current_row.len() >= 2 {
            rows.push(std::mem::take(&mut current_row));
        }
    }
    if !current_row.is_empty() {
        rows.push(current_row);
    }

    if total_pages > 1 {
        let mut nav_row = Vec::new();
        if page > 0 {
            nav_row.push(InlineKeyboardButton::callback(
                LOCALE.t("ru", "huya.skills_prev").to_string(),
                format!("huya_skill_page:{}", page.saturating_sub(1)),
            ));
        }
        if page + 1 < total_pages {
            nav_row.push(InlineKeyboardButton::callback(
                LOCALE.t("ru", "huya.skills_next").to_string(),
                format!("huya_skill_page:{}", page + 1),
            ));
        }
        if !nav_row.is_empty() {
            rows.push(nav_row);
        }
    }

    InlineKeyboardMarkup::new(rows)
}

fn skills_text(h: &Huya, name: &str) -> String {
    let header = LOCALE.t_fmt("ru", "huya.skills_header", &[
        ("name", &escape_html(name)),
        ("sp", &h.skill_points.to_string()),
    ]);
    let mut out = vec![header];

    let line = |skill: &str, l: i32, c: i32, bonus: &str| {
        format!("{}  {}\t{}", LOCALE.t("ru", &format!("huya.skill_name_{}", skill)), Huya::skill_bar(l, c), bonus)
    };

    // Tier 1 — всегда виден как базовый
    out.push(LOCALE.t("ru", "huya.skills_tier1").to_string());
    out.push(line("shaft", h.skill_shaft, CAP_T1, &format!("+{}% ATK", h.skill_shaft * 4)));
    out.push(line("skin", h.skill_skin, CAP_T1, &format!("-{}% dmg", h.skill_skin * 3)));
    out.push(line("balls", h.skill_balls, CAP_T1, &format!("+{} maxHP", h.skill_balls * 15)));
    out.push(line("cunning", h.skill_cunning, CAP_T1, &format!("+{}% steal", (h.skill_cunning as f64 * 2.5) as i32)));
    out.push(line("stamina", h.skill_stamina, CAP_T1, &format!("+{} HP/act", h.skill_stamina * 3)));

    // Tier 2 — показываем только если ветка реально открыта (есть уровень или выполнены условия разблокировки)
    let mut tier2_lines: Vec<String> = Vec::new();
    if h.skill_shaft >= 8 || h.skill_pierce > 0 {
        tier2_lines.push(line("pierce", h.skill_pierce, CAP_T2, &format!("-{}% enemy DEF", h.skill_pierce * 4)));
    }
    if h.skill_skin >= 8 || h.skill_scales > 0 {
        tier2_lines.push(line("scales", h.skill_scales, CAP_T2, &format!("-{}% steal vs you", (h.skill_scales as f64 * 2.5) as i32)));
    }
    if h.skill_balls >= 8 || h.skill_spirit > 0 {
        tier2_lines.push(line("spirit", h.skill_spirit, CAP_T2, "+12 HP/round win"));
    }
    if h.skill_cunning >= 8 || h.skill_pickpocket > 0 {
        tier2_lines.push(line("pickpocket", h.skill_pickpocket, CAP_T2, "steal XP"));
    }
    if h.skill_stamina >= 8 || h.skill_dynamo > 0 {
        tier2_lines.push(line("dynamo", h.skill_dynamo, CAP_T2, &format!("+{} max act", h.skill_dynamo / 5)));
    }
    if !tier2_lines.is_empty() {
        out.push(LOCALE.t("ru", "huya.skills_tier2").to_string());
        out.extend(tier2_lines);
    }

    // Tier 3 — показываем только разблокированные комбо (есть уровень или оба родителя >=5)
    let mut tier3_lines: Vec<String> = Vec::new();
    if (h.skill_pierce >= 5 && h.skill_spirit >= 5) || h.skill_eggtwist > 0 {
        tier3_lines.push(line("eggtwist", h.skill_eggtwist, CAP_T3, "rnd3 x2 dmg"));
    }
    if (h.skill_pierce >= 5 && h.skill_pickpocket >= 5) || h.skill_bloodsucker > 0 {
        tier3_lines.push(line("bloodsucker", h.skill_bloodsucker, CAP_T3, "round win=steal"));
    }
    if (h.skill_scales >= 5 && h.skill_spirit >= 5) || h.skill_ironballs > 0 {
        tier3_lines.push(line("ironballs", h.skill_ironballs, CAP_T3, "counter on dodge"));
    }
    if (h.skill_spirit >= 5 && h.skill_dynamo >= 5) || h.skill_vortex > 0 {
        tier3_lines.push(line("vortex", h.skill_vortex, CAP_T3, "rnd1 crit"));
    }
    if (h.skill_pickpocket >= 5 && h.skill_scales >= 5) || h.skill_phantom > 0 {
        tier3_lines.push(line("phantom", h.skill_phantom, CAP_T3, "1 steal at 0 act"));
    }
    if !tier3_lines.is_empty() {
        out.push(LOCALE.t("ru", "huya.skills_tier3").to_string());
        out.extend(tier3_lines);
    }

    if h.t4_visible("berserker") || h.t4_visible("vampire") || h.t4_visible("fortress") || h.t4_visible("speedrun") || h.t4_visible("ghost") {
        out.push(LOCALE.t("ru", "huya.skills_tier4").to_string());
        out.push(line("berserker", h.skill_berserker, CAP_T4, "hp<30% ATK x2"));
        out.push(line("vampire", h.skill_vampire, CAP_T4, "win heals"));
        out.push(line("fortress", h.skill_fortress, CAP_T4, "min 1cm"));
        out.push(line("speedrun", h.skill_speedrun, CAP_T4, "1 rnd fight"));
        out.push(line("ghost", h.skill_ghost, CAP_T4, "30% dodge steal"));
    } else {
        out.push(LOCALE.t("ru", "huya.skills_tier4_hidden").to_string());
    }

    if h.eternal_visible() || h.absolute_visible() {
        out.push(LOCALE.t("ru", "huya.skills_tier5").to_string());
        out.push(line("eternal", h.skill_eternal, CAP_T5, &format!("+{}% all", h.skill_eternal * 15)));
        out.push(line("absolute", h.skill_absolute, CAP_T5, "+25% all + title"));
    } else {
        out.push(LOCALE.t("ru", "huya.skills_tier5_hidden").to_string());
    }

    out.join("\n")
}

pub async fn huyaskills_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    if !msg.chat.is_group() && !msg.chat.is_supergroup() {
        return Ok(());
    }
    let from = match msg.from.as_ref() { Some(f) => f, None => return Ok(()) };
    let tg_id = from.id.0 as i64;
    let name = display_name_from_user(from);

    let (h, _) = huya_db::get_or_create(&pool, msg.chat.id.0, tg_id).await?;

    let mut request = bot.send_message(msg.chat.id, skills_text(&h, &name))
        .parse_mode(teloxide::types::ParseMode::Html)
        .reply_markup(skills_keyboard(&h, 0));
    if let Some(thread) = topic_thread_id(&msg) {
        request = request.message_thread_id(thread);
    }
    request.await?;
    Ok(())
}

/// Callback: huya_skill:{skill}:{page} or huya_skill:{skill} — upgrade a skill.
pub async fn huya_skill_callback(
    bot: Bot,
    query: CallbackQuery,
    pool: PgPool,
) -> Result<(), AppError> {
    let data = query.data.as_deref().unwrap_or("");
    let rest = match data.strip_prefix("huya_skill:") {
        Some(s) => s,
        None => { let _ = bot.answer_callback_query(query.id).await; return Ok(()); }
    };
    let (skill, page) = match rest.split_once(':') {
        Some((s, p)) => (s, p.parse().unwrap_or(0)),
        None => (rest, 0u32),
    };

    let msg_ref = match query.message.as_ref() {
        Some(m) => m,
        None => { let _ = bot.answer_callback_query(query.id).await; return Ok(()); }
    };

    let clicker = query.from.id.0 as i64;
    let (h, _) = huya_db::get_or_create(&pool, msg_ref.chat().id.0, clicker).await?;

    let name = display_name_from_user(&query.from);

    match huya_db::upgrade_skill(&pool, &h, skill).await? {
        Some(updated) => {
            let skill_name_key = format!("huya.skill_name_{}", skill);
            let skill_name = LOCALE.t("ru", &skill_name_key).to_string();
            let (current_val, cap) = match skill {
                "shaft" => (updated.skill_shaft, CAP_T1),
                "skin" => (updated.skill_skin, CAP_T1),
                "balls" => (updated.skill_balls, CAP_T1),
                "cunning" => (updated.skill_cunning, CAP_T1),
                "stamina" => (updated.skill_stamina, CAP_T1),
                "pierce" => (updated.skill_pierce, CAP_T2),
                "scales" => (updated.skill_scales, CAP_T2),
                "spirit" => (updated.skill_spirit, CAP_T2),
                "pickpocket" => (updated.skill_pickpocket, CAP_T2),
                "dynamo" => (updated.skill_dynamo, CAP_T2),
                "eggtwist" => (updated.skill_eggtwist, CAP_T3),
                "bloodsucker" => (updated.skill_bloodsucker, CAP_T3),
                "ironballs" => (updated.skill_ironballs, CAP_T3),
                "vortex" => (updated.skill_vortex, CAP_T3),
                "phantom" => (updated.skill_phantom, CAP_T3),
                "berserker" => (updated.skill_berserker, CAP_T4),
                "vampire" => (updated.skill_vampire, CAP_T4),
                "fortress" => (updated.skill_fortress, CAP_T4),
                "speedrun" => (updated.skill_speedrun, CAP_T4),
                "ghost" => (updated.skill_ghost, CAP_T4),
                "eternal" => (updated.skill_eternal, CAP_T5),
                "absolute" => (updated.skill_absolute, CAP_T5),
                _ => (0, 1),
            };
            let stars = Huya::skill_stars(current_val, cap);
            let toast = LOCALE.t_fmt("ru", "huya.skill_upgraded", &[
                ("skill_name", &skill_name),
                ("stars", &stars),
            ]);
            let _ = bot.answer_callback_query(query.id).text(&*toast).await;

            // Edit message with updated skills display, stay on same page.
            let _ = bot.edit_message_text(msg_ref.chat().id, msg_ref.id(), skills_text(&updated, &name))
                .parse_mode(teloxide::types::ParseMode::Html)
                .reply_markup(skills_keyboard(&updated, page))
                .await;
        }
        None => {
            let text = if h.skill_points <= 0 {
                LOCALE.t("ru", "huya.skills_no_points").to_string()
            } else {
                LOCALE.t("ru", "huya.skill_max").to_string()
            };
            let _ = bot.answer_callback_query(query.id).text(&*text).await;
        }
    }

    Ok(())
}

/// Callback: huya_skill_page:{page} — switch skills page.
pub async fn huya_skill_page_callback(
    bot: Bot,
    query: CallbackQuery,
    pool: PgPool,
) -> Result<(), AppError> {
    let data = query.data.as_deref().unwrap_or("");
    let page_str = match data.strip_prefix("huya_skill_page:") {
        Some(s) => s,
        None => { let _ = bot.answer_callback_query(query.id).await; return Ok(()); }
    };
    let page: u32 = match page_str.parse() {
        Ok(p) => p,
        Err(_) => { let _ = bot.answer_callback_query(query.id).await; return Ok(()); }
    };

    let msg_ref = match query.message.as_ref() {
        Some(m) => m,
        None => { let _ = bot.answer_callback_query(query.id).await; return Ok(()); }
    };

    let clicker = query.from.id.0 as i64;
    let (h, _) = huya_db::get_or_create(&pool, msg_ref.chat().id.0, clicker).await?;

    let name = display_name_from_user(&query.from);

    let _ = bot.answer_callback_query(query.id).await;

    let _ = bot.edit_message_text(msg_ref.chat().id, msg_ref.id(), skills_text(&h, &name))
        .parse_mode(teloxide::types::ParseMode::Html)
        .reply_markup(skills_keyboard(&h, page))
        .await;

    Ok(())
}

// ── Shop (/huyashop) ──────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum ShopSection {
    Boosts,
    Energy,
}

fn parse_shop_section(s: &str) -> ShopSection {
    match s {
        "energy" => ShopSection::Energy,
        _ => ShopSection::Boosts,
    }
}

fn shop_text(h: &Huya, section: ShopSection, small_cost: i32, big_cost: i32) -> String {
    match section {
        ShopSection::Boosts => LOCALE.t_fmt("ru", "huya.shop_header", &[("size", &h.display_cm())]),
        ShopSection::Energy => LOCALE.t_fmt(
            "ru",
            "huya.shop_energy_header",
            &[
                ("size", &h.display_cm()),
                ("small_cost", &mm_to_cm_str(small_cost)),
                ("big_cost", &mm_to_cm_str(big_cost)),
                ("buys_today", &h.energy_buys_today.max(0).to_string()),
            ],
        ),
    }
}

fn shop_keyboard(owner_tg_id: i64, section: ShopSection) -> InlineKeyboardMarkup {
    let mut rows = vec![vec![
        InlineKeyboardButton::callback(
            if section == ShopSection::Boosts { "• Бусты •" } else { "Бусты" },
            format!("huya_shop_view:{owner_tg_id}:boosts"),
        ),
        InlineKeyboardButton::callback(
            if section == ShopSection::Energy { "• Энергия •" } else { "Энергия" },
            format!("huya_shop_view:{owner_tg_id}:energy"),
        ),
    ]];
    match section {
        ShopSection::Boosts => {
            rows.push(vec![
                InlineKeyboardButton::callback(
                    LOCALE.t("ru", "huya.buy_btn_potion").to_string(),
                    format!("huya_buy:{owner_tg_id}:potion"),
                ),
                InlineKeyboardButton::callback(
                    LOCALE.t("ru", "huya.buy_btn_adrenaline").to_string(),
                    format!("huya_buy:{owner_tg_id}:adrenaline"),
                ),
            ]);
            rows.push(vec![
                InlineKeyboardButton::callback(
                    LOCALE.t("ru", "huya.buy_btn_armor").to_string(),
                    format!("huya_buy:{owner_tg_id}:armor"),
                ),
                InlineKeyboardButton::callback(
                    LOCALE.t("ru", "huya.buy_btn_steroid").to_string(),
                    format!("huya_buy:{owner_tg_id}:steroid"),
                ),
            ]);
        }
        ShopSection::Energy => {
            rows.push(vec![
                InlineKeyboardButton::callback(
                    LOCALE.t("ru", "huya.buy_btn_energy_small").to_string(),
                    format!("huya_buy:{owner_tg_id}:energy_small"),
                ),
                InlineKeyboardButton::callback(
                    LOCALE.t("ru", "huya.buy_btn_energy_big").to_string(),
                    format!("huya_buy:{owner_tg_id}:energy_big"),
                ),
            ]);
        }
    }
    rows.push(vec![
        InlineKeyboardButton::callback(
            "🧰 Сундуки",
            format!("huya_chest_menu:{owner_tg_id}"),
        ),
    ]);
    InlineKeyboardMarkup::new(rows)
}

pub async fn huyashop_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    if !msg.chat.is_group() && !msg.chat.is_supergroup() {
        let _ = crate::alerts::notify(
            &pool,
            &format!("drop:huyashop unsupported chat_type chat_id={}", msg.chat.id.0),
        )
        .await;
        let _ = bot
            .send_message(msg.chat.id, "Магазин работает только в группах/супергруппах.")
            .await;
        return Ok(());
    }
    let from = match msg.from.as_ref() {
        Some(f) => f,
        None => {
            let _ = crate::alerts::notify(
                &pool,
                &format!("drop:huyashop no sender chat_id={}", msg.chat.id.0),
            )
            .await;
            let _ = bot
                .send_message(
                    msg.chat.id,
                    "Не удалось определить отправителя команды. Попробуй отправить от личного аккаунта.",
                )
                .await;
            return Ok(());
        }
    };
    let tg_id = from.id.0 as i64;

    let (h, _) = huya_db::get_or_create(&pool, msg.chat.id.0, tg_id).await?;

    let section = ShopSection::Boosts;
    let small_cost = huya_db::energy_price_mm(&h, "energy_small").unwrap_or(0);
    let big_cost = huya_db::energy_price_mm(&h, "energy_big").unwrap_or(0);
    let text = shop_text(&h, section, small_cost, big_cost);

    let mut request = bot.send_message(msg.chat.id, text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .reply_parameters(teloxide::types::ReplyParameters::new(msg.id))
        .reply_markup(shop_keyboard(tg_id, section));
    if let Some(thread) = topic_thread_id(&msg) {
        request = request.message_thread_id(thread);
    }
    request.await?;
    Ok(())
}

/// Callback: huya_buy:{owner_tg_id}:{item_id} — purchase an item.
pub async fn huya_buy_callback(
    bot: Bot,
    query: CallbackQuery,
    pool: PgPool,
) -> Result<(), AppError> {
    let data = query.data.as_deref().unwrap_or("");
    let (owner_tg_id, item_id_opt, section) = if let Some(rest) = data.strip_prefix("huya_shop_view:") {
        let p: Vec<&str> = rest.splitn(2, ':').collect();
        if p.len() < 2 {
            let _ = bot.answer_callback_query(query.id).await;
            return Ok(());
        }
        let owner = match p[0].parse::<i64>() {
            Ok(id) => id,
            Err(_) => { let _ = bot.answer_callback_query(query.id).await; return Ok(()); }
        };
        (owner, None, parse_shop_section(p[1]))
    } else {
        let parts: Vec<&str> = data.splitn(3, ':').collect();
        if parts.len() < 3 {
            let _ = bot.answer_callback_query(query.id).await;
            return Ok(());
        }
        let owner = match parts[1].parse::<i64>() {
            Ok(id) => id,
            Err(_) => { let _ = bot.answer_callback_query(query.id).await; return Ok(()); }
        };
        let item_id = parts[2];
        let section = if item_id.starts_with("energy_") { ShopSection::Energy } else { ShopSection::Boosts };
        (owner, Some(item_id), section)
    };

    let clicker = query.from.id.0 as i64;
    if clicker != owner_tg_id {
        let _ = bot.answer_callback_query(query.id).text("Это не твой магазин!").await;
        return Ok(());
    }

    let msg_ref = match query.message.as_ref() {
        Some(m) => m,
        None => { let _ = bot.answer_callback_query(query.id).await; return Ok(()); }
    };

    let (h, _) = huya_db::get_or_create(&pool, msg_ref.chat().id.0, clicker).await?;

    if item_id_opt.is_none() {
        let small_cost = huya_db::energy_price_mm(&h, "energy_small").unwrap_or(0);
        let big_cost = huya_db::energy_price_mm(&h, "energy_big").unwrap_or(0);
        let text = shop_text(&h, section, small_cost, big_cost);
        let _ = bot.answer_callback_query(query.id).await;
        let _ = bot.edit_message_text(msg_ref.chat().id, msg_ref.id(), text)
            .parse_mode(teloxide::types::ParseMode::Html)
            .reply_markup(shop_keyboard(owner_tg_id, section))
            .await;
        return Ok(());
    }
    let item_id = item_id_opt.unwrap_or("");

    let mut updated_after: Option<Huya> = None;
    if item_id == "energy_small" || item_id == "energy_big" {
        let expected_cost = huya_db::energy_price_mm(&h, item_id).unwrap_or(0);
        if let Some((updated, paid)) = huya_db::buy_energy_item(&pool, &h, item_id, msg_ref.chat().id.0).await? {
            updated_after = Some(updated);
            let toast_key = format!("huya.buy_success_{}", item_id);
            let toast = LOCALE.t_fmt("ru", &toast_key, &[("cost", &mm_to_cm_str(paid))]);
            let _ = bot.answer_callback_query(query.id).text(&*toast).await;
        } else {
            let toast = LOCALE.t_fmt("ru", "huya.buy_fail_broke", &[
                ("cost", &mm_to_cm_str(expected_cost)),
                ("size", &h.display_cm()),
            ]);
            let _ = bot.answer_callback_query(query.id).text(&*toast).await;
        }
    } else if let Some(cost) = huya_db::item_cost_mm(item_id) {
        if h.length_mm < cost {
            let toast = LOCALE.t_fmt("ru", "huya.buy_fail_broke", &[
                ("cost", &mm_to_cm_str(cost)),
                ("size", &h.display_cm()),
            ]);
            let _ = bot.answer_callback_query(query.id).text(&*toast).await;
        } else if let Some(updated) = huya_db::buy_item(&pool, &h, item_id).await? {
            updated_after = Some(updated.clone());
            let toast_key = format!("huya.buy_success_{}", item_id);
            let toast = LOCALE.t_fmt("ru", &toast_key, &[
                ("hp",     &updated.hp.to_string()),
                ("max_hp", &updated.max_hp().to_string()),
            ]);
            let _ = bot.answer_callback_query(query.id).text(&*toast).await;
        } else {
            let toast = LOCALE.t_fmt("ru", "huya.buy_fail_broke", &[
                ("cost", &mm_to_cm_str(cost)),
                ("size", &h.display_cm()),
            ]);
            let _ = bot.answer_callback_query(query.id).text(&*toast).await;
        }
    } else {
        let _ = bot.answer_callback_query(query.id)
            .text(LOCALE.t("ru", "huya.buy_fail_unknown"))
            .await;
    }

    let refreshed = if let Some(u) = updated_after { u } else { huya_db::get_or_create(&pool, msg_ref.chat().id.0, clicker).await?.0 };
    let small_cost = huya_db::energy_price_mm(&refreshed, "energy_small").unwrap_or(0);
    let big_cost = huya_db::energy_price_mm(&refreshed, "energy_big").unwrap_or(0);
    let new_text = shop_text(&refreshed, section, small_cost, big_cost);
    let _ = bot.edit_message_text(msg_ref.chat().id, msg_ref.id(), new_text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .reply_markup(shop_keyboard(owner_tg_id, section))
        .await;

    Ok(())
}

// ── Chests (/huyachest) ───────────────────────────────────────────────────────

fn chest_keyboard(owner_tg_id: i64) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback("🪵 Дешман-ящик", "huya_chest_buy:cheap_crate"),
            InlineKeyboardButton::callback("🥊 Бойцовский", "huya_chest_buy:fighter_crate"),
        ],
        vec![
            InlineKeyboardButton::callback("👑 Царский", "huya_chest_buy:royal_crate"),
            InlineKeyboardButton::callback("🎁 Daily", "huya_chest_daily"),
        ],
        vec![
            InlineKeyboardButton::callback("◀ Магазин", format!("huya_shop_menu:{owner_tg_id}")),
        ],
    ])
}

fn chest_open_keyboard(chest_id: &str) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
        "🔓 Открыть",
        format!("huya_chest_open:{chest_id}"),
    )]])
}

pub async fn huyachest_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    if !msg.chat.is_group() && !msg.chat.is_supergroup() {
        return Ok(());
    }
    let from = match msg.from.as_ref() { Some(f) => f, None => return Ok(()) };
    let tg_id = from.id.0 as i64;
    let (h, _) = huya_db::get_or_create(&pool, msg.chat.id.0, tg_id).await?;
    let text = LOCALE.t_fmt(
        "ru",
        "huya.chest_menu",
        &[("size", &h.display_cm())],
    );
    let mut req = bot
        .send_message(msg.chat.id, text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .reply_markup(chest_keyboard(tg_id));
    if let Some(thread) = topic_thread_id(&msg) {
        req = req.message_thread_id(thread);
    }
    req.await?;
    // Keep chat clean: remove the command message after opening menu.
    let _ = bot.delete_message(msg.chat.id, msg.id).await;
    Ok(())
}

pub async fn huya_chest_callback(
    bot: Bot,
    query: CallbackQuery,
    pool: PgPool,
) -> Result<(), AppError> {
    let data = query.data.as_deref().unwrap_or("");
    let qid = query.id.clone();
    let clicker = query.from.id.0 as i64;
    let msg_ref = match query.message.as_ref() {
        Some(m) => m,
        None => return Ok(()),
    };
    let chat_id_raw = msg_ref.chat().id.0;
    let (h, _) = huya_db::get_or_create(&pool, chat_id_raw, clicker).await?;

    if let Some(owner) = data.strip_prefix("huya_chest_menu:") {
        let owner_tg_id = owner.parse::<i64>().unwrap_or_default();
        if owner_tg_id != 0 && owner_tg_id != clicker {
            let _ = bot.answer_callback_query(qid.clone()).text("Это не твой магазин").await;
            return Ok(());
        }
        let text = LOCALE.t_fmt("ru", "huya.chest_menu", &[("size", &h.display_cm())]);
        let _ = bot.answer_callback_query(qid.clone()).await;
        let _ = bot.edit_message_text(msg_ref.chat().id, msg_ref.id(), text)
            .parse_mode(teloxide::types::ParseMode::Html)
            .reply_markup(chest_keyboard(clicker))
            .await;
        return Ok(());
    }

    if let Some(owner) = data.strip_prefix("huya_shop_menu:") {
        let owner_tg_id = owner.parse::<i64>().unwrap_or_default();
        if owner_tg_id != 0 && owner_tg_id != clicker {
            let _ = bot.answer_callback_query(qid.clone()).text("Это не твой магазин").await;
            return Ok(());
        }
        let section = ShopSection::Boosts;
        let small_cost = huya_db::energy_price_mm(&h, "energy_small").unwrap_or(0);
        let big_cost = huya_db::energy_price_mm(&h, "energy_big").unwrap_or(0);
        let text = shop_text(&h, section, small_cost, big_cost);
        let _ = bot.answer_callback_query(qid.clone()).await;
        let _ = bot.edit_message_text(msg_ref.chat().id, msg_ref.id(), text)
            .parse_mode(teloxide::types::ParseMode::Html)
            .reply_markup(shop_keyboard(clicker, section))
            .await;
        return Ok(());
    }

    if data == "huya_chest_daily" {
        let (ok, remain) = huya_db::claim_daily_chest(&pool, chat_id_raw, clicker, "daily_free_crate").await?;
        if !ok {
            let sec = remain.unwrap_or(0);
            let hh = sec / 3600;
            let mm = (sec % 3600) / 60;
            let _ = bot.answer_callback_query(qid.clone())
                .text(LOCALE.t_fmt("ru", "huya.chest_daily_cooldown", &[("remaining", &format!("{hh}ч {mm}м"))]))
                .await;
            return Ok(());
        }
        let loot = huya_db::open_chest(&pool, chat_id_raw, clicker, "daily_free_crate", true).await?;
        if let Some(item) = loot {
            let _ = bot.answer_callback_query(qid.clone())
                .text(LOCALE.t("ru", "huya.chest_daily_claimed"))
                .await;
            let item_name = item_label_ru(&item.item_id);
            let rarity_name = rarity_label_ru(&item.rarity);
            let trait_name = trait_label_ru(item.trait_name.as_deref());
            let text = LOCALE.t_rand_fmt(
                "ru",
                "huya.chest_open_lines",
                &[
                    ("item", &item_name),
                    ("rarity", &rarity_name),
                    ("trait", &trait_name),
                ],
            );
            let _ = bot.edit_message_text(msg_ref.chat().id, msg_ref.id(), text)
                .parse_mode(teloxide::types::ParseMode::Html)
                .reply_markup(chest_keyboard(clicker))
                .await;
        }
        return Ok(());
    }

    if let Some(chest_id) = data.strip_prefix("huya_chest_buy:") {
        let def = huya_db::chest_defs().into_iter().find(|c| c.id == chest_id);
        let Some(def) = def else {
            let _ = bot.answer_callback_query(qid.clone()).await;
            return Ok(());
        };
        if h.length_mm < def.price_mm {
            let _ = bot.answer_callback_query(qid.clone()).text(
                LOCALE.t_fmt("ru", "huya.buy_fail_broke", &[
                    ("cost", &mm_to_cm_str(def.price_mm)),
                    ("size", &h.display_cm()),
                ]),
            ).await;
            return Ok(());
        }
        let _ = bot.answer_callback_query(qid.clone()).text("Куплено, крути рулетку").await;
        let chest_name = chest_label_ru(chest_id);
        let _ = bot.edit_message_text(msg_ref.chat().id, msg_ref.id(),
                LOCALE.t_fmt("ru", "huya.chest_bought", &[("chest", &chest_name)]))
            .parse_mode(teloxide::types::ParseMode::Html)
            .reply_markup(chest_open_keyboard(chest_id))
            .await;
        return Ok(());
    }

    if let Some(chest_id) = data.strip_prefix("huya_chest_open:") {
        let loot = huya_db::open_chest(&pool, chat_id_raw, clicker, chest_id, false).await?;
        match loot {
            Some(item) => {
                let _ = bot.answer_callback_query(qid.clone()).text("Крутится...").await;
                // Roulette-like animation: fast item scroll, then stop on final drop.
                let pool_items: Vec<String> = huya_db::item_templates()
                    .into_iter()
                    .filter(|t| {
                        match chest_id {
                            "cheap_crate" => matches!(t.rarity, "trash" | "common" | "rare"),
                            "fighter_crate" => matches!(t.rarity, "common" | "rare" | "epic" | "legendary"),
                            "royal_crate" => matches!(t.rarity, "rare" | "epic" | "legendary"),
                            _ => true,
                        }
                    })
                    .map(|t| t.id.to_string())
                    .collect();
                let pool_items = if pool_items.is_empty() {
                    vec![item.item_id.clone()]
                } else {
                    pool_items
                };
                let frames = 9_i32;
                for i in 0..frames {
                    let left = {
                        let mut rng = rand::rng();
                        pool_items.get(rng.random_range(0..pool_items.len())).cloned().unwrap_or_else(|| "???".to_string())
                    };
                    let center = if i == frames - 1 {
                        item.item_id.clone()
                    } else {
                        let mut rng = rand::rng();
                        pool_items.get(rng.random_range(0..pool_items.len())).cloned().unwrap_or_else(|| "???".to_string())
                    };
                    let right = {
                        let mut rng = rand::rng();
                        pool_items.get(rng.random_range(0..pool_items.len())).cloned().unwrap_or_else(|| "???".to_string())
                    };
                    let left_mark = rarity_emoji(&item_rarity(&left));
                    let center_mark = rarity_emoji(&item_rarity(&center));
                    let right_mark = rarity_emoji(&item_rarity(&right));
                    let frame_text = LOCALE.t_fmt(
                        "ru",
                        "huya.chest_spin_frame",
                        &[
                            ("left", &escape_html(&format!("{left_mark} {}", item_label_ru(&left)))),
                            ("center", &escape_html(&format!("{center_mark} {}", item_label_ru(&center)))),
                            ("right", &escape_html(&format!("{right_mark} {}", item_label_ru(&right)))),
                        ],
                    );
                    let _ = bot.edit_message_text(msg_ref.chat().id, msg_ref.id(), frame_text)
                        .parse_mode(teloxide::types::ParseMode::Html)
                        .reply_markup(InlineKeyboardMarkup::new::<Vec<Vec<InlineKeyboardButton>>>(vec![]))
                        .await;
                    tokio::time::sleep(Duration::from_millis(180 + (i as u64) * 70)).await;
                }

                let item_name = item_label_ru(&item.item_id);
                let rarity_name = format!("{} {}", rarity_emoji(&item.rarity), rarity_label_ru(&item.rarity));
                let trait_name = trait_label_ru(item.trait_name.as_deref());
                let toast = LOCALE.t_fmt("ru", "huya.chest_open_toast", &[("item", &item_name)]);
                let _ = bot.answer_callback_query(qid.clone()).text(toast).await;
                let line = LOCALE.t_rand_fmt(
                    "ru",
                    "huya.chest_open_lines",
                    &[
                        ("item", &item_name),
                    ("rarity", &rarity_name),
                        ("trait", &trait_name),
                    ],
                );
                let edit_res = bot.edit_message_text(msg_ref.chat().id, msg_ref.id(), line.clone())
                    .parse_mode(teloxide::types::ParseMode::Html)
                    .reply_markup(chest_keyboard(clicker))
                    .await;
                if edit_res.is_err() {
                    // Always send final loot line even if animation message update fails.
                    let _ = bot.send_message(msg_ref.chat().id, line)
                        .parse_mode(teloxide::types::ParseMode::Html)
                        .await;
                }
            }
            None => {
                let _ = bot.answer_callback_query(qid).text("Не удалось открыть сундук").await;
            }
        }
        return Ok(());
    }

    Ok(())
}

// ── Inventory (/huyainv) ──────────────────────────────────────────────────────

// Inventory UI v2 state machine (<= 9 buttons per screen).
#[derive(Debug, Clone, PartialEq, Eq)]
enum InventoryCategory {
    Equipment,
    Boosters,
    Gems,
    Other,
}

impl InventoryCategory {
    fn code(&self) -> &'static str {
        match self {
            InventoryCategory::Equipment => "equip",
            InventoryCategory::Boosters => "booster",
            InventoryCategory::Gems => "gem",
            InventoryCategory::Other => "other",
        }
    }

    fn from_code(s: &str) -> Self {
        match s {
            "equip" => InventoryCategory::Equipment,
            "booster" => InventoryCategory::Boosters,
            "gem" => InventoryCategory::Gems,
            _ => InventoryCategory::Other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum InventoryEquipPart {
    TipHead,
    Base,
    Balls,
    Rings,
    Piercing,
    All,
}

impl InventoryEquipPart {
    fn code(&self) -> &'static str {
        match self {
            InventoryEquipPart::TipHead => "tip",
            InventoryEquipPart::Base => "base",
            InventoryEquipPart::Balls => "balls",
            InventoryEquipPart::Rings => "rings",
            InventoryEquipPart::Piercing => "piercing",
            InventoryEquipPart::All => "all",
        }
    }

    fn from_code(s: &str) -> Self {
        match s {
            "tip" => InventoryEquipPart::TipHead,
            "base" => InventoryEquipPart::Base,
            "balls" => InventoryEquipPart::Balls,
            "rings" => InventoryEquipPart::Rings,
            "piercing" => InventoryEquipPart::Piercing,
            _ => InventoryEquipPart::All,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum InvScreen {
    Overview,
    EquipPart(InventoryEquipPart),
    ItemsMenu,
    ItemsList { category: InventoryCategory, page: usize },
    ItemDetail { item_id: i32, return_state: String },
}

fn decode_screen_from_state(state: &str) -> Option<InvScreen> {
    if state == "o" {
        return Some(InvScreen::Overview);
    }
    if state == "m" {
        return Some(InvScreen::ItemsMenu);
    }
    if let Some(rest) = state.strip_prefix("e_") {
        return Some(InvScreen::EquipPart(InventoryEquipPart::from_code(rest)));
    }
    if let Some(rest) = state.strip_prefix("l_") {
        let mut p = rest.splitn(2, '_');
        let cat = p.next().unwrap_or("other");
        let page = p.next().and_then(|x| x.parse::<usize>().ok()).unwrap_or(0);
        return Some(InvScreen::ItemsList {
            category: InventoryCategory::from_code(cat),
            page,
        });
    }
    None
}

fn parse_screen_from_callback_data(data: &str) -> Option<InvScreen> {
    let rest = data.strip_prefix("huya_inv_s:")?;
    // Item detail: huya_inv_s:d:<item_id>:<return_state>
    if let Some(s) = rest.strip_prefix("d:") {
        let mut p = s.splitn(2, ':');
        let item_id = p.next()?.parse::<i32>().ok()?;
        let return_state = p.next()?.to_string();
        return Some(InvScreen::ItemDetail { item_id, return_state });
    }

    if rest == "o" {
        return Some(InvScreen::Overview);
    }
    if rest == "m" {
        return Some(InvScreen::ItemsMenu);
    }
    if let Some(part) = rest.strip_prefix("e_") {
        return Some(InvScreen::EquipPart(InventoryEquipPart::from_code(part)));
    }
    if let Some(list) = rest.strip_prefix("l_") {
        let mut p = list.splitn(2, '_');
        let cat = p.next().unwrap_or("other");
        let page = p.next().and_then(|x| x.parse::<usize>().ok()).unwrap_or(0);
        return Some(InvScreen::ItemsList {
            category: InventoryCategory::from_code(cat),
            page,
        });
    }
    None
}

fn encode_return_state_for_item_detail(return_screen: &InvScreen) -> String {
    match return_screen {
        InvScreen::Overview => "o".to_string(),
        InvScreen::ItemsMenu => "m".to_string(),
        InvScreen::EquipPart(part) => format!("e_{}", part.code()),
        InvScreen::ItemsList { category, page } => format!("l_{}_{}", category.code(), page),
        InvScreen::ItemDetail { return_state, .. } => return_state.clone(),
    }
}

fn equip_slots_for_part(part: &InventoryEquipPart) -> Vec<&'static str> {
    match part {
        InventoryEquipPart::TipHead => vec!["tip", "piercing_tip_1", "piercing_tip_2", "piercing_tip_3"],
        InventoryEquipPart::Base => vec!["base", "piercing_base_1", "piercing_base_2"],
        InventoryEquipPart::Balls => vec!["balls"],
        InventoryEquipPart::Rings => vec!["ring_1", "ring_2", "ring_3", "ring_4", "ring_5", "ring_6"],
        InventoryEquipPart::Piercing => vec![
            "piercing_tip_1",
            "piercing_tip_2",
            "piercing_tip_3",
            "piercing_shaft_1",
            "piercing_shaft_2",
            "piercing_shaft_3",
            "piercing_base_1",
            "piercing_base_2",
        ],
        InventoryEquipPart::All => vec![
            "tip",
            "base",
            "balls",
            "ring_1",
            "ring_2",
            "ring_3",
            "ring_4",
            "ring_5",
            "ring_6",
            "piercing_tip_1",
            "piercing_tip_2",
            "piercing_tip_3",
            "piercing_shaft_1",
            "piercing_shaft_2",
            "piercing_shaft_3",
            "piercing_base_1",
            "piercing_base_2",
        ],
    }
}

fn slot_label(slot: &str) -> String {
    match slot {
        "tip" => "головка".to_string(),
        "base" => "основание".to_string(),
        "balls" => "яйца".to_string(),
        s if s.starts_with("ring_") => format!("кольцо {}", s.trim_start_matches("ring_")),
        s if s.starts_with("piercing_tip_") => {
            format!("пирсинг головки {}", s.trim_start_matches("piercing_tip_"))
        }
        s if s.starts_with("piercing_shaft_") => {
            format!("пирсинг ствола {}", s.trim_start_matches("piercing_shaft_"))
        }
        s if s.starts_with("piercing_base_") => {
            format!("пирсинг основания {}", s.trim_start_matches("piercing_base_"))
        }
        _ => slot.to_string(),
    }
}

fn equipped_item_labels_by_slot(
    equ: &[crate::db::models::HuyaEquipmentSlot],
    inv: &[crate::db::models::HuyaInventoryItem],
) -> std::collections::HashMap<String, String> {
    let mut by_slot: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for e in equ {
        if let Some(item) = inv.iter().find(|i| i.id == e.inventory_id) {
            by_slot.insert(e.slot.clone(), item_label_ru(&item.item_id));
        }
    }
    by_slot
}

fn paginate_items_with_boundaries(total_len: usize, boundary: usize, middle: usize) -> Vec<(usize, usize)> {
    if total_len == 0 {
        return vec![];
    }
    if total_len <= boundary {
        return vec![(0, total_len)];
    }

    let mut pages: Vec<(usize, usize)> = Vec::new();
    let mut start = 0usize;

    // First boundary page.
    let mut end = (start + boundary).min(total_len);
    pages.push((start, end));
    start = end;

    // Middle pages while remaining > boundary.
    while total_len.saturating_sub(start) > boundary {
        end = (start + middle).min(total_len);
        pages.push((start, end));
        start = end;
    }

    // Last boundary page.
    if start < total_len {
        pages.push((start, total_len));
    }
    pages
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InventoryView {
    Overview,
    Equip,
    Items,
}

fn parse_inventory_view(s: &str) -> InventoryView {
    match s {
        "equip" => InventoryView::Equip,
        "items" => InventoryView::Items,
        _ => InventoryView::Overview,
    }
}

fn inventory_view_key(view: InventoryView) -> &'static str {
    match view {
        InventoryView::Overview => "overview",
        InventoryView::Equip => "equip",
        InventoryView::Items => "items",
    }
}

fn view_items<'a>(items: &'a [crate::db::models::HuyaInventoryItem], view: InventoryView) -> Vec<&'a crate::db::models::HuyaInventoryItem> {
    match view {
        InventoryView::Overview => items.iter().collect(),
        InventoryView::Equip => items.iter().filter(|x| x.item_kind == "equipment").collect(),
        InventoryView::Items => items.iter().filter(|x| x.item_kind != "equipment").collect(),
    }
}

fn render_slot_list(
    h: &Huya,
    equ: &[crate::db::models::HuyaEquipmentSlot],
    inv: &[crate::db::models::HuyaInventoryItem],
) -> String {
    let mut by_slot: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for e in equ {
        if let Some(item) = inv.iter().find(|i| i.id == e.inventory_id) {
            by_slot.insert(e.slot.clone(), item_label_ru(&item.item_id));
        }
    }
    let pick = |slot: &str| -> String {
        if !huya_db::slot_unlocked_for_length(slot, h.length_mm) {
            return "🔒".to_string();
        }
        by_slot.get(slot).cloned().unwrap_or_else(|| "—".to_string())
    };
    // Компактная форма вместо body map: меньше визуального “шума”.
    format!(
        "🗒 <b>Слоты тела</b>\n\
         гол.: {} | осн.: {} | яйца: {}\n\
         кольца: 1:{} 2:{} 3:{} 4:{} 5:{} 6:{}\n\
         пирсинг головки: 1:{} 2:{} 3:{}\n\
         пирсинг ствола: 1:{} 2:{} 3:{}\n\
         пирсинг основания: 1:{} 2:{}",
        pick("tip"),
        pick("base"),
        pick("balls"),
        pick("ring_1"),
        pick("ring_2"),
        pick("ring_3"),
        pick("ring_4"),
        pick("ring_5"),
        pick("ring_6"),
        pick("piercing_tip_1"),
        pick("piercing_tip_2"),
        pick("piercing_tip_3"),
        pick("piercing_shaft_1"),
        pick("piercing_shaft_2"),
        pick("piercing_shaft_3"),
        pick("piercing_base_1"),
        pick("piercing_base_2"),
    )
}

fn inventory_keyboard(
    items: &[crate::db::models::HuyaInventoryItem],
    view: InventoryView,
    page: usize,
) -> InlineKeyboardMarkup {
    let per_page = 6;
    let filtered = view_items(items, view);
    let start = page.saturating_mul(per_page);
    let end = (start + per_page).min(filtered.len());
    let slice = if start >= filtered.len() { &filtered[0..0] } else { &filtered[start..end] };
    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();
    rows.push(vec![
        InlineKeyboardButton::callback(
            if view == InventoryView::Overview { "• Обзор •" } else { "Обзор" },
            format!("huya_inv_view:{}:{}", inventory_view_key(InventoryView::Overview), 0),
        ),
        InlineKeyboardButton::callback(
            if view == InventoryView::Equip { "• Экип •" } else { "Экип" },
            format!("huya_inv_view:{}:{}", inventory_view_key(InventoryView::Equip), 0),
        ),
        InlineKeyboardButton::callback(
            if view == InventoryView::Items { "• Предметы •" } else { "Предметы" },
            format!("huya_inv_view:{}:{}", inventory_view_key(InventoryView::Items), 0),
        ),
    ]);
    for it in slice {
        if it.item_kind == "equipment" {
            rows.push(vec![
                InlineKeyboardButton::callback(
                    format!("🔍 #{}", it.id),
                    format!("huya_inv_detail:{}:{}:{}", it.id, inventory_view_key(view), page),
                ),
                InlineKeyboardButton::callback("💰 Продать", format!("huya_inv_sell:{}", it.id)),
            ]);
        } else if it.item_kind == "booster" {
            rows.push(vec![
                InlineKeyboardButton::callback(
                    format!("🔍 #{}", it.id),
                    format!("huya_inv_detail:{}:{}:{}", it.id, inventory_view_key(view), page),
                ),
                InlineKeyboardButton::callback("💰 Продать", format!("huya_inv_sell:{}", it.id)),
            ]);
        } else {
            rows.push(vec![
                InlineKeyboardButton::callback(
                    format!("🔍 #{}", it.id),
                    format!("huya_inv_detail:{}:{}:{}", it.id, inventory_view_key(view), page),
                ),
                InlineKeyboardButton::callback("💰 Продать", format!("huya_inv_sell:{}", it.id)),
            ]);
        }
    }
    rows.push(vec![
        InlineKeyboardButton::callback("◀", format!("huya_inv_page:{}:{}", inventory_view_key(view), page.saturating_sub(1))),
        InlineKeyboardButton::callback("▶", format!("huya_inv_page:{}:{}", inventory_view_key(view), page + 1)),
    ]);
    InlineKeyboardMarkup::new(rows)
}

fn inventory_detail_keyboard(item_id: i32, back_view: &str, back_page: usize) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback(
            LOCALE.t("ru", "huya.inventory.details_back_btn"),
            format!("huya_inv_page:{}:{}", back_view, back_page),
        )],
        vec![
            InlineKeyboardButton::callback(
                LOCALE.t("ru", "huya.inventory.details_socket_btn"),
                format!("huya_inv_socket_pick:{}:{}:{}", item_id, back_view, back_page),
            ),
            InlineKeyboardButton::callback(
                LOCALE.t("ru", "huya.inventory.details_reforge_btn"),
                format!("huya_inv_reforge_pick:{}:{}:{}", item_id, back_view, back_page),
            ),
        ],
        vec![InlineKeyboardButton::callback(
            LOCALE.t("ru", "huya.inventory.details_sell_btn"),
            format!("huya_inv_sell:{}", item_id),
        )],
    ])
}

fn render_inventory_items_slice_lines(
    items: &[crate::db::models::HuyaInventoryItem],
    gems: &[crate::db::models::HuyaSocketedGem],
    view: InventoryView,
    page: usize,
) -> Vec<String> {
    let per_page = 6;
    let filtered = view_items(items, view);
    let start = page.saturating_mul(per_page);
    let end = (start + per_page).min(filtered.len());
    let slice = if start >= filtered.len() { &filtered[0..0] } else { &filtered[start..end] };

    let mut lines = Vec::new();
    for it in slice {
        let trait_line = trait_label_ru(it.trait_name.as_deref());
        let used_sockets = gems.iter().filter(|g| g.item_inventory_id == it.id).count();
        let slot_line = it.slot.clone().unwrap_or_else(|| "-".to_string());
        let rarity_line = rarity_label_ru(&it.rarity);
        let rarity_badge = format!("{} {}", rarity_emoji(&it.rarity), rarity_line);
        let item_name = item_label_ru(&it.item_id);
        let kind_line = kind_label_ru(&it.item_kind);

        lines.push(format!(
            "#{} <b>{}</b> [{} | {}] {} | трейт:{} | ролл:{} | сокеты:{}/{} | перековка:+{} | цена:{}см",
            it.id,
            escape_html(&item_name),
            rarity_badge,
            kind_line,
            escape_html(&slot_line),
            escape_html(&trait_line),
            it.roll,
            used_sockets,
            it.socket_capacity.max(0),
            it.reforge_level.max(0),
            mm_to_cm_str(it.sell_price_mm)
        ));

        let sockets: Vec<&crate::db::models::HuyaSocketedGem> =
            gems.iter().filter(|g| g.item_inventory_id == it.id).collect();
        if !sockets.is_empty() {
            let gem_line = sockets
                .into_iter()
                .map(|g| {
                    let tr = trait_label_ru(g.gem_trait.as_deref());
                    let gem_name = item_label_ru(&g.gem_item_id);
                    format!("#{}:{}({})+{}", g.socket_index, gem_name, tr, g.gem_roll.max(0))
                })
                .collect::<Vec<String>>()
                .join(" | ");
            lines.push(format!("  ↳ гемы: {}", escape_html(&gem_line)));
        }
    }
    lines
}

fn render_inventory_overview_text(
    h: &Huya,
    equ: &[crate::db::models::HuyaEquipmentSlot],
    items: &[crate::db::models::HuyaInventoryItem],
    gems: &[crate::db::models::HuyaSocketedGem],
    page: usize,
) -> String {
    if items.is_empty() {
        return LOCALE.t("ru", "huya.inventory.empty").to_string();
    }
    let mut lines = vec![LOCALE.t("ru", "huya.inventory.header").to_string()];
    lines.push(render_slot_list(h, equ, items));
    lines.push("📂 Раздел: Обзор".to_string());
    lines.extend(render_inventory_items_slice_lines(items, gems, InventoryView::Overview, page));
    lines.join("\n")
}

fn render_inventory_equip_text(
    h: &Huya,
    equ: &[crate::db::models::HuyaEquipmentSlot],
    items: &[crate::db::models::HuyaInventoryItem],
    gems: &[crate::db::models::HuyaSocketedGem],
    page: usize,
) -> String {
    if items.is_empty() {
        return LOCALE.t("ru", "huya.inventory.empty").to_string();
    }
    let mut lines = vec![LOCALE.t("ru", "huya.inventory.header").to_string()];
    // В “Экип” даём короткую подсказку по слотам тела.
    lines.push(render_slot_list(h, equ, items));
    lines.push("📂 Раздел: Экип".to_string());
    lines.extend(render_inventory_items_slice_lines(items, gems, InventoryView::Equip, page));
    lines.join("\n")
}

fn render_inventory_items_text(
    _h: &Huya,
    _equ: &[crate::db::models::HuyaEquipmentSlot],
    items: &[crate::db::models::HuyaInventoryItem],
    gems: &[crate::db::models::HuyaSocketedGem],
    page: usize,
) -> String {
    if items.is_empty() {
        return LOCALE.t("ru", "huya.inventory.empty").to_string();
    }
    let mut lines = vec![LOCALE.t("ru", "huya.inventory.header").to_string()];
    lines.push("📂 Раздел: Предметы".to_string());
    lines.extend(render_inventory_items_slice_lines(items, gems, InventoryView::Items, page));
    lines.join("\n")
}

fn render_inventory_text(
    h: &Huya,
    equ: &[crate::db::models::HuyaEquipmentSlot],
    items: &[crate::db::models::HuyaInventoryItem],
    gems: &[crate::db::models::HuyaSocketedGem],
    view: InventoryView,
    page: usize,
) -> String {
    match view {
        InventoryView::Overview => render_inventory_overview_text(h, equ, items, gems, page),
        InventoryView::Equip => render_inventory_equip_text(h, equ, items, gems, page),
        InventoryView::Items => render_inventory_items_text(h, equ, items, gems, page),
    }
}

fn render_item_detail(
    h: &Huya,
    _equ: &[crate::db::models::HuyaEquipmentSlot],
    items: &[crate::db::models::HuyaInventoryItem],
    gems: &[crate::db::models::HuyaSocketedGem],
    item_id: i32,
) -> String {
    let Some(it) = items.iter().find(|x| x.id == item_id) else {
        return "Предмет не найден.".to_string();
    };

    let trait_line = trait_label_ru(it.trait_name.as_deref());
    let rarity_line = rarity_label_ru(&it.rarity);
    let rarity_badge = format!("{} {}", rarity_emoji(&it.rarity), rarity_line);
    let item_name = item_label_ru(&it.item_id);
    let kind_line = kind_label_ru(&it.item_kind);
    let slot_line = it.slot.clone().unwrap_or_else(|| "-".to_string());

    let used_sockets = gems.iter().filter(|g| g.item_inventory_id == it.id).count();
    let slot_unlocked = it
        .slot
        .as_deref()
        .map(|s| huya_db::slot_unlocked_for_length(s, h.length_mm))
        .unwrap_or(true);
    let unlocked_badge = if slot_unlocked {
        ""
    } else {
        LOCALE.t("ru", "huya.inventory.details_locked_suffix")
    };

    let sockets: Vec<&crate::db::models::HuyaSocketedGem> =
        gems.iter().filter(|g| g.item_inventory_id == it.id).collect();
    let socket_block = if sockets.is_empty() {
        LOCALE.t("ru", "huya.inventory.details_gems_none").to_string()
    } else {
        let gem_line = sockets
            .into_iter()
            .map(|g| {
                let tr = trait_label_ru(g.gem_trait.as_deref());
                let gem_name = item_label_ru(&g.gem_item_id);
                format!("#{}:{}({})+{}", g.socket_index, gem_name, tr, g.gem_roll.max(0))
            })
            .collect::<Vec<String>>()
            .join(" | ");
        format!(
            "{} {}",
            LOCALE.t("ru", "huya.inventory.details_gems_prefix"),
            escape_html(&gem_line)
        )
    };

    format!(
        "{}\n\
         {}\n\
         #{} <b>{}</b>\n\
         {} | тип: {} | трейт: {}{}\n\
         слот: {} | ролл: {} | перековка:+{} | сокеты: {}/{}\n\
         цена: {}см\n\
         {}\n",
        LOCALE.t("ru", "huya.inventory.header"),
        LOCALE.t("ru", "huya.inventory.details_title"),
        it.id,
        escape_html(&item_name),
        rarity_badge,
        kind_line,
        escape_html(&trait_line),
        unlocked_badge,
        escape_html(&slot_line),
        it.roll,
        it.reforge_level.max(0),
        used_sockets,
        it.socket_capacity.max(0),
        mm_to_cm_str(it.sell_price_mm),
        socket_block
    )
}

fn v2_items_by_category<'a>(items: &'a [crate::db::models::HuyaInventoryItem], category: &InventoryCategory) -> Vec<&'a crate::db::models::HuyaInventoryItem> {
    let kind = match category {
        InventoryCategory::Equipment => "equipment",
        InventoryCategory::Boosters => "booster",
        InventoryCategory::Gems => "gem",
        InventoryCategory::Other => "other",
    };
    match kind {
        "equipment" => items.iter().filter(|x| x.item_kind == "equipment").collect(),
        "booster" => items.iter().filter(|x| x.item_kind == "booster").collect(),
        "gem" => items.iter().filter(|x| x.item_kind == "gem").collect(),
        _ => items
            .iter()
            .filter(|x| x.item_kind != "equipment" && x.item_kind != "booster" && x.item_kind != "gem")
            .collect(),
    }
}

fn v2_items_pagination(total_len: usize) -> Vec<(usize, usize)> {
    // boundary<=6 items, middle==7 items.
    paginate_items_with_boundaries(total_len, 6, 7)
}

fn v2_overview_summary_text(
    h: &Huya,
    equ: &[crate::db::models::HuyaEquipmentSlot],
    items: &[crate::db::models::HuyaInventoryItem],
) -> String {
    let mut equipment = 0usize;
    let mut boosters = 0usize;
    let mut gems = 0usize;
    let mut other = 0usize;
    for it in items {
        match it.item_kind.as_str() {
            "equipment" => equipment += 1,
            "booster" => boosters += 1,
            "gem" => gems += 1,
            _ => other += 1,
        }
    }

    // Слоты показываем компактно (без “стены” предметов).
    let slots = render_slot_list(h, equ, items);
    format!(
        "{}\n\nЭкип: {} | Бустеры: {} | Гемы: {} | Прочее: {}\n\n{}",
        LOCALE.t("ru", "huya.inventory.header"),
        equipment,
        boosters,
        gems,
        other,
        slots
    )
}

fn v2_equip_part_text(
    h: &Huya,
    equ: &[crate::db::models::HuyaEquipmentSlot],
    items: &[crate::db::models::HuyaInventoryItem],
    part: &InventoryEquipPart,
) -> String {
    let by_slot = equipped_item_labels_by_slot(equ, items);
    let slots = equip_slots_for_part(part);

    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("📂 Слоты: {}", match part {
        InventoryEquipPart::TipHead => "головка",
        InventoryEquipPart::Base => "основание",
        InventoryEquipPart::Balls => "яйца",
        InventoryEquipPart::Rings => "кольца",
        InventoryEquipPart::Piercing => "пирсинг",
        InventoryEquipPart::All => "всё тело",
    }));
    for s in slots {
        let unlocked = huya_db::slot_unlocked_for_length(s, h.length_mm);
        if !unlocked {
            lines.push(format!("{}: 🔒", slot_label(s)));
            continue;
        }
        let v = by_slot.get(s).cloned().unwrap_or_else(|| "—".to_string());
        lines.push(format!("{}: {}", slot_label(s), v));
    }

    lines.join("\n")
}

fn v2_items_menu_text(items: &[crate::db::models::HuyaInventoryItem]) -> String {
    let mut equipment = 0usize;
    let mut boosters = 0usize;
    let mut gems = 0usize;
    let mut other = 0usize;
    for it in items {
        match it.item_kind.as_str() {
            "equipment" => equipment += 1,
            "booster" => boosters += 1,
            "gem" => gems += 1,
            _ => other += 1,
        }
    }
    format!(
        "🏪 <b>Предметы</b>\nЭкип: {} | Бустеры: {} | Гемы: {} | Прочее: {}\nВыбери категорию.",
        equipment, boosters, gems, other
    )
}

fn v2_items_list_text(
    items: &[crate::db::models::HuyaInventoryItem],
    category: &InventoryCategory,
    page: usize,
    max_page: usize,
) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!(
        "🎒 <b>Список</b> — {} | Стр. {} / {}",
        category.code(),
        page + 1,
        max_page + 1
    ));

    let filtered = v2_items_by_category(items, category);
    let pages = v2_items_pagination(filtered.len());
    let (start, end) = pages.get(page).copied().unwrap_or((0, 0));
    for it in filtered.get(start..end).unwrap_or(&[]) {
        let name = item_label_ru(&it.item_id);
        let rarity = rarity_label_ru(&it.rarity);
        let badge = format!("{} {}", rarity_emoji(&it.rarity), rarity);
        let trait_line = trait_label_ru(it.trait_name.as_deref());
        lines.push(format!(
            "#{} <b>{}</b> {} | {}",
            it.id,
            escape_html(&name),
            badge,
            escape_html(&trait_line)
        ));
    }

    lines.join("\n")
}

fn v2_back_to_state_code(state: &InvScreen) -> String {
    encode_state_for_callback(state)
}

fn encode_state_for_callback(screen: &InvScreen) -> String {
    match screen {
        InvScreen::Overview => "o".to_string(),
        InvScreen::ItemsMenu => "m".to_string(),
        InvScreen::EquipPart(part) => format!("e_{}", part.code()),
        InvScreen::ItemsList { category, page } => format!("l_{}_{}", category.code(), page),
        InvScreen::ItemDetail { return_state, .. } => return_state.clone(),
    }
}

fn v2_keyboard_overview() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback("Обзор", "huya_inv_s:o"),
            InlineKeyboardButton::callback("Экип", "huya_inv_s:e_all"),
            InlineKeyboardButton::callback("Предметы", "huya_inv_s:m"),
        ],
        vec![
            InlineKeyboardButton::callback("Головка", "huya_inv_s:e_tip"),
            InlineKeyboardButton::callback("Основание", "huya_inv_s:e_base"),
            InlineKeyboardButton::callback("Яйца", "huya_inv_s:e_balls"),
        ],
        vec![
            InlineKeyboardButton::callback("Кольца", "huya_inv_s:e_rings"),
            InlineKeyboardButton::callback("Пирсинг", "huya_inv_s:e_piercing"),
            InlineKeyboardButton::callback("Все", "huya_inv_s:e_all"),
        ],
    ])
}

fn v2_keyboard_equip_part(part: &InventoryEquipPart) -> InlineKeyboardMarkup {
    let mark = |label: &str, code: &str, active: bool| -> String {
        if active {
            format!("• {}", label)
        } else {
            label.to_string()
        }
        };
    let active_tip = matches!(part, InventoryEquipPart::TipHead);
    let active_base = matches!(part, InventoryEquipPart::Base);
    let active_balls = matches!(part, InventoryEquipPart::Balls);
    let active_rings = matches!(part, InventoryEquipPart::Rings);
    let active_piercing = matches!(part, InventoryEquipPart::Piercing);
    let active_all = matches!(part, InventoryEquipPart::All);

    InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback("Обзор", "huya_inv_s:o"),
            InlineKeyboardButton::callback("Экип", "huya_inv_s:e_all"),
            InlineKeyboardButton::callback("Предметы", "huya_inv_s:m"),
        ],
        vec![
            InlineKeyboardButton::callback(mark("Головка", "tip", active_tip).as_str(), "huya_inv_s:e_tip"),
            InlineKeyboardButton::callback(mark("Основание", "base", active_base).as_str(), "huya_inv_s:e_base"),
            InlineKeyboardButton::callback(mark("Яйца", "balls", active_balls).as_str(), "huya_inv_s:e_balls"),
        ],
        vec![
            InlineKeyboardButton::callback(mark("Кольца", "rings", active_rings).as_str(), "huya_inv_s:e_rings"),
            InlineKeyboardButton::callback(mark("Пирсинг", "piercing", active_piercing).as_str(), "huya_inv_s:e_piercing"),
            InlineKeyboardButton::callback(mark("Все", "all", active_all).as_str(), "huya_inv_s:e_all"),
        ],
    ])
}

fn v2_keyboard_items_menu() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback("Обзор", "huya_inv_s:o"),
            InlineKeyboardButton::callback("Экип", "huya_inv_s:e_all"),
            InlineKeyboardButton::callback("Предметы", "huya_inv_s:m"),
        ],
        vec![
            InlineKeyboardButton::callback("Экип", "huya_inv_s:l_equip_0"),
            InlineKeyboardButton::callback("Бустеры", "huya_inv_s:l_booster_0"),
        ],
        vec![
            InlineKeyboardButton::callback("Гемы", "huya_inv_s:l_gem_0"),
            InlineKeyboardButton::callback("Прочее", "huya_inv_s:l_other_0"),
        ],
    ])
}

fn v2_keyboard_items_list(
    items: &[crate::db::models::HuyaInventoryItem],
    category: &InventoryCategory,
    page: usize,
) -> InlineKeyboardMarkup {
    let filtered = v2_items_by_category(items, category);
    let pages = v2_items_pagination(filtered.len());
    let last_page = pages.len().saturating_sub(1);
    let page = page.min(last_page);
    let (start, end) = pages
        .get(page)
        .copied()
        .unwrap_or((0, filtered.len()));

    let slice = filtered.get(start..end).unwrap_or(&[]);
    let return_state = format!("l_{}_{}", category.code(), page);

    // Build item buttons (count is what matters for Telegram: <=9 total per screen).
    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();
    let mut i = 0usize;
    let per_row = 2usize;
    while i < slice.len() {
        let mut row: Vec<InlineKeyboardButton> = Vec::new();
        for _ in 0..per_row {
            if i >= slice.len() {
                break;
            }
            let it = slice[i].clone();
            row.push(InlineKeyboardButton::callback(
                format!("🔍 #{}", it.id),
                format!("huya_inv_s:d:{}:{}", it.id, return_state),
            ));
            i += 1;
        }
        rows.push(row);
    }

    // Pagination controls (tabs hidden on list screens).
    if pages.len() <= 1 {
        // Only one page: show back only (it is both first and last).
        rows.push(vec![InlineKeyboardButton::callback("◀ Назад", "huya_inv_s:m")]);
    } else if page == 0 {
        rows.push(vec![
            InlineKeyboardButton::callback("◀ Назад", "huya_inv_s:m"),
            InlineKeyboardButton::callback("▶", format!("huya_inv_s:l_{}_{}", category.code(), page + 1)),
        ]);
    } else if page == last_page {
        rows.push(vec![
            InlineKeyboardButton::callback("◀", format!("huya_inv_s:l_{}_{}", category.code(), page.saturating_sub(1))),
            InlineKeyboardButton::callback("◀ Назад", "huya_inv_s:m"),
        ]);
    } else {
        rows.push(vec![
            InlineKeyboardButton::callback("◀", format!("huya_inv_s:l_{}_{}", category.code(), page - 1)),
            InlineKeyboardButton::callback("▶", format!("huya_inv_s:l_{}_{}", category.code(), page + 1)),
        ]);
    }

    InlineKeyboardMarkup::new(rows)
}

fn v2_keyboard_item_detail(
    item: &crate::db::models::HuyaInventoryItem,
    equipped: Option<&crate::db::models::HuyaEquipmentSlot>,
    return_state: &InvScreen,
) -> InlineKeyboardMarkup {
    let return_state_code = match return_state {
        InvScreen::ItemDetail { return_state, .. } => return_state.clone(),
        InvScreen::Overview => "o".to_string(),
        InvScreen::ItemsMenu => "m".to_string(),
        InvScreen::EquipPart(part) => format!("e_{}", part.code()),
        InvScreen::ItemsList { category, page } => format!("l_{}_{}", category.code(), page),
        InvScreen::ItemDetail { .. } => "o".to_string(),
    };

    let back_btn = InlineKeyboardButton::callback("◀ Назад", format!("huya_inv_s:{}", return_state_code));

    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();
    rows.push(vec![back_btn]);

    // Actions depend on item kind.
    if item.item_kind == "equipment" {
        let sell_btn = InlineKeyboardButton::callback(
            "💰 Продать",
            format!("huya_inv_sell:{}:{}", item.id, return_state_code),
        );

        let equip_action = if equipped.is_some() {
            let slot = equipped.unwrap().slot.clone();
            InlineKeyboardButton::callback(
                "Снять",
                format!("huya_inv_unequip:{}:{}", slot, return_state_code),
            )
        } else {
            // Equipment item should have its target slot.
            let slot = item.slot.as_deref().unwrap_or("tip");
            InlineKeyboardButton::callback(
                "Надеть",
                format!("huya_inv_equip:{}:{}:{}", item.id, slot, return_state_code),
            )
        };

        rows.push(vec![equip_action, sell_btn]);

        // Socket/reforge actions (only for equipment).
        rows.push(vec![
            InlineKeyboardButton::callback(
                "💎 Инкруст",
                format!("huya_inv_socket_pick:{}:{}", item.id, return_state_code),
            ),
            InlineKeyboardButton::callback(
                "🔥 Перековка",
                format!("huya_inv_reforge_pick:{}:{}", item.id, return_state_code),
            ),
        ]);
    } else if item.item_kind == "booster" {
        let use_btn = InlineKeyboardButton::callback(
            "⚡ Использовать",
            format!("huya_inv_use:{}:{}", item.id, return_state_code),
        );
        let sell_btn = InlineKeyboardButton::callback(
            "💰 Продать",
            format!("huya_inv_sell:{}:{}", item.id, return_state_code),
        );
        rows.push(vec![use_btn, sell_btn]);
    } else {
        let sell_btn = InlineKeyboardButton::callback(
            "💰 Продать",
            format!("huya_inv_sell:{}:{}", item.id, return_state_code),
        );
        rows.push(vec![sell_btn]);
    }

    InlineKeyboardMarkup::new(rows)
}

pub async fn huyainv_handler_legacy(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    if !msg.chat.is_group() && !msg.chat.is_supergroup() {
        return Ok(());
    }
    let from = match msg.from.as_ref() {
        Some(f) => f,
        None => return Ok(()),
    };
    let tg_id = from.id.0 as i64;
    let (h, _) = huya_db::get_or_create(&pool, msg.chat.id.0, tg_id).await?;
    let equ = huya_db::get_equipment(&pool, msg.chat.id.0, tg_id).await?;
    let items = huya_db::get_inventory(&pool, msg.chat.id.0, tg_id).await?;
    let gems = huya_db::all_socketed_gems_for_player(&pool, msg.chat.id.0, tg_id).await?;

    let text = v2_overview_summary_text(&h, &equ, &items);
    let mut req = bot
        .send_message(msg.chat.id, text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .reply_markup(v2_keyboard_overview());
    if let Some(thread) = topic_thread_id(&msg) {
        req = req.message_thread_id(thread);
    }
    req.await?;
    let _ = gems; // keep fetched to avoid changing query pattern too much for now.
    Ok(())
}

pub async fn huyainv_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    if !msg.chat.is_group() && !msg.chat.is_supergroup() {
        return Ok(());
    }
    let from = match msg.from.as_ref() { Some(f) => f, None => return Ok(()) };
    let tg_id = from.id.0 as i64;
    let (h, _) = huya_db::get_or_create(&pool, msg.chat.id.0, tg_id).await?;
    let equ = huya_db::get_equipment(&pool, msg.chat.id.0, tg_id).await?;
    let items = huya_db::get_inventory(&pool, msg.chat.id.0, tg_id).await?;
    let gems = huya_db::all_socketed_gems_for_player(&pool, msg.chat.id.0, tg_id).await?;
    let text = render_inventory_text(&h, &equ, &items, &gems, InventoryView::Overview, 0);
    let mut req = bot.send_message(msg.chat.id, text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .reply_markup(inventory_keyboard(&items, InventoryView::Overview, 0));
    if let Some(thread) = topic_thread_id(&msg) {
        req = req.message_thread_id(thread);
    }
    req.await?;
    Ok(())
}

pub async fn huya_inventory_callback(
    bot: Bot,
    query: CallbackQuery,
    pool: PgPool,
) -> Result<(), AppError> {
    let data = query.data.as_deref().unwrap_or("");
    let qid = query.id.clone();
    let clicker = query.from.id.0 as i64;
    let msg_ref = match query.message.as_ref() {
        Some(m) => m,
        None => return Ok(()),
    };
    let chat_id_raw = msg_ref.chat().id.0;

    if data == "huya_noop" {
        let _ = bot.answer_callback_query(qid.clone()).await;
        return Ok(());
    }

    let mut screen_to_render: Option<InvScreen> = None;

    // ── Actions (all of them re-render to `screen_to_render`)
    if let Some(rest) = data.strip_prefix("huya_inv_sell:") {
        let p: Vec<&str> = rest.splitn(2, ':').collect();
        if p.len() == 2 {
            if let Ok(inv_id) = p[0].parse::<i32>() {
                let return_state = p[1];
                if let Some(mm) = huya_db::sell_inventory_item(&pool, chat_id_raw, clicker, inv_id).await? {
                    let _ = bot
                        .answer_callback_query(qid.clone())
                        .text(LOCALE.t_fmt(
                            "ru",
                            "huya.inventory.sell_success",
                            &[
                                ("item", &inv_id.to_string()),
                                ("refund", &mm_to_cm_str(mm)),
                            ],
                        ))
                        .await;
                } else {
                    let _ = bot.answer_callback_query(qid.clone()).await;
                }
                screen_to_render = decode_screen_from_state(return_state);
            }
        } else if let Ok(inv_id) = rest.parse::<i32>() {
            // Legacy callback: just render overview.
            let _ = huya_db::sell_inventory_item(&pool, chat_id_raw, clicker, inv_id).await?;
            let _ = bot.answer_callback_query(qid.clone()).await;
            screen_to_render = Some(InvScreen::Overview);
        }
    } else if let Some(rest) = data.strip_prefix("huya_inv_use:") {
        let p: Vec<&str> = rest.splitn(2, ':').collect();
        if p.len() == 2 {
            if let Ok(inv_id) = p[0].parse::<i32>() {
                let return_state = p[1];
                if let Some((effect, val)) =
                    huya_db::use_booster_item(&pool, chat_id_raw, clicker, inv_id).await?
                {
                    let _ = bot
                        .answer_callback_query(qid.clone())
                        .text(LOCALE.t_fmt(
                            "ru",
                            "huya.inventory.use_success",
                            &[("effect", &effect), ("value", &val.to_string())],
                        ))
                        .await;
                } else {
                    let _ = bot.answer_callback_query(qid.clone()).text(LOCALE.t("ru", "huya.inventory.use_fail")).await;
                }
                screen_to_render = decode_screen_from_state(return_state);
            }
        } else if let Ok(inv_id) = rest.parse::<i32>() {
            // Legacy callback: render overview.
            let _ = huya_db::use_booster_item(&pool, chat_id_raw, clicker, inv_id).await?;
            let _ = bot.answer_callback_query(qid.clone()).await;
            screen_to_render = Some(InvScreen::Overview);
        }
    } else if let Some(rest) = data.strip_prefix("huya_inv_equip:") {
        // Format: huya_inv_equip:{inv_id}:{slot}:{return_state}
        let p: Vec<&str> = rest.splitn(3, ':').collect();
        if p.len() == 3 {
            if let Ok(inv_id) = p[0].parse::<i32>() {
                let slot = p[1];
                let return_state = p[2];
                let ok = huya_db::equip_item(&pool, chat_id_raw, clicker, slot, inv_id).await?;
                let _ = bot
                    .answer_callback_query(qid.clone())
                    .text(if ok {
                        LOCALE.t("ru", "huya.inventory.equip_success")
                    } else {
                        LOCALE.t("ru", "huya.inventory.equip_fail")
                    })
                    .await;
                screen_to_render = decode_screen_from_state(return_state);
            }
        }
    } else if let Some(rest) = data.strip_prefix("huya_inv_unequip:") {
        // Format: huya_inv_unequip:{slot}:{return_state}
        let p: Vec<&str> = rest.splitn(2, ':').collect();
        if p.len() == 2 {
            let slot = p[0];
            let return_state = p[1];
            let ok = huya_db::unequip_item(&pool, chat_id_raw, clicker, slot).await?;
            let _ = bot
                .answer_callback_query(qid.clone())
                .text(if ok {
                    LOCALE.t("ru", "huya.inventory.unequip_success")
                } else {
                    LOCALE.t("ru", "huya.inventory.equip_fail")
                })
                .await;
            screen_to_render = decode_screen_from_state(return_state);
        }
    } else if let Some(rest) = data.strip_prefix("huya_inv_socket_pick:") {
        // Format: huya_inv_socket_pick:{item_id}:{return_state}
        let p: Vec<&str> = rest.splitn(2, ':').collect();
        if p.len() == 2 {
            if let Ok(item_id) = p[0].parse::<i32>() {
                let return_state = p[1];
                let gems = huya_db::available_gems(&pool, chat_id_raw, clicker).await?;
                if gems.is_empty() {
                    let _ = bot
                        .answer_callback_query(qid.clone())
                        .text(LOCALE.t("ru", "huya.inventory.no_gems"))
                        .await;
                    return Ok(());
                }

                let mut rows: Vec<Vec<InlineKeyboardButton>> = gems
                    .into_iter()
                    .take(8)
                    .map(|g| {
                        vec![InlineKeyboardButton::callback(
                            format!(
                                "💎 {} {} +{}",
                                rarity_emoji(&g.rarity),
                                item_label_ru(&g.item_id),
                                g.roll.max(0)
                            ),
                            format!("huya_inv_socket_do:{}:{}:{}", item_id, g.id, return_state),
                        )]
                    })
                    .collect();

                rows.push(vec![InlineKeyboardButton::callback(
                    "↩ Назад",
                    format!("huya_inv_s:{}", return_state),
                )]);

                let _ = bot.answer_callback_query(qid.clone()).await;
                let _ = bot
                    .edit_message_reply_markup(msg_ref.chat().id, msg_ref.id())
                    .reply_markup(InlineKeyboardMarkup::new(rows))
                    .await;
                return Ok(());
            }
        }
    } else if let Some(rest) = data.strip_prefix("huya_inv_socket_do:") {
        // Format: huya_inv_socket_do:{item_id}:{gem_id}:{return_state}
        let p: Vec<&str> = rest.splitn(3, ':').collect();
        if p.len() == 3 {
            if let Ok(item_id) = p[0].parse::<i32>() && let Ok(gem_id) = p[1].parse::<i32>() {
                let return_state = p[2];
                let ok = huya_db::socket_gem_into_item(&pool, chat_id_raw, clicker, item_id, gem_id).await?;
                let _ = bot
                    .answer_callback_query(qid.clone())
                    .text(if ok {
                        LOCALE.t("ru", "huya.inventory.socket_success")
                    } else {
                        LOCALE.t("ru", "huya.inventory.socket_fail")
                    })
                    .await;
                screen_to_render = decode_screen_from_state(return_state);
            }
        }
    } else if let Some(rest) = data.strip_prefix("huya_inv_reforge_pick:") {
        // Format: huya_inv_reforge_pick:{item_id}:{return_state}
        let p: Vec<&str> = rest.splitn(2, ':').collect();
        if p.len() == 2 {
            if let Ok(item_id) = p[0].parse::<i32>() {
                let return_state = p[1];
                let gems = huya_db::available_gems(&pool, chat_id_raw, clicker).await?;
                if gems.is_empty() {
                    let _ = bot
                        .answer_callback_query(qid.clone())
                        .text(LOCALE.t("ru", "huya.inventory.no_gems"))
                        .await;
                    return Ok(());
                }

                let mut rows: Vec<Vec<InlineKeyboardButton>> = gems
                    .into_iter()
                    .take(8)
                    .map(|g| {
                        vec![InlineKeyboardButton::callback(
                            format!(
                                "🧪 {} {} +{}",
                                rarity_emoji(&g.rarity),
                                item_label_ru(&g.item_id),
                                g.roll.max(0)
                            ),
                            format!(
                                "huya_inv_reforge_do:{}:{}:{}",
                                item_id, g.id, return_state
                            ),
                        )]
                    })
                    .collect();

                rows.push(vec![InlineKeyboardButton::callback(
                    "↩ Назад",
                    format!("huya_inv_s:{}", return_state),
                )]);

                let _ = bot.answer_callback_query(qid.clone()).await;
                let _ = bot
                    .edit_message_reply_markup(msg_ref.chat().id, msg_ref.id())
                    .reply_markup(InlineKeyboardMarkup::new(rows))
                    .await;
                return Ok(());
            }
        }
    } else if let Some(rest) = data.strip_prefix("huya_inv_reforge_do:") {
        // Format: huya_inv_reforge_do:{item_id}:{gem_id}:{return_state}
        let p: Vec<&str> = rest.splitn(3, ':').collect();
        if p.len() == 3 {
            if let Ok(item_id) = p[0].parse::<i32>() && let Ok(gem_id) = p[1].parse::<i32>() {
                let return_state = p[2];
                if let Some(res) = huya_db::reforge_item_with_gem(&pool, chat_id_raw, clicker, item_id, gem_id).await? {
                    let text = match res.outcome {
                        huya_db::ReforgeOutcome::Success => LOCALE.t_fmt(
                            "ru",
                            "huya.inventory.reforge_success",
                            &[("old", &res.old_roll.to_string()), ("new", &res.new_roll.to_string())],
                        ),
                        huya_db::ReforgeOutcome::Fail => LOCALE.t("ru", "huya.inventory.reforge_fail").to_string(),
                        huya_db::ReforgeOutcome::CritFail => LOCALE.t("ru", "huya.inventory.reforge_crit_fail").to_string(),
                    };
                    let _ = bot.answer_callback_query(qid.clone()).text(text).await;
                } else {
                    let _ = bot.answer_callback_query(qid.clone()).text(LOCALE.t("ru", "huya.inventory.reforge_fail")).await;
                }
                screen_to_render = decode_screen_from_state(return_state);
            }
        }
    }

    // ── Navigation (huya_inv_s:*)
    if screen_to_render.is_none() {
        screen_to_render = parse_screen_from_callback_data(data);
    }
    let screen_to_render = screen_to_render.unwrap_or(InvScreen::Overview);

    let (h, _) = huya_db::get_or_create(&pool, chat_id_raw, clicker).await?;
    let equ = huya_db::get_equipment(&pool, chat_id_raw, clicker).await?;
    let items = huya_db::get_inventory(&pool, chat_id_raw, clicker).await?;
    let gems = huya_db::all_socketed_gems_for_player(&pool, chat_id_raw, clicker).await?;

    let (text, keyboard) = match &screen_to_render {
        InvScreen::Overview => (
            v2_overview_summary_text(&h, &equ, &items),
            v2_keyboard_overview(),
        ),
        InvScreen::EquipPart(part) => (
            v2_equip_part_text(&h, &equ, &items, part),
            v2_keyboard_equip_part(part),
        ),
        InvScreen::ItemsMenu => (
            v2_items_menu_text(&items),
            v2_keyboard_items_menu(),
        ),
        InvScreen::ItemsList { category, page } => {
            let filtered = v2_items_by_category(&items, category);
            let pages = v2_items_pagination(filtered.len());
            let last_page = pages.len().saturating_sub(1);
            let page = (*page).min(last_page);
            let text = v2_items_list_text(&items, category, page, last_page);
            let keyboard = v2_keyboard_items_list(&items, category, page);
            (text, keyboard)
        }
        InvScreen::ItemDetail { item_id, return_state } => {
            let item = items.iter().find(|x| x.id == *item_id);
            let equipped = equ.iter().find(|s| s.inventory_id == *item_id);
            let text = render_item_detail(&h, &equ, &items, &gems, *item_id);
            let keyboard = if let Some(it) = item {
                let screen_copy = InvScreen::ItemDetail {
                    item_id: *item_id,
                    return_state: return_state.clone(),
                };
                v2_keyboard_item_detail(it, equipped, &screen_copy)
            } else {
                // Fallback: item vanished, back to overview.
                v2_keyboard_overview()
            };
            (text, keyboard)
        }
    };

    let _ = bot
        .edit_message_text(msg_ref.chat().id, msg_ref.id(), text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .reply_markup(keyboard)
        .await;
    Ok(())
}
