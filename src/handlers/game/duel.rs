//! Pidor duel: challenge, accept, mini-games (tictactoe/dice/coin/rps), ELO, achievements.

use rand::RngExt;
use serde_json::json;
use sqlx::PgPool;
use teloxide::prelude::*;
use teloxide::types::{CallbackQuery, ChatId, InlineKeyboardButton, InlineKeyboardMarkup, Message};
use teloxide::utils::html::escape as escape_html;

use crate::db;
use crate::db::duel as duel_db;
use crate::db::models::DuelGame;
use crate::error::AppError;
use crate::i18n::LOCALE;
use crate::telegram::topic_routing::{send_text_in_origin_topic, topic_thread_id};
use crate::telegram::target_resolver::{resolve_target as resolve_target_global, ResolvedTarget};

fn name_from_tg_user(u: &teloxide::types::User) -> String {
    u.username
        .as_ref()
        .map(|s| format!("@{}", s))
        .unwrap_or_else(|| {
            u.last_name
                .as_ref()
                .map(|l| format!("{} {}", u.first_name, l))
                .unwrap_or_else(|| u.first_name.clone())
        })
}

async fn get_display_name(bot: &Bot, pool: &PgPool, chat_id: ChatId, tg_id: i64) -> String {
    match bot
        .get_chat_member(chat_id, teloxide::types::UserId(tg_id as u64))
        .await
    {
        Ok(m) => name_from_tg_user(&m.user),
        Err(_) => {
            if let Ok(Some(u)) = db::user::get_by_tg_id(pool, tg_id).await {
                u.full_username(true)
            } else {
                "???".to_string()
            }
        }
    }
}

fn duel_accept_keyboard(duel_id: i32, tagged: bool) -> InlineKeyboardMarkup {
    let accept_btn = InlineKeyboardButton::callback(
        LOCALE.t("ru", "duel.static.accept_btn").to_string(),
        format!("duel_accept:{}:yes", duel_id),
    );
    if tagged {
        let decline_btn = InlineKeyboardButton::callback(
            LOCALE.t("ru", "duel.static.decline_btn").to_string(),
            format!("duel_accept:{}:no", duel_id),
        );
        InlineKeyboardMarkup::new(vec![vec![accept_btn, decline_btn]])
    } else {
        InlineKeyboardMarkup::new(vec![vec![accept_btn]])
    }
}

fn board_keyboard(d: &DuelGame) -> InlineKeyboardMarkup {
    let board = d.board.chars().chain(std::iter::repeat(' ')).take(9).collect::<String>();
    let mut rows = Vec::new();
    for row in 0..3 {
        let mut row_btns = Vec::new();
        for col in 0..3 {
            let idx = row * 3 + col;
            let ch = board.chars().nth(idx).unwrap_or(' ');
            let (text, callback) = match ch {
                '1' => ("🍑".to_string(), format!("duel:{}:{}", d.id, idx)),
                '2' => ("🍆".to_string(), format!("duel:{}:{}", d.id, idx)),
                _ => ("·".to_string(), format!("duel:{}:{}", d.id, idx)),
            };
            row_btns.push(InlineKeyboardButton::callback(text, callback));
        }
        rows.push(row_btns);
    }
    InlineKeyboardMarkup::new(rows)
}

pub async fn pidorduel_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    if !msg.chat.is_group() && !msg.chat.is_supergroup() {
        return Ok(());
    }
    let chat_id = msg.chat.id.0;
    let from = match msg.from.as_ref() {
        Some(f) => f,
        None => {
            send_text_in_origin_topic(&bot, &msg, LOCALE.t("ru", "duel.static.anon_cant_duel"))
                .await?;
            return Ok(());
        }
    };
    let challenger_tg_id = from.id.0 as i64;
    let challenger_name = escape_html(&name_from_tg_user(from));

    let invited_tg_id = match resolve_target_global(&pool, &msg, "").await {
        ResolvedTarget::User(id) => Some(id),
        _ => msg
            .reply_to_message()
            .as_ref()
            .and_then(|r| r.from.as_ref())
            .map(|u| u.id.0 as i64),
    };
    if let Some(inv_id) = invited_tg_id {
        if inv_id == challenger_tg_id {
            send_text_in_origin_topic(&bot, &msg, LOCALE.t("ru", "duel.static.challenge_self"))
                .await?;
            return Ok(());
        }
    }

    if duel_db::get_pending_or_active_by_chat(&pool, chat_id).await?.is_some() {
        send_text_in_origin_topic(&bot, &msg, LOCALE.t("ru", "duel.static.duel_in_progress"))
            .await?;
        return Ok(());
    }

    let d = duel_db::create(
        &pool,
        chat_id,
        challenger_tg_id,
        invited_tg_id,
        None,
    )
    .await?;

    let (text, tagged) = if let Some(inv_id) = invited_tg_id {
        let invited_name = get_display_name(&bot, &pool, msg.chat.id, inv_id).await;
        let invited_name_esc = escape_html(&invited_name);
        (
            LOCALE.t_fmt("ru", "duel.static.invite_tagged", &[
                ("challenger", &challenger_name),
                ("invited", &invited_name_esc),
            ]),
            true,
        )
    } else {
        (
            LOCALE.t_fmt("ru", "duel.static.invite_open", &[("challenger", &challenger_name)]),
            false,
        )
    };

    let mut request = bot
        .send_message(msg.chat.id, text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .reply_markup(duel_accept_keyboard(d.id, tagged));
    if let Some(thread) = topic_thread_id(&msg) {
        request = request.message_thread_id(thread);
    }
    let sent = request.await?;
    duel_db::set_invite_message_id(&pool, d.id, sent.id.0 as i64).await?;
    Ok(())
}

