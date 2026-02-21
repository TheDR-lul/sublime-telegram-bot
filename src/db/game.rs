//! Game, GamePlayer, GameResult: get or create game, register, run draw, stats.

use sqlx::PgPool;

use crate::db::models::{Game, GameResult, TgUser, UserWithCount};
use crate::error::AppError;

const GAME_SELECT: &str = "id, chat_id, autorun_enabled, autorun_morning, autorun_day, autorun_evening";

pub async fn get_or_create_game(pool: &PgPool, chat_id: i64) -> Result<Game, AppError> {
    if let Some(g) = sqlx::query_as::<_, Game>(&format!("SELECT {} FROM game WHERE chat_id = $1", GAME_SELECT))
        .bind(chat_id)
        .fetch_optional(pool)
        .await?
    {
        return Ok(g);
    }
    let game = sqlx::query_as::<_, Game>(&format!(
        "INSERT INTO game (chat_id) VALUES ($1) RETURNING {}",
        GAME_SELECT
    ))
    .bind(chat_id)
    .fetch_one(pool)
    .await?;
    Ok(game)
}

pub async fn list_games(pool: &PgPool) -> Result<Vec<Game>, AppError> {
    let rows = sqlx::query_as::<_, Game>(&format!("SELECT {} FROM game", GAME_SELECT))
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

/// List games that have autorun enabled for the given slot (morning/day/evening).
pub async fn list_games_for_autorun_slot(
    pool: &PgPool,
    slot_column: &str,
) -> Result<Vec<Game>, AppError> {
    let query = format!(
        "SELECT {} FROM game WHERE autorun_enabled = true AND {} = true",
        GAME_SELECT, slot_column
    );
    let rows = sqlx::query_as::<_, Game>(&query).fetch_all(pool).await?;
    Ok(rows)
}

pub async fn update_autorun_settings(
    pool: &PgPool,
    chat_id: i64,
    autorun_enabled: Option<bool>,
    autorun_morning: Option<bool>,
    autorun_day: Option<bool>,
    autorun_evening: Option<bool>,
) -> Result<(), AppError> {
    let mut updates = Vec::new();
    let mut bind_idx = 1i32;
    if autorun_enabled.is_some() {
        updates.push(format!("autorun_enabled = ${}", bind_idx));
        bind_idx += 1;
    }
    if autorun_morning.is_some() {
        updates.push(format!("autorun_morning = ${}", bind_idx));
        bind_idx += 1;
    }
    if autorun_day.is_some() {
        updates.push(format!("autorun_day = ${}", bind_idx));
        bind_idx += 1;
    }
    if autorun_evening.is_some() {
        updates.push(format!("autorun_evening = ${}", bind_idx));
        bind_idx += 1;
    }
    if updates.is_empty() {
        return Ok(());
    }
    let where_param = bind_idx;
    let set_clause = updates.join(", ");
    let sql = format!(
        "UPDATE game SET {} WHERE chat_id = ${}",
        set_clause, where_param
    );
    let mut q = sqlx::query(&sql);
    if let Some(v) = autorun_enabled {
        q = q.bind(v);
    }
    if let Some(v) = autorun_morning {
        q = q.bind(v);
    }
    if let Some(v) = autorun_day {
        q = q.bind(v);
    }
    if let Some(v) = autorun_evening {
        q = q.bind(v);
    }
    q = q.bind(chat_id);
    q.execute(pool).await?;
    Ok(())
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

pub async fn remove_player_by_chat_and_tg_id(
    pool: &PgPool,
    chat_id: i64,
    tg_id: i64,
) -> Result<bool, AppError> {
    let r = sqlx::query(
        r#"
        DELETE FROM gameplayer
        WHERE game_id = (SELECT id FROM game WHERE chat_id = $1)
          AND user_id = (SELECT id FROM tguser WHERE tg_id = $2)
        "#,
    )
    .bind(chat_id)
    .bind(tg_id)
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

/// Get result for (game_id, year, day, slot). Slot: "manual" | "morning" | "day" | "evening".
pub async fn get_today_result_for_slot(
    pool: &PgPool,
    game_id: i32,
    year: i32,
    day: i32,
    slot: &str,
) -> Result<Option<GameResult>, AppError> {
    let row = sqlx::query_as::<_, GameResult>(
        "SELECT id, game_id, winner_id, year, day, slot FROM gameresult WHERE game_id = $1 AND year = $2 AND day = $3 AND slot = $4",
    )
    .bind(game_id)
    .bind(year)
    .bind(day)
    .bind(slot)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn insert_result_with_slot(
    pool: &PgPool,
    game_id: i32,
    winner_id: i32,
    year: i32,
    day: i32,
    slot: &str,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO gameresult (game_id, winner_id, year, day, slot) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (game_id, year, day, slot) DO NOTHING",
    )
    .bind(game_id)
    .bind(winner_id)
    .bind(year)
    .bind(day)
    .bind(slot)
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

/// Record that this user was seen in this chat (for "call unregistered" feature).
pub async fn record_chat_member(
    pool: &PgPool,
    chat_id: i64,
    user_id: i32,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO chat_member (chat_id, user_id, last_seen_at)
        VALUES ($1, $2, NOW())
        ON CONFLICT (chat_id, user_id) DO UPDATE SET last_seen_at = NOW()
        "#,
    )
    .bind(chat_id)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Remove user from chat_member when they leave the chat (so we don't tag them in "call unregistered").
pub async fn remove_chat_member_by_tg_id(
    pool: &PgPool,
    chat_id: i64,
    tg_id: i64,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        DELETE FROM chat_member
        WHERE chat_id = $1 AND user_id = (SELECT id FROM tguser WHERE tg_id = $2)
        "#,
    )
    .bind(chat_id)
    .bind(tg_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Users seen in this chat who are not registered in the game (for admin "call unregistered").
pub async fn get_unregistered_in_chat(
    pool: &PgPool,
    chat_id: i64,
) -> Result<Vec<TgUser>, AppError> {
    let rows = sqlx::query_as::<_, TgUser>(
        r#"
        SELECT u.id, u.tg_id, u.username, u.first_name, u.last_name, u.lang_code, u.is_blocked,
               u.created_at, u.updated_at, u.last_seen_at
        FROM chat_member cm
        INNER JOIN tguser u ON u.id = cm.user_id
        WHERE cm.chat_id = $1
          AND NOT EXISTS (
              SELECT 1 FROM game g
              INNER JOIN gameplayer gp ON gp.game_id = g.id
              WHERE g.chat_id = cm.chat_id AND gp.user_id = cm.user_id
          )
        "#,
    )
    .bind(chat_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
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
