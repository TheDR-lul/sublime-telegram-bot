use sqlx::PgPool;
use teloxide::prelude::*;
use teloxide::sugar::request::RequestLinkPreviewExt;
use teloxide::types::{CallbackQuery, ChatId, InlineKeyboardButton, InlineKeyboardMarkup, Message, MessageId};

use crate::config::Config;
use crate::error::AppError;
use crate::handlers::about;
use crate::handlers::game::commands as game_commands;
use crate::i18n::LOCALE;

use rand::prelude::*;
use std::sync::LazyLock;
use teloxide::types::ParseMode;
use teloxide::utils::html::escape as escape_html;

fn raw_name_from_msg(msg: &Message) -> String {
    msg.from
        .as_ref()
        .map(|u| {
            u.username
                .as_deref()
                .unwrap_or(u.first_name.as_str())
                .to_string()
        })
        .unwrap_or_else(|| "someone".to_string())
}

pub async fn slap_handler(
    bot: Bot,
    msg: Message,
    _cmd: crate::handlers::commands::Cmd,
) -> Result<(), AppError> {
    let who = escape_html(&raw_name_from_msg(&msg));
    
    let target = if let Some(reply_to) = msg.reply_to_message() {
        reply_to
            .from
            .as_ref()
            .map(|u| {
                escape_html(
                    &u.username
                        .as_deref()
                        .unwrap_or(&u.first_name)
                        .to_string()
                )
            })
            .unwrap_or_else(|| "кто-то".to_string())
    } else {
        bot.send_message(msg.chat.id, LOCALE.t("ru", "slap.no_reply"))
            .await?;
        return Ok(());
    };
    
    let text = LOCALE.t_rand_fmt("ru", "slap.phrases", &[("who", &who), ("target", &target)]);
    
    bot.send_message(msg.chat.id, text)
        .parse_mode(ParseMode::Html)
        .await?;
    Ok(())
}

pub async fn rpg_disabled_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
) -> Result<(), AppError> {
    bot.send_message(msg.chat.id, LOCALE.t("ru", "rpg.disabled")).await?;
    Ok(())
}

pub async fn rpg_disabled_callback(
    bot: Bot,
    query: teloxide::types::CallbackQuery,
) -> Result<(), AppError> {
    if let Some(chat_id) = query.message.as_ref().map(|m| m.chat().id) {
        bot.send_message(chat_id, LOCALE.t("ru", "rpg.disabled")).await?;
    }
    bot.answer_callback_query(query.id).await?;
    Ok(())
}

pub async fn shrug_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
) -> Result<(), AppError> {
    bot.send_message(msg.chat.id, r"¯\_(ツ)_/¯").await?;
    Ok(())
}

pub async fn me_handler(
    bot: Bot,
    msg: Message,
    cmd: crate::handlers::commands::Cmd,
) -> Result<(), AppError> {
    let who = escape_html(&raw_name_from_msg(&msg));
    let text = match &cmd {
        crate::handlers::commands::Cmd::Me(s) => {
            if s.is_empty() {
                return Ok(());
            }
            format!("<b>{}</b> {}", who, escape_html(s))
        }
        _ => return Ok(()),
    };
    bot.send_message(msg.chat.id, text)
        .parse_mode(ParseMode::Html)
        .await?;
    Ok(())
}

pub async fn google_handler(
    bot: Bot,
    msg: Message,
    cmd: crate::handlers::commands::Cmd,
) -> Result<(), AppError> {
    let query = match &cmd {
        crate::handlers::commands::Cmd::Google(s) => s,
        _ => return Ok(()),
    };
    if query.is_empty() {
        bot.send_message(msg.chat.id, LOCALE.t("ru", "inline.google_no_query"))
            .await?;
        return Ok(());
    }
    let url = format!("https://lmgtfy.com/?q={}", url::form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>());
    bot.send_message(msg.chat.id, url)
        .disable_link_preview(true)
        .await?;
    Ok(())
}