pub async fn duel_accept_callback(
    bot: Bot,
    query: CallbackQuery,
    pool: PgPool,
) -> Result<(), AppError> {
    let data = query.data.as_deref().unwrap_or("");
    let parts: Vec<&str> = data.splitn(3, ':').collect();
    let (id, action) = match parts[..] {
        ["duel_accept", id, act] => (
            id.parse::<i32>().ok(),
            act,
        ),
        _ => (None, ""),
    };
    let duel_id = match id {
        Some(i) => i,
        None => {
            let _ = bot.answer_callback_query(query.id).await;
            return Ok(());
        }
    };
    let chat_id = match query.message.as_ref().map(|m| m.chat().id.0) {
        Some(c) => c,
        None => {
            let _ = bot.answer_callback_query(query.id).await;
            return Ok(());
        }
    };
    let accepter_tg_id = query.from.id.0 as i64;

    if action == "no" {
        let d = match duel_db::get_by_id(&pool, duel_id).await? {
            Some(x) => x,
            None => {
                let _ = bot.answer_callback_query(query.id).await;
                return Ok(());
            }
        };
        if d.status != "pending_accept" {
            let _ = bot.answer_callback_query(query.id).await;
            return Ok(());
        }
        if d.invited_tg_id != Some(accepter_tg_id) {
            let _ = bot.answer_callback_query(query.id).text(LOCALE.t("ru", "duel.static.only_invited_decline")).await;
            return Ok(());
        }
        duel_db::decline(&pool, duel_id).await?;
        let _ = bot.answer_callback_query(query.id).await;
        let decliner_name = get_display_name(&bot, &pool, ChatId(chat_id), accepter_tg_id).await;
        let text = LOCALE.t_fmt("ru", "duel.static.declined", &[("name", &escape_html(&decliner_name))]);
        if let Some(ref msg) = query.message {
            bot.edit_message_text(msg.chat().id, msg.id(), text)
                .parse_mode(teloxide::types::ParseMode::Html)
                .await?;
        }
        return Ok(());
    }

    if action != "yes" {
        let _ = bot.answer_callback_query(query.id).await;
        return Ok(());
    }

    let d = match duel_db::accept(&pool, duel_id, accepter_tg_id).await? {
        Some(x) => x,
        None => {
            let _ = bot
                .answer_callback_query(query.id)
                .text(LOCALE.t("ru", "duel.static.expired_or_invalid"))
                .await;
            if let Ok(Some(d)) = duel_db::get_by_id(&pool, duel_id).await {
                if d.status == "cancelled" {
                    if let Some(mid) = d.invite_message_id {
                        if let Some(ref msg) = query.message {
                            if msg.id().0 as i64 == mid {
                                let _ = bot
                                    .edit_message_text(msg.chat().id, msg.id(), LOCALE.t("ru", "duel.static.expired"))
                                    .await;
                            }
                            let _ = bot.edit_message_reply_markup(msg.chat().id, msg.id()).await;
                        }
                    }
                }
            }
            return Ok(());
        }
    };

    let _ = bot.answer_callback_query(query.id).await;

    // Route to the appropriate mini-game based on game_type.
    let sent = match d.game_type.as_str() {
        "dice" => {
            let p1_name = get_display_name(&bot, &pool, ChatId(chat_id), d.player1_tg_id.unwrap()).await;
            let p2_name = get_display_name(&bot, &pool, ChatId(chat_id), d.player2_tg_id.unwrap()).await;
            let text = LOCALE.t("ru", "duel.minigame.dice_start");
            bot.send_message(ChatId(chat_id), text)
                .parse_mode(teloxide::types::ParseMode::Html)
                .reply_markup(dice_keyboard(d.id, d.player1_tg_id.unwrap(), d.player2_tg_id.unwrap(), &p1_name, &p2_name, None, None))
                .await?
        }
        "coin" => {
            let challenger_name = get_display_name(&bot, &pool, ChatId(chat_id), d.challenger_tg_id).await;
            let text = LOCALE.t_fmt("ru", "duel.minigame.coin_start", &[("challenger", &escape_html(&challenger_name))]);
            bot.send_message(ChatId(chat_id), text)
                .parse_mode(teloxide::types::ParseMode::Html)
                .reply_markup(coin_keyboard(d.id))
                .await?
        }
        "rps" => {
            let text = LOCALE.t("ru", "duel.minigame.rps_start");
            bot.send_message(ChatId(chat_id), text)
                .parse_mode(teloxide::types::ParseMode::Html)
                .reply_markup(rps_keyboard(d.id))
                .await?
        }
        _ => {
            // Default: tic-tac-toe
            let player1_name = get_display_name(&bot, &pool, ChatId(chat_id), d.player1_tg_id.unwrap()).await;
            let caption = LOCALE.t_fmt("ru", "duel.static.board_turn", &[("name", &escape_html(&player1_name)), ("emoji", "🍑")]);
            bot.send_message(ChatId(chat_id), caption)
                .parse_mode(teloxide::types::ParseMode::Html)
                .reply_markup(board_keyboard(&d))
                .await?
        }
    };
    duel_db::set_message_id(&pool, duel_id, sent.id.0 as i64).await?;
    if let Some(ref msg) = query.message {
        let _ = bot.edit_message_reply_markup(msg.chat().id, msg.id()).await;
    }
    Ok(())
}

