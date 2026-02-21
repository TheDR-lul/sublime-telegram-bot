use rand::prelude::*;
use reqwest::Client;
use std::time::Duration;
use teloxide::prelude::*;
use teloxide::types::{
    CallbackQuery, InlineKeyboardButton, InlineKeyboardMarkup, Message,
    MaybeInaccessibleMessage,
};

use crate::config::Config;
use crate::error::AppError;

const MEME_REFRESH: &str = "meme_en_refresh";
const MEME_SAVE: &str = "meme_en_save";
const MEMERU_REFRESH: &str = "meme_ru_refresh";
const MEMERU_SAVE: &str = "meme_ru_save";

fn generate_keyboard(link: &str, save_text: &str, refresh_text: &str) -> Result<InlineKeyboardMarkup, AppError> {
    let url = url::Url::parse(link)?;
    Ok(InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::url("🔗".to_string(), url),
        InlineKeyboardButton::callback("💾".to_string(), save_text.to_string()),
        InlineKeyboardButton::callback("🔁".to_string(), refresh_text.to_string()),
    ]]))
}

async fn get_random_en_meme() -> Result<(String, String), AppError> {
    let mut rng = rand::make_rng::<rand::rngs::StdRng>();
    if rng.random_bool(0.5) {
        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()?;
        match client.get("https://imgflip.com/ajax_img_flip").send().await {
            Ok(resp) => {
                if let Ok(body) = resp.text().await
                    && body.len() > 3
                {
                    let meme_id = &body[3..];
                    let meme_link = format!("https://i.imgflip.com/{}.jpg", meme_id);
                    let source_link = format!("https://imgflip.com/i/{}", meme_id);
                    return Ok((meme_link, source_link));
                }
            }
            Err(e) => {
                tracing::warn!("imgflip request failed: {:?}", e);
            }
        }
    }
    let meme_id = rng.random_range(6..=19791);
    let meme_link = format!("https://t.me/bestmemes/{}", meme_id);
    Ok((meme_link.clone(), meme_link))
}

fn get_random_ru_meme(config: &Config) -> String {
    let mut rng = rand::make_rng::<rand::rngs::StdRng>();
    if config.meme_ru_channels.is_empty() {
        return "https://t.me/beobanka/1000".to_string();
    }
    let channel = config
        .meme_ru_channels
        .choose(&mut rng)
        .expect("meme_ru_channels non-empty after is_empty check");
    let meme_id = rng.random_range(channel.start_id..=channel.end_id);
    format!("{}/{}", channel.url, meme_id)
}

pub async fn meme_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
) -> Result<(), AppError> {
    match get_random_en_meme().await {
        Ok((meme_link, source_link)) => {
            let url = url::Url::parse(&meme_link)?;
            bot.send_photo(msg.chat.id, teloxide::types::InputFile::url(url))
                .reply_markup(generate_keyboard(&source_link, MEME_SAVE, MEME_REFRESH)?)
                .await?;
        }
        Err(e) => {
            tracing::warn!("Failed to get meme: {:?}", e);
            bot.send_message(msg.chat.id, "Srry, smth went wrong(").await?;
        }
    }
    Ok(())
}

pub async fn memeru_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
    config: Config,
) -> Result<(), AppError> {
    let meme_link = get_random_ru_meme(&config);
    let url = url::Url::parse(&meme_link)?;
    match bot
        .send_photo(msg.chat.id, teloxide::types::InputFile::url(url))
        .reply_markup(generate_keyboard(&meme_link, MEMERU_SAVE, MEMERU_REFRESH)?)
        .await
    {
        Ok(_) => {}
        Err(e) => {
            tracing::warn!("Failed to send RU meme {}: {:?}", meme_link, e);
            bot.send_message(msg.chat.id, "Srry, smth went wrong(").await?;
        }
    }
    Ok(())
}

pub async fn meme_refresh_callback(
    bot: Bot,
    query: CallbackQuery,
) -> Result<(), AppError> {
    match get_random_en_meme().await {
        Ok((meme_link, source_link)) => {
            if let Some(msg) = &query.message {
                let chat_id = msg.chat().id;
                let message_id = msg.id();
                let url = url::Url::parse(&meme_link)?;
                match bot
                    .edit_message_media(
                        chat_id,
                        message_id,
                        teloxide::types::InputMedia::Photo(teloxide::types::InputMediaPhoto::new(
                            teloxide::types::InputFile::url(url),
                        )),
                    )
                    .reply_markup(generate_keyboard(&source_link, MEME_SAVE, MEME_REFRESH)?)
                    .await
                {
                    Ok(_) => {
                        bot.answer_callback_query(query.id.clone()).await?;
                    }
                    Err(e) => {
                        tracing::warn!("Failed to edit meme media: {:?}", e);
                        bot.answer_callback_query(query.id.clone())
                            .text("Error! Try again")
                            .await?;
                    }
                }
            }
        }
        Err(e) => {
            tracing::warn!("Failed to get meme for refresh: {:?}", e);
            bot.answer_callback_query(query.id.clone())
                .text("Error! Try again")
                .await?;
        }
    }
    Ok(())
}

