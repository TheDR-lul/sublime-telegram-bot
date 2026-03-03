//! Pidor duel: challenge, accept, tic-tac-toe with cell TTL, victory message and roasts.

use rand::prelude::*;
use sqlx::PgPool;
use teloxide::prelude::*;
use teloxide::types::{CallbackQuery, ChatId, InlineKeyboardButton, InlineKeyboardMarkup, Message};
use teloxide::utils::html::escape as escape_html;

use crate::db;
use crate::db::duel as duel_db;
use crate::db::models::DuelGame;
use crate::error::AppError;
use crate::handlers::game::phrases::{duel_roasts, duel_victory_phrases};

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
        "Принять".to_string(),
        format!("duel_accept:{}:yes", duel_id),
    );
    if tagged {
        let decline_btn = InlineKeyboardButton::callback(
            "Отказаться".to_string(),
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
            bot.send_message(msg.chat.id, "Только пользователи могут вызывать на дуэль.")
                .await?;
            return Ok(());
        }
    };
    let challenger_tg_id = from.id.0 as i64;
    let challenger_name = escape_html(&name_from_tg_user(from));

    let invited_tg_id = msg
        .reply_to_message()
        .as_ref()
        .and_then(|r| r.from.as_ref())
        .map(|u| u.id.0 as i64);
    if let Some(inv_id) = invited_tg_id {
        if inv_id == challenger_tg_id {
            bot.send_message(msg.chat.id, "Вызови кого-то другого.")
                .await?;
            return Ok(());
        }
    }

    if duel_db::get_pending_or_active_by_chat(&pool, chat_id).await?.is_some() {
        bot.send_message(
            msg.chat.id,
            "Сначала дождись окончания текущего дуэля.",
        )
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
            format!(
                "{} вызывает {} на пидор-дуэль. {}, принять? (1 мин)",
                challenger_name,
                invited_name_esc,
                invited_name_esc
            ),
            true,
        )
    } else {
        (
            format!(
                "{} ищет соперника на пидор-дуэль. Кто примет? (1 мин)",
                challenger_name
            ),
            false,
        )
    };

    let sent = bot
        .send_message(msg.chat.id, text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .reply_markup(duel_accept_keyboard(d.id, tagged))
        .await?;
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
            let _ = bot.answer_callback_query(query.id).text("Только приглашённый может отказаться.").await;
            return Ok(());
        }
        duel_db::decline(&pool, duel_id).await?;
        let _ = bot.answer_callback_query(query.id).await;
        let decliner_name = get_display_name(&bot, &pool, ChatId(chat_id), accepter_tg_id).await;
        let text = format!("{} отказался. Трусливый пидор.", escape_html(&decliner_name));
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
                .text("Вызов уже истёк или принять нельзя.")
                .await;
            if let Ok(Some(d)) = duel_db::get_by_id(&pool, duel_id).await {
                if d.status == "cancelled" {
                    if let Some(mid) = d.invite_message_id {
                        if let Some(ref msg) = query.message {
                            if msg.id().0 as i64 == mid {
                                let _ = bot
                                    .edit_message_text(msg.chat().id, msg.id(), "Вызов истёк (1 мин).")
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

    let player1_name = get_display_name(&bot, &pool, ChatId(chat_id), d.player1_tg_id.unwrap()).await;
    let caption = format!("Ход: {} (🍑)", escape_html(&player1_name));
    let sent = bot
        .send_message(ChatId(chat_id), caption)
        .parse_mode(teloxide::types::ParseMode::Html)
        .reply_markup(board_keyboard(&d))
        .await?;
    duel_db::set_message_id(&pool, duel_id, sent.id.0 as i64).await?;
    if let Some(ref msg) = query.message {
        let _ = bot.edit_message_reply_markup(msg.chat().id, msg.id()).await;
    }
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
            .text("Не твой ход.")
            .show_alert(false)
            .await;
        return Ok(());
    }

    let result = duel_db::make_move(&pool, duel_id, cell, player_tg_id).await;
    let (d, winner_tg_id) = match result {
        Ok((game, w)) => (game, w),
        Err(e) => {
            let msg = match e {
                AppError::GameLogic(ref s) if s.contains("not your turn") => "Не твой ход.",
                AppError::GameLogic(ref s) if s.contains("occupied") => {
                    "Клетка занята или уже освободилась."
                }
                _ => "Нельзя походить.",
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
        let winner_name = get_display_name(&bot, &pool, ChatId(chat_id), winner_tg_id).await;
        let loser_name = get_display_name(&bot, &pool, ChatId(chat_id), loser_tg_id).await;
        let winner_esc = escape_html(&winner_name);
        let loser_esc = escape_html(&loser_name);

        let mut rng = rand::make_rng::<rand::rngs::StdRng>();
        let victory_phrase = duel_victory_phrases::PHRASES
            .choose(&mut rng)
            .unwrap_or(&duel_victory_phrases::PHRASES[0]);
        let victory_text = victory_phrase
            .replace("{winner}", &winner_esc)
            .replace("{loser}", &loser_esc);
        let roast_phrase = duel_roasts::PHRASES
            .choose(&mut rng)
            .unwrap_or(&duel_roasts::PHRASES[0]);
        let roast_text = roast_phrase.replace("{username}", &loser_esc);
        let final_text = format!("{}\n\n{}", victory_text, roast_text);

        if let Some(ref msg) = query.message {
            bot.edit_message_text(msg.chat().id, msg.id(), final_text)
                .parse_mode(teloxide::types::ParseMode::Html)
                .reply_markup(InlineKeyboardMarkup::new::<Vec<Vec<InlineKeyboardButton>>>(vec![]))
                .await?;
        }

        let elo_result = duel_db::update_elo_after_duel(&pool, chat_id, winner_tg_id, loser_tg_id).await;
        grant_duel_achievements(&bot, &pool, ChatId(chat_id), winner_tg_id, loser_tg_id, &elo_result).await?;
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
    let caption = format!("Ход: {} ({})", escape_html(&current_name), emoji);
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
        title: &str,
    ) -> Result<(), AppError> {
        if db::achievements::grant(pool, user_id, code).await? {
            let _ = bot.send_message(chat_id, format!("🏅 Новая ачивка: {}", title)).await;
        }
        Ok(())
    }

    let winner_uid_opt = duel_db::user_id_by_tg_id(pool, winner_tg_id).await?;
    let loser_uid_opt = duel_db::user_id_by_tg_id(pool, loser_tg_id).await?;

    if let Some(winner_uid) = winner_uid_opt {
        let wins = duel_db::count_wins(pool, winner_tg_id).await?;
        if wins >= 1 {
            let _ = do_grant(bot, pool, chat_id, winner_uid, "duel_first_win", "Первая победа в дуэле").await;
        }
        if wins >= 5 {
            let _ = do_grant(bot, pool, chat_id, winner_uid, "duel_won_5", "Пять раз загнал 🍆 в чужую 🍑").await;
        }
        if wins >= 10 {
            let _ = do_grant(bot, pool, chat_id, winner_uid, "duel_won_10", "Десятка в дуэлях").await;
        }
    }
    if let Some(loser_uid) = loser_uid_opt {
        let losses = duel_db::count_losses(pool, loser_tg_id).await?;
        if losses >= 1 {
            let _ = do_grant(bot, pool, chat_id, loser_uid, "duel_first_loss", "Первое поражение в дуэле").await;
        }
        if losses >= 3 {
            let _ = do_grant(bot, pool, chat_id, loser_uid, "duel_lost_3", "Уже трижды в роли 🍑").await;
        }
        if losses >= 5 {
            let _ = do_grant(bot, pool, chat_id, loser_uid, "duel_lost_5", "Пять раз в роли жопы").await;
        }
        if losses >= 10 {
            let _ = do_grant(bot, pool, chat_id, loser_uid, "duel_lost_10", "Ведро для кабачков").await;
        }
        let played = duel_db::count_played(pool, loser_tg_id).await?;
        if played >= 1 {
            let _ = do_grant(bot, pool, chat_id, loser_uid, "duel_played_1", "Зашёл в дуэль").await;
        }
    }
    if let Some(winner_uid) = winner_uid_opt {
        let played = duel_db::count_played(pool, winner_tg_id).await?;
        if played >= 1 {
            let _ = do_grant(bot, pool, chat_id, winner_uid, "duel_played_1", "Зашёл в дуэль").await;
        }
    }

    if let Ok((w_elo, _)) = elo_result {
        if let Some(winner_uid) = winner_uid_opt {
            if w_elo.elo >= 1200 {
                let _ = do_grant(bot, pool, chat_id, winner_uid, "elo_gold", "🥇 Золотой 🍆 (Elo 1200+)").await;
            }
            if w_elo.elo >= 1600 {
                let _ = do_grant(bot, pool, chat_id, winner_uid, "elo_diamond", "💎 Алмазный кабачок (Elo 1600+)").await;
            }
            if w_elo.elo >= 2000 {
                let _ = do_grant(bot, pool, chat_id, winner_uid, "elo_grandmaster", "👑 Гроссмейстер пидорства (Elo 2000+)").await;
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
        bot.send_message(msg.chat.id, "Дуэлей ещё не было. Начни с /pidorduel!")
            .await?;
        return Ok(());
    }

    let mut text = String::from("<b>🏆 Рейтинг дуэлей:</b>\n\n");
    for (i, entry) in leaderboard.iter().enumerate() {
        let name = get_display_name(&bot, &pool, msg.chat.id, entry.tg_id).await;
        let rank = duel_db::elo_rank(entry.elo);
        let medal = match i {
            0 => "🥇",
            1 => "🥈",
            2 => "🥉",
            _ => "•",
        };
        text.push_str(&format!(
            "{} <b>{}</b> — {} Elo ({}/{}W/L)\n    {}\n",
            medal,
            escape_html(&name),
            entry.elo,
            entry.wins,
            entry.losses,
            rank,
        ));
    }

    if let Some(from) = msg.from.as_ref() {
        let my_tg_id = from.id.0 as i64;
        let in_top = leaderboard.iter().any(|e| e.tg_id == my_tg_id);
        if !in_top {
            if let Ok(my_elo) = duel_db::get_or_create_elo(&pool, chat_id, my_tg_id).await {
                if my_elo.wins > 0 || my_elo.losses > 0 {
                    let name = get_display_name(&bot, &pool, msg.chat.id, my_tg_id).await;
                    text.push_str(&format!(
                        "\n<b>Ты:</b> {} — {} Elo ({}/{}W/L)\n    {}\n",
                        escape_html(&name),
                        my_elo.elo,
                        my_elo.wins,
                        my_elo.losses,
                        duel_db::elo_rank(my_elo.elo),
                    ));
                }
            }
        }
    }

    bot.send_message(msg.chat.id, text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
    Ok(())
}

/// Message when duel is cancelled due to no moves for 1+ minute.
const DUEL_INACTIVITY_CANCELLED_MSG: &str = "Дуэль отменена: нет ходов больше минуты.";

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
                .edit_message_text(ChatId(chat_id), teloxide::types::MessageId(mid as i32), "Вызов истёк (1 мин).")
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
                .edit_message_text(ChatId(chat_id), teloxide::types::MessageId(mid as i32), DUEL_INACTIVITY_CANCELLED_MSG)
                .await
            {
                tracing::debug!("Failed to edit stale duel message: {:?}", e);
            }
        }
    }
    Ok(())
}