// ── Dice mini-game ────────────────────────────────────────────────────────────

fn dice_keyboard(
    duel_id: i32,
    p1_tg_id: i64,
    p2_tg_id: i64,
    p1_name: &str,
    p2_name: &str,
    p1_roll: Option<i32>,
    p2_roll: Option<i32>,
) -> InlineKeyboardMarkup {
    let mut btns = Vec::new();
    if p1_roll.is_none() {
        btns.push(InlineKeyboardButton::callback(
            format!("🎲 {}", escape_html(p1_name)),
            format!("duel_dice:{}:{}", duel_id, p1_tg_id),
        ));
    }
    if p2_roll.is_none() {
        btns.push(InlineKeyboardButton::callback(
            format!("🎲 {}", escape_html(p2_name)),
            format!("duel_dice:{}:{}", duel_id, p2_tg_id),
        ));
    }
    InlineKeyboardMarkup::new(vec![btns])
}

pub async fn duel_dice_callback(
    bot: Bot,
    query: CallbackQuery,
    pool: PgPool,
) -> Result<(), AppError> {
    let data = query.data.as_deref().unwrap_or("");
    let parts: Vec<&str> = data.splitn(3, ':').collect();
    if parts.len() < 3 || parts[0] != "duel_dice" {
        let _ = bot.answer_callback_query(query.id).await;
        return Ok(());
    }
    let duel_id = match parts[1].parse::<i32>() {
        Ok(i) => i,
        Err(_) => { let _ = bot.answer_callback_query(query.id).await; return Ok(()); }
    };
    let btn_tg_id = match parts[2].parse::<i64>() {
        Ok(i) => i,
        Err(_) => { let _ = bot.answer_callback_query(query.id).await; return Ok(()); }
    };

    let clicker_tg_id = query.from.id.0 as i64;
    if clicker_tg_id != btn_tg_id {
        let _ = bot.answer_callback_query(query.id).text(LOCALE.t("ru", "duel.static.not_your_turn")).await;
        return Ok(());
    }

    let chat_id = match query.message.as_ref().map(|m| m.chat().id.0) {
        Some(c) => c,
        None => { let _ = bot.answer_callback_query(query.id).await; return Ok(()); }
    };

    let d = match duel_db::get_by_id(&pool, duel_id).await? {
        Some(g) if g.status == "active" && g.game_type == "dice" => g,
        _ => { let _ = bot.answer_callback_query(query.id).await; return Ok(()); }
    };

    let state = d.game_state.clone().unwrap_or_else(|| json!({}));
    let p1_roll: Option<i32> = state.get("p1_roll").and_then(|v| v.as_i64()).map(|v| v as i32);
    let p2_roll: Option<i32> = state.get("p2_roll").and_then(|v| v.as_i64()).map(|v| v as i32);

    let is_p1 = d.player1_tg_id == Some(clicker_tg_id);
    let is_p2 = d.player2_tg_id == Some(clicker_tg_id);

    if !is_p1 && !is_p2 {
        let _ = bot.answer_callback_query(query.id).text(LOCALE.t("ru", "duel.static.not_your_turn")).await;
        return Ok(());
    }
    // Already rolled?
    if is_p1 && p1_roll.is_some() || is_p2 && p2_roll.is_some() {
        let _ = bot.answer_callback_query(query.id).text("Ты уже бросил!").await;
        return Ok(());
    }

    // Scope rng so ThreadRng is dropped before any await.
    let roll: i32 = { let mut rng = rand::rng(); rng.random_range(1..=6) + rng.random_range(1..=6) };

    let new_p1 = if is_p1 { Some(roll) } else { p1_roll };
    let new_p2 = if is_p2 { Some(roll) } else { p2_roll };

    let new_state = json!({"p1_roll": new_p1, "p2_roll": new_p2});
    duel_db::set_game_state(&pool, duel_id, new_state).await?;

    let _ = bot.answer_callback_query(query.id).text(format!("Ты выбросил {}!", roll)).await;

    let p1_name = get_display_name(&bot, &pool, ChatId(chat_id), d.player1_tg_id.unwrap()).await;
    let p2_name = get_display_name(&bot, &pool, ChatId(chat_id), d.player2_tg_id.unwrap()).await;

    // Check if both rolled
    if let (Some(r1), Some(r2)) = (new_p1, new_p2) {
        // Determine winner (re-roll on tie automatically — no awaits in this block).
        let (winner_tg_id, loser_tg_id, final_r1, final_r2) = if r1 != r2 {
            if r1 > r2 {
                (d.player1_tg_id.unwrap(), d.player2_tg_id.unwrap(), r1, r2)
            } else {
                (d.player2_tg_id.unwrap(), d.player1_tg_id.unwrap(), r1, r2)
            }
        } else {
            // Tie: bot re-rolls automatically (all in sync scope, no await).
            loop {
                let (nr1, nr2): (i32, i32) = {
                    let mut rng = rand::rng();
                    (rng.random_range(1..=6) + rng.random_range(1..=6),
                     rng.random_range(1..=6) + rng.random_range(1..=6))
                };
                if nr1 != nr2 {
                    if nr1 > nr2 {
                        break (d.player1_tg_id.unwrap(), d.player2_tg_id.unwrap(), nr1, nr2);
                    } else {
                        break (d.player2_tg_id.unwrap(), d.player1_tg_id.unwrap(), nr1, nr2);
                    }
                }
            }
        };

        let winner_name = if winner_tg_id == d.player1_tg_id.unwrap() { &p1_name } else { &p2_name };

        let result_text = LOCALE.t_fmt("ru", "duel.minigame.dice_result", &[
            ("p1", &escape_html(&p1_name)),
            ("r1", &final_r1.to_string()),
            ("p2", &escape_html(&p2_name)),
            ("r2", &final_r2.to_string()),
            ("winner", &escape_html(winner_name)),
        ]);

        if let Some(ref msg) = query.message {
            let _ = bot.edit_message_text(msg.chat().id, msg.id(), result_text)
                .parse_mode(teloxide::types::ParseMode::Html)
                .reply_markup(InlineKeyboardMarkup::new::<Vec<Vec<InlineKeyboardButton>>>(vec![]))
                .await;
        }

        finish_duel(&bot, &pool, ChatId(chat_id), duel_id, winner_tg_id, loser_tg_id).await?;
        return Ok(());
    }

    // One player rolled, waiting for the other
    let waiting_text = if is_p1 {
        LOCALE.t_fmt("ru", "duel.minigame.dice_waiting", &[
            ("name", &escape_html(&p1_name)),
            ("result", &roll.to_string()),
            ("other", &escape_html(&p2_name)),
        ])
    } else {
        LOCALE.t_fmt("ru", "duel.minigame.dice_waiting", &[
            ("name", &escape_html(&p2_name)),
            ("result", &roll.to_string()),
            ("other", &escape_html(&p1_name)),
        ])
    };

    if let Some(ref msg) = query.message {
        let _ = bot.edit_message_text(msg.chat().id, msg.id(), waiting_text)
            .parse_mode(teloxide::types::ParseMode::Html)
            .reply_markup(dice_keyboard(duel_id, d.player1_tg_id.unwrap(), d.player2_tg_id.unwrap(), &p1_name, &p2_name, new_p1, new_p2))
            .await;
    }
    Ok(())
}

