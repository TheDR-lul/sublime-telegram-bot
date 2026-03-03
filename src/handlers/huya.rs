//! HuyActa tamagotchi game handlers.
//! Commands: /huya [stat|grow|fight @user|steal @user], /huyatop
//! Inline button "Погладить" via callback huya_grow:{tg_id}.

use sqlx::{self, PgPool};
use teloxide::prelude::*;
use teloxide::types::{CallbackQuery, InlineKeyboardButton, InlineKeyboardMarkup};
use teloxide::utils::html::escape as escape_html;

use crate::db::huya as huya_db;
use crate::db::models::Huya;
use crate::db::user;
use crate::error::AppError;
use crate::i18n::LOCALE;

/// Convert millimetres to a "X.Y" cm string (e.g. 35 → "3.5").
fn mm_to_cm_str(mm: i32) -> String {
    let abs = mm.unsigned_abs();
    format!("{}.{}", abs / 10, abs % 10)
}

fn huya_status_text(h: &Huya, name: &str) -> String {
    let base = if h.is_ass() {
        LOCALE.t_fmt("ru", "huya.status_ass", &[
            ("name", &escape_html(name)),
            ("size", &h.display_cm()),
            ("actions_left", &h.actions_left.to_string()),
        ])
    } else {
        LOCALE.t_fmt("ru", "huya.status", &[
            ("name", &escape_html(name)),
            ("size", &h.display_cm()),
            ("level", &h.level.to_string()),
            ("xp", &h.xp.to_string()),
            ("actions_left", &h.actions_left.to_string()),
        ])
    };
    format!("{}\n\n{}", base, LOCALE.t("ru", "huya.hint"))
}

fn huya_stat_keyboard(tg_id: i64, has_actions: bool) -> InlineKeyboardMarkup {
    let btn_label = if has_actions {
        LOCALE.t("ru", "huya.stroke_btn").to_string()
    } else {
        LOCALE.t("ru", "huya.no_actions_btn").to_string()
    };
    InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::callback(btn_label, format!("huya_grow:{}", tg_id)),
    ]])
}

pub async fn huya_handler(
    bot: Bot,
    msg: Message,
    cmd: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    if !msg.chat.is_group() && !msg.chat.is_supergroup() {
        return Ok(());
    }
    let chat_id = msg.chat.id.0;
    let from = match msg.from.as_ref() {
        Some(f) => f,
        None => return Ok(()),
    };
    let tg_id = from.id.0 as i64;
    let user_name = from
        .username
        .as_ref()
        .map(|u| format!("@{}", u))
        .unwrap_or_else(|| from.first_name.clone());

    user::upsert_tg_user(&pool, from).await?;

    use crate::handlers::commands::Cmd;
    match &cmd {
        Cmd::Huyagrow => {
            handle_grow(&bot, &pool, msg.chat.id, chat_id, tg_id, &user_name).await
        }
        Cmd::Huyafight(s) => {
            let arg = s.trim().to_string();
            handle_fight(&bot, &pool, &msg, chat_id, tg_id, &user_name, &arg).await
        }
        Cmd::Huyasteal(s) => {
            let arg = s.trim().to_string();
            handle_steal(&bot, &pool, &msg, chat_id, tg_id, &user_name, &arg).await
        }
        Cmd::Huya(s) => {
            let arg = s.trim().to_string();
            let parts: Vec<&str> = arg.splitn(2, ' ').collect();
            let sub = parts.first().copied().unwrap_or("stat");
            let sub_arg = parts.get(1).copied().unwrap_or("").trim();
            match sub {
                "grow" => handle_grow(&bot, &pool, msg.chat.id, chat_id, tg_id, &user_name).await,
                "fight" => handle_fight(&bot, &pool, &msg, chat_id, tg_id, &user_name, sub_arg).await,
                "steal" => handle_steal(&bot, &pool, &msg, chat_id, tg_id, &user_name, sub_arg).await,
                _ => handle_stat(&bot, &pool, msg.chat.id, chat_id, tg_id, &user_name).await,
            }
        }
        _ => handle_stat(&bot, &pool, msg.chat.id, chat_id, tg_id, &user_name).await,
    }
}

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
        let reg_text = LOCALE.t_rand_fmt("ru", "huya.reg_messages", &[("name", &escape_html(name))]);
        bot.send_message(chat_id, reg_text)
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;
    }
    let has_actions = h.actions_left > 0 || h.actions_reset_at < chrono::Utc::now().date_naive();
    bot.send_message(chat_id, huya_status_text(&h, name))
        .parse_mode(teloxide::types::ParseMode::Html)
        .reply_markup(huya_stat_keyboard(tg_id, has_actions))
        .await?;
    Ok(())
}

