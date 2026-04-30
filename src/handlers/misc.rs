use sqlx::PgPool;
use teloxide::prelude::*;
use teloxide::sugar::request::RequestLinkPreviewExt;
use teloxide::types::{
    CallbackQuery, ChatId, InlineKeyboardButton, InlineKeyboardMarkup, Message, MessageId,
    ReplyParameters, ThreadId,
};

use crate::config::Config;
use crate::error::AppError;
use crate::handlers::about;
use crate::handlers::game::commands as game_commands;
use crate::i18n::LOCALE;
use crate::telegram::topic_routing::send_text_in_origin_topic;
use crate::db::chat_topics;
use crate::db::kv;

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

fn topic_thread_id(msg: &Message) -> Option<ThreadId> {
    if let Some(thread_id) = msg.thread_id {
        return Some(thread_id);
    }

    if let Some(reply) = msg.reply_to_message() {
        if let Some(thread_id) = reply.thread_id {
            return Some(thread_id);
        }
    }

    None
}

const MAIN_TOPIC_KEY: &str = "bot_main_topic_id";

async fn get_main_topic_id(pool: &PgPool, chat_id: i64) -> Result<Option<i64>, AppError> {
    let raw = kv::get(pool, chat_id, MAIN_TOPIC_KEY)
        .await?
        .map(|v| v.value);
    Ok(raw.and_then(|s| s.parse::<i64>().ok()))
}

async fn refresh_topics_menu_message(
    bot: &Bot,
    pool: &PgPool,
    chat_id: ChatId,
    message_id: Option<MessageId>,
    regular_msg: Option<&Message>,
) -> Result<(), AppError> {
    if let Some(mid) = message_id {
        let (text, markup) = build_topics_menu_view(pool, chat_id.0, regular_msg).await?;
        // Ignore edit race errors (e.g. outdated message state after fast taps).
        let _ = bot
            .edit_message_text(chat_id, mid, text)
            .reply_markup(markup)
            .await;
    }
    Ok(())
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
        send_text_in_origin_topic(&bot, &msg, LOCALE.t("ru", "slap.no_reply")).await?;
        return Ok(());
    };
    
    let text = LOCALE.t_rand_fmt("ru", "slap.phrases", &[("who", &who), ("target", &target)]);
    
    let mut request = bot.send_message(msg.chat.id, text).parse_mode(ParseMode::Html);
    if let Some(thread) = topic_thread_id(&msg) {
        request = request.message_thread_id(thread);
    }
    request.await?;
    Ok(())
}

pub async fn shrug_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
) -> Result<(), AppError> {
    send_text_in_origin_topic(&bot, &msg, r"¯\_(ツ)_/¯").await?;
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
    let mut request = bot.send_message(msg.chat.id, text).parse_mode(ParseMode::Html);
    if let Some(thread) = topic_thread_id(&msg) {
        request = request.message_thread_id(thread);
    }
    request.await?;
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
        send_text_in_origin_topic(&bot, &msg, LOCALE.t("ru", "inline.google_no_query")).await?;
        return Ok(());
    }
    let url = format!("https://lmgtfy.com/?q={}", url::form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>());
    let mut request = bot.send_message(msg.chat.id, url).disable_link_preview(true);
    if let Some(thread) = topic_thread_id(&msg) {
        request = request.message_thread_id(thread);
    }
    request.await?;
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
    send_text_in_origin_topic(&bot, &msg, intro).await?;

    tokio::time::sleep(tokio::time::Duration::from_millis(700)).await;
    let analysis = LOCALE.t_rand("ru", "pidorscan.analysis");
    send_text_in_origin_topic(&bot, &msg, analysis).await?;

    tokio::time::sleep(tokio::time::Duration::from_millis(850)).await;
    let algo = LOCALE.t_rand("ru", "pidorscan.algo");
    send_text_in_origin_topic(&bot, &msg, algo).await?;

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

    let mut request = bot.send_message(msg.chat.id, verdict).parse_mode(ParseMode::Html);
    if let Some(thread) = topic_thread_id(&msg) {
        request = request.message_thread_id(thread);
    }
    request.await?;

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
        vec![InlineKeyboardButton::callback("⚓ Уведомления штурвала", "menu:action:helm_notify_toggle")],
        vec![InlineKeyboardButton::callback("🧵 Топики бота", "menu:action:topics")],
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