// ── Coin flip mini-game ───────────────────────────────────────────────────────

fn coin_keyboard(duel_id: i32) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::callback(LOCALE.t("ru", "duel.minigame.coin_heads"), format!("duel_coin:{}:heads", duel_id)),
        InlineKeyboardButton::callback(LOCALE.t("ru", "duel.minigame.coin_tails"), format!("duel_coin:{}:tails", duel_id)),
    ]])
}

pub async fn duel_coin_callback(
    bot: Bot,
    query: CallbackQuery,
    pool: PgPool,
) -> Result<(), AppError> {
    let data = query.data.as_deref().unwrap_or("");
    let parts: Vec<&str> = data.splitn(3, ':').collect();
    if parts.len() < 3 || parts[0] != "duel_coin" {
        let _ = bot.answer_callback_query(query.id).await;
        return Ok(());
    }
    let duel_id = match parts[1].parse::<i32>() {
        Ok(i) => i,
        Err(_) => { let _ = bot.answer_callback_query(query.id).await; return Ok(()); }
    };
    let pick = parts[2];
    if pick != "heads" && pick != "tails" {
        let _ = bot.answer_callback_query(query.id).await;
        return Ok(());
    }

    let clicker_tg_id = query.from.id.0 as i64;
    let chat_id = match query.message.as_ref().map(|m| m.chat().id.0) {
        Some(c) => c,
        None => { let _ = bot.answer_callback_query(query.id).await; return Ok(()); }
    };

    let d = match duel_db::get_by_id(&pool, duel_id).await? {
        Some(g) if g.status == "active" && g.game_type == "coin" => g,
        _ => { let _ = bot.answer_callback_query(query.id).await; return Ok(()); }
    };

    // Only challenger picks
    if clicker_tg_id != d.challenger_tg_id {
        let _ = bot.answer_callback_query(query.id).text(LOCALE.t("ru", "duel.minigame.coin_only_challenger")).await;
        return Ok(());
    }

    let _ = bot.answer_callback_query(query.id).await;

    // Scope rng so ThreadRng is dropped before any await.
    let (coin_result, coin_emoji) = {
        let mut rng = rand::rng();
        if rng.random::<bool>() { ("heads", "🦅 Орёл") } else { ("tails", "🦔 Решка") }
    };

    let challenger_name = get_display_name(&bot, &pool, ChatId(chat_id), d.challenger_tg_id).await;
    let defender_tg_id = if d.player1_tg_id == Some(d.challenger_tg_id) {
        d.player2_tg_id.unwrap()
    } else {
        d.player1_tg_id.unwrap()
    };
    let defender_name = get_display_name(&bot, &pool, ChatId(chat_id), defender_tg_id).await;

    let (winner_tg_id, loser_tg_id, result_key) = if pick == coin_result {
        (d.challenger_tg_id, defender_tg_id, "duel.minigame.coin_result_win")
    } else {
        (defender_tg_id, d.challenger_tg_id, "duel.minigame.coin_result_loss")
    };

    let result_text = LOCALE.t_fmt("ru", result_key, &[
        ("result", coin_emoji),
        ("challenger", &escape_html(&challenger_name)),
        ("defender", &escape_html(&defender_name)),
    ]);

    if let Some(ref msg) = query.message {
        let _ = bot.edit_message_text(msg.chat().id, msg.id(), result_text)
            .parse_mode(teloxide::types::ParseMode::Html)
            .reply_markup(InlineKeyboardMarkup::new::<Vec<Vec<InlineKeyboardButton>>>(vec![]))
            .await;
    }

    finish_duel(&bot, &pool, ChatId(chat_id), duel_id, winner_tg_id, loser_tg_id).await?;
    Ok(())
}