/// Callback handler for the inline "Погладить" button: huya_grow:{tg_id}
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

    let clicker_tg_id = query.from.id.0 as i64;
    if clicker_tg_id != owner_tg_id {
        let _ = bot.answer_callback_query(query.id).text("Это не твоя хуяка!").await;
        return Ok(());
    }

    let chat_id = match query.message.as_ref().map(|m| m.chat().id) {
        Some(c) => c,
        None => { let _ = bot.answer_callback_query(query.id).await; return Ok(()); }
    };

    let from = &query.from;
    let user_name = from.username
        .as_ref()
        .map(|u| format!("@{}", u))
        .unwrap_or_else(|| from.first_name.clone());

    let (h, _) = huya_db::get_or_create(&pool, chat_id.0, clicker_tg_id).await?;

    if !huya_db::consume_action(&pool, &h).await? {
        let _ = bot.answer_callback_query(query.id)
            .text(LOCALE.t("ru", "huya.no_actions"))
            .await;
        return Ok(());
    }

    let (updated, grow_mm, xp_gain, leveled_up) = huya_db::grow(&pool, &h).await?;

    let grow_cm_str = mm_to_cm_str(grow_mm);
    let answer_text = if leveled_up {
        LOCALE.t_fmt("ru", "huya.grow_levelup", &[
            ("name", &escape_html(&user_name)),
            ("grow_cm", &grow_cm_str),
            ("xp", &xp_gain.to_string()),
            ("level", &updated.level.to_string()),
            ("size", &updated.display_cm()),
        ])
    } else {
        LOCALE.t_fmt("ru", "huya.grow", &[
            ("name", &escape_html(&user_name)),
            ("grow_cm", &grow_cm_str),
            ("xp", &xp_gain.to_string()),
            ("size", &updated.display_cm()),
        ])
    };

    let _ = bot.answer_callback_query(query.id).text(format!("+{} см", grow_cm_str)).await;

    // Edit original stat message with updated state.
    let has_actions = updated.actions_left > 0;
    let new_stat = huya_status_text(&updated, &user_name);
    if let Some(ref msg) = query.message {
        let _ = bot.edit_message_text(msg.chat().id, msg.id(), new_stat)
            .parse_mode(teloxide::types::ParseMode::Html)
            .reply_markup(huya_stat_keyboard(clicker_tg_id, has_actions))
            .await;
    }

    // Also send the grow result as a separate message.
    bot.send_message(chat_id, answer_text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;

    Ok(())
}

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
        let reg_text = LOCALE.t_rand_fmt("ru", "huya.reg_messages", &[("name", &escape_html(name))]);
        bot.send_message(chat_id, reg_text)
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;
    }

    if !huya_db::consume_action(pool, &h).await? {
        bot.send_message(chat_id, LOCALE.t("ru", "huya.no_actions")).await?;
        return Ok(());
    }

    let (updated, grow_mm, xp_gain, leveled_up) = huya_db::grow(pool, &h).await?;

    let grow_cm_str = mm_to_cm_str(grow_mm);
    let text = if leveled_up {
        LOCALE.t_fmt("ru", "huya.grow_levelup", &[
            ("name", &escape_html(name)),
            ("grow_cm", &grow_cm_str),
            ("xp", &xp_gain.to_string()),
            ("level", &updated.level.to_string()),
            ("size", &updated.display_cm()),
        ])
    } else {
        LOCALE.t_fmt("ru", "huya.grow", &[
            ("name", &escape_html(name)),
            ("grow_cm", &grow_cm_str),
            ("xp", &xp_gain.to_string()),
            ("size", &updated.display_cm()),
        ])
    };

    bot.send_message(chat_id, text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
    Ok(())
}

