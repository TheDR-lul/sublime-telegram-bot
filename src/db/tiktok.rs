//! TikTok link cache: by link or share_link.

use sqlx::PgPool;
use tokio::time::{sleep, Duration};

use crate::db::models::TiktokLink;
use crate::error::AppError;

pub async fn find_by_link_or_share(
    pool: &PgPool,
    link: &str,
    share_link: &str,
) -> Result<Option<TiktokLink>, AppError> {
    let row = sqlx::query_as::<_, TiktokLink>(
        "SELECT id, link, share_link, telegram_message_id, created_at, updated_at FROM tiktoklink WHERE link = $1 OR share_link = $2",
    )
    .bind(link)
    .bind(share_link)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn insert(
    pool: &PgPool,
    link: &str,
    share_link: Option<&str>,
    telegram_message_id: &str,
) -> Result<TiktokLink, AppError> {
    let row = sqlx::query_as::<_, TiktokLink>(
        r#"
        INSERT INTO tiktoklink (link, share_link, telegram_message_id, created_at, updated_at)
        VALUES ($1, $2, $3, NOW(), NOW())
        RETURNING id, link, share_link, telegram_message_id, created_at, updated_at
        "#,
    )
    .bind(link)
    .bind(share_link)
    .bind(telegram_message_id)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn delete_by_id(pool: &PgPool, id: i32) -> Result<(), AppError> {
    sqlx::query("DELETE FROM tiktoklink WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn cleanup_old_cache(pool: &PgPool) -> Result<u64, AppError> {
    const BATCH_SIZE: i64 = 5_000;
    let mut total_deleted = 0_u64;
    loop {
        let deleted = sqlx::query(
            "WITH doomed AS (
                SELECT ctid
                FROM tiktoklink
                WHERE updated_at < NOW() - INTERVAL '30 days'
                LIMIT $1
            )
            DELETE FROM tiktoklink
            WHERE ctid IN (SELECT ctid FROM doomed)",
        )
        .bind(BATCH_SIZE)
        .execute(pool)
        .await?
        .rows_affected();
        total_deleted += deleted;
        if deleted < BATCH_SIZE as u64 {
            break;
        }
        sleep(Duration::from_millis(120)).await;
    }
    Ok(total_deleted)
}