// ── Pidor-RPS mini-game ───────────────────────────────────────────────────────
// Rules: 🍆 Dick beats 🍑 Ass, 🍑 Ass beats 💦 Lube, 💦 Lube beats 🍆 Dick

fn rps_keyboard(duel_id: i32) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::callback("🍆", format!("duel_rps:{}:dick", duel_id)),
        InlineKeyboardButton::callback("🍑", format!("duel_rps:{}:ass", duel_id)),
        InlineKeyboardButton::callback("💦", format!("duel_rps:{}:lube", duel_id)),
    ]])
}

fn rps_beats(a: &str, b: &str) -> Option<bool> {
    match (a, b) {
        ("dick", "ass") | ("ass", "lube") | ("lube", "dick") => Some(true),
        ("ass", "dick") | ("lube", "ass") | ("dick", "lube") => Some(false),
        _ => None, // tie
    }
}

fn rps_emoji(pick: &str) -> &'static str {
    match pick {
        "dick" => "🍆",
        "ass" => "🍑",
        "lube" => "💦",
        _ => "?",
    }
}

pub async fn duel_rps_callback(
    bot: Bot,
    query: CallbackQuery,
    pool: PgPool,
) -> Result<(), AppError> {
    let data = query.data.as_deref().unwrap_or("");
    let parts: Vec<&str> = data.splitn(3, ':').collect();
    if parts.len() < 3 || parts[0] != "duel_rps" {
        let _ = bot.answer_callback_query(query.id).await;
        return Ok(());
    }
    let duel_id = match parts[1].parse::<i32>() {
        Ok(i) => i,
        Err(_) => { let _ = bot.answer_callback_query(query.id).await; return Ok(()); }
    };
    let pick = parts[2];
    if !["dick", "ass", "lube"].contains(&pick) {
        let _ = bot.answer_callback_query(query.id).await;
        return Ok(());
    }

    let clicker_tg_id = query.from.id.0 as i64;
    let chat_id = match query.message.as_ref().map(|m| m.chat().id.0) {
        Some(c) => c,
        None => { let _ = bot.answer_callback_query(query.id).await; return Ok(()); }
    };

    let d = match duel_db::get_by_id(&pool, duel_id).await? {
        Some(g) if g.status == "active" && g.game_type == "rps" => g,
        _ => { let _ = bot.answer_callback_query(query.id).await; return Ok(()); }
    };

    let is_p1 = d.player1_tg_id == Some(clicker_tg_id);
    let is_p2 = d.player2_tg_id == Some(clicker_tg_id);
    if !is_p1 && !is_p2 {
        let _ = bot.answer_callback_query(query.id).text(LOCALE.t("ru", "duel.static.not_your_turn")).await;
        return Ok(());
    }

    let state = d.game_state.clone().unwrap_or_else(|| json!({}));
    let p1_pick: Option<String> = state.get("p1_pick").and_then(|v| v.as_str()).map(|s| s.to_string());
    let p2_pick: Option<String> = state.get("p2_pick").and_then(|v| v.as_str()).map(|s| s.to_string());

    // Already picked?
    if is_p1 && p1_pick.is_some() || is_p2 && p2_pick.is_some() {
        let _ = bot.answer_callback_query(query.id).text("Ты уже выбрал!").await;
        return Ok(());
    }

    let new_p1 = if is_p1 { Some(pick.to_string()) } else { p1_pick.clone() };
    let new_p2 = if is_p2 { Some(pick.to_string()) } else { p2_pick.clone() };
    let new_state = json!({"p1_pick": new_p1, "p2_pick": new_p2});
    duel_db::set_game_state(&pool, duel_id, new_state).await?;

    let _ = bot.answer_callback_query(query.id).text(format!("Выбрал {}!", rps_emoji(pick))).await;

    let p1_name = get_display_name(&bot, &pool, ChatId(chat_id), d.player1_tg_id.unwrap()).await;
    let p2_name = get_display_name(&bot, &pool, ChatId(chat_id), d.player2_tg_id.unwrap()).await;

    // If both picked, resolve
    if let (Some(ref w1), Some(ref w2)) = (new_p1.as_ref(), new_p2.as_ref()) {
        match rps_beats(w1, w2) {
            Some(true) => {
                // p1 wins
                let text = LOCALE.t_fmt("ru", "duel.minigame.rps_result", &[
                    ("p1", &escape_html(&p1_name)), ("w1", rps_emoji(w1)),
                    ("p2", &escape_html(&p2_name)), ("w2", rps_emoji(w2)),
                    ("winner", &escape_html(&p1_name)),
                ]);
                if let Some(ref msg) = query.message {
                    let _ = bot.edit_message_text(msg.chat().id, msg.id(), text)
                        .parse_mode(teloxide::types::ParseMode::Html)
                        .reply_markup(InlineKeyboardMarkup::new::<Vec<Vec<InlineKeyboardButton>>>(vec![]))
                        .await;
                }
                finish_duel(&bot, &pool, ChatId(chat_id), duel_id, d.player1_tg_id.unwrap(), d.player2_tg_id.unwrap()).await?;
            }
            Some(false) => {
                // p2 wins
                let text = LOCALE.t_fmt("ru", "duel.minigame.rps_result", &[
                    ("p1", &escape_html(&p1_name)), ("w1", rps_emoji(w1)),
                    ("p2", &escape_html(&p2_name)), ("w2", rps_emoji(w2)),
                    ("winner", &escape_html(&p2_name)),
                ]);
                if let Some(ref msg) = query.message {
                    let _ = bot.edit_message_text(msg.chat().id, msg.id(), text)
                        .parse_mode(teloxide::types::ParseMode::Html)
                        .reply_markup(InlineKeyboardMarkup::new::<Vec<Vec<InlineKeyboardButton>>>(vec![]))
                        .await;
                }
                finish_duel(&bot, &pool, ChatId(chat_id), duel_id, d.player2_tg_id.unwrap(), d.player1_tg_id.unwrap()).await?;
            }
            None => {
                // Tie: reset state, show "re-pick" message
                duel_db::set_game_state(&pool, duel_id, json!({})).await?;
                let tie_text = format!("{}\n{}", LOCALE.t_fmt("ru", "duel.minigame.rps_result", &[
                    ("p1", &escape_html(&p1_name)), ("w1", rps_emoji(w1)),
                    ("p2", &escape_html(&p2_name)), ("w2", rps_emoji(w2)),
                    ("winner", &LOCALE.t("ru", "duel.minigame.rps_tie")),
                ]), LOCALE.t("ru", "duel.minigame.rps_start"));
                if let Some(ref msg) = query.message {
                    let _ = bot.edit_message_text(msg.chat().id, msg.id(), tie_text)
                        .parse_mode(teloxide::types::ParseMode::Html)
                        .reply_markup(rps_keyboard(duel_id))
                        .await;
                }
            }
        }
        return Ok(());
    }

    // One player picked, waiting
    let waiting_name = if is_p1 { &p1_name } else { &p2_name };
    let other_name = if is_p1 { &p2_name } else { &p1_name };
    let waiting_text = LOCALE.t_fmt("ru", "duel.minigame.rps_picked", &[
        ("name", &escape_html(waiting_name)),
        ("other", &escape_html(other_name)),
    ]);
    if let Some(ref msg) = query.message {
        let _ = bot.edit_message_text(msg.chat().id, msg.id(), waiting_text)
            .parse_mode(teloxide::types::ParseMode::Html)
            .reply_markup(rps_keyboard(duel_id))
            .await;
    }
    Ok(())
}

