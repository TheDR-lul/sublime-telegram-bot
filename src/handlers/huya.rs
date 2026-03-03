//! HuyActa tamagotchi game handlers.
//! Commands: /huya, /huyagrow, /huyafight @user, /huyasteal @user,
//!           /huyatop, /huyareg, /huyaskills, /huyashop

use sqlx::{self, PgPool};
use teloxide::prelude::*;
use teloxide::types::{CallbackQuery, InlineKeyboardButton, InlineKeyboardMarkup};
use teloxide::utils::html::escape as escape_html;

use crate::db::huya as huya_db;
use crate::db::models::Huya;
use crate::db::user;
use crate::error::AppError;
use crate::i18n::LOCALE;

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Convert millimetres to a "X.Y" cm string (e.g. 35 → "3.5").
fn mm_to_cm_str(mm: i32) -> String {
    let abs = mm.unsigned_abs();
    format!("{}.{}", abs / 10, abs % 10)
}

/// HP bar (10 chars) for an arbitrary current/max pair.
fn hp_bar_str(current: i32, max: i32) -> String {
    let max = max.max(1);
    let filled = ((current.max(0) as f64 / max as f64) * 10.0).round() as usize;
    let filled = filled.min(10);
    format!("[{}{}]", "█".repeat(filled), "░".repeat(10 - filled))
}