/// Resolved target for fight/steal commands.
enum TargetResult {
    /// Valid human target with this tg_id.
    User(i64),
    /// Target resolved but it's a Telegram bot.
    IsBot,
    /// Could not resolve a target.
    NotFound,
}

async fn resolve_target(
    msg: &Message,
    pool: &PgPool,
    sub_arg: &str,
) -> TargetResult {
    // TextMention entity (inline mention of a user without @).
    if let Some(entities) = msg.entities() {
        for e in entities {
            if let teloxide::types::MessageEntityKind::TextMention { user } = &e.kind {
                if user.is_bot {
                    return TargetResult::IsBot;
                }
                return TargetResult::User(user.id.0 as i64);
            }
        }
    }
    // @username argument.
    let username = sub_arg.trim_start_matches('@');
    if !username.is_empty() {
        if let Ok(Some(u)) = user::get_by_username(pool, username).await {
            return TargetResult::User(u.tg_id);
        }
    }
    // Reply-to message.
    if let Some(reply) = msg.reply_to_message() {
        if let Some(ref from) = reply.from {
            if from.is_bot {
                return TargetResult::IsBot;
            }
            return TargetResult::User(from.id.0 as i64);
        }
    }
    TargetResult::NotFound
}

// ── Fight mini-game helpers ───────────────────────────────────────────────────

/// Encode the move label to its i32 id (0/1/2) used in DB and callbacks.
fn move_id(label: &str) -> Option<i32> {
    match label {
        "0" => Some(0),
        "1" => Some(1),
        "2" => Some(2),
        _ => None,
    }
}

/// Human-readable emoji for a move id.
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
        InlineKeyboardButton::callback("🍆 Напор",   format!("huya_fm:{}:0", fight_id)),
        InlineKeyboardButton::callback("🫸 Финт",    format!("huya_fm:{}:1", fight_id)),
        InlineKeyboardButton::callback("🎯 В шары",  format!("huya_fm:{}:2", fight_id)),
    ]])
}

