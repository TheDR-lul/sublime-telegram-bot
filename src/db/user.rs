//! Upsert TGUser from Telegram user info.

use sqlx::PgPool;
use teloxide::types::User;

use crate::db::models::TgUser;
use crate::error::AppError;

pub async fn upsert_tg_user(pool: &PgPool, from: &User) -> Result<TgUser, AppError> {
    let tg_id = from.id.0 as i64;
    let username = from.username.as_deref().map(String::from);
    let first_name = from.first_name.clone();
    let last_name = from.last_name.clone();
    let lang_code = from
        .language_code
        .as_deref()
        .unwrap_or("en")
        .to_string();

    let row = sqlx::query_as::<_, TgUser>(
        r#"
        INSERT INTO tguser (tg_id, username, first_name, last_name, lang_code, is_blocked, created_at, updated_at, last_seen_at)
        VALUES ($1, $2, $3, $4, $5, false, NOW(), NOW(), NOW())
        ON CONFLICT (tg_id) DO UPDATE SET
            username = EXCLUDED.username,
            first_name = EXCLUDED.first_name,
            last_name = EXCLUDED.last_name,
            lang_code = EXCLUDED.lang_code,
            updated_at = CASE
                WHEN tguser.username IS DISTINCT FROM EXCLUDED.username
                  OR tguser.first_name IS DISTINCT FROM EXCLUDED.first_name
                  OR tguser.last_name IS DISTINCT FROM EXCLUDED.last_name
                  OR tguser.lang_code IS DISTINCT FROM EXCLUDED.lang_code
                THEN NOW() ELSE tguser.updated_at END,
            last_seen_at = NOW()
        RETURNING id, tg_id, username, first_name, last_name, lang_code, is_blocked, created_at, updated_at, last_seen_at
        "#,
    )
    .bind(tg_id)
    .bind(&username)
    .bind(&first_name)
    .bind(&last_name)
    .bind(&lang_code)
    .fetch_one(pool)
    .await?;

    Ok(row)
}
