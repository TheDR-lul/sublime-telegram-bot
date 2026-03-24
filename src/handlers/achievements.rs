use sqlx::PgPool;
use teloxide::prelude::*;
use teloxide::types::{
    CallbackQuery, ChatId, InlineKeyboardButton, InlineKeyboardMarkup, Message, MessageId,
    ParseMode,
};
use teloxide::utils::html::escape as escape_html;

use crate::db;
use crate::error::AppError;
use crate::i18n::LOCALE;
use crate::telegram::topic_routing::{send_text_in_origin_topic, topic_thread_id};

const ACH_DELETE_AFTER_SECS: u64 = 60;

fn schedule_ach_delete(bot: Bot, chat_id: ChatId, message_id: MessageId) {
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(ACH_DELETE_AFTER_SECS)).await;
        let _ = bot.delete_message(chat_id, message_id).await;
    });
}

const PAGE_SIZE: usize = 9;

struct AchMeta {
    emoji: &'static str,
    title: &'static str,
    description: &'static str,
}

fn code_to_meta(code: &str) -> AchMeta {
    let emoji_key = format!("achievements.list.{}_emoji", code);
    let title_key = format!("achievements.list.{}_title", code);
    let desc_key  = format!("achievements.list.{}_desc", code);
    AchMeta {
        emoji: LOCALE.t_opt("ru", &emoji_key).unwrap_or(LOCALE.t("ru", "achievements.list.unknown_emoji")),
        title: LOCALE.t_opt("ru", &title_key).unwrap_or(LOCALE.t("ru", "achievements.list.unknown_title")),
        description: LOCALE.t_opt("ru", &desc_key).unwrap_or(LOCALE.t("ru", "achievements.list.unknown_desc")),
    }
}

pub fn code_to_title(code: &str) -> String {
    let m = code_to_meta(code);
    format!("{} {}", m.emoji, m.title)
}

fn build_grid_keyboard(
    achievements: &[db::models::Achievement],
    user_id: i32,
    page: usize,
) -> InlineKeyboardMarkup {
    let total = achievements.len();
    let start = page * PAGE_SIZE;
    let end = (start + PAGE_SIZE).min(total);
    let page_items = &achievements[start..end];

    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();
    for chunk in page_items.chunks(3) {
        let row: Vec<InlineKeyboardButton> = chunk
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let idx = start + rows.len() * 3 + i;
                let meta = code_to_meta(&a.code);
                InlineKeyboardButton::callback(
                    meta.emoji.to_string(),
                    format!("ach:v:{}:{}", user_id, idx),
                )
            })
            .collect();
        rows.push(row);
    }

    let total_pages = (total + PAGE_SIZE - 1) / PAGE_SIZE;
    if total_pages > 1 {
        let mut nav = Vec::new();
        if page > 0 {
            nav.push(InlineKeyboardButton::callback(
                "◀".to_string(),
                format!("ach:p:{}:{}", user_id, page - 1),
            ));
        }
        nav.push(InlineKeyboardButton::callback(
            format!("{}/{}", page + 1, total_pages),
            "ach:noop".to_string(),
        ));
        if page + 1 < total_pages {
            nav.push(InlineKeyboardButton::callback(
                "▶".to_string(),
                format!("ach:p:{}:{}", user_id, page + 1),
            ));
        }
        rows.push(nav);
    }

    InlineKeyboardMarkup::new(rows)
}

