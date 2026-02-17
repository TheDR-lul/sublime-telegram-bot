//! KV store: get, list, set, del by chat_id.

use sqlx::PgPool;

use crate::db::models::KvItem;
use crate::error::AppError;

pub async fn get(pool: &PgPool, chat_id: i64, key: &str) -> Result<Option<KvItem>, AppError> {
    let row = sqlx::query_as::<_, KvItem>(
        "SELECT id, chat_id, key, value, created_at, updated_at FROM kvitem WHERE chat_id = $1 AND key = $2",
    )
    .bind(chat_id)
    .bind(key)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn list(pool: &PgPool, chat_id: i64) -> Result<Vec<KvItem>, AppError> {
    let rows = sqlx::query_as::<_, KvItem>(
        "SELECT id, chat_id, key, value, created_at, updated_at FROM kvitem WHERE chat_id = $1 ORDER BY key",
    )
    .bind(chat_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn set(
    pool: &PgPool,
    chat_id: i64,
    key: &str,
    value: &str,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO kvitem (chat_id, key, value, created_at, updated_at)
        VALUES ($1, $2, $3, NOW(), NOW())
        ON CONFLICT (chat_id, key) DO UPDATE SET value = EXCLUDED.value, updated_at = NOW()
        "#,
    )
    .bind(chat_id)
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn del(pool: &PgPool, chat_id: i64, key: &str) -> Result<bool, AppError> {
    let r = sqlx::query("DELETE FROM kvitem WHERE chat_id = $1 AND key = $2")
        .bind(chat_id)
        .bind(key)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}
