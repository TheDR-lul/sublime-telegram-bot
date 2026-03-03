use sqlx::PgPool;
use teloxide::prelude::*;
use teloxide::sugar::request::RequestLinkPreviewExt;
use teloxide::types::{CallbackQuery, ChatId, InlineKeyboardButton, InlineKeyboardMarkup, Message, MessageId};

use crate::config::Config;
use crate::error::AppError;
use crate::handlers::about;
use crate::handlers::game::commands as game_commands;

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
        bot.send_message(msg.chat.id, "Ответь на сообщение, чтобы шлепнуть кого-то!")
            .await?;
        return Ok(());
    };
    
    let mut rng = rand::make_rng::<rand::rngs::StdRng>();
    let phrase = slap_phrases::PHRASES
        .choose(&mut rng)
        .expect("slap_phrases::PHRASES is non-empty");
    let text = phrase.replace("{who}", &who).replace("{target}", &target);
    
    bot.send_message(msg.chat.id, text)
        .parse_mode(ParseMode::Html)
        .await?;
    Ok(())
}

mod slap_phrases {
    pub const PHRASES: &[&str] = &[
        "<b>{who}</b> смачно шлепнул хуйцом по лицу <i>{target}</i>",
        "<b>{who}</b> мощно вмазал хуйцом в рожу <i>{target}</i>",
        "<b>{who}</b> со всей дури ударил хуйцом по физиономии <i>{target}</i>",
        "<b>{who}</b> от души приложил хуйцом к лицу <i>{target}</i>",
        "<b>{who}</b> звонко шлепнул хуйцом по щеке <i>{target}</i>",
        "<b>{who}</b> резко врезал хуйцом в морду <i>{target}</i>",
        "<b>{who}</b> сочно ударил хуйцом по лицу <i>{target}</i>",
        "<b>{who}</b> мощно треснул хуйцом по физиономии <i>{target}</i>",
        "<b>{who}</b> с размаху влепил хуйцом в рожу <i>{target}</i>",
        "<b>{who}</b> крепко шлепнул хуйцом по лицу <i>{target}</i>",
        "<b>{who}</b> звучно ударил хуйцом по щеке <i>{target}</i>",
        "<b>{who}</b> со всей силы вмазал хуйцом в морду <i>{target}</i>",
        "<b>{who}</b> резко приложил хуйцом к физиономии <i>{target}</i>",
        "<b>{who}</b> мощно врезал хуйцом по лицу <i>{target}</i>",
        "<b>{who}</b> смачно треснул хуйцом в рожу <i>{target}</i>",
    ];
}

/// RPG disabled stub: development for future.
pub const RPG_DISABLED_MSG: &str = "Pidor-Royale RPG — в разработке на будущее. Следите за обновлениями.";

pub async fn rpg_disabled_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
) -> Result<(), AppError> {
    bot.send_message(msg.chat.id, RPG_DISABLED_MSG).await?;
    Ok(())
}

pub async fn rpg_disabled_callback(
    bot: Bot,
    query: teloxide::types::CallbackQuery,
) -> Result<(), AppError> {
    if let Some(chat_id) = query.message.as_ref().map(|m| m.chat().id) {
        bot.send_message(chat_id, RPG_DISABLED_MSG).await?;
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
        bot.send_message(msg.chat.id, "What should I search for?")
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

    let intro_templates = [
        "Запускаю пидор-детектор для {}...",
        "Так, ну-ка подойдите поближе, {}... запускаю сканер.",
        "Подключаюсь к базам пидоров РФ, {}...",
        "Включаю режим *глубокого* пидор-сканирования для {}...",
        "Сканирую аурочку {} на предмет пидорства...",
    ];
    let intro_raw = intro_templates
        .choose(&mut rng)
        .copied()
        .unwrap_or("Запускаю пидор-детектор для {}...");
    let intro = intro_raw.replace("{}", &target_name);
    bot.send_message(msg.chat.id, intro).await?;

    tokio::time::sleep(tokio::time::Duration::from_millis(700)).await;
    let analysis_templates = [
        "Анализирую историю сообщений, мемы и карму...",
        "Считаю количество /pidor и жалоб в чате...",
        "Смотрю, сколько раз этот персонаж уже оправдывался, что он \"не пидор\"...",
        "Подгружаю статистику позора из облака...",
    ];
    let analysis = analysis_templates
        .choose(&mut rng)
        .copied()
        .unwrap_or("Анализирую историю сообщений, мемы и карму...");
    bot.send_message(msg.chat.id, analysis).await?;

    tokio::time::sleep(tokio::time::Duration::from_millis(850)).await;
    let algo_templates = [
        "Почти готово, подрубаю квантовый пидор-алгоритм...",
        "Сверяю сигнатуру пидора с эталоном Роскомпидора...",
        "Намешиваю немного машинного обучения и человеческой ненависти...",
        "Достаю старый добрый аналоговый пидор-детектор из 2016 года...",
    ];
    let algo = algo_templates
        .choose(&mut rng)
        .copied()
        .unwrap_or("Почти готово, подрубаю квантовый пидор-алгоритм...");
    bot.send_message(msg.chat.id, algo).await?;

    tokio::time::sleep(tokio::time::Duration::from_millis(1100)).await;

    let name_escaped = escape_html(&target_name);

    let verdict = if percent == 0 {
        format!(
            "<b>Вердикт:</b> {} вообще не пидор\n<i>Подозрительно, конечно</i>",
            name_escaped
        )
    } else if percent < 30 {
        format!(
            "<b>Вердикт:</b> вероятность, что {} пидор — <b>{}%</b>\nПока живи, но мы за тобой следим",
            name_escaped, percent
        )
    } else if percent < 70 {
        format!(
            "<b>Вердикт:</b> {} пидор на <b>{}%</b>\nЕщё чуть-чуть — и мама расстроится",
            name_escaped, percent
        )
    } else if percent < 100 {
        format!(
            "<b>Вердикт:</b> {} пидор примерно на <b>{}%</b>\nЭто уже почти приговор",
            name_escaped, percent
        )
    } else {
        format!(
            "<b>Вердикт:</b> {} — <b>100% пидор</b>\nБез права на обжалование",
            name_escaped
        )
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
            title: "Echo".to_string(),
            description: Some("Just echo what you have typed".to_string()),
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
            title: "Caps".to_string(),
            description: Some("Make query text upper case".to_string()),
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
            title: "Shuffle".to_string(),
            description: Some("Shuffle all the letters inside words".to_string()),
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
        vec![InlineKeyboardButton::callback("🎮 Игра", "menu:game")],
        vec![InlineKeyboardButton::callback("📋 Прочее", "menu:other")],
        vec![InlineKeyboardButton::callback("⚙ Админ", "menu:admin")],
    ])
}