// ── Finish duel helper ────────────────────────────────────────────────────────

/// Called after any mini-game finishes. Updates ELO, grants achievements, sends ELO message.
async fn finish_duel(
    bot: &Bot,
    pool: &PgPool,
    chat_id: ChatId,
    duel_id: i32,
    winner_tg_id: i64,
    loser_tg_id: i64,
) -> Result<(), AppError> {
    // Mark duel as finished
    sqlx::query("UPDATE duel_game SET status = 'finished', winner_tg_id = $1 WHERE id = $2")
        .bind(winner_tg_id)
        .bind(duel_id)
        .execute(pool)
        .await?;

    let winner_name = get_display_name(bot, pool, chat_id, winner_tg_id).await;
    let loser_name = get_display_name(bot, pool, chat_id, loser_tg_id).await;
    let winner_esc = escape_html(&winner_name);
    let loser_esc = escape_html(&loser_name);

    let victory_text = LOCALE.t_rand_fmt("ru", "duel.victory", &[("winner", &winner_esc), ("loser", &loser_esc)]);
    let roast_text = LOCALE.t_rand_fmt("ru", "duel.roasts", &[("username", &loser_esc)]);
    let _ = bot.send_message(chat_id, format!("{}\n\n{}", victory_text, roast_text))
        .parse_mode(teloxide::types::ParseMode::Html)
        .await;

    let elo_result = duel_db::update_elo_after_duel(pool, chat_id.0, winner_tg_id, loser_tg_id).await;
    grant_duel_achievements(bot, pool, chat_id, winner_tg_id, loser_tg_id, &elo_result).await?;
    Ok(())
}