pub async fn pidorscan_handler(
    bot: Bot,
    msg: Message,
    cmd: crate::handlers::commands::Cmd,
    dedup: std::sync::Arc<crate::dedup::PidorscanDedup>,
) -> Result<(), AppError> {
    // Avoid duplicate replies when the same update is processed twice (e.g. webhook retry).
    if !dedup.try_acquire(msg.chat.id.0, msg.id.0).await {
        return Ok(());
    }
    let target_name = if let Some(reply) = msg.reply_to_message() {
        reply
            .from
            .as_ref()
            .map(|u| u.full_name())
            .unwrap_or_else(|| "кто-то".to_string())
    } else {
        match &cmd {
            crate::handlers::commands::Cmd::Pidorscan(s) if !s.trim().is_empty() => s.trim().to_string(),
            _ => msg
                .from
                .as_ref()
                .map(|u| u.full_name())
                .unwrap_or_else(|| "кто-то".to_string()),
        }
    };

    let mut rng = rand::make_rng::<rand::rngs::StdRng>();
    let percent: u8 = rng.random_range(0..=100);

    let intro = LOCALE.t_rand_fmt("ru", "pidorscan.intro", &[("name", &target_name)]);
    bot.send_message(msg.chat.id, intro).await?;

    tokio::time::sleep(tokio::time::Duration::from_millis(700)).await;
    let analysis = LOCALE.t_rand("ru", "pidorscan.analysis");
    bot.send_message(msg.chat.id, analysis).await?;

    tokio::time::sleep(tokio::time::Duration::from_millis(850)).await;
    let algo = LOCALE.t_rand("ru", "pidorscan.algo");
    bot.send_message(msg.chat.id, algo).await?;

    tokio::time::sleep(tokio::time::Duration::from_millis(1100)).await;

    let name_escaped = escape_html(&target_name);
    let percent_str = percent.to_string();

    let verdict = if percent == 0 {
        LOCALE.t_fmt("ru", "pidorscan.verdict_zero", &[("name", &name_escaped)])
    } else if percent < 30 {
        LOCALE.t_fmt("ru", "pidorscan.verdict_low", &[("name", &name_escaped), ("percent", &percent_str)])
    } else if percent < 70 {
        LOCALE.t_fmt("ru", "pidorscan.verdict_mid", &[("name", &name_escaped), ("percent", &percent_str)])
    } else if percent < 100 {
        LOCALE.t_fmt("ru", "pidorscan.verdict_high", &[("name", &name_escaped), ("percent", &percent_str)])
    } else {
        LOCALE.t_fmt("ru", "pidorscan.verdict_max", &[("name", &name_escaped), ("percent", &percent_str)])
    };

    bot.send_message(msg.chat.id, verdict)
        .parse_mode(ParseMode::Html)
        .await?;

    Ok(())
}

pub async fn inline_handler(
    bot: Bot,
    query: teloxide::types::InlineQuery,
) -> Result<(), AppError> {
    let q = query.query.trim();
    if q.is_empty() {
        return Ok(());
    }

    use rand::prelude::*;
    use teloxide::types::{InlineQueryResult, InlineQueryResultArticle, InputMessageContent, InputMessageContentText};

    static WORD_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"([^\W\d_]{4,})").expect("word regex is valid")
    });

    let mut shuffled = Vec::new();
    for word in WORD_RE.split(q) {
        if word.chars().all(|c| c.is_alphanumeric()) && word.len() >= 4 {
            let mut chars: Vec<char> = word.chars().collect();
            let first = chars.remove(0);
            let last = chars.pop().expect("word length >= 4 so at least 2 chars after first");
            let mut rng = rand::make_rng::<rand::rngs::StdRng>();
            chars.shuffle(&mut rng);
            shuffled.push(format!("{}{}{}", first, chars.iter().collect::<String>(), last));
        } else {
            shuffled.push(word.to_string());
        }
    }

    let results = vec![
        InlineQueryResult::Article(InlineQueryResultArticle {
            id: uuid::Uuid::new_v4().to_string(),
            title: LOCALE.t("ru", "inline.echo_title").to_string(),
            description: Some(LOCALE.t("ru", "inline.echo_desc").to_string()),
            input_message_content: InputMessageContent::Text(InputMessageContentText {
                message_text: q.to_string(),
                parse_mode: None,
                entities: None,
                link_preview_options: None,
            }),
            reply_markup: None,
            url: None,
            thumbnail_url: None,
            thumbnail_width: None,
            thumbnail_height: None,
        }),
        InlineQueryResult::Article(InlineQueryResultArticle {
            id: uuid::Uuid::new_v4().to_string(),
            title: LOCALE.t("ru", "inline.caps_title").to_string(),
            description: Some(LOCALE.t("ru", "inline.caps_desc").to_string()),
            input_message_content: InputMessageContent::Text(InputMessageContentText {
                message_text: q.to_uppercase(),
                parse_mode: None,
                entities: None,
                link_preview_options: None,
            }),
            reply_markup: None,
            url: None,
            thumbnail_url: None,
            thumbnail_width: None,
            thumbnail_height: None,
        }),
        InlineQueryResult::Article(InlineQueryResultArticle {
            id: uuid::Uuid::new_v4().to_string(),
            title: LOCALE.t("ru", "inline.shuffle_title").to_string(),
            description: Some(LOCALE.t("ru", "inline.shuffle_desc").to_string()),
            input_message_content: InputMessageContent::Text(InputMessageContentText {
                message_text: shuffled.join(""),
                parse_mode: None,
                entities: None,
                link_preview_options: None,
            }),
            reply_markup: None,
            url: None,
            thumbnail_url: None,
            thumbnail_width: None,
            thumbnail_height: None,
        }),
    ];

    bot.answer_inline_query(query.id, results).cache_time(0).await?;
    Ok(())
}