// ── /huya fight ───────────────────────────────────────────────────────────────

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

    let target_tg_id = match resolve_target(msg, pool, sub_arg).await {
        TargetResult::User(id) => id,
        TargetResult::IsBot => {
            bot.send_message(chat_id, LOCALE.t("ru", "huya.fight_self_bot"))
                .parse_mode(teloxide::types::ParseMode::Html)
                .await?;
            return Ok(());
        }
        TargetResult::NotFound => {
            bot.send_message(chat_id, LOCALE.t("ru", "huya.fight_no_target")).await?;
            return Ok(());
        }
    };

    // Self-attack: funny penalty.
    if target_tg_id == attacker_tg_id {
        let (h, _) = huya_db::get_or_create(pool, chat_id_raw, attacker_tg_id).await?;
        if !huya_db::consume_action(pool, &h).await? {
            bot.send_message(chat_id, LOCALE.t("ru", "huya.no_actions")).await?;
            return Ok(());
        }
        let updated = huya_db::self_fight(pool, chat_id_raw, attacker_tg_id).await?;
        bot.send_message(chat_id, LOCALE.t_fmt("ru", "huya.fight_self", &[
            ("name", &escape_html(attacker_name)),
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

    let target_user = user::get_by_tg_id(pool, target_tg_id).await?;
    let target_name = target_user
        .as_ref()
        .map(|u| u.full_username(true))
        .unwrap_or_else(|| "???".to_string());

    // Create a pending fight record.
    let fight = huya_db::create_fight(pool, chat_id_raw, attacker_tg_id, target_tg_id).await?;

    let challenge_text = LOCALE.t_fmt("ru", "huya.fight_challenge", &[
        ("challenger", &escape_html(attacker_name)),
        ("target", &escape_html(&target_name)),
    ]);

    let sent = bot.send_message(chat_id, challenge_text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .reply_markup(fight_challenge_keyboard(fight.id))
        .await?;

    // Save message_id so callbacks can edit it.
    let _ = huya_db::set_fight_message_id(pool, fight.id, sent.id.0).await;

    Ok(())
}

// ── Callbacks: accept, decline, move ─────────────────────────────────────────

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

    let ch_user = user::get_by_tg_id(&pool, fight.challenger_tg_id).await?;
    let tg_user = user::get_by_tg_id(&pool, fight.target_tg_id).await?;
    let ch_name = ch_user.as_ref().map(|u| u.full_username(true)).unwrap_or_else(|| "???".to_string());
    let tg_name = tg_user.as_ref().map(|u| u.full_username(true)).unwrap_or_else(|| "???".to_string());

    let text = LOCALE.t_fmt("ru", "huya.fight_picking", &[
        ("challenger", &escape_html(&ch_name)),
        ("target", &escape_html(&tg_name)),
    ]);

    if let Some(ref msg) = query.message {
        let _ = bot.edit_message_text(msg.chat().id, msg.id(), text)
            .parse_mode(teloxide::types::ParseMode::Html)
            .reply_markup(fight_pick_keyboard(fight_id))
            .await;
    }

    Ok(())
}

/// huya_fd:{fight_id} — target declines the challenge.
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

    // Refund action to challenger since target declined.
    let (ch_huya, _) = huya_db::get_or_create(&pool, fight.chat_id, fight.challenger_tg_id).await?;
    sqlx::query("UPDATE huya SET actions_left = LEAST(actions_left + 1, $1) WHERE id = $2")
        .bind(4_i32)
        .bind(ch_huya.id)
        .execute(&pool)
        .await?;

    let decliner_name = query.from.username
        .as_ref()
        .map(|u| format!("@{}", u))
        .unwrap_or_else(|| query.from.first_name.clone());

    let text = LOCALE.t_fmt("ru", "huya.fight_declined", &[
        ("name", &escape_html(&decliner_name)),
    ]);

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
    let pick_id = match move_id(parts[2]) {
        Some(p) => p,
        None => { let _ = bot.answer_callback_query(query.id).await; return Ok(()); }
    };

    let clicker = query.from.id.0 as i64;

    // Load fight to validate clicker is a participant.
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

    // Store pick; returns updated fight.
    let updated = match huya_db::store_pick(&pool, fight_id, clicker, pick_id).await? {
        Some(f) => f,
        None => {
            let _ = bot.answer_callback_query(query.id).text("Ты уже сделал выбор!").await;
            return Ok(());
        }
    };

    let _ = bot.answer_callback_query(query.id)
        .text(format!("Ты выбрал {}!", move_emoji(pick_id)))
        .await;

    // If only one player has picked, show waiting message.
    if updated.challenger_pick.is_none() || updated.target_pick.is_none() {
        let picker_name = query.from.username
            .as_ref()
            .map(|u| format!("@{}", u))
            .unwrap_or_else(|| query.from.first_name.clone());

        let wait_text = LOCALE.t_fmt("ru", "huya.fight_waiting", &[
            ("name", &escape_html(&picker_name)),
        ]);

        if let Some(ref msg) = query.message {
            let _ = bot.edit_message_text(msg.chat().id, msg.id(), wait_text)
                .parse_mode(teloxide::types::ParseMode::Html)
                .reply_markup(fight_pick_keyboard(fight_id))
                .await;
        }
        return Ok(());
    }

    // Both picked — resolve!
    let ch_pick = updated.challenger_pick.unwrap();
    let tg_pick = updated.target_pick.unwrap();

    let (winner_tg_id, loser_tg_id, steal_mm, elo_gain, tie) =
        huya_db::resolve_huya_fight(
            &pool,
            updated.chat_id,
            updated.challenger_tg_id,
            updated.target_tg_id,
            ch_pick,
            tg_pick,
        )
        .await?;

    let _ = huya_db::finish_huya_fight(&pool, fight_id).await;

    let ch_user = user::get_by_tg_id(&pool, updated.challenger_tg_id).await?;
    let tg_user = user::get_by_tg_id(&pool, updated.target_tg_id).await?;
    let ch_name = ch_user.as_ref().map(|u| u.full_username(true)).unwrap_or_else(|| "???".to_string());
    let tg_name = tg_user.as_ref().map(|u| u.full_username(true)).unwrap_or_else(|| "???".to_string());

    let result_text = if tie {
        LOCALE.t_fmt("ru", "huya.fight_result_tie", &[
            ("ch_pick", move_emoji(ch_pick)),
            ("tg_pick", move_emoji(tg_pick)),
        ])
    } else {
        let winner_name = if winner_tg_id == updated.challenger_tg_id { &ch_name } else { &tg_name };
        let loser_name  = if loser_tg_id  == updated.challenger_tg_id { &ch_name } else { &tg_name };
        let w_pick = if winner_tg_id == updated.challenger_tg_id { ch_pick } else { tg_pick };
        let l_pick = if loser_tg_id  == updated.challenger_tg_id { ch_pick } else { tg_pick };

        // Award ELO to winner.
        let _ = crate::db::duel::add_huya_elo(&pool, updated.chat_id, winner_tg_id, elo_gain).await;

        LOCALE.t_fmt("ru", "huya.fight_result_win", &[
            ("challenger_pick", move_emoji(w_pick)),
            ("target_pick",     move_emoji(l_pick)),
            ("winner",  &escape_html(winner_name)),
            ("loser",   &escape_html(loser_name)),
            ("steal_cm", &mm_to_cm_str(steal_mm)),
            ("elo_gain", &elo_gain.to_string()),
        ])
    };

    if let Some(ref msg) = query.message {
        let _ = bot.edit_message_text(msg.chat().id, msg.id(), result_text)
            .parse_mode(teloxide::types::ParseMode::Html)
            .reply_markup(InlineKeyboardMarkup::new::<Vec<Vec<InlineKeyboardButton>>>(vec![]))
            .await;
    }

    Ok(())
}

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

    let target_tg_id = match resolve_target(msg, pool, sub_arg).await {
        TargetResult::User(id) => id,
        TargetResult::IsBot => {
            bot.send_message(chat_id, LOCALE.t("ru", "huya.steal_self_bot"))
                .parse_mode(teloxide::types::ParseMode::Html)
                .await?;
            return Ok(());
        }
        TargetResult::NotFound => {
            bot.send_message(chat_id, LOCALE.t("ru", "huya.steal_no_target")).await?;
            return Ok(());
        }
    };
    // Self-steal: same penalty as self-fight.
    if target_tg_id == attacker_tg_id {
        let (h, _) = huya_db::get_or_create(pool, chat_id_raw, attacker_tg_id).await?;
        if !huya_db::consume_action(pool, &h).await? {
            bot.send_message(chat_id, LOCALE.t("ru", "huya.no_actions")).await?;
            return Ok(());
        }
        let updated = huya_db::self_fight(pool, chat_id_raw, attacker_tg_id).await?;
        let text = LOCALE.t_fmt("ru", "huya.steal_self", &[
            ("name", &escape_html(attacker_name)),
            ("new_size", &updated.display_cm()),
        ]);
        bot.send_message(chat_id, text)
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

    let target_user = user::get_by_tg_id(pool, target_tg_id).await?;
    let target_name = target_user
        .as_ref()
        .map(|u| u.full_username(true))
        .unwrap_or_else(|| "???".to_string());

    let steal_cm = mm_to_cm_str(result.steal_mm);
    let chance_str = result.chance_pct.to_string();

    let text = if result.success {
        LOCALE.t_fmt("ru", "huya.steal_success", &[
            ("attacker", &escape_html(attacker_name)),
            ("target", &escape_html(&target_name)),
            ("steal_cm", &steal_cm),
            ("new_size", &result.attacker.display_cm()),
            ("chance_pct", &chance_str),
        ])
    } else {
        LOCALE.t_fmt("ru", "huya.steal_fail", &[
            ("attacker", &escape_html(attacker_name)),
            ("target", &escape_html(&target_name)),
            ("chance_pct", &chance_str),
        ])
    };

    bot.send_message(chat_id, text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
    Ok(())
}

/// /huyareg — explicit registration. Shows random greeting on first use,
/// "already registered" message with hint on subsequent calls.
pub async fn huyareg_handler(
    bot: Bot,
    msg: Message,
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
    let chat_id_raw = msg.chat.id.0;
    let user_name = from.username
        .as_ref()
        .map(|u| format!("@{}", u))
        .unwrap_or_else(|| from.first_name.clone());

    user::upsert_tg_user(&pool, from).await?;

    let (h, was_created) = huya_db::get_or_create(&pool, chat_id_raw, tg_id).await?;

    let text = if was_created {
        LOCALE.t_rand_fmt("ru", "huya.reg_messages", &[("name", &escape_html(&user_name))])
    } else {
        // Already registered — show stats too.
        let already_msg = LOCALE.t("ru", "huya.already_registered").to_string();
        format!("{}\n\n{}", already_msg, huya_status_text(&h, &user_name))
    };

    let has_actions = h.actions_left > 0 || h.actions_reset_at < chrono::Utc::now().date_naive();
    let mut req = bot.send_message(msg.chat.id, text)
        .parse_mode(teloxide::types::ParseMode::Html);
    if !was_created {
        req = req.reply_markup(huya_stat_keyboard(tg_id, has_actions));
    }
    req.await?;
    Ok(())
}

pub async fn huyatop_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    if !msg.chat.is_group() && !msg.chat.is_supergroup() {
        return Ok(());
    }
    let chat_id = msg.chat.id.0;
    let rows = huya_db::top(&pool, chat_id, 10).await?;

    if rows.is_empty() {
        bot.send_message(msg.chat.id, LOCALE.t("ru", "huya.top_empty")).await?;
        return Ok(());
    }

    let mut text = LOCALE.t("ru", "huya.top_header").to_string();
    for (i, (h, tg_id)) in rows.iter().enumerate() {
        let db_user = user::get_by_tg_id(&pool, *tg_id).await?;
        let name = db_user
            .as_ref()
            .map(|u| u.full_username(false))
            .unwrap_or_else(|| format!("user_{}", tg_id));
        let medal = match i {
            0 => "🥇",
            1 => "🥈",
            2 => "🥉",
            _ => "•",
        };
        let line = if h.is_ass() {
            LOCALE.t_fmt("ru", "huya.top_entry_ass", &[
                ("medal", medal),
                ("name", &escape_html(&name)),
                ("size", &h.display_cm()),
            ])
        } else {
            LOCALE.t_fmt("ru", "huya.top_entry", &[
                ("medal", medal),
                ("name", &escape_html(&name)),
                ("size", &h.display_cm()),
                ("level", &h.level.to_string()),
            ])
        };
        text.push_str(&line);
    }

    bot.send_message(msg.chat.id, text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
    Ok(())
}


