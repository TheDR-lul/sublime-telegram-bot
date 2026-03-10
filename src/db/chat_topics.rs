use sqlx::PgPool;

use crate::error::AppError;

pub async fn is_topic_enabled(pool: &PgPool, chat_id: i64, topic_id: i64) -> Result<bool, AppError> {
    let row: Option<(i64,)> = sqlx::query_as(
        "SELECT 1 FROM chat_topics WHERE chat_id = $1 AND topic_id = $2",
    )
    .bind(chat_id)
    .bind(topic_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.is_some())
}

pub async fn count_enabled_topics(pool: &PgPool, chat_id: i64) -> Result<i64, AppError> {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM chat_topics WHERE chat_id = $1",
    )
    .bind(chat_id)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

pub async fn add_topic(pool: &PgPool, chat_id: i64, topic_id: i64) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO chat_topics (chat_id, topic_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    )
    .bind(chat_id)
    .bind(topic_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn remove_topic(pool: &PgPool, chat_id: i64, topic_id: i64) -> Result<(), AppError> {
    sqlx::query(
        "DELETE FROM chat_topics WHERE chat_id = $1 AND topic_id = $2",
    )
    .bind(chat_id)
    .bind(topic_id)
    .execute(pool)
    .await?;
    Ok(())
}

