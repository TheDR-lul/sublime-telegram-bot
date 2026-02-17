use teloxide::prelude::*;
use teloxide::types::Message;

use crate::error::AppError;

use rand::seq::SliceRandom;
use rand::{rngs::StdRng, Rng, SeedableRng};
use teloxide::types::ParseMode;
use teloxide::utils::markdown::escape as escape_md2;

pub async fn hello_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
) -> Result<(), AppError> {
    let name = msg
        .from()
        .map(|u| u.first_name.as_str())
        .unwrap_or("there");
    bot.send_message(msg.chat.id, format!("Hello, {}!", name))
        .await?;
    Ok(())
}

fn raw_name_from_msg(msg: &Message) -> String {
    msg.from()
        .map(|u| {
            u.username
                .as_deref()
                .unwrap_or(u.first_name.as_str())
                .to_string()
        })
        .unwrap_or_else(|| "someone".to_string())
}

/// Escape for MarkdownV2: escape _ * [ ] ( ) ~ ` > # + - = | { } . !
fn escape_md2_local(s: &str) -> String {
    escape_md2(s)
}

pub async fn slap_handler(
    bot: Bot,
    msg: Message,
    cmd: crate::handlers::commands::Cmd,
) -> Result<(), AppError> {
    let who = raw_name_from_msg(&msg);
    let target = match &cmd {
        crate::handlers::commands::Cmd::Slap(s) => escape_md2(s),
        _ => "void".to_string(),
    };
    let text = format!(r"\*{}* slaps _{}_ around a bit with a large trout", who, target);
    bot.send_message(msg.chat.id, text)
        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
        .await?;
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
    let who = raw_name_from_msg(&msg);
    let text = match &cmd {
        crate::handlers::commands::Cmd::Me(s) => {
            if s.is_empty() {
                return Ok(());
            }
            format!(r"\*{}* {}", who, escape_md2(s))
        }
        _ => return Ok(()),
    };
    bot.send_message(msg.chat.id, text)
        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
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
        .disable_web_page_preview(true)
        .await?;
    Ok(())
}

pub async fn pin_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
) -> Result<(), AppError> {
    if let Some(reply_to) = msg.reply_to_message() {
        match bot.pin_chat_message(msg.chat.id, reply_to.id).await {
            Ok(_) => {}
            Err(e) => {
                tracing::warn!("Failed to pin message: {:?}", e);
                bot.send_message(msg.chat.id, "Не могу закрепить: нет прав или сообщение уже закреплено")
                    .await?;
            }
        }
    } else {
        bot.send_message(msg.chat.id, r"reply to message you want to _pin_")
            .parse_mode(teloxide::types::ParseMode::MarkdownV2)
            .await?;
    }
    Ok(())
}

pub async fn echo_handler(
    bot: Bot,
    msg: Message,
    cmd: crate::handlers::commands::Cmd,
) -> Result<(), AppError> {
    let text = match &cmd {
        crate::handlers::commands::Cmd::Echo(s) => {
            let name = msg.from().map(|u| u.full_name()).unwrap_or_else(|| "someone".to_string());
            format!("{} said {}", name, s)
        }
        _ => return Ok(()),
    };
    bot.send_message(msg.chat.id, text).await?;
    Ok(())
}

pub async fn pidorscan_handler(
    bot: Bot,
    msg: Message,
    cmd: crate::handlers::commands::Cmd,
) -> Result<(), AppError> {
    let target_name = if let Some(reply) = msg.reply_to_message() {
        reply
            .from()
            .map(|u| u.full_name())
            .unwrap_or_else(|| "кто-то".to_string())
    } else {
        match &cmd {
            crate::handlers::commands::Cmd::Pidorscan(s) if !s.trim().is_empty() => s.trim().to_string(),
            _ => msg
                .from()
                .map(|u| u.full_name())
                .unwrap_or_else(|| "кто-то".to_string()),
        }
    };

    let mut rng = StdRng::from_entropy();
    let percent: u8 = rng.gen_range(0..=100);

    let intro_templates = vec![
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
    let analysis_templates = vec![
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
    let algo_templates = vec![
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

    let name_escaped = escape_md2_local(&target_name);

    let verdict = if percent == 0 {
        format!(
            "*Вердикт:* {} вообще не пидор\n_Подозрительно, конечно_",
            name_escaped
        )
    } else if percent < 30 {
        format!(
            "*Вердикт:* вероятность, что {} пидор — *{}%*\nПока живи, но мы за тобой следим",
            name_escaped, percent
        )
    } else if percent < 70 {
        format!(
            "*Вердикт:* {} пидор на *{}%*\nЕщё чуть-чуть — и мама расстроится",
            name_escaped, percent
        )
    } else if percent < 100 {
        format!(
            "*Вердикт:* {} пидор примерно на *{}%*\nЭто уже почти приговор",
            name_escaped, percent
        )
    } else {
        format!(
            "*Вердикт:* {} — *100% пидор*\nБез права на обжалование",
            name_escaped
        )
    };

    bot.send_message(msg.chat.id, verdict)
        .parse_mode(ParseMode::MarkdownV2)
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

    use rand::seq::SliceRandom;
    use rand::{rngs::StdRng, SeedableRng};
    use teloxide::types::{InlineQueryResult, InlineQueryResultArticle, InputMessageContent, InputMessageContentText};

    let mut shuffled = Vec::new();
    let re = regex::Regex::new(r"([^\W\d_]{4,})").unwrap();
    for word in re.split(q) {
        if word.chars().all(|c| c.is_alphanumeric()) && word.len() >= 4 {
            let mut chars: Vec<char> = word.chars().collect();
            let first = chars.remove(0);
            let last = chars.pop().unwrap();
            let mut rng = StdRng::from_entropy();
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
                disable_web_page_preview: None,
            }),
            reply_markup: None,
            url: None,
            hide_url: None,
            thumb_url: None,
            thumb_width: None,
            thumb_height: None,
        }),
        InlineQueryResult::Article(InlineQueryResultArticle {
            id: uuid::Uuid::new_v4().to_string(),
            title: "Caps".to_string(),
            description: Some("Make query text upper case".to_string()),
            input_message_content: InputMessageContent::Text(InputMessageContentText {
                message_text: q.to_uppercase(),
                parse_mode: None,
                entities: None,
                disable_web_page_preview: None,
            }),
            reply_markup: None,
            url: None,
            hide_url: None,
            thumb_url: None,
            thumb_width: None,
            thumb_height: None,
        }),
        InlineQueryResult::Article(InlineQueryResultArticle {
            id: uuid::Uuid::new_v4().to_string(),
            title: "Shuffle".to_string(),
            description: Some("Shuffle all the letters inside words".to_string()),
            input_message_content: InputMessageContent::Text(InputMessageContentText {
                message_text: shuffled.join(""),
                parse_mode: None,
                entities: None,
                disable_web_page_preview: None,
            }),
            reply_markup: None,
            url: None,
            hide_url: None,
            thumb_url: None,
            thumb_width: None,
            thumb_height: None,
        }),
    ];

    bot.answer_inline_query(&query.id, results).cache_time(0).await?;
    Ok(())
}
