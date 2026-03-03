use sqlx::PgPool;

use crate::db::models::PidorBet;
use crate::error::AppError;

pub async fn place_bet(
    pool: &PgPool,
    chat_id: i64,
    bettor_tg_id: i64,
    target_tg_id: i64,
    year: i32,
    day: i32,
    slot: &str,
) -> Result<PidorBet, AppError> {
    let row = sqlx::query_as::<_, PidorBet>(
        "INSERT INTO pidor_bet (chat_id, bettor_tg_id, target_tg_id, year, day, slot)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (chat_id, bettor_tg_id, year, day, slot)
         DO UPDATE SET target_tg_id = EXCLUDED.target_tg_id
         RETURNING id, chat_id, bettor_tg_id, target_tg_id, year, day, slot, correct, created_at",
    )
    .bind(chat_id)
    .bind(bettor_tg_id)
    .bind(target_tg_id)
    .bind(year)
    .bind(day)
    .bind(slot)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn get_bet(
    pool: &PgPool,
    chat_id: i64,
    bettor_tg_id: i64,
    year: i32,
    day: i32,
    slot: &str,
) -> Result<Option<PidorBet>, AppError> {
    let row = sqlx::query_as::<_, PidorBet>(
        "SELECT id, chat_id, bettor_tg_id, target_tg_id, year, day, slot, correct, created_at
         FROM pidor_bet WHERE chat_id = $1 AND bettor_tg_id = $2 AND year = $3 AND day = $4 AND slot = $5",
    )
    .bind(chat_id)
    .bind(bettor_tg_id)
    .bind(year)
    .bind(day)
    .bind(slot)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Resolve all bets for the given chat/day/slot. Returns list of correct bettors' tg_ids.
pub async fn resolve_bets(
    pool: &PgPool,
    chat_id: i64,
    year: i32,
    day: i32,
    slot: &str,
    winner_tg_id: i64,
) -> Result<Vec<PidorBet>, AppError> {
    sqlx::query(
        "UPDATE pidor_bet SET correct = (target_tg_id = $1)
         WHERE chat_id = $2 AND year = $3 AND day = $4 AND slot = $5 AND correct IS NULL",
    )
    .bind(winner_tg_id)
    .bind(chat_id)
    .bind(year)
    .bind(day)
    .bind(slot)
    .execute(pool)
    .await?;

    let winners = sqlx::query_as::<_, PidorBet>(
        "SELECT id, chat_id, bettor_tg_id, target_tg_id, year, day, slot, correct, created_at
         FROM pidor_bet WHERE chat_id = $1 AND year = $2 AND day = $3 AND slot = $4 AND correct = true",
    )
    .bind(chat_id)
    .bind(year)
    .bind(day)
    .bind(slot)
    .fetch_all(pool)
    .await?;
    Ok(winners)
}

pub async fn count_correct_bets(pool: &PgPool, chat_id: i64, bettor_tg_id: i64) -> Result<i64, AppError> {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::bigint FROM pidor_bet WHERE chat_id = $1 AND bettor_tg_id = $2 AND correct = true",
    )
    .bind(chat_id)
    .bind(bettor_tg_id)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

pub async fn count_correct_streak(pool: &PgPool, chat_id: i64, bettor_tg_id: i64) -> Result<i64, AppError> {
    let rows: Vec<(Option<bool>,)> = sqlx::query_as(
        "SELECT correct FROM pidor_bet
         WHERE chat_id = $1 AND bettor_tg_id = $2 AND correct IS NOT NULL
         ORDER BY year DESC, day DESC, id DESC",
    )
    .bind(chat_id)
    .bind(bettor_tg_id)
    .fetch_all(pool)
    .await?;

    let mut streak = 0i64;
    for (c,) in rows {
        if c == Some(true) {
            streak += 1;
        } else {
            break;
        }
    }
    Ok(streak)
}
