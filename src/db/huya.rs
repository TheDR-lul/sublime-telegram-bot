//! Database operations for the HuyActa tamagotchi game.
//!
//! Each chat × user pair has one Huya row.
//! length_mm < 0 means the user is growing an ass instead of a dick.
//! Daily action limit = 20 during development (base from max_actions(); later tuned via Dynamo skill).
//!
//! HP is persistent; replenishes on consume_action (+10 + skill_stamina*3, capped at max_hp()).
//! Skills: 20-tier tree (T1-5) levelled via upgrade_skill() using skill_points earned on level-up.
//! Shop boosts: atk_boost / def_boost / grow_boost — temporary, reset after use.

use chrono::{DateTime, Utc};
use rand::RngExt;
use sqlx::PgPool;

use crate::db::models::Huya;
use crate::error::AppError;

const HUYA_SELECT: &str =
    "id, chat_id, tg_id, length_mm, level, xp, actions_left, actions_reset_at, created_at, \
     hp, skill_points, \
     skill_shaft, skill_skin, skill_balls, skill_cunning, skill_stamina, \
     skill_pierce, skill_scales, skill_spirit, skill_pickpocket, skill_dynamo, \
     skill_eggtwist, skill_bloodsucker, skill_ironballs, skill_vortex, skill_phantom, \
     skill_berserker, skill_vampire, skill_fortress, skill_speedrun, skill_ghost, \
     skill_eternal, skill_absolute, \
     fights_won, fights_lost, \
     atk_boost, def_boost, grow_boost";

const XP_PER_LEVEL: i32 = 100;
const LEVEL_UP_BONUS_MM: i32 = 50;
const MAX_ROUNDS: i32 = 5;

// Skill caps per tier.
const CAP_T1: i32 = 20;
const CAP_T2: i32 = 15;
const CAP_T3: i32 = 10;
const CAP_T4: i32 = 5;
const CAP_T5: i32 = 3;

// SP costs per tier level-up.
const COST_T1: i32 = 1;
const COST_T2: i32 = 2;
const COST_T3: i32 = 3;
const COST_T4: i32 = 5;
const COST_T5: i32 = 7;

// ── Core CRUD ────────────────────────────────────────────────────────────────