/// Single-column layout so buttons render evenly on narrow screens (mobile).
fn menu_main_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback(LOCALE.t("ru", "menu.buttons.game"), "menu:game")],
        vec![InlineKeyboardButton::callback(LOCALE.t("ru", "menu.buttons.other"), "menu:other")],
        vec![InlineKeyboardButton::callback(LOCALE.t("ru", "menu.buttons.admin"), "menu:admin")],
    ])
}

fn menu_admin_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback("🔧 Настройки автопидора", "menu:action:pidorset")],
        vec![InlineKeyboardButton::callback("📢 Позвать участников", "menu:action:pidorcall")],
        vec![InlineKeyboardButton::callback("🌐 Язык чата (/lang ru)", "menu:action:lang_info")],
        vec![InlineKeyboardButton::callback("← Назад", "menu:main")],
    ])
}

fn menu_game_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback(LOCALE.t("ru", "menu.buttons.rules"), "menu:action:pidorules")],
        vec![InlineKeyboardButton::callback(LOCALE.t("ru", "menu.buttons.duel"), "menu:action:pidorduel")],
        vec![
            InlineKeyboardButton::callback(LOCALE.t("ru", "menu.buttons.stats"), "menu:action:pidorstats"),
            InlineKeyboardButton::callback(LOCALE.t("ru", "menu.buttons.all_time"), "menu:action:pidorall"),
        ],
        vec![InlineKeyboardButton::callback(LOCALE.t("ru", "menu.buttons.back"), "menu:main")],
    ])
}

fn menu_other_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback(LOCALE.t("ru", "menu.buttons.about"), "menu:action:about")],
        vec![InlineKeyboardButton::callback(LOCALE.t("ru", "menu.buttons.back"), "menu:main")],
    ])
}

pub async fn menu_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
) -> Result<(), AppError> {
    let chat_id = msg.chat.id;
    let _ = bot.delete_message(chat_id, msg.id).await;
    let sent = bot
        .send_message(chat_id, LOCALE.t("ru", "menu.choose_section"))
        .reply_markup(menu_main_keyboard())
        .await?;
    game_commands::schedule_delete_message(bot, chat_id, sent.id);
    Ok(())
}

