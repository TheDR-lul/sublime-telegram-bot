use sqlx::PgPool;

use crate::error::AppError;

pub async fn is_topic_enabled(pool: &PgPool, chat_id: i64, topic_id: i64) -> Result<bool, AppError> {
    // Use EXISTS to avoid INT4/INT8 decode mismatches across old schemas.
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM chat_topics WHERE chat_id = $1 AND topic_id = $2)",
    )
    .bind(chat_id)
    .bind(topic_id)
    .fetch_one(pool)
    .await?;
    Ok(exists)
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

pub async fn list_topics(pool: &PgPool, chat_id: i64) -> Result<Vec<i64>, AppError> {
    // Явно приводим topic_id к BIGINT, чтобы тип в результате всегда был INT8,
    // даже если в какой-то БД столбец когда-то создавался как INT4.
    let rows: Vec<(i64,)> = sqlx::query_as(
        "SELECT topic_id::BIGINT FROM chat_topics WHERE chat_id = $1 ORDER BY topic_id ASC",
    )
    .bind(chat_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|row| row.0).collect())
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

