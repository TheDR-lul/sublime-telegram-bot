use chrono::{DateTime, Local};
use sqlx::PgPool;
use teloxide::prelude::*;
use teloxide::types::{Message, ParseMode};
use teloxide::utils::html::escape as escape_html;

use crate::db;
use crate::error::AppError;

fn code_to_title(code: &str) -> &'static str {
    match code {
        "first_pidoreg" => "🏅 Я в деле (первая регистрация в игре)",
        "first_pidor_win" => "🥇 Один раз не... (первая победа в Пидор Дня)",
        "three_pidor_wins" => "🏆 Почётный пидор чата (3 победы)",
        "pidor_series_2" => "🔥 Серийный пидор (2 победы подряд)",
        "night_pidor" => "🌙 На крыльях ночи",
        "duel_first_win" => "Первая победа в дуэли",
        "duel_won_5" => "Пять раз загнал 🍆 в чужую 🍑",
        "duel_won_10" => "Десятка в дуэлях",
        "duel_first_loss" => "Первое поражение в дуэли",
        "duel_lost_3" => "Уже трижды в роли 🍑",
        "duel_lost_5" => "Пять раз в роли жопы",
        "duel_lost_10" => "Ведро для кабачков",
        "duel_played_1" => "Зашёл в дуэль",
        "elo_gold" => "🥇 Золотой 🍆 (Elo 1200+)",
        "elo_diamond" => "💎 Алмазный 🍆 (Elo 1600+)",
        "elo_grandmaster" => "👑 Гроссмейстер пидорства (Elo 2000+)",
        "bet_first" => "🎲 Сначала ставил деньги, а потом очко (первая ставка)",
        "bet_correct_1" => "🎯 Пидоралитик (первое верное предсказание)",
        "bet_correct_3" => "🔮 Хуясновидящий (3 верных предсказания)",
        "bet_self_correct" => "🪞 Самопидор-пророк (поставил на себя и угадал)",
        "bet_streak_3" => "📜 Ностраданус (3 верных предсказания подряд)",
        _ => "✨ Неизвестная ачивка",
    }
}

fn format_time(ts: &DateTime<chrono::Utc>) -> String {
    let local: DateTime<Local> = DateTime::from(*ts);
    local.format("%Y-%m-%d %H:%M").to_string()
}

pub async fn achievements_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    let from_user = match msg.from.as_ref() {
        Some(u) => u,
        None => {
            bot.send_message(msg.chat.id, "Cannot show achievements for anonymous user.")
                .await?;
            return Ok(());
        }
    };

    let tg_user = crate::db::user::upsert_tg_user(&pool, from_user).await?;
    let list = db::achievements::list_for_user(&pool, tg_user.id).await?;

    if list.is_empty() {
        bot.send_message(msg.chat.id, "У тебя пока нет ачивок. Всё впереди.")
            .await?;
        return Ok(());
    }

    let mut text = String::from("<b>Твои ачивки:</b>\n");
    for a in list {
        let title = escape_html(code_to_title(&a.code));
        let when = escape_html(&format_time(&a.earned_at));
        text.push_str(&format!("• <b>{}</b> — <i>{}</i>\n", title, when));
    }

    bot.send_message(msg.chat.id, text)
        .parse_mode(ParseMode::Html)
        .await?;
    Ok(())
}