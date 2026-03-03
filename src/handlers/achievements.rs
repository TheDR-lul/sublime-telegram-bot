use sqlx::PgPool;
use teloxide::prelude::*;
use teloxide::types::{
    CallbackQuery, InlineKeyboardButton, InlineKeyboardMarkup, Message, ParseMode,
};
use teloxide::utils::html::escape as escape_html;

use crate::db;
use crate::error::AppError;

const PAGE_SIZE: usize = 9;

struct AchMeta {
    emoji: &'static str,
    title: &'static str,
    description: &'static str,
}

fn code_to_meta(code: &str) -> AchMeta {
    match code {
        "first_pidoreg" => AchMeta {
            emoji: "🏅",
            title: "Я в деле",
            description: "Первая регистрация в игре «Пидор Дня». Добро пожаловать в клуб.",
        },
        "first_pidor_win" => AchMeta {
            emoji: "🥇",
            title: "Один раз не...",
            description: "Первая победа в «Пидор Дня». Дальше будет только хуже.",
        },
        "three_pidor_wins" => AchMeta {
            emoji: "🏆",
            title: "Почётный пидор чата",
            description: "3 победы в «Пидор Дня». Это уже не случайность.",
        },
        "pidor_series_2" => AchMeta {
            emoji: "🔥",
            title: "Серийный пидор",
            description: "2 победы в «Пидор Дня» подряд. Кто-то явно на разогреве.",
        },
        "night_pidor" => AchMeta {
            emoji: "🌙",
            title: "На крыльях ночи",
            description: "Победа в «Пидор Дня» ночью (00:00–06:00). Пока все спали.",
        },
        "duel_played_1" => AchMeta {
            emoji: "⚔️",
            title: "Зашёл в дуэль",
            description: "Первое участие в пидор-дуэли. Ты смелый или тупой.",
        },
        "duel_first_win" => AchMeta {
            emoji: "🗡️",
            title: "Первая победа в дуэли",
            description: "Загнал 🍆 в чужую 🍑 впервые. Запомни это чувство.",
        },
        "duel_won_5" => AchMeta {
            emoji: "🍆",
            title: "Пять раз загнал 🍆",
            description: "5 побед в дуэлях. Твой 🍆 уже легенда.",
        },
        "duel_won_10" => AchMeta {
            emoji: "💪",
            title: "Десятка в дуэлях",
            description: "10 побед в дуэлях. Мастер тактического 🍆.",
        },
        "duel_first_loss" => AchMeta {
            emoji: "💔",
            title: "Первое поражение",
            description: "Первое поражение в дуэли. Бывает, 🍑 не выбирают.",
        },
        "duel_lost_3" => AchMeta {
            emoji: "🍑",
            title: "Трижды 🍑",
            description: "3 поражения в дуэлях. Уже трижды в роли 🍑.",
        },
        "duel_lost_5" => AchMeta {
            emoji: "🕳️",
            title: "Опытный 🍑",
            description: "5 поражений в дуэлях. Профессиональная 🍑.",
        },
        "duel_lost_10" => AchMeta {
            emoji: "🪣",
            title: "Ведро для кабачков",
            description: "10 поражений в дуэлях. Вмещает всё.",
        },
        "elo_gold" => AchMeta {
            emoji: "🥇",
            title: "Золотой 🍆",
            description: "Достигнут Elo 1200+ в дуэлях. Ты опасен.",
        },
        "elo_diamond" => AchMeta {
            emoji: "💎",
            title: "Алмазный 🍆",
            description: "Достигнут Elo 1600+ в дуэлях. Тебя уже боятся.",
        },
        "elo_grandmaster" => AchMeta {
            emoji: "👑",
            title: "Гроссмейстер пидорства",
            description: "Достигнут Elo 2000+ в дуэлях. Абсолютный чемпион.",
        },
        "bet_first" => AchMeta {
            emoji: "🎲",
            title: "Букмекер",
            description: "Первая ставка на пидора дня. Сначала ставил деньги, потом — очко.",
        },
        "bet_correct_1" => AchMeta {
            emoji: "🎯",
            title: "Пидоралитик",
            description: "Первое верное предсказание пидора дня. Начало карьеры.",
        },
        "bet_correct_3" => AchMeta {
            emoji: "🔮",
            title: "Хуясновидящий",
            description: "3 верных предсказания пидора дня. Ты видишь будущее.",
        },
        "bet_self_correct" => AchMeta {
            emoji: "🪞",
            title: "Самопидор-пророк",
            description: "Поставил на себя как пидора дня — и угадал. Самопознание.",
        },
        "bet_streak_3" => AchMeta {
            emoji: "📜",
            title: "Ностраданус",
            description: "3 верных предсказания подряд. Мишель, ты ли это?",
        },
        _ => AchMeta {
            emoji: "✨",
            title: "Неизвестная ачивка",
            description: "Ты нашёл что-то загадочное.",
        },
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
    let start = page * PAGE_SIZE;
    let end = (start + PAGE_SIZE).min(achievements.len());
    let page_items = &achievements[start..end];
    let mut text = format!(
        "<b>Ачивки ({}):</b>\n",
        achievements.len()
    );
    for a in page_items {
        let meta = code_to_meta(&a.code);
        text.push_str(&format!("{} {} ", meta.emoji, escape_html(meta.title)));
    }
    text.push_str("\n\n<i>Нажми на иконку для подробностей</i>");
    text
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

    let text = grid_text(&list, 0);
    let kb = build_grid_keyboard(&list, tg_user.id, 0);

    bot.send_message(msg.chat.id, text)
        .parse_mode(ParseMode::Html)
        .reply_markup(kb)
        .await?;
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
                .text("Это не твои ачивки.")
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
            let text = format!(
                "{} <b>{}</b>\n\n{}\n\n<i>Получена: {}</i>",
                meta.emoji,
                escape_html(meta.title),
                escape_html(meta.description),
                when,
            );
            let page = idx / PAGE_SIZE;
            let back_kb = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                "← Назад".to_string(),
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