fn grid_text(achievements: &[db::models::Achievement], page: usize) -> String {
    let total = achievements.len();
    let total_pages = (total + PAGE_SIZE - 1) / PAGE_SIZE;
    let page_num = page + 1;
    let header = LOCALE.t_fmt("ru", "achievements.header", &[("count", &total.to_string())]);
    let page_info = if total_pages > 1 {
        format!(" ({}/{})", page_num, total_pages)
    } else {
        String::new()
    };
    format!(
        "<b>{}{}</b>\n\n<i>{}</i>",
        header,
        page_info,
        LOCALE.t("ru", "achievements.tap_hint"),
    )
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
            send_text_in_origin_topic(&bot, &msg, LOCALE.t("ru", "achievements.anonymous")).await?;
            return Ok(());
        }
    };

    let tg_user = crate::db::user::upsert_tg_user(&pool, from_user).await?;
    let list = db::achievements::list_for_user(&pool, tg_user.id).await?;

    if list.is_empty() {
        send_text_in_origin_topic(&bot, &msg, LOCALE.t("ru", "achievements.no_achievements")).await?;
        return Ok(());
    }

    let text = grid_text(&list, 0);
    let kb = build_grid_keyboard(&list, tg_user.id, 0);

    let mut request = bot.send_message(msg.chat.id, text)
        .parse_mode(ParseMode::Html)
        .reply_markup(kb);
    if let Some(thread) = topic_thread_id(&msg) {
        request = request.message_thread_id(thread);
    }
    let sent = request.await?;

    schedule_ach_delete(bot, msg.chat.id, sent.id);
    Ok(())
}

pub async fn achievements_callback(
    bot: Bot,
    query: CallbackQuery,
    pool: PgPool,
) -> Result<(), AppError> {
    let data = query.data.as_deref().unwrap_or("");

    if data == "ach:noop" {
        let _ = bot.answer_callback_query(query.id).await;
        return Ok(());
    }

    let parts: Vec<&str> = data.splitn(4, ':').collect();
    let (action, user_id_str, arg) = match parts[..] {
        ["ach", action, uid, arg] => (action, uid, arg),
        _ => {
            let _ = bot.answer_callback_query(query.id).await;
            return Ok(());
        }
    };

    let user_id: i32 = match user_id_str.parse() {
        Ok(v) => v,
        Err(_) => {
            let _ = bot.answer_callback_query(query.id).await;
            return Ok(());
        }
    };

    let caller_tg_id = query.from.id.0 as i64;
    let owner = crate::db::user::get_by_id(&pool, user_id).await?;
    if let Some(ref owner) = owner {
        if owner.tg_id != caller_tg_id {
            let _ = bot
                .answer_callback_query(query.id)
                .text(LOCALE.t("ru", "achievements.not_yours"))
                .show_alert(false)
                .await;
            return Ok(());
        }
    }

    let list = db::achievements::list_for_user(&pool, user_id).await?;
    if list.is_empty() {
        let _ = bot.answer_callback_query(query.id).await;
        return Ok(());
    }

    match action {
        "p" => {
            let page: usize = arg.parse().unwrap_or(0);
            let total_pages = (list.len() + PAGE_SIZE - 1) / PAGE_SIZE;
            let page = page.min(total_pages.saturating_sub(1));
            let text = grid_text(&list, page);
            let kb = build_grid_keyboard(&list, user_id, page);
            if let Some(ref msg) = query.message {
                let _ = bot
                    .edit_message_text(msg.chat().id, msg.id(), text)
                    .parse_mode(ParseMode::Html)
                    .reply_markup(kb)
                    .await;
            }
        }
        "v" => {
            let idx: usize = match arg.parse() {
                Ok(v) => v,
                Err(_) => {
                    let _ = bot.answer_callback_query(query.id).await;
                    return Ok(());
                }
            };
            if idx >= list.len() {
                let _ = bot.answer_callback_query(query.id).await;
                return Ok(());
            }
            let a = &list[idx];
            let meta = code_to_meta(&a.code);
            let when = a.earned_at.format("%Y-%m-%d %H:%M UTC");
            let text = LOCALE.t_fmt("ru", "achievements.detail_fmt", &[
                ("emoji", meta.emoji),
                ("title", &escape_html(meta.title)),
                ("description", &escape_html(meta.description)),
                ("date", &when.to_string()),
            ]);
            let page = idx / PAGE_SIZE;
            let back_kb = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                LOCALE.t("ru", "achievements.back_btn").to_string(),
                format!("ach:p:{}:{}", user_id, page),
            )]]);
            if let Some(ref msg) = query.message {
                let _ = bot
                    .edit_message_text(msg.chat().id, msg.id(), text)
                    .parse_mode(ParseMode::Html)
                    .reply_markup(back_kb)
                    .await;
            }
        }
        _ => {}
    }

    let _ = bot.answer_callback_query(query.id).await;
    Ok(())
}
