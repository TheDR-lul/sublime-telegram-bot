use teloxide::prelude::*;
use teloxide::sugar::request::RequestLinkPreviewExt;
use teloxide::types::Message;

use crate::error::AppError;

const ABOUT_HTML: &str = "Бот для игры «Пидор Дня» и развлечений в чате. Исходный код: <a href=\"https://github.com/TheDR-lul/sublime\">GitHub</a>";

pub async fn about_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
) -> Result<(), AppError> {
    send_about(&bot, msg.chat.id).await
}

pub async fn send_about(bot: &Bot, chat_id: teloxide::types::ChatId) -> Result<(), AppError> {
    bot.send_message(chat_id, ABOUT_HTML)
        .parse_mode(teloxide::types::ParseMode::Html)
        .disable_link_preview(true)
        .await?;
    Ok(())
}
