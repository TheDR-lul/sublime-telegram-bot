use teloxide::prelude::*;
use teloxide::sugar::request::RequestLinkPreviewExt;
use teloxide::types::{Message, ThreadId};

use crate::error::AppError;
use crate::i18n::LOCALE;
use crate::telegram::topic_routing::topic_thread_id;

pub async fn about_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
) -> Result<(), AppError> {
    send_about(&bot, msg.chat.id, topic_thread_id(&msg)).await
}

pub async fn send_about(
    bot: &Bot,
    chat_id: teloxide::types::ChatId,
    thread_id: Option<ThreadId>,
) -> Result<(), AppError> {
    let mut request = bot
        .send_message(chat_id, LOCALE.t("ru", "about.text"))
        .parse_mode(teloxide::types::ParseMode::Html)
        .disable_link_preview(true);
    if let Some(thread) = thread_id {
        request = request.message_thread_id(thread);
    }
    request.await?;
    Ok(())
}
