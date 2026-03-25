use crate::db::kv;
use crate::error::AppError;
use teloxide::prelude::Requester;

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
    if let Err(err) = bot
        .send_message(teloxide::types::ChatId(target), text.to_string())
        .await
    {
        tracing::warn!("failed to send alert message to {}: {:?}", target, err);
    }
    Ok(())
}
