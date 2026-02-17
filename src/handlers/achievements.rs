use chrono::{DateTime, Local};
use sqlx::PgPool;
use teloxide::prelude::*;
use teloxide::types::{Message, ParseMode};
use teloxide::utils::markdown::escape as escape_md2;

use crate::db;
use crate::error::AppError;

fn code_to_title(code: &str) -> &'static str {
    match code {
        "first_pidoreg" => "🏅 Я в деле (первая регистрация в игре)",
        "first_pidor_win" => "🥇 Первый пошёл (первая победа в Пидор Дня)",
        "three_pidor_wins" => "🏆 Почётный пидор чата (3 победы)",
        "pidor_series_2" => "🔥 Пидор‑серийник (2 победы подряд)",
        "night_pidor" => "🌙 Ночной пидор (победа ночью)",
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
    let from_user = match msg.from() {
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

    let mut text = String::from("*Твои ачивки:*\n");
    for a in list {
        let title = escape_md2(code_to_title(&a.code));
        let when = escape_md2(&format_time(&a.earned_at));
        text.push_str(&format!("• *{}* — _{}_\n", title, when));
    }

    bot.send_message(msg.chat.id, text)
        .parse_mode(ParseMode::MarkdownV2)
        .await?;
    Ok(())
}