pub async fn meme_save_callback(
    bot: Bot,
    query: CallbackQuery,
) -> Result<(), AppError> {
    if let Some(MaybeInaccessibleMessage::Regular(msg)) = &query.message {
        let msg = msg.as_ref();
        if let Some(markup) = msg.reply_markup()
            && let Some(row) = markup.inline_keyboard.first()
            && let Some(btn) = row.first()
        {
            let old_url = match &btn.kind {
                teloxide::types::InlineKeyboardButtonKind::Url(u) => Some(u.as_str()),
                _ => None,
            };
            if let Some(old_url) = old_url {
                match get_random_en_meme().await {
                            Ok((meme_link, source_link)) => {
                                let old_url_parsed = url::Url::parse(old_url)?;
                                bot.edit_message_reply_markup(msg.chat.id, msg.id)
                                    .reply_markup(InlineKeyboardMarkup::new(vec![vec![
                                        InlineKeyboardButton::url("🔗".to_string(), old_url_parsed),
                                    ]]))
                                    .await?;
                                let url = url::Url::parse(&meme_link)?;
                                bot.send_photo(
                                    msg.chat.id,
                                    teloxide::types::InputFile::url(url),
                                )
                                .reply_markup(generate_keyboard(&source_link, MEME_SAVE, MEME_REFRESH)?)
                                .await?;
                                bot.answer_callback_query(query.id.clone()).await?;
                            }
                            Err(e) => {
                                tracing::warn!("Failed to get meme for save: {:?}", e);
                                bot.answer_callback_query(query.id.clone())
                                    .text("Error! Try again")
                                    .await?;
                            }
                        }
            }
        }
    }
    Ok(())
}

pub async fn memeru_refresh_callback(
    bot: Bot,
    query: CallbackQuery,
    config: Config,
) -> Result<(), AppError> {
    let meme_link = get_random_ru_meme(&config);
    if let Some(msg) = &query.message {
        let chat_id = msg.chat().id;
        let message_id = msg.id();
        let url = url::Url::parse(&meme_link)?;
        match bot
            .edit_message_media(
                chat_id,
                message_id,
                teloxide::types::InputMedia::Photo(teloxide::types::InputMediaPhoto::new(
                    teloxide::types::InputFile::url(url),
                )),
            )
            .reply_markup(generate_keyboard(&meme_link, MEMERU_SAVE, MEMERU_REFRESH)?)
            .await
        {
            Ok(_) => {
                bot.answer_callback_query(query.id.clone()).await?;
            }
            Err(e) => {
                tracing::warn!("Failed to edit memeru media: {:?}", e);
                bot.answer_callback_query(query.id.clone())
                    .text("Error! Try again")
                    .await?;
            }
        }
    }
    Ok(())
}

pub async fn memeru_save_callback(
    bot: Bot,
    query: CallbackQuery,
    config: Config,
) -> Result<(), AppError> {
    if let Some(MaybeInaccessibleMessage::Regular(msg)) = &query.message {
        let msg = msg.as_ref();
        if let Some(markup) = msg.reply_markup()
            && let Some(row) = markup.inline_keyboard.first()
            && let Some(btn) = row.first()
        {
            let old_url = match &btn.kind {
                teloxide::types::InlineKeyboardButtonKind::Url(u) => Some(u.as_str()),
                _ => None,
            };
            if let Some(old_url) = old_url {
                let new_meme_link = get_random_ru_meme(&config);
                        let old_url_parsed = url::Url::parse(old_url)?;
                        match bot
                            .edit_message_reply_markup(msg.chat.id, msg.id)
                            .reply_markup(InlineKeyboardMarkup::new(vec![vec![
                                InlineKeyboardButton::url("🔗".to_string(), old_url_parsed),
                            ]]))
                            .await
                        {
                            Ok(_) => {
                                let url = url::Url::parse(&new_meme_link)?;
                                match bot
                                    .send_photo(
                                        msg.chat.id,
                                        teloxide::types::InputFile::url(url),
                                    )
                                    .reply_markup(generate_keyboard(
                                        &new_meme_link,
                                        MEMERU_SAVE,
                                        MEMERU_REFRESH,
                                    )?)
                                    .await
                                {
                                    Ok(_) => {
                                        bot.answer_callback_query(query.id.clone()).await?;
                                    }
                                    Err(e) => {
                                        tracing::warn!("Failed to send memeru: {:?}", e);
                                        bot.answer_callback_query(query.id.clone())
                                            .text("Error! Try again")
                                            .await?;
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!("Failed to edit markup: {:?}", e);
                                bot.answer_callback_query(query.id.clone())
                                    .text("Error! Try again")
                                    .await?;
                            }
                        }
            }
        }
    }
    Ok(())
}