pub async fn duel_move_callback(
    bot: Bot,
    query: CallbackQuery,
    pool: PgPool,
) -> Result<(), AppError> {
    let data = query.data.as_deref().unwrap_or("");
    let parts: Vec<&str> = data.splitn(3, ':').collect();
    let (id, cell_str) = match parts[..] {
        ["duel", id, cell] => (id.parse::<i32>().ok(), cell.parse::<usize>().ok()),
        _ => (None, None),
    };
    let duel_id = match id {
        Some(i) => i,
        None => {
            let _ = bot.answer_callback_query(query.id).await;
            return Ok(());
        }
    };
    let cell = match cell_str {
        Some(c) if c < 9 => c,
        _ => {
            let _ = bot.answer_callback_query(query.id).await;
            return Ok(());
        }
    };
    let chat_id = match query.message.as_ref().map(|m| m.chat().id.0) {
        Some(c) => c,
        None => {
            let _ = bot.answer_callback_query(query.id).await;
            return Ok(());
        }
    };
    let player_tg_id = query.from.id.0 as i64;

    // Ignore clicks from non-current player: do not call make_move so DB is never touched.
    let d = match duel_db::get_by_id(&pool, duel_id).await? {
        Some(g) => g,
        None => {
            let _ = bot.answer_callback_query(query.id).await;
            return Ok(());
        }
    };
    if d.status != "active" {
        let _ = bot.answer_callback_query(query.id).await;
        return Ok(());
    }
    let current_tg_id = if d.turn == 1 {
        d.player1_tg_id
    } else {
        d.player2_tg_id
    };
    if current_tg_id != Some(player_tg_id) {
        let _ = bot
            .answer_callback_query(query.id)
            .text(LOCALE.t("ru", "duel.static.not_your_turn"))
            .show_alert(false)
            .await;
        return Ok(());
    }

    let result = duel_db::make_move(&pool, duel_id, cell, player_tg_id).await;
    let (d, winner_tg_id) = match result {
        Ok((game, w)) => (game, w),
        Err(e) => {
            let msg = match e {
                AppError::GameLogic(ref s) if s.contains("not your turn") => LOCALE.t("ru", "duel.static.not_your_turn"),
                AppError::GameLogic(ref s) if s.contains("occupied") => LOCALE.t("ru", "duel.static.cell_occupied"),
                _ => LOCALE.t("ru", "duel.static.cant_move"),
            };
            let _ = bot.answer_callback_query(query.id).text(msg).await;
            return Ok(());
        }
    };

    let _ = bot.answer_callback_query(query.id).await;

    if let Some(winner_tg_id) = winner_tg_id {
        let loser_tg_id = if d.player1_tg_id == Some(winner_tg_id) {
            d.player2_tg_id.unwrap()
        } else {
            d.player1_tg_id.unwrap()
        };

        if let Some(ref msg) = query.message {
            bot.edit_message_reply_markup(msg.chat().id, msg.id())
                .reply_markup(InlineKeyboardMarkup::new::<Vec<Vec<InlineKeyboardButton>>>(vec![]))
                .await?;
        }

        finish_duel(&bot, &pool, ChatId(chat_id), duel_id, winner_tg_id, loser_tg_id).await?;
        return Ok(());
    }

    let current_tg_id = if d.turn == 1 {
        d.player1_tg_id
    } else {
        d.player2_tg_id
    };
    let current_name = match current_tg_id {
        Some(id) => get_display_name(&bot, &pool, ChatId(chat_id), id).await,
        None => "???".to_string(),
    };
    let emoji = if d.turn == 1 { "🍑" } else { "🍆" };
    let caption = LOCALE.t_fmt("ru", "duel.static.board_turn", &[("name", &escape_html(&current_name)), ("emoji", emoji)]);
    if let Some(ref msg) = query.message {
        bot.edit_message_text(msg.chat().id, msg.id(), caption)
            .parse_mode(teloxide::types::ParseMode::Html)
            .reply_markup(board_keyboard(&d))
            .await?;
    }
    Ok(())
}

async fn grant_duel_achievements(
    bot: &Bot,
    pool: &PgPool,
    chat_id: ChatId,
    winner_tg_id: i64,
    loser_tg_id: i64,
    elo_result: &Result<(crate::db::models::DuelElo, crate::db::models::DuelElo), AppError>,
) -> Result<(), AppError> {
    async fn do_grant(
        bot: &Bot,
        pool: &PgPool,
        chat_id: ChatId,
        user_id: i32,
        code: &str,
    ) -> Result<(), AppError> {
        if db::achievements::grant(pool, user_id, code).await? {
            let notif_key = format!("achievements.notifications.{}", code);
            let text = LOCALE.t_opt("ru", &notif_key)
                .unwrap_or("🏅 Новая ачивка!")
                .to_owned();
            let _ = bot.send_message(chat_id, text).await;
        }
        Ok(())
    }

    let winner_uid_opt = duel_db::user_id_by_tg_id(pool, winner_tg_id).await?;
    let loser_uid_opt = duel_db::user_id_by_tg_id(pool, loser_tg_id).await?;

    if let Some(winner_uid) = winner_uid_opt {
        let wins = duel_db::count_wins(pool, winner_tg_id).await?;
        if wins >= 1 {
            let _ = do_grant(bot, pool, chat_id, winner_uid, "duel_first_win").await;
        }
        if wins >= 5 {
            let _ = do_grant(bot, pool, chat_id, winner_uid, "duel_won_5").await;
        }
        if wins >= 10 {
            let _ = do_grant(bot, pool, chat_id, winner_uid, "duel_won_10").await;
        }
    }
    if let Some(loser_uid) = loser_uid_opt {
        let losses = duel_db::count_losses(pool, loser_tg_id).await?;
        if losses >= 1 {
            let _ = do_grant(bot, pool, chat_id, loser_uid, "duel_first_loss").await;
        }
        if losses >= 3 {
            let _ = do_grant(bot, pool, chat_id, loser_uid, "duel_lost_3").await;
        }
        if losses >= 5 {
            let _ = do_grant(bot, pool, chat_id, loser_uid, "duel_lost_5").await;
        }
        if losses >= 10 {
            let _ = do_grant(bot, pool, chat_id, loser_uid, "duel_lost_10").await;
        }
        let played = duel_db::count_played(pool, loser_tg_id).await?;
        if played >= 1 {
            let _ = do_grant(bot, pool, chat_id, loser_uid, "duel_played_1").await;
        }
    }
    if let Some(winner_uid) = winner_uid_opt {
        let played = duel_db::count_played(pool, winner_tg_id).await?;
        if played >= 1 {
            let _ = do_grant(bot, pool, chat_id, winner_uid, "duel_played_1").await;
        }
    }

    if let Ok((w_elo, _)) = elo_result {
        if let Some(winner_uid) = winner_uid_opt {
            let total = w_elo.total_elo();
            if total >= 1200 {
                let _ = do_grant(bot, pool, chat_id, winner_uid, "elo_gold").await;
            }
            if total >= 1600 {
                let _ = do_grant(bot, pool, chat_id, winner_uid, "elo_diamond").await;
            }
            if total >= 2000 {
                let _ = do_grant(bot, pool, chat_id, winner_uid, "elo_grandmaster").await;
            }
        }
    }
    Ok(())
}