fn menu_admin_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback("🔧 Настройки автопидора", "menu:action:pidorset")],
        vec![InlineKeyboardButton::callback("📢 Позвать участников", "menu:action:pidorcall")],
        vec![InlineKeyboardButton::callback("← Назад", "menu:main")],
    ])
}

fn menu_game_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback("📜 Правила", "menu:action:pidorules")],
        vec![InlineKeyboardButton::callback("⚔ Пидор-дуэль", "menu:action:pidorduel")],
        vec![
            InlineKeyboardButton::callback("📊 За год", "menu:action:pidorstats"),
            InlineKeyboardButton::callback("📊 Всё время", "menu:action:pidorall"),
        ],
        vec![InlineKeyboardButton::callback("← Назад", "menu:main")],
    ])
}

fn menu_other_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback("ℹ О боте", "menu:action:about")],
        vec![InlineKeyboardButton::callback("← Назад", "menu:main")],
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
        .send_message(chat_id, "Выберите раздел:")
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
            bot.edit_message_text(chat_id, mid, "Выберите раздел:")
                .reply_markup(menu_main_keyboard())
                .await?;
            schedule_menu_delete(chat_id, mid);
        }
        return Ok(());
    }
    if data == "menu:game" {
        bot.answer_callback_query(query.id).await?;
        if let Some(mid) = message_id {
            bot.edit_message_text(chat_id, mid, "Игра Пидор Дня")
                .reply_markup(menu_game_keyboard())
                .await?;
            schedule_menu_delete(chat_id, mid);
        }
        return Ok(());
    }
    if data == "menu:other" {
        bot.answer_callback_query(query.id).await?;
        if let Some(mid) = message_id {
            bot.edit_message_text(chat_id, mid, "Прочее")
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
                .text("Раздел только для групповых чатов.")
                .await?;
            return Ok(());
        }
        if !game_commands::is_chat_admin(&bot, chat_id, user_id).await {
            bot.answer_callback_query(query.id)
                .text("Только для администраторов чата.")
                .await?;
            return Ok(());
        }
        bot.answer_callback_query(query.id).await?;
        if let Some(mid) = message_id {
            bot.edit_message_text(chat_id, mid, "Администрирование")
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
                    bot.send_message(chat_id, "Настройки автопидора только в групповых чатах.")
                        .await?;
                } else if !game_commands::is_chat_admin(&bot, chat_id, user_id).await {
                    bot.send_message(chat_id, "Только администраторы чата могут менять настройки.")
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
                    bot.send_message(chat_id, "Команда только для групповых чатов.")
                        .await?;
                } else if !game_commands::is_chat_admin(&bot, chat_id, user_id).await {
                    bot.send_message(chat_id, "Только администраторы чата могут вызывать эту команду.")
                        .await?;
                } else {
                    game_commands::send_pidorcall_message(&bot, &pool, chat_id).await?;
                }
            }
            "pidorduel" => {
                bot.send_message(
                    chat_id,
                    "/pidorduel — искать любого соперника; ответь на сообщение юзера и напиши /pidorduel — вызвать конкретного. (1 мин на принятие.)",
                )
                .await?;
            }
            _ => {}
        }
    }
    Ok(())
}
