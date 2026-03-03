//! Database operations for the HuyActa tamagotchi game.
//!
//! Each chat × user pair has one Huya row.
//! length_mm < 0 means the user is growing an ass instead of a dick.
//! Daily action limit = 4, reset each calendar day (UTC).

use chrono::{DateTime, Utc};
use rand::RngExt;
use sqlx::PgPool;

use crate::db::models::Huya;
use crate::error::AppError;

const HUYA_SELECT: &str =
    "id, chat_id, tg_id, length_mm, level, xp, actions_left, actions_reset_at, created_at";

const DAILY_ACTIONS: i32 = 4;
const XP_PER_LEVEL: i32 = 100;
const LEVEL_UP_BONUS_MM: i32 = 50;

/// Returns (Huya, was_created). `was_created` is true only when the row did not
/// exist before this call, allowing callers to show a first-time registration message.
pub async fn get_or_create(pool: &PgPool, chat_id: i64, tg_id: i64) -> Result<(Huya, bool), AppError> {
    // Try a pure INSERT; if the row already exists DO NOTHING and return no row.
    let inserted = sqlx::query_as::<_, Huya>(
        &format!("INSERT INTO huya (chat_id, tg_id) VALUES ($1, $2)
         ON CONFLICT (chat_id, tg_id) DO NOTHING
         RETURNING {HUYA_SELECT}"),
    )
    .bind(chat_id)
    .bind(tg_id)
    .fetch_optional(pool)
    .await?;

    if let Some(h) = inserted {
        return Ok((h, true));
    }

    let h = sqlx::query_as::<_, Huya>(
        &format!("SELECT {HUYA_SELECT} FROM huya WHERE chat_id = $1 AND tg_id = $2"),
    )
    .bind(chat_id)
    .bind(tg_id)
    .fetch_one(pool)
    .await?;

    Ok((h, false))
}

/// Consume one action. Returns false if the player has no actions left today.
/// Automatically resets counter if actions_reset_at < today.
pub async fn consume_action(pool: &PgPool, huya: &Huya) -> Result<bool, AppError> {
    let today = Utc::now().date_naive();

    if huya.actions_reset_at < today {
        // New day: reset to DAILY_ACTIONS - 1 (consuming one right now).
        sqlx::query(
            "UPDATE huya SET actions_left = $1, actions_reset_at = $2 WHERE id = $3",
        )
        .bind(DAILY_ACTIONS - 1)
        .bind(today)
        .bind(huya.id)
        .execute(pool)
        .await?;
        return Ok(true);
    }

    if huya.actions_left <= 0 {
        return Ok(false);
    }

    sqlx::query("UPDATE huya SET actions_left = actions_left - 1 WHERE id = $1")
        .bind(huya.id)
        .execute(pool)
        .await?;
    Ok(true)
}

/// Grow the dick: +rand(5..=30) mm, +rand(5..=15) XP, handle level-up.
/// Returns the updated Huya.
pub async fn grow(pool: &PgPool, huya: &Huya) -> Result<(Huya, i32, i32, bool), AppError> {
    // Scope rng so ThreadRng is dropped before any .await.
    let (grow_mm, xp_gain): (i32, i32) = {
        let mut rng = rand::rng();
        (rng.random_range(5..=30), rng.random_range(5..=15))
    };

    let new_length = huya.length_mm + grow_mm;
    let new_xp_raw = huya.xp + xp_gain;
    let mut new_level = huya.level;
    let mut new_xp = new_xp_raw;
    let mut leveled_up = false;

    if new_xp >= XP_PER_LEVEL {
        new_level += 1;
        new_xp -= XP_PER_LEVEL;
        leveled_up = true;
    }

    // Bonus mm on level-up
    let final_length = if leveled_up { new_length + LEVEL_UP_BONUS_MM } else { new_length };

    let updated = sqlx::query_as::<_, Huya>(
        &format!("UPDATE huya SET length_mm = $1, xp = $2, level = $3
         WHERE id = $4 RETURNING {HUYA_SELECT}"),
    )
    .bind(final_length)
    .bind(new_xp)
    .bind(new_level)
    .bind(huya.id)
    .fetch_one(pool)
    .await?;

    Ok((updated, grow_mm + if leveled_up { LEVEL_UP_BONUS_MM } else { 0 }, xp_gain, leveled_up))
}

pub struct FightResult {
    pub winner_tg_id: i64,
    pub loser_tg_id: i64,
    pub steal_mm: i32,
    pub elo_gain: i32,
    /// Raw attack score of challenger (for display).
    pub challenger_score: i32,
    /// Raw defense score of target (for display).
    pub target_score: i32,
    /// Expected win chance of challenger in percent (pre-roll, size-based).
    pub win_chance_pct: u8,
    pub challenger: Huya,
    pub target: Huya,
}

/// Battle: challenger vs target.
///
/// Steal formula: the amount stolen scales with size parity — attacking a much
/// smaller opponent yields little reward, incentivising equal-size fights.
///   similarity  = min(ch_len, tg_len) / max(ch_len, tg_len)  [0..1]
///   steal_mm    = loser_len * similarity * 0.25 + rand(5..20)
///   capped at max(winner_len * 0.40, 10) to prevent single-hit wipes
pub async fn fight(
    pool: &PgPool,
    chat_id: i64,
    challenger_tg_id: i64,
    target_tg_id: i64,
) -> Result<FightResult, AppError> {
    let (ch, _) = get_or_create(pool, chat_id, challenger_tg_id).await?;
    let (tg, _) = get_or_create(pool, chat_id, target_tg_id).await?;

    let ch_len = ch.length_mm.max(1) as f64;
    let tg_len = tg.length_mm.max(1) as f64;

    // Expected win probability shown to the user (pure power, no luck component).
    let ch_power = (ch.length_mm.max(1) * ch.level) as f64;
    let tg_power = (tg.length_mm.max(1) * tg.level) as f64;
    let win_chance_pct = (ch_power / (ch_power + tg_power) * 100.0).round() as u8;

    // Similarity ratio: 1.0 = equal size, approaches 0 as sizes diverge.
    let similarity = ch_len.min(tg_len) / ch_len.max(tg_len);

    // Scope rng so ThreadRng is dropped before any .await.
    let (atk, def, steal_mm, elo_gain) = {
        let mut rng = rand::rng();
        let a = ch.length_mm.max(1) * ch.level + rng.random_range(0..=50);
        let d_val = tg.length_mm.max(1) * tg.level + rng.random_range(0..=50);
        let loser_len_f = ch_len.min(tg_len); // mm of future loser (conservative)
        let raw_steal = (loser_len_f * similarity * 0.25) as i32 + rng.random_range(5..=20);
        // Never wipe out more than 40% of winner's length in a single fight.
        let cap = ((ch_len.max(tg_len) * 0.40) as i32).max(10);
        let s = raw_steal.min(cap).max(5);
        let e: i32 = rng.random_range(5..=25);
        (a, d_val, s, e)
    };

    let (winner_tg_id, loser_tg_id, winner_id, loser_id, winner_len, loser_len) = if atk >= def {
        (challenger_tg_id, target_tg_id, ch.id, tg.id, ch.length_mm + steal_mm, tg.length_mm - steal_mm)
    } else {
        (target_tg_id, challenger_tg_id, tg.id, ch.id, tg.length_mm + steal_mm, ch.length_mm - steal_mm)
    };

    sqlx::query("UPDATE huya SET length_mm = $1 WHERE id = $2")
        .bind(winner_len)
        .bind(winner_id)
        .execute(pool)
        .await?;
    sqlx::query("UPDATE huya SET length_mm = $1 WHERE id = $2")
        .bind(loser_len)
        .bind(loser_id)
        .execute(pool)
        .await?;

    let (challenger_updated, _) = get_or_create(pool, chat_id, challenger_tg_id).await?;
    let (target_updated, _) = get_or_create(pool, chat_id, target_tg_id).await?;

    Ok(FightResult {
        winner_tg_id,
        loser_tg_id,
        steal_mm,
        elo_gain,
        challenger_score: atk,
        target_score: def,
        win_chance_pct,
        challenger: challenger_updated,
        target: target_updated,
    })
}

/// Self-fight penalty: deducts 5 mm (0.5 cm) and returns the updated record.
pub async fn self_fight(pool: &PgPool, chat_id: i64, tg_id: i64) -> Result<Huya, AppError> {
    let (h, _) = get_or_create(pool, chat_id, tg_id).await?;
    let updated = sqlx::query_as::<_, Huya>(
        &format!("UPDATE huya SET length_mm = length_mm - 5 WHERE id = $1 RETURNING {HUYA_SELECT}"),
    )
    .bind(h.id)
    .fetch_one(pool)
    .await?;
    Ok(updated)
}

pub struct StealResult {
    pub success: bool,
    pub steal_mm: i32,
    /// Steal chance in percent shown to the user.
    pub chance_pct: u8,
    pub attacker: Huya,
    pub target: Huya,
}

/// Steal attempt.
///
/// Success probability uses the same size-parity principle as fights:
///   base_chance = attacker_len / (attacker_len + target_len)
///   parity_mult = min(a, t) / max(a, t)  [0..1]
///   final_chance = base_chance * (0.5 + 0.5 * parity_mult)
/// This caps realistic steal chance at ~50% against equal-sized opponents and
/// makes stealing from tiny players nearly impossible.
///
/// Stolen amount also scales with parity so bullying small players is
/// unprofitable even on success.
pub async fn steal_attempt(
    pool: &PgPool,
    chat_id: i64,
    attacker_tg_id: i64,
    target_tg_id: i64,
) -> Result<StealResult, AppError> {
    let (att, _) = get_or_create(pool, chat_id, attacker_tg_id).await?;
    let (tgt, _) = get_or_create(pool, chat_id, target_tg_id).await?;

    let att_len = att.length_mm.max(1) as f64;
    let tgt_len = tgt.length_mm.max(1) as f64;
    let parity = att_len.min(tgt_len) / att_len.max(tgt_len);
    let base_chance = att_len / (att_len + tgt_len);
    let chance = base_chance * (0.5 + 0.5 * parity);
    let chance_pct = (chance * 100.0).round().min(99.0) as u8;

    // Stolen amount: target_size * parity * 0.20 + rand(5..15), cap 40% of target.
    let raw_steal_f = tgt_len * parity * 0.20;
    let steal_cap = (tgt_len * 0.40) as i32;

    // Scope rng so ThreadRng is dropped before any .await.
    let (success, steal_mm) = {
        let mut rng = rand::rng();
        let roll: f64 = rng.random_range(0.0_f64..1.0_f64);
        let s = ((raw_steal_f as i32) + rng.random_range(5..=15)).min(steal_cap).max(5);
        (roll < chance, s)
    };

    if success {
        sqlx::query("UPDATE huya SET length_mm = length_mm + $1 WHERE id = $2")
            .bind(steal_mm)
            .bind(att.id)
            .execute(pool)
            .await?;
        sqlx::query("UPDATE huya SET length_mm = length_mm - $1 WHERE id = $2")
            .bind(steal_mm)
            .bind(tgt.id)
            .execute(pool)
            .await?;
    }

    let (att_updated, _) = get_or_create(pool, chat_id, attacker_tg_id).await?;
    let (tgt_updated, _) = get_or_create(pool, chat_id, target_tg_id).await?;

    Ok(StealResult {
        success,
        steal_mm,
        chance_pct,
        attacker: att_updated,
        target: tgt_updated,
    })
}

/// Top-N players by length_mm in a chat.
pub async fn top(
    pool: &PgPool,
    chat_id: i64,
    limit: i64,
) -> Result<Vec<(Huya, i64)>, AppError> {
    let rows = sqlx::query_as::<_, (i32, i64, i64, i32, i32, i32, i32, chrono::NaiveDate, chrono::DateTime<Utc>)>(
        "SELECT id, chat_id, tg_id, length_mm, level, xp, actions_left, actions_reset_at, created_at
         FROM huya WHERE chat_id = $1
         ORDER BY length_mm DESC LIMIT $2",
    )
    .bind(chat_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    let result = rows
        .into_iter()
        .map(|(id, chat_id, tg_id, length_mm, level, xp, actions_left, actions_reset_at, created_at)| {
            let huya = Huya { id, chat_id, tg_id, length_mm, level, xp, actions_left, actions_reset_at, created_at };
            (huya, tg_id)
        })
        .collect();
    Ok(result)
}

// ── Interactive fight (huya_fight table) ─────────────────────────────────────

/// A pending or active huya fight between two players.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct HuyaFight {
    pub id: i32,
    pub chat_id: i64,
    pub challenger_tg_id: i64,
    pub target_tg_id: i64,
    /// 0=Напор, 1=Финт, 2=В шары; NULL = not chosen yet.
    pub challenger_pick: Option<i32>,
    pub target_pick: Option<i32>,
    /// "pending" | "active" | "done"
    pub status: String,
    pub message_id: i32,
    pub created_at: DateTime<Utc>,
}

const FIGHT_SELECT: &str =
    "id, chat_id, challenger_tg_id, target_tg_id, challenger_pick, target_pick, status, message_id, created_at";

/// Create a new pending fight challenge. Returns the created record.
pub async fn create_fight(
    pool: &PgPool,
    chat_id: i64,
    challenger_tg_id: i64,
    target_tg_id: i64,
) -> Result<HuyaFight, AppError> {
    let row = sqlx::query_as::<_, HuyaFight>(
        &format!("INSERT INTO huya_fight (chat_id, challenger_tg_id, target_tg_id)
         VALUES ($1, $2, $3) RETURNING {FIGHT_SELECT}"),
    )
    .bind(chat_id)
    .bind(challenger_tg_id)
    .bind(target_tg_id)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

/// Save the message_id after the challenge message is sent.
pub async fn set_fight_message_id(pool: &PgPool, fight_id: i32, message_id: i32) -> Result<(), AppError> {
    sqlx::query("UPDATE huya_fight SET message_id = $1 WHERE id = $2")
        .bind(message_id)
        .bind(fight_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Target accepts the fight. Returns None if fight not found or not pending.
pub async fn accept_fight(pool: &PgPool, fight_id: i32, target_tg_id: i64) -> Result<Option<HuyaFight>, AppError> {
    let row = sqlx::query_as::<_, HuyaFight>(
        &format!("UPDATE huya_fight SET status = 'active'
         WHERE id = $1 AND target_tg_id = $2 AND status = 'pending'
         RETURNING {FIGHT_SELECT}"),
    )
    .bind(fight_id)
    .bind(target_tg_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Target declines. Returns the fight if it was pending.
pub async fn decline_fight(pool: &PgPool, fight_id: i32, target_tg_id: i64) -> Result<Option<HuyaFight>, AppError> {
    let row = sqlx::query_as::<_, HuyaFight>(
        &format!("UPDATE huya_fight SET status = 'done'
         WHERE id = $1 AND target_tg_id = $2 AND status = 'pending'
         RETURNING {FIGHT_SELECT}"),
    )
    .bind(fight_id)
    .bind(target_tg_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Fetch a fight by id.
pub async fn get_fight(pool: &PgPool, fight_id: i32) -> Result<Option<HuyaFight>, AppError> {
    let row = sqlx::query_as::<_, HuyaFight>(
        &format!("SELECT {FIGHT_SELECT} FROM huya_fight WHERE id = $1"),
    )
    .bind(fight_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Store a player's pick. Returns the updated fight.
/// Returns None if fight not found, not active, or pick already stored.
pub async fn store_pick(
    pool: &PgPool,
    fight_id: i32,
    tg_id: i64,
    pick: i32,
) -> Result<Option<HuyaFight>, AppError> {
    // Determine whether the player is the challenger or target and only update
    // their column if it's still NULL (first pick wins, no changing your mind).
    let row = sqlx::query_as::<_, HuyaFight>(
        &format!("UPDATE huya_fight SET
           challenger_pick = CASE WHEN challenger_tg_id = $2 AND challenger_pick IS NULL THEN $3 ELSE challenger_pick END,
           target_pick     = CASE WHEN target_tg_id     = $2 AND target_pick     IS NULL THEN $3 ELSE target_pick     END
         WHERE id = $1 AND status = 'active' AND (challenger_tg_id = $2 OR target_tg_id = $2)
         RETURNING {FIGHT_SELECT}"),
    )
    .bind(fight_id)
    .bind(tg_id)
    .bind(pick)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Mark fight as done.
pub async fn finish_huya_fight(pool: &PgPool, fight_id: i32) -> Result<(), AppError> {
    sqlx::query("UPDATE huya_fight SET status = 'done' WHERE id = $1")
        .bind(fight_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Resolve fight result: apply size transfer based on picks and sizes.
/// Returns (winner_tg_id, loser_tg_id, steal_mm, elo_gain, tie: bool).
pub async fn resolve_huya_fight(
    pool: &PgPool,
    chat_id: i64,
    challenger_tg_id: i64,
    target_tg_id: i64,
    ch_pick: i32,
    tg_pick: i32,
) -> Result<(i64, i64, i32, i32, bool), AppError> {
    let (ch, _) = get_or_create(pool, chat_id, challenger_tg_id).await?;
    let (tg, _) = get_or_create(pool, chat_id, target_tg_id).await?;

    // Tie check (same move).
    if ch_pick == tg_pick {
        // Both lose 3mm on a tie.
        sqlx::query("UPDATE huya SET length_mm = length_mm - 3 WHERE chat_id = $1 AND tg_id IN ($2, $3)")
            .bind(chat_id)
            .bind(challenger_tg_id)
            .bind(target_tg_id)
            .execute(pool)
            .await?;
        return Ok((challenger_tg_id, target_tg_id, 0, 0, true));
    }

    // RPS: Напор(0) > В шары(2) > Финт(1) > Напор(0)
    let ch_wins = matches!(
        (ch_pick, tg_pick),
        (0, 2) | (2, 1) | (1, 0)
    );

    let ch_len = ch.length_mm.max(1) as f64;
    let tg_len = tg.length_mm.max(1) as f64;
    let similarity = ch_len.min(tg_len) / ch_len.max(tg_len);

    // Scope rng so ThreadRng is dropped before any .await.
    let (steal_mm, elo_gain) = {
        let mut rng = rand::rng();
        let loser_len_f = if ch_wins { tg_len } else { ch_len };
        let raw = (loser_len_f * similarity * 0.30) as i32 + rng.random_range(5..=20);
        let cap = ((ch_len.max(tg_len) * 0.40) as i32).max(10);
        let s = raw.min(cap).max(5);
        let e: i32 = rng.random_range(5..=25);
        (s, e)
    };

    let (winner_id, loser_id, winner_len, loser_len) = if ch_wins {
        (ch.id, tg.id, ch.length_mm + steal_mm, tg.length_mm - steal_mm)
    } else {
        (tg.id, ch.id, tg.length_mm + steal_mm, ch.length_mm - steal_mm)
    };

    sqlx::query("UPDATE huya SET length_mm = $1 WHERE id = $2")
        .bind(winner_len)
        .bind(winner_id)
        .execute(pool)
        .await?;
    sqlx::query("UPDATE huya SET length_mm = $1 WHERE id = $2")
        .bind(loser_len)
        .bind(loser_id)
        .execute(pool)
        .await?;

    let winner_tg_id = if ch_wins { challenger_tg_id } else { target_tg_id };
    let loser_tg_id  = if ch_wins { target_tg_id } else { challenger_tg_id };
    Ok((winner_tg_id, loser_tg_id, steal_mm, elo_gain, false))
}