async fn build_topics_menu_view(
    pool: &PgPool,
    chat_id_raw: i64,
    regular_msg: Option<&Message>,
) -> Result<(String, InlineKeyboardMarkup), AppError> {
    let topics = chat_topics::list_topics(pool, chat_id_raw).await?;
    let count = topics.len();
    let main_topic_id = get_main_topic_id(pool, chat_id_raw).await?;

    let mut text = String::new();
    text.push_str("Активные топики бота в этом чате:\n");
    if count == 0 {
        text.push_str("— нет ни одного активного топика.\n");
    } else {
        for (idx, topic_id) in topics.iter().enumerate() {
            let mark = if Some(*topic_id) == main_topic_id {
                " (основной)"
            } else {
                ""
            };
            text.push_str(&format!("{}. topic_id = {}{}\n", idx + 1, topic_id, mark));
        }
    }
    if main_topic_id.is_none() {
        text.push_str("\nОсновной топик: не выбран.\n");
    }
    text.push_str(
        "\nМаксимум: 3 топика на чат.\nВключать /bothere внутри нужного топика, выключать /bothereoff.\nФоновые сообщения бота идут в основной топик (если выбран).",
    );

    let mut rows = Vec::new();
    if let Some(msg) = regular_msg
        && let Some(thread) = topic_thread_id(msg)
    {
        let current_topic_id = i64::from(thread.0 .0);
        let current_enabled = chat_topics::is_topic_enabled(pool, chat_id_raw, current_topic_id).await?;
        let label;
        let data;

        if current_enabled {
            label = "🔴 Выключить бота в этом топике";
            data = "topics:toggle_current";
        } else if count < 3 {
            label = "🟢 Включить бота в этом топике";
            data = "topics:toggle_current";
        } else {
            label = "🔒 Лимит 3 топика (сначала выключите другой)";
            data = "topics:limit";
        }
        rows.push(vec![InlineKeyboardButton::callback(label, data)]);

        if current_enabled {
            rows.push(vec![InlineKeyboardButton::callback(
                "⭐ Сделать этот топик основным",
                "topics:set_main_current",
            )]);
        }
    }

    for topic_id in topics.iter() {
        let label = format!("✖ Выключить topic_id = {}", topic_id);
        let data = format!("topics:disable:{}", topic_id);
        rows.push(vec![InlineKeyboardButton::callback(label, data)]);
        if Some(*topic_id) != main_topic_id {
            rows.push(vec![InlineKeyboardButton::callback(
                format!("⭐ Сделать основным topic_id = {}", topic_id),
                format!("topics:set_main:{}", topic_id),
            )]);
        }
    }

    if main_topic_id.is_some() {
        rows.push(vec![InlineKeyboardButton::callback(
            "🚫 Сбросить основной топик",
            "topics:clear_main",
        )]);
    }

    rows.push(vec![InlineKeyboardButton::callback("← Назад", "menu:admin")]);
    Ok((text, InlineKeyboardMarkup::new(rows)))
}

