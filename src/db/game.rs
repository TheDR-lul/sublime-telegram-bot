//! Game, GamePlayer, GameResult: get or create game, register, run draw, stats.

use sqlx::PgPool;

use crate::db::models::{Game, GameResult, TgUser, UserWithCount};
use crate::error::AppError;

pub async fn get_or_create_game(pool: &PgPool, chat_id: i64) -> Result<Game, AppError> {
    if let Some(g) = sqlx::query_as::<_, Game>("SELECT id, chat_id FROM game WHERE chat_id = $1")
        .bind(chat_id)
        .fetch_optional(pool)
        .await?
    {
        return Ok(g);
    }
    let game = sqlx::query_as::<_, Game>(
        "INSERT INTO game (chat_id) VALUES ($1) RETURNING id, chat_id",
    )
    .bind(chat_id)
    .fetch_one(pool)
    .await?;
    Ok(game)
}

pub async fn add_player(pool: &PgPool, game_id: i32, user_id: i32) -> Result<(), AppError> {
    sqlx::query("INSERT INTO gameplayer (game_id, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
        .bind(game_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn remove_player(pool: &PgPool, game_id: i32, user_id: i32) -> Result<bool, AppError> {
    let r = sqlx::query("DELETE FROM gameplayer WHERE game_id = $1 AND user_id = $2")
        .bind(game_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn get_players(pool: &PgPool, game_id: i32) -> Result<Vec<TgUser>, AppError> {
    let rows = sqlx::query_as::<_, TgUser>(
        r#"
        SELECT u.id, u.tg_id, u.username, u.first_name, u.last_name, u.lang_code, u.is_blocked,
               u.created_at, u.updated_at, u.last_seen_at
        FROM tguser u
        INNER JOIN gameplayer gp ON gp.user_id = u.id
        WHERE gp.game_id = $1
        "#,
    )
    .bind(game_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get_today_result(
    pool: &PgPool,
    game_id: i32,
    year: i32,
    day: i32,
) -> Result<Option<GameResult>, AppError> {
    let row = sqlx::query_as::<_, GameResult>(
        "SELECT id, game_id, winner_id, year, day FROM gameresult WHERE game_id = $1 AND year = $2 AND day = $3",
    )
    .bind(game_id)
    .bind(year)
    .bind(day)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn insert_result(
    pool: &PgPool,
    game_id: i32,
    winner_id: i32,
    year: i32,
    day: i32,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO gameresult (game_id, winner_id, year, day) VALUES ($1, $2, $3, $4) ON CONFLICT (game_id, year, day) DO NOTHING",
    )
    .bind(game_id)
    .bind(winner_id)
    .bind(year)
    .bind(day)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn is_player_in_game(pool: &PgPool, game_id: i32, user_id: i32) -> Result<bool, AppError> {
    let row: (i64,) = sqlx::query_as(
        "SELECT 1 FROM gameplayer WHERE game_id = $1 AND user_id = $2 LIMIT 1",
    )
    .bind(game_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?
    .unwrap_or((0,));
    Ok(row.0 > 0)
}

pub async fn get_user_by_id(pool: &PgPool, user_id: i32) -> Result<Option<TgUser>, AppError> {
    let row = sqlx::query_as::<_, TgUser>(
        "SELECT id, tg_id, username, first_name, last_name, lang_code, is_blocked, created_at, updated_at, last_seen_at FROM tguser WHERE id = $1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Stats: (TgUser, win_count) for current year, ordered by count desc, limit 10.
pub async fn stats_current_year(
    pool: &PgPool,
    game_id: i32,
    year: i32,
) -> Result<Vec<(TgUser, i64)>, AppError> {
    let rows = sqlx::query_as::<_, UserWithCount>(
        r#"
        SELECT u.id, u.tg_id, u.username, u.first_name, u.last_name, u.lang_code, u.is_blocked,
               u.created_at, u.updated_at, u.last_seen_at,
               COUNT(r.id)::bigint as count
        FROM tguser u
        INNER JOIN gameresult r ON r.winner_id = u.id
        WHERE r.game_id = $1 AND r.year = $2
        GROUP BY u.id, u.tg_id, u.username, u.first_name, u.last_name, u.lang_code, u.is_blocked, u.created_at, u.updated_at, u.last_seen_at
        ORDER BY COUNT(r.id) DESC
        LIMIT 10
        "#,
    )
    .bind(game_id)
    .bind(year)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| (r.to_tg_user(), r.count)).collect())
}

/// Stats all time, limit 10.
pub async fn stats_all_time(
    pool: &PgPool,
    game_id: i32,
) -> Result<Vec<(TgUser, i64)>, AppError> {
    let rows = sqlx::query_as::<_, UserWithCount>(
        r#"
        SELECT u.id, u.tg_id, u.username, u.first_name, u.last_name, u.lang_code, u.is_blocked,
               u.created_at, u.updated_at, u.last_seen_at,
               COUNT(r.id)::bigint as count
        FROM tguser u
        INNER JOIN gameresult r ON r.winner_id = u.id
        WHERE r.game_id = $1
        GROUP BY u.id, u.tg_id, u.username, u.first_name, u.last_name, u.lang_code, u.is_blocked, u.created_at, u.updated_at, u.last_seen_at
        ORDER BY COUNT(r.id) DESC
        LIMIT 10
        "#,
    )
    .bind(game_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| (r.to_tg_user(), r.count)).collect())
}

/// Personal stats: (TgUser, count) for one user.
pub async fn stats_personal(
    pool: &PgPool,
    game_id: i32,
    user_id: i32,
) -> Result<Option<(TgUser, i64)>, AppError> {
    let row = sqlx::query_as::<_, UserWithCount>(
        r#"
        SELECT u.id, u.tg_id, u.username, u.first_name, u.last_name, u.lang_code, u.is_blocked,
               u.created_at, u.updated_at, u.last_seen_at,
               COUNT(r.id)::bigint as count
        FROM tguser u
        INNER JOIN gameresult r ON r.winner_id = u.id
        WHERE r.game_id = $1 AND u.id = $2
        GROUP BY u.id, u.tg_id, u.username, u.first_name, u.last_name, u.lang_code, u.is_blocked, u.created_at, u.updated_at, u.last_seen_at
        "#,
    )
    .bind(game_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| (r.to_tg_user(), r.count)))
}

/// Year results: top 50 for given year.
pub async fn stats_year(
    pool: &PgPool,
    game_id: i32,
    year: i32,
) -> Result<Vec<(TgUser, i64)>, AppError> {
    let rows = sqlx::query_as::<_, UserWithCount>(
        r#"
        SELECT u.id, u.tg_id, u.username, u.first_name, u.last_name, u.lang_code, u.is_blocked,
               u.created_at, u.updated_at, u.last_seen_at,
               COUNT(r.id)::bigint as count
        FROM tguser u
        INNER JOIN gameresult r ON r.winner_id = u.id
        WHERE r.game_id = $1 AND r.year = $2
        GROUP BY u.id, u.tg_id, u.username, u.first_name, u.last_name, u.lang_code, u.is_blocked, u.created_at, u.updated_at, u.last_seen_at
        ORDER BY COUNT(r.id) DESC
        LIMIT 50
        "#,
    )
    .bind(game_id)
    .bind(year)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| (r.to_tg_user(), r.count)).collect())
}