pub async fn duelstats_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    let chat_id = msg.chat.id.0;
    let leaderboard = duel_db::get_duel_leaderboard(&pool, chat_id, 10).await?;
    if leaderboard.is_empty() {
        send_text_in_origin_topic(&bot, &msg, LOCALE.t("ru", "duel.stats.no_duels")).await?;
        return Ok(());
    }

    let mut text = LOCALE.t("ru", "duel.stats.header").to_string();
    for (i, entry) in leaderboard.iter().enumerate() {
        let name = get_display_name(&bot, &pool, msg.chat.id, entry.tg_id).await;
        let total = entry.total_elo();
        let rank = duel_db::elo_rank(total);
        let medal = match i {
            0 => "🥇",
            1 => "🥈",
            2 => "🥉",
            _ => "•",
        };
        text.push_str(&LOCALE.t_fmt("ru", "duel.stats.entry", &[
            ("medal", medal),
            ("name", &escape_html(&name)),
            ("elo", &total.to_string()),
            ("wins", &entry.wins.to_string()),
            ("losses", &entry.losses.to_string()),
            ("rank", rank),
        ]));
        text.push_str(&LOCALE.t_fmt("ru", "duel.stats.elo_breakdown", &[
            ("duel_elo", &entry.elo.to_string()),
            ("pidor_elo", &entry.pidor_elo.to_string()),
            ("huya_elo", &entry.huya_elo.to_string()),
        ]));
    }

    if let Some(from) = msg.from.as_ref() {
        let my_tg_id = from.id.0 as i64;
        let in_top = leaderboard.iter().any(|e| e.tg_id == my_tg_id);
        if !in_top {
            if let Ok(my_elo) = duel_db::get_or_create_elo(&pool, chat_id, my_tg_id).await {
                if my_elo.wins > 0 || my_elo.losses > 0 || my_elo.pidor_elo > 0 || my_elo.huya_elo > 0 {
                    let total = my_elo.total_elo();
                    let name = get_display_name(&bot, &pool, msg.chat.id, my_tg_id).await;
                    text.push_str(&LOCALE.t_fmt("ru", "duel.stats.my_entry", &[
                        ("name", &escape_html(&name)),
                        ("elo", &total.to_string()),
                        ("wins", &my_elo.wins.to_string()),
                        ("losses", &my_elo.losses.to_string()),
                        ("rank", duel_db::elo_rank(total)),
                    ]));
                    text.push_str(&LOCALE.t_fmt("ru", "duel.stats.elo_breakdown", &[
                        ("duel_elo", &my_elo.elo.to_string()),
                        ("pidor_elo", &my_elo.pidor_elo.to_string()),
                        ("huya_elo", &my_elo.huya_elo.to_string()),
                    ]));
                }
            }
        }
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

/// Key for message when duel is cancelled due to no moves for 1+ minute.
const DUEL_INACTIVITY_CANCELLED_KEY: &str = "duel.static.inactivity_cancelled";

/// Cancel expired pending duels and stale active duels (no move for 1 min). Edits messages. Call periodically.
pub async fn cancel_expired_duels(bot: &Bot, pool: &PgPool) -> Result<(), AppError> {
    let expired = sqlx::query_as::<_, (i32, i64, Option<i64>)>(
        r#"
        SELECT id, chat_id, invite_message_id FROM duel_game
        WHERE status = 'pending_accept' AND created_at < NOW() - INTERVAL '1 minute'
        "#,
    )
    .fetch_all(pool)
    .await?;
    for (id, chat_id, invite_message_id) in expired {
        duel_db::set_cancelled(pool, id).await?;
        if let Some(mid) = invite_message_id {
            if let Err(e) = bot
                .edit_message_text(ChatId(chat_id), teloxide::types::MessageId(mid as i32), LOCALE.t("ru", "duel.static.expired"))
                .await
            {
                tracing::debug!("Failed to edit expired duel message: {:?}", e);
            }
        }
    }

    let stale = duel_db::get_stale_active_duels(pool, duel_db::ACTIVE_INACTIVITY_TIMEOUT_SECS).await?;
    for (id, chat_id, message_id) in stale {
        duel_db::set_cancelled(pool, id).await?;
        if let Some(mid) = message_id {
            if let Err(e) = bot
                .edit_message_text(ChatId(chat_id), teloxide::types::MessageId(mid as i32), LOCALE.t("ru", DUEL_INACTIVITY_CANCELLED_KEY))
                .await
            {
                tracing::debug!("Failed to edit stale duel message: {:?}", e);
            }
        }
    }
    Ok(())
}