pub async fn menu_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
) -> Result<(), AppError> {
    let chat_id = msg.chat.id;
    let thread_id = topic_thread_id(&msg);

    let _ = bot.delete_message(chat_id, msg.id).await;

    let mut request = bot
        .send_message(chat_id, LOCALE.t("ru", "menu.choose_section"))
        .reply_markup(menu_main_keyboard());

    if let Some(thread) = thread_id {
        request = request.message_thread_id(thread);
    }

    let sent = request.await?;
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
    let regular_msg = query.regular_message().cloned();

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
                about::send_about(
                    &bot,
                    chat_id,
                    regular_msg
                        .as_ref()
                        .and_then(|msg| msg.thread_id.or_else(|| msg.reply_to_message().and_then(|r| r.thread_id))),
                )
                .await?;
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
            "helm_notify_toggle" => {
                let user_id = query.from.id.0 as u64;
                if chat_id.0 >= 0 {
                    bot.send_message(chat_id, LOCALE.t("ru", "menu.group_only"))
                        .await?;
                } else if !game_commands::is_chat_admin(&bot, chat_id, user_id).await {
                    bot.send_message(chat_id, LOCALE.t("ru", "menu.admin_only"))
                        .await?;
                } else {
                    let key = "huya_dutch_helm_notify";
                    let current_enabled = kv::get(&pool, chat_id.0, key)
                        .await?
                        .map(|v| v.value != "0")
                        .unwrap_or(true);
                    let new_enabled = !current_enabled;
                    kv::set(
                        &pool,
                        chat_id.0,
                        key,
                        if new_enabled { "1" } else { "0" },
                    )
                    .await?;
                    let text = if new_enabled {
                        "Уведомления штурвала включены."
                    } else {
                        "Уведомления штурвала выключены."
                    };
                    bot.send_message(chat_id, text).await?;
                }
            }
            "topics" => {
                let chat_id_raw = chat_id.0;
                let (text, markup) = build_topics_menu_view(&pool, chat_id_raw, regular_msg.as_ref()).await?;
                if let Some(mid) = message_id {
                    bot.edit_message_text(chat_id, mid, text)
                        .reply_markup(markup)
                        .await?;
                    schedule_menu_delete(chat_id, mid);
                } else {
                    bot.send_message(chat_id, text)
                        .reply_markup(markup)
                        .await?;
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
        return Ok(());
    }

    if let Some(rest) = data.strip_prefix("topics:") {
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

        let query_id = query.id.clone();
        let parts: Vec<&str> = rest.split(':').collect();
        match parts.as_slice() {
            ["toggle_current"] => {
                bot.answer_callback_query(query_id.clone()).await?;
                let msg = match regular_msg.as_ref() {
                    Some(m) => m,
                    None => return Ok(()),
                };
                let thread = match topic_thread_id(msg) {
                    Some(t) => t,
                    None => return Ok(()),
                };
                let chat_id_raw = chat_id.0;
                let topic_id = i64::from(thread.0 .0);
                let enabled = chat_topics::is_topic_enabled(&pool, chat_id_raw, topic_id).await?;
                if enabled {
                    chat_topics::remove_topic(&pool, chat_id_raw, topic_id).await?;
                    if get_main_topic_id(&pool, chat_id_raw).await? == Some(topic_id) {
                        let _ = kv::del(&pool, chat_id_raw, MAIN_TOPIC_KEY).await?;
                    }
                } else {
                    let count = chat_topics::count_enabled_topics(&pool, chat_id_raw).await?;
                    if count >= 3 {
                        bot.answer_callback_query(query_id)
                            .text("Лимит 3 топика. Сначала выключите бот в другом топике.")
                            .await?;
                        return Ok(());
                    }
                    chat_topics::add_topic(&pool, chat_id_raw, topic_id).await?;
                }
            }
            ["disable", id_str] => {
                bot.answer_callback_query(query.id).await?;
                if let Ok(topic_id) = id_str.parse::<i64>() {
                    let chat_id_raw = chat_id.0;
                    if chat_topics::is_topic_enabled(&pool, chat_id_raw, topic_id).await? {
                        chat_topics::remove_topic(&pool, chat_id_raw, topic_id).await?;
                        if get_main_topic_id(&pool, chat_id_raw).await? == Some(topic_id) {
                            let _ = kv::del(&pool, chat_id_raw, MAIN_TOPIC_KEY).await?;
                        }
                    }
                }
            }
            ["set_main_current"] => {
                bot.answer_callback_query(query_id.clone()).await?;
                let msg = match regular_msg.as_ref() {
                    Some(m) => m,
                    None => return Ok(()),
                };
                let thread = match topic_thread_id(msg) {
                    Some(t) => t,
                    None => return Ok(()),
                };
                let chat_id_raw = chat_id.0;
                let topic_id = i64::from(thread.0 .0);
                if !chat_topics::is_topic_enabled(&pool, chat_id_raw, topic_id).await? {
                    bot.answer_callback_query(query_id)
                        .text("Сначала включите бота в этом топике.")
                        .await?;
                    return Ok(());
                }
                kv::set(&pool, chat_id_raw, MAIN_TOPIC_KEY, &topic_id.to_string()).await?;
                bot.answer_callback_query(query_id)
                    .text("Топик назначен основным.")
                    .await?;
            }
            ["set_main", id_str] => {
                bot.answer_callback_query(query_id.clone()).await?;
                if let Ok(topic_id) = id_str.parse::<i64>() {
                    let chat_id_raw = chat_id.0;
                    if chat_topics::is_topic_enabled(&pool, chat_id_raw, topic_id).await? {
                        kv::set(&pool, chat_id_raw, MAIN_TOPIC_KEY, &topic_id.to_string()).await?;
                    } else {
                        bot.answer_callback_query(query_id.clone())
                            .text("Этот топик не активен для бота.")
                            .await?;
                    }
                }
            }
            ["clear_main"] => {
                bot.answer_callback_query(query.id).await?;
                let chat_id_raw = chat_id.0;
                let _ = kv::del(&pool, chat_id_raw, MAIN_TOPIC_KEY).await?;
            }
            ["limit"] => {
                bot.answer_callback_query(query_id)
                    .text("Лимит 3 топика. Сначала выключите бот в другом топике.")
                    .await?;
            }
            _ => {
                bot.answer_callback_query(query_id).await?;
            }
        }
        if let Some(mid) = message_id {
            refresh_topics_menu_message(&bot, &pool, chat_id, message_id, regular_msg.as_ref()).await?;
            schedule_menu_delete(chat_id, mid);
        }
        return Ok(());
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
        send_text_in_origin_topic(&bot, &msg, "Язык можно менять только в групповых чатах.").await?;
        return Ok(());
    }

    let user_id = match msg.from.as_ref() {
        Some(u) => u.id.0,
        None => return Ok(()),
    };

    if !game_commands::is_chat_admin(&bot, msg.chat.id, user_id).await {
        send_text_in_origin_topic(&bot, &msg, "Только администраторы могут менять язык чата.").await?;
        return Ok(());
    }

    let arg = match &cmd {
        crate::handlers::commands::Cmd::Lang(s) => s.trim().to_lowercase(),
        _ => String::new(),
    };

    if arg.is_empty() {
        send_text_in_origin_topic(&bot, &msg, "Использование: /lang ru").await?;
        return Ok(());
    }

    const SUPPORTED: &[&str] = &["ru"];
    if !SUPPORTED.contains(&arg.as_str()) {
        send_text_in_origin_topic(
            &bot,
            &msg,
            format!("Неизвестный язык. Доступны: {}", SUPPORTED.join(", ")),
        )
        .await?;
        return Ok(());
    }

    crate::db::game::set_chat_lang(&pool, msg.chat.id.0, &arg).await?;
    send_text_in_origin_topic(&bot, &msg, format!("Язык чата изменён на: {arg}")).await?;
    Ok(())
}

pub async fn bothere_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    let chat = &msg.chat;
    if !chat.is_supergroup() {
        return Ok(());
    }
    let thread_id = match topic_thread_id(&msg) {
        Some(id) => id,
        None => {
            let reply = ReplyParameters {
                message_id: msg.id,
                chat_id: None,
                allow_sending_without_reply: None,
                quote: None,
                quote_parse_mode: None,
                quote_entities: None,
                quote_position: None,
            };
            bot.send_message(msg.chat.id, "This command must be used inside a topic.")
                .reply_parameters(reply)
                .await?;
            return Ok(());
        }
    };
    let topic_id = i64::from(thread_id.0 .0);

    let from = match msg.from.as_ref() {
        Some(u) => u,
        None => return Ok(()),
    };

    if !game_commands::is_chat_admin(&bot, msg.chat.id, from.id.0 as u64).await {
        send_text_in_origin_topic(
            &bot,
            &msg,
            "Only chat administrators can enable the bot in a topic.",
        )
        .await?;
        return Ok(());
    }

    let chat_id = msg.chat.id.0;
    let current_count = chat_topics::count_enabled_topics(&pool, chat_id).await?;
    if chat_topics::is_topic_enabled(&pool, chat_id, topic_id).await? {
        let mut request = bot.send_message(msg.chat.id, "The bot is already enabled in this topic.");
        request = request.message_thread_id(thread_id);
        request.await?;
        return Ok(());
    }
    if current_count >= 3 {
        let mut request = bot.send_message(
            msg.chat.id,
            "Topic limit reached (3 per chat). Disable the bot in another topic first with /bothereoff.",
        );
        request = request.message_thread_id(thread_id);
        request.await?;
        return Ok(());
    }

    chat_topics::add_topic(&pool, chat_id, topic_id).await?;
    let mut request = bot.send_message(msg.chat.id, "The bot is now enabled in this topic.");
    request = request.message_thread_id(thread_id);
    request.await?;
    Ok(())
}