/// Build the status text for a player.
fn huya_status_text(h: &Huya, name: &str) -> String {
    let max_hp = h.max_hp();
    let base = if h.is_ass() {
        LOCALE.t_fmt("ru", "huya.status_ass", &[
            ("name",         &escape_html(name)),
            ("size",         &h.display_cm()),
            ("actions_left", &h.actions_left.to_string()),
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
            ("hp",           &h.hp.to_string()),
            ("max_hp",       &max_hp.to_string()),
            ("hp_bar",       &h.hp_bar()),
        ])
    };
    format!("{}\n\n{}", base, LOCALE.t("ru", "huya.hint"))
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

fn actions_available(h: &Huya) -> bool {
    h.actions_left > 0 || h.actions_reset_at < chrono::Utc::now().date_naive()
}

// ── Grow helpers ─────────────────────────────────────────────────────────────

/// Pick the correct locale key for a grow result.
fn grow_locale_key(leveled_up: bool, boost_active: bool) -> &'static str {
    match (leveled_up, boost_active) {
        (true,  true)  => "huya.grow_levelup_boosted",
        (true,  false) => "huya.grow_levelup",
        (false, true)  => "huya.grow_boosted",
        (false, false) => "huya.grow",
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

// ── Target resolution ─────────────────────────────────────────────────────────

enum TargetResult {
    User(i64),
    IsBot,
    NotFound,
}

async fn resolve_target(msg: &Message, pool: &PgPool, sub_arg: &str) -> TargetResult {
    if let Some(entities) = msg.entities() {
        for e in entities {
            if let teloxide::types::MessageEntityKind::TextMention { user } = &e.kind {
                return if user.is_bot { TargetResult::IsBot } else { TargetResult::User(user.id.0 as i64) };
            }
        }
    }
    let username = sub_arg.trim_start_matches('@');
    if !username.is_empty() {
        if let Ok(Some(u)) = user::get_by_username(pool, username).await {
            return TargetResult::User(u.tg_id);
        }
    }
    if let Some(reply) = msg.reply_to_message() {
        if let Some(ref from) = reply.from {
            return if from.is_bot { TargetResult::IsBot } else { TargetResult::User(from.id.0 as i64) };
        }
    }
    TargetResult::NotFound
}

// ── Main dispatcher ───────────────────────────────────────────────────────────

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
    let from = match msg.from.as_ref() { Some(f) => f, None => return Ok(()) };
    let tg_id = from.id.0 as i64;
    let name = from.username.as_ref()
        .map(|u| format!("@{}", u))
        .unwrap_or_else(|| from.first_name.clone());

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
        Cmd::Huya(s) => {
            let s = s.trim();
            let mut parts = s.splitn(2, ' ');
            let sub = parts.next().unwrap_or("stat");
            let arg = parts.next().unwrap_or("").trim();
            match sub {
                "grow"  => handle_grow(&bot, &pool, msg.chat.id, chat_id, tg_id, &name).await,
                "fight" => handle_fight(&bot, &pool, &msg, chat_id, tg_id, &name, arg).await,
                "steal" => handle_steal(&bot, &pool, &msg, chat_id, tg_id, &name, arg).await,
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
    bot.send_message(chat_id, huya_status_text(&h, name))
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
    let key = grow_locale_key(leveled_up, boost_active);
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
    let name = from.username.as_ref()
        .map(|u| format!("@{}", u))
        .unwrap_or_else(|| from.first_name.clone());

    let (h, _) = huya_db::get_or_create(&pool, chat_id.0, clicker).await?;

    if !huya_db::consume_action(&pool, &h).await? {
        let _ = bot.answer_callback_query(query.id)
            .text(LOCALE.t("ru", "huya.no_actions"))
            .await;
        return Ok(());
    }

    let (updated, grow_mm, xp_gain, leveled_up, boost_active) = huya_db::grow(&pool, &h).await?;
    let key = grow_locale_key(leveled_up, boost_active);
    let answer_text = LOCALE.t_fmt("ru", key, &[
        ("name",    &escape_html(&name)),
        ("grow_cm", &mm_to_cm_str(grow_mm)),
        ("xp",      &xp_gain.to_string()),
        ("level",   &updated.level.to_string()),
        ("size",    &updated.display_cm()),
    ]);

    let _ = bot.answer_callback_query(query.id)
        .text(format!("+{} см", mm_to_cm_str(grow_mm)))
        .await;

    let _ = bot.edit_message_text(msg_ref.chat().id, msg_ref.id(), huya_status_text(&updated, &name))
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

    if target_tg_id == attacker_tg_id {
        let (h, _) = huya_db::get_or_create(pool, chat_id_raw, attacker_tg_id).await?;
        if !huya_db::consume_action(pool, &h).await? {
            bot.send_message(chat_id, LOCALE.t("ru", "huya.no_actions")).await?;
            return Ok(());
        }
        let updated = huya_db::self_fight(pool, chat_id_raw, attacker_tg_id).await?;
        bot.send_message(chat_id, LOCALE.t_fmt("ru", "huya.fight_self", &[
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

    let decliner = query.from.username.as_ref()
        .map(|u| format!("@{}", u))
        .unwrap_or_else(|| query.from.first_name.clone());

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
        let picker_name = query.from.username.as_ref()
            .map(|u| format!("@{}", u))
            .unwrap_or_else(|| query.from.first_name.clone());

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

    if target_tg_id == attacker_tg_id {
        let (h, _) = huya_db::get_or_create(pool, chat_id_raw, attacker_tg_id).await?;
        if !huya_db::consume_action(pool, &h).await? {
            bot.send_message(chat_id, LOCALE.t("ru", "huya.no_actions")).await?;
            return Ok(());
        }
        let updated = huya_db::self_fight(pool, chat_id_raw, attacker_tg_id).await?;
        bot.send_message(chat_id, LOCALE.t_fmt("ru", "huya.steal_self", &[
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
    let name = from.username.as_ref()
        .map(|u| format!("@{}", u))
        .unwrap_or_else(|| from.first_name.clone());

    user::upsert_tg_user(&pool, from).await?;

    let (h, was_created) = huya_db::get_or_create(&pool, chat_id_raw, tg_id).await?;

    let text = if was_created {
        LOCALE.t_rand_fmt("ru", "huya.reg_messages", &[("name", &escape_html(&name))])
    } else {
        format!(
            "{}\n\n{}",
            LOCALE.t("ru", "huya.already_registered"),
            huya_status_text(&h, &name),
        )
    };

    let mut req = bot.send_message(msg.chat.id, text)
        .parse_mode(teloxide::types::ParseMode::Html);
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
        bot.send_message(msg.chat.id, LOCALE.t("ru", "huya.top_empty")).await?;
        return Ok(());
    }

    let mut text = LOCALE.t("ru", "huya.top_header").to_string();
    for (i, (h, tg_id)) in rows.iter().enumerate() {
        let name = user::get_by_tg_id(&pool, *tg_id).await?
            .map(|u| u.full_username(false))
            .unwrap_or_else(|| format!("user_{}", tg_id));
        let medal = match i { 0 => "🥇", 1 => "🥈", 2 => "🥉", _ => "•" };
        let line = if h.is_ass() {
            LOCALE.t_fmt("ru", "huya.top_entry_ass", &[
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

    bot.send_message(msg.chat.id, text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
    Ok(())
}

// ── Skills (/huyaskills) ──────────────────────────────────────────────────────

const CAP_T1: i32 = 20;
const CAP_T2: i32 = 15;
const CAP_T3: i32 = 10;
const CAP_T4: i32 = 5;
const CAP_T5: i32 = 3;

fn skill_lock_reason(h: &Huya, skill: &str) -> Option<String> {
    let s = match skill {
        "pierce" if h.skill_shaft < 8 => format!("{} 8 ({})", LOCALE.t("ru", "huya.skill_name_shaft"), h.skill_shaft),
        "scales" if h.skill_skin < 8 => format!("{} 8 ({})", LOCALE.t("ru", "huya.skill_name_skin"), h.skill_skin),
        "spirit" if h.skill_balls < 8 => format!("{} 8 ({})", LOCALE.t("ru", "huya.skill_name_balls"), h.skill_balls),
        "pickpocket" if h.skill_cunning < 8 => format!("{} 8 ({})", LOCALE.t("ru", "huya.skill_name_cunning"), h.skill_cunning),
        "dynamo" if h.skill_stamina < 8 => format!("{} 8 ({})", LOCALE.t("ru", "huya.skill_name_stamina"), h.skill_stamina),
        "eggtwist" if h.skill_pierce < 5 || h.skill_spirit < 5 => format!("{} 5 + {} 5", LOCALE.t("ru", "huya.skill_name_pierce"), LOCALE.t("ru", "huya.skill_name_spirit")),
        "bloodsucker" if h.skill_pierce < 5 || h.skill_pickpocket < 5 => format!("{} 5 + {} 5", LOCALE.t("ru", "huya.skill_name_pierce"), LOCALE.t("ru", "huya.skill_name_pickpocket")),
        "ironballs" if h.skill_scales < 5 || h.skill_spirit < 5 => format!("{} 5 + {} 5", LOCALE.t("ru", "huya.skill_name_scales"), LOCALE.t("ru", "huya.skill_name_spirit")),
        "vortex" if h.skill_spirit < 5 || h.skill_dynamo < 5 => format!("{} 5 + {} 5", LOCALE.t("ru", "huya.skill_name_spirit"), LOCALE.t("ru", "huya.skill_name_dynamo")),
        "phantom" if h.skill_pickpocket < 5 || h.skill_scales < 5 => format!("{} 5 + {} 5", LOCALE.t("ru", "huya.skill_name_pickpocket"), LOCALE.t("ru", "huya.skill_name_scales")),
        "berserker" if h.skill_eggtwist < 7 => format!("{} 7", LOCALE.t("ru", "huya.skill_name_eggtwist")),
        "vampire" if h.skill_bloodsucker < 7 => format!("{} 7", LOCALE.t("ru", "huya.skill_name_bloodsucker")),
        "fortress" if h.skill_ironballs < 7 => format!("{} 7", LOCALE.t("ru", "huya.skill_name_ironballs")),
        "speedrun" if h.skill_vortex < 7 => format!("{} 7", LOCALE.t("ru", "huya.skill_name_vortex")),
        "ghost" if h.skill_phantom < 7 => format!("{} 7", LOCALE.t("ru", "huya.skill_name_phantom")),
        "eternal" if h.skill_berserker < 3 || h.skill_fortress < 3 => format!("{} 3 + {} 3", LOCALE.t("ru", "huya.skill_name_berserker"), LOCALE.t("ru", "huya.skill_name_fortress")),
        "absolute" if h.skill_berserker < 1 || h.skill_vampire < 1 || h.skill_fortress < 1 || h.skill_speedrun < 1 || h.skill_ghost < 1 => LOCALE.t("ru", "huya.skill_lock_all_t4").to_string(),
        _ => return None,
    };
    Some(s)
}

fn skills_keyboard(huya: &Huya) -> InlineKeyboardMarkup {
    let has_pts = huya.skill_points > 0;
    let cost = |s: &str| match s {
        "shaft"|"skin"|"balls"|"cunning"|"stamina" => 1,
        "pierce"|"scales"|"spirit"|"pickpocket"|"dynamo" => 2,
        "eggtwist"|"bloodsucker"|"ironballs"|"vortex"|"phantom" => 3,
        "berserker"|"vampire"|"fortress"|"speedrun"|"ghost" => 5,
        "eternal"|"absolute" => 7,
        _ => 99,
    };
    let get_lc = |s: &str| -> (i32, i32) {
        let l = match s {
            "shaft" => huya.skill_shaft, "skin" => huya.skill_skin, "balls" => huya.skill_balls,
            "cunning" => huya.skill_cunning, "stamina" => huya.skill_stamina,
            "pierce" => huya.skill_pierce, "scales" => huya.skill_scales, "spirit" => huya.skill_spirit,
            "pickpocket" => huya.skill_pickpocket, "dynamo" => huya.skill_dynamo,
            "eggtwist" => huya.skill_eggtwist, "bloodsucker" => huya.skill_bloodsucker,
            "ironballs" => huya.skill_ironballs, "vortex" => huya.skill_vortex, "phantom" => huya.skill_phantom,
            "berserker" => huya.skill_berserker, "vampire" => huya.skill_vampire, "fortress" => huya.skill_fortress,
            "speedrun" => huya.skill_speedrun, "ghost" => huya.skill_ghost,
            "eternal" => huya.skill_eternal, "absolute" => huya.skill_absolute,
            _ => 0,
        };
        let c = match s {
            "shaft"|"skin"|"balls"|"cunning"|"stamina" => CAP_T1,
            "pierce"|"scales"|"spirit"|"pickpocket"|"dynamo" => CAP_T2,
            "eggtwist"|"bloodsucker"|"ironballs"|"vortex"|"phantom" => CAP_T3,
            "berserker"|"vampire"|"fortress"|"speedrun"|"ghost" => CAP_T4,
            "eternal"|"absolute" => CAP_T5,
            _ => 1,
        };
        (l, c)
    };

    let make_btn = |skill: &str, label: &str| {
        let (l, c) = get_lc(skill);
        let locked = skill_lock_reason(huya, skill);
        let can_up = locked.is_none() && l < c && has_pts && huya.skill_points >= cost(skill);
        let text = if let Some(reason) = locked {
            format!("🔒 {} ({})", label, reason)
        } else if !can_up {
            format!("{} {}/{}", label, l, c)
        } else {
            format!("{} +1", label)
        };
        InlineKeyboardButton::callback(text, format!("huya_skill:{}", skill))
    };

    let mut rows = vec![];

    // Tier 1
    rows.push(vec![
        make_btn("shaft", LOCALE.t("ru", "huya.skill_name_shaft")),
        make_btn("skin", LOCALE.t("ru", "huya.skill_name_skin")),
    ]);
    rows.push(vec![
        make_btn("balls", LOCALE.t("ru", "huya.skill_name_balls")),
        make_btn("cunning", LOCALE.t("ru", "huya.skill_name_cunning")),
        make_btn("stamina", LOCALE.t("ru", "huya.skill_name_stamina")),
    ]);

    // Tier 2
    rows.push(vec![
        make_btn("pierce", LOCALE.t("ru", "huya.skill_name_pierce")),
        make_btn("scales", LOCALE.t("ru", "huya.skill_name_scales")),
    ]);
    rows.push(vec![
        make_btn("spirit", LOCALE.t("ru", "huya.skill_name_spirit")),
        make_btn("pickpocket", LOCALE.t("ru", "huya.skill_name_pickpocket")),
        make_btn("dynamo", LOCALE.t("ru", "huya.skill_name_dynamo")),
    ]);

    // Tier 3
    rows.push(vec![
        make_btn("eggtwist", LOCALE.t("ru", "huya.skill_name_eggtwist")),
        make_btn("bloodsucker", LOCALE.t("ru", "huya.skill_name_bloodsucker")),
    ]);
    rows.push(vec![
        make_btn("ironballs", LOCALE.t("ru", "huya.skill_name_ironballs")),
        make_btn("vortex", LOCALE.t("ru", "huya.skill_name_vortex")),
        make_btn("phantom", LOCALE.t("ru", "huya.skill_name_phantom")),
    ]);

    // Tier 4 (hidden until T3>=7)
    let t4_skills = [
        ("berserker", "huya.skill_name_berserker", huya.t4_visible("berserker")),
        ("vampire", "huya.skill_name_vampire", huya.t4_visible("vampire")),
        ("fortress", "huya.skill_name_fortress", huya.t4_visible("fortress")),
        ("speedrun", "huya.skill_name_speedrun", huya.t4_visible("speedrun")),
        ("ghost", "huya.skill_name_ghost", huya.t4_visible("ghost")),
    ];
    let t4_row: Vec<_> = t4_skills.iter()
        .map(|(s, key, vis)| {
            let label: &str = if *vis { LOCALE.t("ru", key) } else { "???" };
            make_btn(s, label)
        })
        .collect();
    rows.push(t4_row);

    // Tier 5
    let t5_eternal_vis = huya.eternal_visible();
    let t5_abs_vis = huya.absolute_visible();
    rows.push(vec![
        make_btn("eternal", if t5_eternal_vis { LOCALE.t("ru", "huya.skill_name_eternal") } else { "⬛⬛⬛" }),
        make_btn("absolute", if t5_abs_vis { LOCALE.t("ru", "huya.skill_name_absolute") } else { "⬛⬛⬛" }),
    ]);

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

    out.push(LOCALE.t("ru", "huya.skills_tier1").to_string());
    out.push(line("shaft", h.skill_shaft, CAP_T1, &format!("+{}% ATK", h.skill_shaft * 4)));
    out.push(line("skin", h.skill_skin, CAP_T1, &format!("-{}% dmg", h.skill_skin * 3)));
    out.push(line("balls", h.skill_balls, CAP_T1, &format!("+{} maxHP", h.skill_balls * 15)));
    out.push(line("cunning", h.skill_cunning, CAP_T1, &format!("+{}% steal", (h.skill_cunning as f64 * 2.5) as i32)));
    out.push(line("stamina", h.skill_stamina, CAP_T1, &format!("+{} HP/act", h.skill_stamina * 3)));

    out.push(LOCALE.t("ru", "huya.skills_tier2").to_string());
    out.push(line("pierce", h.skill_pierce, CAP_T2, &format!("-{}% enemy DEF", h.skill_pierce * 4)));
    out.push(line("scales", h.skill_scales, CAP_T2, &format!("-{}% steal vs you", (h.skill_scales as f64 * 2.5) as i32)));
    out.push(line("spirit", h.skill_spirit, CAP_T2, "+12 HP/round win"));
    out.push(line("pickpocket", h.skill_pickpocket, CAP_T2, "steal XP"));
    out.push(line("dynamo", h.skill_dynamo, CAP_T2, &format!("+{} max act", h.skill_dynamo / 5)));

    out.push(LOCALE.t("ru", "huya.skills_tier3").to_string());
    out.push(line("eggtwist", h.skill_eggtwist, CAP_T3, "rnd3 x2 dmg"));
    out.push(line("bloodsucker", h.skill_bloodsucker, CAP_T3, "round win=steal"));
    out.push(line("ironballs", h.skill_ironballs, CAP_T3, "counter on dodge"));
    out.push(line("vortex", h.skill_vortex, CAP_T3, "rnd1 crit"));
    out.push(line("phantom", h.skill_phantom, CAP_T3, "1 steal at 0 act"));

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
    let name = from.username.as_ref()
        .map(|u| format!("@{}", u))
        .unwrap_or_else(|| from.first_name.clone());

    let (h, _) = huya_db::get_or_create(&pool, msg.chat.id.0, tg_id).await?;

    bot.send_message(msg.chat.id, skills_text(&h, &name))
        .parse_mode(teloxide::types::ParseMode::Html)
        .reply_markup(skills_keyboard(&h))
        .await?;
    Ok(())
}

/// Callback: huya_skill:{skill} — upgrade a skill.
pub async fn huya_skill_callback(
    bot: Bot,
    query: CallbackQuery,
    pool: PgPool,
) -> Result<(), AppError> {
    let data = query.data.as_deref().unwrap_or("");
    let skill = match data.strip_prefix("huya_skill:") {
        Some(s) => s,
        None => { let _ = bot.answer_callback_query(query.id).await; return Ok(()); }
    };

    let msg_ref = match query.message.as_ref() {
        Some(m) => m,
        None => { let _ = bot.answer_callback_query(query.id).await; return Ok(()); }
    };

    let clicker = query.from.id.0 as i64;
    let (h, _) = huya_db::get_or_create(&pool, msg_ref.chat().id.0, clicker).await?;

    let name = query.from.username.as_ref()
        .map(|u| format!("@{}", u))
        .unwrap_or_else(|| query.from.first_name.clone());

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

            // Edit message with updated skills display.
            let _ = bot.edit_message_text(msg_ref.chat().id, msg_ref.id(), skills_text(&updated, &name))
                .parse_mode(teloxide::types::ParseMode::Html)
                .reply_markup(skills_keyboard(&updated))
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

// ── Shop (/huyashop) ──────────────────────────────────────────────────────────

fn shop_keyboard(owner_tg_id: i64) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback(
                LOCALE.t("ru", "huya.buy_btn_potion").to_string(),
                format!("huya_buy:{}:potion", owner_tg_id),
            ),
            InlineKeyboardButton::callback(
                LOCALE.t("ru", "huya.buy_btn_adrenaline").to_string(),
                format!("huya_buy:{}:adrenaline", owner_tg_id),
            ),
        ],
        vec![
            InlineKeyboardButton::callback(
                LOCALE.t("ru", "huya.buy_btn_armor").to_string(),
                format!("huya_buy:{}:armor", owner_tg_id),
            ),
            InlineKeyboardButton::callback(
                LOCALE.t("ru", "huya.buy_btn_steroid").to_string(),
                format!("huya_buy:{}:steroid", owner_tg_id),
            ),
        ],
        vec![
            InlineKeyboardButton::callback(
                LOCALE.t("ru", "huya.buy_btn_energy").to_string(),
                format!("huya_buy:{}:energy", owner_tg_id),
            ),
        ],
    ])
}

pub async fn huyashop_handler(
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

    let text = LOCALE.t_fmt("ru", "huya.shop_header", &[("size", &h.display_cm())]);

    bot.send_message(msg.chat.id, text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .reply_markup(shop_keyboard(tg_id))
        .await?;
    Ok(())
}

/// Callback: huya_buy:{owner_tg_id}:{item_id} — purchase an item.
pub async fn huya_buy_callback(
    bot: Bot,
    query: CallbackQuery,
    pool: PgPool,
) -> Result<(), AppError> {
    let data = query.data.as_deref().unwrap_or("");
    let parts: Vec<&str> = data.splitn(3, ':').collect();
    // expected: ["huya_buy", tg_id, item_id]
    if parts.len() < 3 {
        let _ = bot.answer_callback_query(query.id).await;
        return Ok(());
    }
    let owner_tg_id: i64 = match parts[1].parse() {
        Ok(id) => id,
        Err(_) => { let _ = bot.answer_callback_query(query.id).await; return Ok(()); }
    };
    let item_id = parts[2];

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

    let cost = match huya_db::item_cost_mm(item_id) {
        Some(c) => c,
        None => {
            let _ = bot.answer_callback_query(query.id)
                .text(LOCALE.t("ru", "huya.buy_fail_unknown"))
                .await;
            return Ok(());
        }
    };

    if h.length_mm < cost {
        let toast = LOCALE.t_fmt("ru", "huya.buy_fail_broke", &[
            ("cost", &mm_to_cm_str(cost)),
            ("size", &h.display_cm()),
        ]);
        let _ = bot.answer_callback_query(query.id).text(&*toast).await;
        return Ok(());
    }

    match huya_db::buy_item(&pool, &h, item_id).await? {
        Some(updated) => {
            let toast_key = format!("huya.buy_success_{}", item_id);
            let toast = LOCALE.t_fmt("ru", &toast_key, &[
                ("hp",     &updated.hp.to_string()),
                ("max_hp", &updated.max_hp().to_string()),
            ]);
            let _ = bot.answer_callback_query(query.id).text(&*toast).await;

            // Refresh shop message with updated balance.
            let new_text = LOCALE.t_fmt("ru", "huya.shop_header", &[("size", &updated.display_cm())]);
            let _ = bot.edit_message_text(msg_ref.chat().id, msg_ref.id(), new_text)
                .parse_mode(teloxide::types::ParseMode::Html)
                .reply_markup(shop_keyboard(owner_tg_id))
                .await;
        }
        None => {
            // Race condition or amount mismatch.
            let toast = LOCALE.t_fmt("ru", "huya.buy_fail_broke", &[
                ("cost", &mm_to_cm_str(cost)),
                ("size", &h.display_cm()),
            ]);
            let _ = bot.answer_callback_query(query.id).text(&*toast).await;
        }
    }

    Ok(())
}
