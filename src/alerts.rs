use crate::db::kv;
use crate::error::AppError;
use std::sync::RwLock;
use teloxide::payloads::SendMessageSetters;
use teloxide::prelude::Requester;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RewardContext {
    pub chat_id: i64,
    pub tg_id: i64,
}

static LAST_REWARD_CONTEXT: std::sync::LazyLock<RwLock<Option<RewardContext>>> =
    std::sync::LazyLock::new(|| RwLock::new(None));

fn parse_i64_after(text: &str, key: &str) -> Option<i64> {
    let needle = format!("{key}=");
    let start = text.find(&needle)? + needle.len();
    let tail = &text[start..];
    let digits_len = tail
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-')
        .count();
    if digits_len == 0 {
        return None;
    }
    tail[..digits_len].parse::<i64>().ok()
}

pub fn parse_reward_context(text: &str) -> Option<RewardContext> {
    let chat_id = parse_i64_after(text, "chat_id")?;
    let tg_id = parse_i64_after(text, "tg_id")
        .or_else(|| parse_i64_after(text, "from_tg_id"))
        .or_else(|| parse_i64_after(text, "user_id"))?;
    Some(RewardContext { chat_id, tg_id })
}

pub fn set_last_reward_context(chat_id: i64, tg_id: i64) {
    if let Ok(mut guard) = LAST_REWARD_CONTEXT.write() {
        *guard = Some(RewardContext { chat_id, tg_id });
    }
}

pub fn inject_last_context(text: &str) -> String {
    if parse_reward_context(text).is_some() {
        return text.to_string();
    }
    if let Ok(guard) = LAST_REWARD_CONTEXT.read()
        && let Some(ctx) = *guard
    {
        return format!("{text} chat_id={} tg_id={}", ctx.chat_id, ctx.tg_id);
    }
    text.to_string()
}

pub async fn is_enabled(pool: &sqlx::PgPool) -> Result<bool, AppError> {
    Ok(kv::get(pool, 0, "watchdog_alerts_enabled")
        .await?
        .map(|x| x.value == "1")
        .unwrap_or(false))
}

pub async fn get_target(pool: &sqlx::PgPool) -> Result<Option<i64>, AppError> {
    if !is_enabled(pool).await? {
        return Ok(None);
    }
    let db_target = kv::get(pool, 0, "watchdog_alerts_recipient_tg_id")
        .await?
        .and_then(|x| x.value.parse::<i64>().ok());
    if db_target.is_some() {
        return Ok(db_target);
    }
    Ok(std::env::var("ALERT_CHAT_ID")
        .ok()
        .and_then(|v| v.parse::<i64>().ok()))
}

pub async fn notify(pool: &sqlx::PgPool, text: &str) -> Result<(), AppError> {
    let token = match std::env::var("NOTIFICATION_BOT_TOKEN") {
        Ok(v) if !v.trim().is_empty() => v,
        _ => return Ok(()),
    };
    let Some(target) = get_target(pool).await? else {
        return Ok(());
    };
    let bot = teloxide::Bot::new(token);
    let mut req = bot.send_message(teloxide::types::ChatId(target), text.to_string());
    if let Some(ctx) = parse_reward_context(text) {
        req = req.reply_markup(InlineKeyboardMarkup::new(vec![vec![
            InlineKeyboardButton::callback(
                "Reward trigger",
                format!("reward_err:{}:{}", ctx.chat_id, ctx.tg_id),
            ),
        ]]));
    }
    if let Err(err) = req.await {
        tracing::warn!("failed to send alert message to {}: {:?}", target, err);
    }
    Ok(())
}
