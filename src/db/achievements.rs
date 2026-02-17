//! Simple achievement storage and helpers.
//!
//! Tables:
//! - achievement(user_id, code, earned_at)

use sqlx::PgPool;

use crate::db::models::Achievement;
use crate::error::AppError;

/// Try to grant achievement with given code for user.
/// Returns true if a new achievement was created, false if it already existed.
pub async fn grant(pool: &PgPool, user_id: i32, code: &str) -> Result<bool, AppError> {
    let row = sqlx::query(
        r#"
        INSERT INTO achievement (user_id, code)
        VALUES ($1, $2)
        ON CONFLICT (user_id, code) DO NOTHING
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(code)
    .fetch_optional(pool)
    .await?;

    Ok(row.is_some())
}

/// List all achievements for given user, ordered by time.
pub async fn list_for_user(pool: &PgPool, user_id: i32) -> Result<Vec<Achievement>, AppError> {
    let rows = sqlx::query_as::<_, Achievement>(
        r#"
        SELECT id, user_id, code, earned_at
        FROM achievement
        WHERE user_id = $1
        ORDER BY earned_at
        "#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

