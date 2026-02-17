//! TikTok link cache: by link or share_link.

use sqlx::PgPool;

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
