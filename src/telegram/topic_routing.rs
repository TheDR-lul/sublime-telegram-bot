use teloxide::prelude::*;
use teloxide::types::{Message, ThreadId};

use crate::error::AppError;

pub fn topic_thread_id(msg: &Message) -> Option<ThreadId> {
    msg.thread_id
        .or_else(|| msg.reply_to_message().and_then(|reply| reply.thread_id))
}

pub async fn send_text_in_origin_topic(
    bot: &Bot,
    msg: &Message,
    text: impl Into<String>,
) -> Result<teloxide::types::Message, AppError> {
    let mut request = bot.send_message(msg.chat.id, text.into());
    if let Some(thread) = topic_thread_id(msg) {
        request = request.message_thread_id(thread);
    }
    Ok(request.await?)
}