pub async fn bothereoff_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    let chat = &msg.chat;
    if !chat.is_supergroup() {
        return Ok(());
    }

    let from = match msg.from.as_ref() {
        Some(u) => u,
        None => return Ok(()),
    };

    if !game_commands::is_chat_admin(&bot, msg.chat.id, from.id.0 as u64).await {
        send_text_in_origin_topic(
            &bot,
            &msg,
            "Only chat administrators can disable the bot in a topic.",
        )
        .await?;
        return Ok(());
    }

    let thread_id = match topic_thread_id(&msg) {
        Some(id) => id,
        None => {
            // If the command is used outside of any topic, treat it as
            // "turn the bot off in all topics in this chat" for convenience.
            let chat_id = msg.chat.id.0;
            let active_topics = chat_topics::list_topics(&pool, chat_id).await?;

            if active_topics.is_empty() {
                send_text_in_origin_topic(
                    &bot,
                    &msg,
                    "The bot is already disabled in all topics in this chat.",
                )
                .await?;
                return Ok(());
            }

            for topic_id in &active_topics {
                chat_topics::remove_topic(&pool, chat_id, *topic_id).await?;
            }
            let _ = kv::del(&pool, chat_id, MAIN_TOPIC_KEY).await?;

            let reply = ReplyParameters {
                message_id: msg.id,
                chat_id: None,
                allow_sending_without_reply: None,
                quote: None,
                quote_parse_mode: None,
                quote_entities: None,
                quote_position: None,
            };

            bot.send_message(
                msg.chat.id,
                format!(
                    "The bot is now disabled in all topics in this chat ({} total).",
                    active_topics.len()
                ),
            )
            .reply_parameters(reply)
            .await?;

            return Ok(());
        }
    };
    let topic_id = i64::from(thread_id.0 .0);

    let chat_id = msg.chat.id.0;
    if !chat_topics::is_topic_enabled(&pool, chat_id, topic_id).await? {
        let mut request = bot.send_message(msg.chat.id, "The bot is already disabled in this topic.");
        request = request.message_thread_id(thread_id);
        request.await?;
        return Ok(());
    }

    chat_topics::remove_topic(&pool, chat_id, topic_id).await?;
    if get_main_topic_id(&pool, chat_id).await? == Some(topic_id) {
        let _ = kv::del(&pool, chat_id, MAIN_TOPIC_KEY).await?;
    }
    let mut request = bot.send_message(msg.chat.id, "The bot is now disabled in this topic.");
    request = request.message_thread_id(thread_id);
    request.await?;
    Ok(())
}
