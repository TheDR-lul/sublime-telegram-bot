use sqlx::PgPool;
use teloxide::prelude::*;
use teloxide::types::Message;

use crate::db::kv;
use crate::error::AppError;

pub async fn get_handler(
    bot: Bot,
    msg: Message,
    cmd: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    let key = match &cmd {
        crate::handlers::commands::Cmd::Get(k) => k,
        _ => return Ok(()),
    };
    if key.is_empty() {
        list_handler(bot, msg, cmd, pool).await?;
        return Ok(());
    }
    let chat_id = msg.chat.id.0;
    match kv::get(&pool, chat_id, key).await? {
        Some(item) => {
            bot.send_message(msg.chat.id, &item.value).await?;
        }
        None => {
            bot.send_message(msg.chat.id, "no such key(").await?;
        }
    }
    Ok(())
}

pub async fn list_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    let chat_id = msg.chat.id.0;
    let items = kv::list(&pool, chat_id).await?;
    if items.is_empty() {
        bot.send_message(msg.chat.id, "no keys yet(").await?;
        return Ok(());
    }
    let mut text = String::new();
    for (i, item) in items.iter().enumerate() {
        text.push_str(&format!("{}) {} - {}\n", i + 1, item.key, item.value));
        if text.len() > 4096 {
            text.truncate(4090);
            text.push_str("\n... (обрезано)");
            break;
        }
    }
    bot.send_message(msg.chat.id, text).await?;
    Ok(())
}

pub async fn set_handler(
    bot: Bot,
    msg: Message,
    cmd: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    let (key, value) = match &cmd {
        crate::handlers::commands::Cmd::Set(s) => {
            let parts: Vec<&str> = s.splitn(2, ' ').collect();
            if parts.len() < 2 {
                bot.send_message(msg.chat.id, "provide key and value please")
                    .await?;
                return Ok(());
            }
            (parts[0], parts[1])
        }
        _ => return Ok(()),
    };
    let chat_id = msg.chat.id.0;
    kv::set(&pool, chat_id, key, value).await?;
    bot.send_message(msg.chat.id, format!("Key {} successfully added", key))
        .await?;
    Ok(())
}

pub async fn del_handler(
    bot: Bot,
    msg: Message,
    cmd: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    let key = match &cmd {
        crate::handlers::commands::Cmd::Del(k) => k,
        _ => return Ok(()),
    };
    if key.is_empty() {
        bot.send_message(msg.chat.id, "give me the keeeey").await?;
        return Ok(());
    }
    let chat_id = msg.chat.id.0;
    if kv::del(&pool, chat_id, key).await? {
        bot.send_message(msg.chat.id, format!("OK! Key {} successfully deleted", key))
            .await?;
    } else {
        bot.send_message(msg.chat.id, "no such key").await?;
    }
    Ok(())
}