/// Returns (Huya, was_created). `was_created` is true only on first insert.
pub async fn get_or_create(pool: &PgPool, chat_id: i64, tg_id: i64) -> Result<(Huya, bool), AppError> {
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

// ── Actions ───────────────────────────────────────────────────────────────────

/// Consume one action and regenerate HP (+10 + skill_stamina*3, capped at max_hp).
/// Returns false if no actions left today.
pub async fn consume_action(pool: &PgPool, huya: &Huya) -> Result<bool, AppError> {
    let today = Utc::now().date_naive();
    let max_hp = huya.max_hp();
    let max_actions = huya.max_actions();
    let hp_regen = 10 + huya.skill_stamina * 3;

    if huya.actions_reset_at < today {
        sqlx::query(
            "UPDATE huya SET actions_left = $1, actions_reset_at = $2,
             hp = LEAST(hp + $3, $4) WHERE id = $5",
        )
        .bind(max_actions - 1)
        .bind(today)
        .bind(hp_regen)
        .bind(max_hp)
        .bind(huya.id)
        .execute(pool)
        .await?;
        return Ok(true);
    }

    if huya.actions_left <= 0 {
        return Ok(false);
    }

    sqlx::query(
        "UPDATE huya SET actions_left = actions_left - 1,
         hp = LEAST(hp + $1, $2) WHERE id = $3",
    )
    .bind(hp_regen)
    .bind(max_hp)
    .bind(huya.id)
    .execute(pool)
    .await?;
    Ok(true)
}

// ── Grow ─────────────────────────────────────────────────────────────────────

/// Grow the dick.
/// Returns (updated, grow_mm, xp_gain, leveled_up, boost_was_active).
/// If grow_boost is set, grow_mm is doubled and boost is consumed.
/// Level-up grants +1 skill_point and LEVEL_UP_BONUS_MM length.
pub async fn grow(pool: &PgPool, huya: &Huya) -> Result<(Huya, i32, i32, bool, bool), AppError> {
    let (base_mm, xp_gain): (i32, i32) = {
        let mut rng = rand::rng();
        (rng.random_range(5..=30), rng.random_range(5..=15))
    };

    let boost_active = huya.grow_boost > 0;
    let grow_mm = if boost_active { base_mm * 2 } else { base_mm };

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

    let final_length = if leveled_up { new_length + LEVEL_UP_BONUS_MM } else { new_length };
    let sp_delta: i32 = if leveled_up { 1 } else { 0 };

    let updated = sqlx::query_as::<_, Huya>(
        &format!("UPDATE huya SET length_mm = $1, xp = $2, level = $3,
         skill_points = skill_points + $4, grow_boost = 0
         WHERE id = $5 RETURNING {HUYA_SELECT}"),
    )
    .bind(final_length)
    .bind(new_xp)
    .bind(new_level)
    .bind(sp_delta)
    .bind(huya.id)
    .fetch_one(pool)
    .await?;

    let total_grow = grow_mm + if leveled_up { LEVEL_UP_BONUS_MM } else { 0 };
    Ok((updated, total_grow, xp_gain, leveled_up, boost_active))
}

// ── Non-interactive fight (legacy, kept for stats display) ────────────────────

pub struct FightResult {
    pub winner_tg_id: i64,
    pub loser_tg_id: i64,
    pub steal_mm: i32,
    pub elo_gain: i32,
    pub challenger_score: i32,
    pub target_score: i32,
    pub win_chance_pct: u8,
    pub challenger: Huya,
    pub target: Huya,
}

/// Old non-interactive fight (used for /huya fight without the mini-game flow).
/// Still used as a fallback; HP-based damage is NOT applied here.
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
    let ch_power = (ch.length_mm.max(1) * ch.level) as f64;
    let tg_power = (tg.length_mm.max(1) * tg.level) as f64;
    let win_chance_pct = (ch_power / (ch_power + tg_power) * 100.0).round() as u8;
    let similarity = ch_len.min(tg_len) / ch_len.max(tg_len);

    let (atk, def, steal_mm, elo_gain) = {
        let mut rng = rand::rng();
        let a = ch.length_mm.max(1) * ch.level + rng.random_range(0..=50);
        let d_val = tg.length_mm.max(1) * tg.level + rng.random_range(0..=50);
        let loser_len_f = ch_len.min(tg_len);
        let raw_steal = (loser_len_f * similarity * 0.25) as i32 + rng.random_range(5..=20);
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

    sqlx::query("UPDATE huya SET length_mm = $1 WHERE id = $2").bind(winner_len).bind(winner_id).execute(pool).await?;
    sqlx::query("UPDATE huya SET length_mm = $1 WHERE id = $2").bind(loser_len).bind(loser_id).execute(pool).await?;

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

/// Self-fight penalty: -5mm.
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

// ── Steal ─────────────────────────────────────────────────────────────────────

pub struct StealResult {
    pub success: bool,
    pub steal_mm: i32,
    pub chance_pct: u8,
    pub attacker: Huya,
    pub target: Huya,
}

/// Steal attempt with size-parity formula + cunning/scales/ghost skills.
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
    let cunning_bonus = att.skill_cunning as f64 * 0.025;
    let scales_penalty = tgt.skill_scales as f64 * 0.025;
    let eternal_bonus = att.skill_eternal as f64 * 0.15;
    let chance = (base_chance * (0.5 + 0.5 * parity) + cunning_bonus - scales_penalty + eternal_bonus)
        .clamp(0.05, 0.85);
    let chance_pct = (chance * 100.0).round() as u8;

    let raw_steal_f = tgt_len * parity * 0.20;
    let steal_cap = (tgt_len * 0.40) as i32;

    let ghost_dodge = {
        let mut rng = rand::rng();
        tgt.skill_ghost > 0 && rng.random_range(0.0_f64..1.0_f64) < 0.30
    };
    if ghost_dodge {
        let (att_updated, _) = get_or_create(pool, chat_id, attacker_tg_id).await?;
        let (tgt_updated, _) = get_or_create(pool, chat_id, target_tg_id).await?;
        return Ok(StealResult {
            success: false,
            steal_mm: 0,
            chance_pct,
            attacker: att_updated,
            target: tgt_updated,
        });
    }

    let (success, steal_mm) = {
        let mut rng = rand::rng();
        let roll: f64 = rng.random_range(0.0_f64..1.0_f64);
        let s = ((raw_steal_f as i32) + rng.random_range(5..=15)).min(steal_cap).max(5);
        (roll < chance, s)
    };

    if success {
        sqlx::query("UPDATE huya SET length_mm = length_mm + $1 WHERE id = $2")
            .bind(steal_mm).bind(att.id).execute(pool).await?;
        sqlx::query("UPDATE huya SET length_mm = length_mm - $1 WHERE id = $2")
            .bind(steal_mm).bind(tgt.id).execute(pool).await?;
    }

    let (att_updated, _) = get_or_create(pool, chat_id, attacker_tg_id).await?;
    let (tgt_updated, _) = get_or_create(pool, chat_id, target_tg_id).await?;

    Ok(StealResult { success, steal_mm, chance_pct, attacker: att_updated, target: tgt_updated })
}

// ── Leaderboard ───────────────────────────────────────────────────────────────

pub async fn top(pool: &PgPool, chat_id: i64, limit: i64) -> Result<Vec<(Huya, i64)>, AppError> {
    let rows = sqlx::query_as::<_, Huya>(
        &format!("SELECT {HUYA_SELECT} FROM huya WHERE chat_id = $1 ORDER BY length_mm DESC LIMIT $2"),
    )
    .bind(chat_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|h| { let id = h.tg_id; (h, id) }).collect())
}

// ── Skills ────────────────────────────────────────────────────────────────────

/// Check if a skill can be upgraded (prereqs met, under cap, enough SP).
/// Returns (col, cost, cap, prereq_ok).
fn skill_upgrade_info(huya: &Huya, skill: &str) -> Option<(&'static str, i32, i32, bool)> {
    let (col, current, cap, cost, prereq_ok) = match skill {
        "shaft" => ("skill_shaft", huya.skill_shaft, CAP_T1, COST_T1, true),
        "skin" => ("skill_skin", huya.skill_skin, CAP_T1, COST_T1, true),
        "balls" => ("skill_balls", huya.skill_balls, CAP_T1, COST_T1, true),
        "cunning" => ("skill_cunning", huya.skill_cunning, CAP_T1, COST_T1, true),
        "stamina" => ("skill_stamina", huya.skill_stamina, CAP_T1, COST_T1, true),
        "pierce" => ("skill_pierce", huya.skill_pierce, CAP_T2, COST_T2, huya.skill_shaft >= 8),
        "scales" => ("skill_scales", huya.skill_scales, CAP_T2, COST_T2, huya.skill_skin >= 8),
        "spirit" => ("skill_spirit", huya.skill_spirit, CAP_T2, COST_T2, huya.skill_balls >= 8),
        "pickpocket" => ("skill_pickpocket", huya.skill_pickpocket, CAP_T2, COST_T2, huya.skill_cunning >= 8),
        "dynamo" => ("skill_dynamo", huya.skill_dynamo, CAP_T2, COST_T2, huya.skill_stamina >= 8),
        "eggtwist" => ("skill_eggtwist", huya.skill_eggtwist, CAP_T3, COST_T3,
            huya.skill_pierce >= 5 && huya.skill_spirit >= 5),
        "bloodsucker" => ("skill_bloodsucker", huya.skill_bloodsucker, CAP_T3, COST_T3,
            huya.skill_pierce >= 5 && huya.skill_pickpocket >= 5),
        "ironballs" => ("skill_ironballs", huya.skill_ironballs, CAP_T3, COST_T3,
            huya.skill_scales >= 5 && huya.skill_spirit >= 5),
        "vortex" => ("skill_vortex", huya.skill_vortex, CAP_T3, COST_T3,
            huya.skill_spirit >= 5 && huya.skill_dynamo >= 5),
        "phantom" => ("skill_phantom", huya.skill_phantom, CAP_T3, COST_T3,
            huya.skill_pickpocket >= 5 && huya.skill_scales >= 5),
        "berserker" => ("skill_berserker", huya.skill_berserker, CAP_T4, COST_T4,
            huya.skill_eggtwist >= 7),
        "vampire" => ("skill_vampire", huya.skill_vampire, CAP_T4, COST_T4,
            huya.skill_bloodsucker >= 7),
        "fortress" => ("skill_fortress", huya.skill_fortress, CAP_T4, COST_T4,
            huya.skill_ironballs >= 7),
        "speedrun" => ("skill_speedrun", huya.skill_speedrun, CAP_T4, COST_T4,
            huya.skill_vortex >= 7),
        "ghost" => ("skill_ghost", huya.skill_ghost, CAP_T4, COST_T4,
            huya.skill_phantom >= 7),
        "eternal" => ("skill_eternal", huya.skill_eternal, CAP_T5, COST_T5,
            huya.skill_berserker >= 3 && huya.skill_fortress >= 3),
        "absolute" => ("skill_absolute", huya.skill_absolute, CAP_T5, COST_T5,
            huya.skill_berserker >= 1 && huya.skill_vampire >= 1 && huya.skill_fortress >= 1
                && huya.skill_speedrun >= 1 && huya.skill_ghost >= 1),
        _ => return None,
    };
    if !prereq_ok || current >= cap || huya.skill_points < cost {
        return None;
    }
    Some((col, cost, cap, true))
}

/// Spend skill_points to level up a skill. Returns updated Huya on success.
pub async fn upgrade_skill(pool: &PgPool, huya: &Huya, skill: &str) -> Result<Option<Huya>, AppError> {
    let Some((col, cost, _cap, _)) = skill_upgrade_info(huya, skill) else {
        return Ok(None);
    };

    let updated = sqlx::query_as::<_, Huya>(
        &format!("UPDATE huya SET {col} = {col} + 1, skill_points = skill_points - $1
         WHERE id = $2 RETURNING {HUYA_SELECT}"),
    )
    .bind(cost)
    .bind(huya.id)
    .fetch_one(pool)
    .await?;

    Ok(Some(updated))
}

// ── Shop ──────────────────────────────────────────────────────────────────────

/// Item costs in mm.
pub fn item_cost_mm(item_id: &str) -> Option<i32> {
    match item_id {
        "potion"     => Some(20),
        "adrenaline" => Some(30),
        "armor"      => Some(30),
        "steroid"    => Some(20),
        "energy"     => Some(50),
        _ => None,
    }
}

/// Buy an item from the shop. Deducts cost from length_mm and applies effect.
/// Returns updated Huya on success, None if broke or unknown item.
pub async fn buy_item(pool: &PgPool, huya: &Huya, item_id: &str) -> Result<Option<Huya>, AppError> {
    let cost = match item_cost_mm(item_id) {
        Some(c) => c,
        None => return Ok(None),
    };

    if huya.length_mm < cost {
        return Ok(None);
    }

    let max_hp = huya.max_hp();

    // Each branch uses hardcoded SQL (no user input in query text) — safe from injection.
    let updated = match item_id {
        "potion" => sqlx::query_as::<_, Huya>(
            &format!("UPDATE huya SET length_mm = length_mm - $1,
             hp = LEAST(hp + 50, $2) WHERE id = $3 AND length_mm >= $1
             RETURNING {HUYA_SELECT}"),
        )
        .bind(cost).bind(max_hp).bind(huya.id).fetch_optional(pool).await?,

        "adrenaline" => sqlx::query_as::<_, Huya>(
            &format!("UPDATE huya SET length_mm = length_mm - $1,
             atk_boost = LEAST(atk_boost + 35, 70) WHERE id = $2 AND length_mm >= $1
             RETURNING {HUYA_SELECT}"),
        )
        .bind(cost).bind(huya.id).fetch_optional(pool).await?,

        "armor" => sqlx::query_as::<_, Huya>(
            &format!("UPDATE huya SET length_mm = length_mm - $1,
             def_boost = LEAST(def_boost + 35, 70) WHERE id = $2 AND length_mm >= $1
             RETURNING {HUYA_SELECT}"),
        )
        .bind(cost).bind(huya.id).fetch_optional(pool).await?,

        "steroid" => sqlx::query_as::<_, Huya>(
            &format!("UPDATE huya SET length_mm = length_mm - $1,
             grow_boost = 1 WHERE id = $2 AND length_mm >= $1
             RETURNING {HUYA_SELECT}"),
        )
        .bind(cost).bind(huya.id).fetch_optional(pool).await?,

        "energy" => sqlx::query_as::<_, Huya>(
            &format!("UPDATE huya SET length_mm = length_mm - $1,
             actions_left = actions_left + 1 WHERE id = $2 AND length_mm >= $1
             RETURNING {HUYA_SELECT}"),
        )
        .bind(cost).bind(huya.id).fetch_optional(pool).await?,

        _ => return Ok(None),
    };

    Ok(updated)
}

// ── Interactive fight (huya_fight table) ─────────────────────────────────────

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct HuyaFight {
    pub id: i32,
    pub chat_id: i64,
    pub challenger_tg_id: i64,
    pub target_tg_id: i64,
    pub challenger_pick: Option<i32>,
    pub target_pick: Option<i32>,
    /// "pending" | "active" | "done"
    pub status: String,
    pub message_id: i32,
    pub created_at: DateTime<Utc>,
    pub ch_hp: i32,
    pub tg_hp: i32,
    pub round: i32,
}

const FIGHT_SELECT: &str =
    "id, chat_id, challenger_tg_id, target_tg_id, challenger_pick, target_pick, \
     status, message_id, created_at, ch_hp, tg_hp, round";

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
    .bind(chat_id).bind(challenger_tg_id).bind(target_tg_id)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn set_fight_message_id(pool: &PgPool, fight_id: i32, message_id: i32) -> Result<(), AppError> {
    sqlx::query("UPDATE huya_fight SET message_id = $1 WHERE id = $2")
        .bind(message_id).bind(fight_id).execute(pool).await?;
    Ok(())
}

/// Accept fight: set status to active and snapshot both players' current HP.
pub async fn accept_fight(pool: &PgPool, fight_id: i32, target_tg_id: i64) -> Result<Option<HuyaFight>, AppError> {
    let fight = match get_fight(pool, fight_id).await? {
        Some(f) if f.status == "pending" && f.target_tg_id == target_tg_id => f,
        _ => return Ok(None),
    };

    let (ch, _) = get_or_create(pool, fight.chat_id, fight.challenger_tg_id).await?;
    let (tg, _) = get_or_create(pool, fight.chat_id, fight.target_tg_id).await?;

    let row = sqlx::query_as::<_, HuyaFight>(
        &format!("UPDATE huya_fight SET status = 'active', ch_hp = $1, tg_hp = $2
         WHERE id = $3 AND target_tg_id = $4 AND status = 'pending'
         RETURNING {FIGHT_SELECT}"),
    )
    .bind(ch.hp).bind(tg.hp).bind(fight_id).bind(target_tg_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn decline_fight(pool: &PgPool, fight_id: i32, target_tg_id: i64) -> Result<Option<HuyaFight>, AppError> {
    let row = sqlx::query_as::<_, HuyaFight>(
        &format!("UPDATE huya_fight SET status = 'done'
         WHERE id = $1 AND target_tg_id = $2 AND status = 'pending'
         RETURNING {FIGHT_SELECT}"),
    )
    .bind(fight_id).bind(target_tg_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn get_fight(pool: &PgPool, fight_id: i32) -> Result<Option<HuyaFight>, AppError> {
    let row = sqlx::query_as::<_, HuyaFight>(
        &format!("SELECT {FIGHT_SELECT} FROM huya_fight WHERE id = $1"),
    )
    .bind(fight_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Store a player's pick (immutable once set). Returns updated fight or None.
pub async fn store_pick(pool: &PgPool, fight_id: i32, tg_id: i64, pick: i32) -> Result<Option<HuyaFight>, AppError> {
    let row = sqlx::query_as::<_, HuyaFight>(
        &format!("UPDATE huya_fight SET
           challenger_pick = CASE WHEN challenger_tg_id = $2 AND challenger_pick IS NULL THEN $3 ELSE challenger_pick END,
           target_pick     = CASE WHEN target_tg_id     = $2 AND target_pick     IS NULL THEN $3 ELSE target_pick     END
         WHERE id = $1 AND status = 'active' AND (challenger_tg_id = $2 OR target_tg_id = $2)
         RETURNING {FIGHT_SELECT}"),
    )
    .bind(fight_id).bind(tg_id).bind(pick)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn finish_huya_fight(pool: &PgPool, fight_id: i32) -> Result<(), AppError> {
    sqlx::query("UPDATE huya_fight SET status = 'done' WHERE id = $1")
        .bind(fight_id).execute(pool).await?;
    Ok(())
}

// ── Multi-round fight logic ───────────────────────────────────────────────────

pub struct RoundResult {
    /// Updated fight record (ch_hp, tg_hp, round advanced, picks cleared).
    pub fight: HuyaFight,
    /// Damage received by challenger this round (0 if they won).
    pub ch_damage: i32,
    /// Damage received by target this round (0 if they won).
    pub tg_damage: i32,
    /// tg_id of the round winner; None on tie.
    pub round_winner_tg_id: Option<i64>,
    /// True when the fight should end (HP ≤ 0 or max rounds reached).
    pub fight_over: bool,
    /// tg_id of the fight winner; None on HP-draw.
    pub fight_winner_tg_id: Option<i64>,
}

/// Process one RPS round. Calculates damage with full skill tree, updates fight HP/round, clears picks.
/// Applies: shaft, skin, pierce, berserker, eternal, vortex, eggtwist, spirit (heal on win).
pub async fn process_round(
    pool: &PgPool,
    fight: &HuyaFight,
    ch_pick: i32,
    tg_pick: i32,
) -> Result<RoundResult, AppError> {
    let (ch, _) = get_or_create(pool, fight.chat_id, fight.challenger_tg_id).await?;
    let (tg, _) = get_or_create(pool, fight.chat_id, fight.target_tg_id).await?;

    let is_tie = ch_pick == tg_pick;
    // RPS: Напор(0) > В шары(2) > Финт(1) > Напор(0)
    let ch_wins_round = !is_tie && matches!((ch_pick, tg_pick), (0, 2) | (2, 1) | (1, 0));

    let (ch_damage, tg_damage, round_winner_tg_id) = if is_tie {
        (5, 5, None)
    } else {
        let (winner, loser) = if ch_wins_round { (&ch, &tg) } else { (&tg, &ch) };
        let (winner_hp, _loser_hp) = if ch_wins_round { (fight.ch_hp, fight.tg_hp) } else { (fight.tg_hp, fight.ch_hp) };
        let damage: i32 = {
            let mut rng = rand::rng();
            let base = winner.length_mm.max(1) as f64 * 0.05
                + rng.random_range(10.0_f64..25.0_f64);
            let pierce_factor = 1.0 / (1.0 - winner.skill_pierce as f64 * 0.04).max(0.1);
            let berserker_mult = if (winner_hp as f64) < (winner.max_hp() as f64) * 0.3 && winner.skill_berserker > 0 {
                2.0
            } else {
                1.0
            };
            let eternal_mult = 1.0 + winner.skill_eternal as f64 * 0.15;
            let vortex_crit = if fight.round == 1 && winner.skill_vortex > 0 { 1.5 } else { 1.0 };
            let eggtwist_crit = if fight.round % 3 == 0 && winner.skill_eggtwist > 0 { 2.0 } else { 1.0 };
            let atk_factor = (1.0 + winner.skill_shaft as f64 * 0.04 + winner.atk_boost as f64 / 100.0)
                * berserker_mult * eternal_mult * vortex_crit * eggtwist_crit;
            let def_factor = (1.0 - loser.skill_skin as f64 * 0.03 - loser.def_boost as f64 / 100.0)
                .max(0.15) * pierce_factor;
            ((base * atk_factor * def_factor) as i32).max(5)
        };
        if ch_wins_round {
            (0, damage, Some(fight.challenger_tg_id))
        } else {
            (damage, 0, Some(fight.target_tg_id))
        }
    };

    let mut new_ch_hp = (fight.ch_hp - ch_damage).max(0);
    let mut new_tg_hp = (fight.tg_hp - tg_damage).max(0);

    // Spirit: heal on round win
    if let Some(winner_tg_id) = round_winner_tg_id {
        let (winner, winner_hp) = if winner_tg_id == fight.challenger_tg_id {
            (&ch, &mut new_ch_hp)
        } else {
            (&tg, &mut new_tg_hp)
        };
        if winner.skill_spirit > 0 {
            *winner_hp = (*winner_hp + winner.skill_spirit * 12).min(winner.max_hp());
        }
    }

    let new_round = fight.round + 1; let fight_over = new_ch_hp == 0 || new_tg_hp == 0 || fight.round >= MAX_ROUNDS;

    let fight_winner_tg_id = if fight_over {
        if new_ch_hp > new_tg_hp {
            Some(fight.challenger_tg_id)
        } else if new_tg_hp > new_ch_hp {
            Some(fight.target_tg_id)
        } else {
            None
        }
    } else {
        None
    };

    let updated = sqlx::query_as::<_, HuyaFight>(
        &format!("UPDATE huya_fight SET
         ch_hp = $1, tg_hp = $2, round = $3,
         challenger_pick = NULL, target_pick = NULL
         WHERE id = $4 RETURNING {FIGHT_SELECT}"),
    )
    .bind(new_ch_hp).bind(new_tg_hp).bind(new_round).bind(fight.id)
    .fetch_one(pool)
    .await?;

    Ok(RoundResult {
        fight: updated,
        ch_damage,
        tg_damage,
        round_winner_tg_id,
        fight_over,
        fight_winner_tg_id,
    })
}

/// Apply length transfer after fight ends. Writes HP back, updates fights_won/lost,
/// applies fortress (min 10mm), bloodsucker bonus, vampire heal. Resets boosts.
pub async fn finalize_fight_result(
    pool: &PgPool,
    fight: &HuyaFight,
    winner_tg_id: i64,
    loser_tg_id: i64,
) -> Result<(i32, i32), AppError> {
    let (ch, _) = get_or_create(pool, fight.chat_id, fight.challenger_tg_id).await?;
    let (tg, _) = get_or_create(pool, fight.chat_id, fight.target_tg_id).await?;

    let (winner, loser) = if winner_tg_id == fight.challenger_tg_id {
        (&ch, &tg)
    } else {
        (&tg, &ch)
    };

    let ch_len = ch.length_mm.max(1) as f64;
    let tg_len = tg.length_mm.max(1) as f64;
    let similarity = ch_len.min(tg_len) / ch_len.max(tg_len);

    let (steal_mm, elo_gain) = {
        let mut rng = rand::rng();
        let loser_len_f = if loser_tg_id == fight.challenger_tg_id { ch_len } else { tg_len };
        let raw = (loser_len_f * similarity * 0.30) as i32 + rng.random_range(5..=20);
        let cap = ((ch_len.max(tg_len) * 0.40) as i32).max(10);
        let s = raw.min(cap).max(5);
        let bloodsucker_bonus = winner.skill_bloodsucker * fight.round;
        let s = s + bloodsucker_bonus;
        let e: i32 = rng.random_range(5..=25);
        (s, e)
    };

    let loser_new_len = loser.length_mm - steal_mm;
    let steal_actual = if loser.skill_fortress > 0 && loser_new_len < 10 {
        (loser.length_mm - 10).max(0)
    } else {
        steal_mm
    };

    sqlx::query("UPDATE huya SET length_mm = length_mm + $1, fights_won = fights_won + 1 WHERE id = $2")
        .bind(steal_actual).bind(winner.id).execute(pool).await?;
    sqlx::query("UPDATE huya SET length_mm = length_mm - $1, fights_lost = fights_lost + 1 WHERE id = $2")
        .bind(steal_actual).bind(loser.id).execute(pool).await?;

    // Fortress: clamp loser to 10mm min
    if loser.skill_fortress > 0 && loser_new_len < 10 {
        sqlx::query("UPDATE huya SET length_mm = 10 WHERE id = $1 AND length_mm < 10")
            .bind(loser.id).execute(pool).await?;
    }

    // Vampire: heal winner proportional to damage (use steal as proxy)
    let winner_max_hp = winner.max_hp();
    if winner.skill_vampire > 0 && steal_actual > 0 {
        let heal = (steal_actual * 2).min(50);
        sqlx::query("UPDATE huya SET hp = LEAST(hp + $1, $2) WHERE id = $3")
            .bind(heal).bind(winner_max_hp).bind(winner.id).execute(pool).await?;
    }

    // Write final HP back (min 1) and clear boosts.
    sqlx::query("UPDATE huya SET hp = $1, atk_boost = 0, def_boost = 0 WHERE id = $2")
        .bind(fight.ch_hp.max(1)).bind(ch.id).execute(pool).await?;
    sqlx::query("UPDATE huya SET hp = $1, atk_boost = 0, def_boost = 0 WHERE id = $2")
        .bind(fight.tg_hp.max(1)).bind(tg.id).execute(pool).await?;

    Ok((steal_actual, elo_gain))
}