pub async fn menu_callback(
    bot: Bot,
    query: CallbackQuery,
    pool: PgPool,
    _config: Config,
) -> Result<(), AppError> {
    let chat_id = match query.message.as_ref().map(|m| m.chat().id) {
        Some(id) => id,
        None => return Ok(()),
    };
    let message_id = query.message.as_ref().map(|m| m.id());
    let data = query.data.as_deref().unwrap_or("");

    let schedule_menu_delete = |cid: ChatId, mid: MessageId| {
        game_commands::schedule_delete_message(bot.clone(), cid, mid);
    };
    if data == "menu:main" {
        bot.answer_callback_query(query.id).await?;
        if let Some(mid) = message_id {
            bot.edit_message_text(chat_id, mid, LOCALE.t("ru", "menu.choose_section"))
                .reply_markup(menu_main_keyboard())
                .await?;
            schedule_menu_delete(chat_id, mid);
        }
        return Ok(());
    }
    if data == "menu:game" {
        bot.answer_callback_query(query.id).await?;
        if let Some(mid) = message_id {
            bot.edit_message_text(chat_id, mid, LOCALE.t("ru", "menu.game_section"))
                .reply_markup(menu_game_keyboard())
                .await?;
            schedule_menu_delete(chat_id, mid);
        }
        return Ok(());
    }
    if data == "menu:other" {
        bot.answer_callback_query(query.id).await?;
        if let Some(mid) = message_id {
            bot.edit_message_text(chat_id, mid, LOCALE.t("ru", "menu.other_section"))
                .reply_markup(menu_other_keyboard())
                .await?;
            schedule_menu_delete(chat_id, mid);
        }
        return Ok(());
    }
    if data == "menu:admin" {
        let user_id = query.from.id.0 as u64;
        if chat_id.0 >= 0 {
            bot.answer_callback_query(query.id)
                .text(LOCALE.t("ru", "menu.group_only"))
                .await?;
            return Ok(());
        }
        if !game_commands::is_chat_admin(&bot, chat_id, user_id).await {
            bot.answer_callback_query(query.id)
                .text(LOCALE.t("ru", "menu.admin_only"))
                .await?;
            return Ok(());
        }
        bot.answer_callback_query(query.id).await?;
        if let Some(mid) = message_id {
            bot.edit_message_text(chat_id, mid, LOCALE.t("ru", "menu.admin_section"))
                .reply_markup(menu_admin_keyboard())
                .await?;
            schedule_menu_delete(chat_id, mid);
        }
        return Ok(());
    }
    if let Some(action) = data.strip_prefix("menu:action:") {
        bot.answer_callback_query(query.id).await?;
        match action {
            "pidorules" => {
                let invoker = if chat_id.0 < 0 {
                    Some(query.from.id.0 as i64)
                } else {
                    None
                };
                if let Ok(tg_user) = crate::db::user::upsert_tg_user(&pool, &query.from).await {
                    let _ = crate::db::game::record_chat_member(&pool, chat_id.0, tg_user.id).await;
                }
                game_commands::send_pidorules(&bot, chat_id, invoker).await?;
            }
            "pidorstats" => {
                game_commands::send_pidorstats(&bot, &pool, chat_id).await?;
            }
            "pidorall" => {
                game_commands::send_pidorall(&bot, &pool, chat_id).await?;
            }
            "about" => {
                about::send_about(&bot, chat_id).await?;
            }
            "pidorset" => {
                let user_id = query.from.id.0 as u64;
                if chat_id.0 >= 0 {
                    bot.send_message(chat_id, LOCALE.t("ru", "pidor.settings.group_only"))
                        .await?;
                } else if !game_commands::is_chat_admin(&bot, chat_id, user_id).await {
                    bot.send_message(chat_id, LOCALE.t("ru", "pidor.settings.admin_only"))
                        .await?;
                } else if let Some(mid) = message_id {
                    if let Err(e) = game_commands::edit_message_to_pidorset(&bot, &pool, chat_id, mid).await {
                        tracing::warn!("edit_message_to_pidorset failed, sending new message: {:?}", e);
                        game_commands::send_pidorset_message(&bot, &pool, chat_id).await?;
                    } else {
                        schedule_menu_delete(chat_id, mid);
                    }
                } else {
                    game_commands::send_pidorset_message(&bot, &pool, chat_id).await?;
                }
            }
            "pidorcall" => {
                let user_id = query.from.id.0 as u64;
                if chat_id.0 >= 0 {
                    bot.send_message(chat_id, LOCALE.t("ru", "pidor.settings.call_group_only"))
                        .await?;
                } else if !game_commands::is_chat_admin(&bot, chat_id, user_id).await {
                    bot.send_message(chat_id, LOCALE.t("ru", "pidor.settings.call_admin_only"))
                        .await?;
                } else {
                    game_commands::send_pidorcall_message(&bot, &pool, chat_id).await?;
                }
            }
            "pidorduel" => {
                bot.send_message(chat_id, LOCALE.t("ru", "menu.duel_help"))
                    .await?;
            }
            "lang_info" => {
                bot.send_message(chat_id, "Смените язык командой /lang ru (доступны: ru).")
                    .await?;
            }
            _ => {}
        }
    }
    Ok(())
}

// ── /lang handler ──────────────────────────────────────────
pub async fn lang_handler(
    bot: Bot,
    msg: Message,
    cmd: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    if !msg.chat.is_group() && !msg.chat.is_supergroup() {
        bot.send_message(msg.chat.id, "Язык можно менять только в групповых чатах.")
            .await?;
        return Ok(());
    }

    let user_id = match msg.from.as_ref() {
        Some(u) => u.id.0,
        None => return Ok(()),
    };

    if !game_commands::is_chat_admin(&bot, msg.chat.id, user_id).await {
        bot.send_message(msg.chat.id, "Только администраторы могут менять язык чата.")
            .await?;
        return Ok(());
    }

    let arg = match &cmd {
        crate::handlers::commands::Cmd::Lang(s) => s.trim().to_lowercase(),
        _ => String::new(),
    };

    if arg.is_empty() {
        bot.send_message(msg.chat.id, "Использование: /lang ru")
            .await?;
        return Ok(());
    }

    const SUPPORTED: &[&str] = &["ru"];
    if !SUPPORTED.contains(&arg.as_str()) {
        bot.send_message(msg.chat.id, format!("Неизвестный язык. Доступны: {}", SUPPORTED.join(", ")))
            .await?;
        return Ok(());
    }

    crate::db::game::set_chat_lang(&pool, msg.chat.id.0, &arg).await?;
    bot.send_message(msg.chat.id, format!("Язык чата изменён на: {arg}"))
        .await?;
    Ok(())
}
