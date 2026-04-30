//! Database operations for the HuyActa tamagotchi game.
//!
//! Each chat × user pair has one Huya row.
//! length_mm < 0 means the user is in pussy mode instead of dick mode.
//! Daily action limit = max_actions() (base 4 + Dynamo bonus).
//!
//! HP is persistent; replenishes on consume_action (+10 + skill_stamina*3, capped at max_hp()).
//! Skills: 20-tier tree (T1-5) levelled via upgrade_skill() using skill_points earned on level-up.
//! Shop boosts: atk_boost / def_boost / grow_boost — temporary, reset after use.

use chrono::{DateTime, NaiveDate, Utc};
use rand::RngExt;
use sqlx::PgPool;
use tokio::time::{sleep, Duration};

use crate::db::kv;
use crate::db::models::{
    Huya, HuyaDutchHelmEvent, HuyaEquipmentSlot, HuyaInventoryItem, HuyaSocketedGem,
};
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
     atk_boost, def_boost, grow_boost, \
     steal_boost, \
     pet_energy_left, pet_energy_reset_at, \
     energy_buys_today, energy_buys_reset_at";

const XP_PER_LEVEL: i32 = 100;
const LEVEL_UP_BONUS_MM: i32 = 50;

fn calc_level_progress(current_xp: i32, current_level: i32, gained_xp: i32) -> (i32, i32, i32) {
    let gained_xp = gained_xp.max(0);
    let total_xp = current_xp + gained_xp;
    let level_ups = total_xp / XP_PER_LEVEL;
    let new_xp = total_xp % XP_PER_LEVEL;
    let new_level = current_level + level_ups;
    (new_xp, new_level, level_ups)
}

async fn apply_xp_gain(pool: &PgPool, huya_id: i32, gained_xp: i32) -> Result<Huya, AppError> {
    let current = sqlx::query_as::<_, Huya>(&format!(
        "SELECT {HUYA_SELECT} FROM huya WHERE id = $1"
    ))
    .bind(huya_id)
    .fetch_one(pool)
    .await?;

    let (new_xp, new_level, level_ups) = calc_level_progress(current.xp, current.level, gained_xp);
    if level_ups == 0 {
        return Ok(current);
    }

    let updated = sqlx::query_as::<_, Huya>(&format!(
        "UPDATE huya
         SET xp = $1,
             level = $2,
             skill_points = skill_points + $3
         WHERE id = $4
         RETURNING {HUYA_SELECT}"
    ))
    .bind(new_xp)
    .bind(new_level)
    .bind(level_ups)
    .bind(huya_id)
    .fetch_one(pool)
    .await?;
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::calc_level_progress;

    #[test]
    fn level_progress_handles_overflow() {
        let (xp, level, level_ups) = calc_level_progress(95, 3, 220);
        assert_eq!(xp, 15);
        assert_eq!(level, 6);
        assert_eq!(level_ups, 3);
    }
}

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
    // Global Huya per user: first try to find by tg_id regardless of chat.
    if let Some(existing) = sqlx::query_as::<_, Huya>(
        &format!("SELECT {HUYA_SELECT} FROM huya WHERE tg_id = $1 ORDER BY created_at LIMIT 1"),
    )
    .bind(tg_id)
    .fetch_optional(pool)
    .await?
    {
        return Ok((existing, false));
    }

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
        &format!("SELECT {HUYA_SELECT} FROM huya WHERE tg_id = $1 ORDER BY created_at LIMIT 1"),
    )
    .bind(tg_id)
    .fetch_one(pool)
    .await?;

    Ok((h, false))
}

// ── Actions ───────────────────────────────────────────────────────────────────

async fn energy_limit_enabled(pool: &PgPool, chat_id: i64) -> Result<bool, AppError> {
    // Per-plan: allow turning energy limit on/off at runtime via KV.
    // Key: "huya_energy_limit", chat_id=0 for global flag.
    if let Some(item) = kv::get(pool, 0, "huya_energy_limit").await? {
        Ok(item.value != "0")
    } else {
        Ok(true)
    }
}

/// Consume one action and regenerate HP (+10 + skill_stamina*3, capped at max_hp).
/// Returns false if no actions left today.
pub async fn consume_action(pool: &PgPool, huya: &Huya) -> Result<bool, AppError> {
    let today = Utc::now().date_naive();
    let max_hp = huya.max_hp();
    let max_actions = huya.max_actions();
    let hp_regen = 10 + huya.skill_stamina * 3;

    let limit_enabled = energy_limit_enabled(pool, huya.chat_id).await?;

    // If limit is globally disabled — only regenerate HP, do not touch actions_left.
    if !limit_enabled {
        sqlx::query(
            "UPDATE huya SET hp = LEAST(hp + $1, $2) WHERE id = $3",
        )
        .bind(hp_regen)
        .bind(max_hp)
        .bind(huya.id)
        .execute(pool)
        .await?;
        return Ok(true);
    }

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
    let (new_xp, new_level, level_ups) = calc_level_progress(huya.xp, huya.level, xp_gain);
    let leveled_up = level_ups > 0;
    let final_length = new_length + LEVEL_UP_BONUS_MM * level_ups;
    let sp_delta: i32 = level_ups;

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

    let total_grow = grow_mm + LEVEL_UP_BONUS_MM * level_ups;
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
        (
            challenger_tg_id,
            target_tg_id,
            ch.id,
            tg.id,
            ch.length_mm + steal_mm,
            (tg.length_mm - steal_mm).max(0),
        )
    } else {
        (
            target_tg_id,
            challenger_tg_id,
            tg.id,
            ch.id,
            tg.length_mm + steal_mm,
            (ch.length_mm - steal_mm).max(0),
        )
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
    pub backlash_mm: i32,
    pub xp_gain: i32,
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
    let att_fx = equipment_effects_for_player(pool, chat_id, attacker_tg_id).await.unwrap_or_default();
    let tgt_fx = equipment_effects_for_player(pool, chat_id, target_tg_id).await.unwrap_or_default();

    let att_len = att.length_mm.max(1) as f64;
    let tgt_len = tgt.length_mm.max(1) as f64;
    let parity = att_len.min(tgt_len) / att_len.max(tgt_len);
    let base_chance = att_len / (att_len + tgt_len);
    let cunning_bonus = att.skill_cunning as f64 * 0.025;
    let scales_penalty = tgt.skill_scales as f64 * 0.025;
    let eternal_bonus = att.skill_eternal as f64 * 0.15;
    let booster_bonus = att.steal_boost as f64 * 0.01;
    let form_chance_bonus = if att.is_pussy() { 0.06 } else { 0.0 };
    let form_resist_bonus = if tgt.is_pussy() { 0.06 } else { 0.0 };
    let chance = (base_chance * (0.5 + 0.5 * parity) + cunning_bonus - scales_penalty + eternal_bonus)
        + att_fx.steal_chance_pct
        + booster_bonus
        + form_chance_bonus
        - tgt_fx.steal_resist_pct
        - form_resist_bonus;
    let chance = chance.clamp(0.05, 0.85);
    let chance_pct = (chance * 100.0).round() as u8;

    let steal_cap = (tgt_len * 0.28) as i32;

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
            backlash_mm: 0,
            xp_gain: 0,
            chance_pct,
            attacker: att_updated,
            target: tgt_updated,
        });
    }

    let (success, steal_mm, backlash_mm) = {
        let mut rng = rand::rng();
        let roll: f64 = rng.random_range(0.0_f64..1.0_f64);
        let success = roll < chance;

        // Weighted steal distribution: most steals are small, large steals are rare.
        let ratio = match rng.random_range(0.0_f64..1.0_f64) {
            x if x < 0.60 => rng.random_range(0.04_f64..0.10_f64),
            x if x < 0.90 => rng.random_range(0.10_f64..0.16_f64),
            _ => rng.random_range(0.16_f64..0.22_f64),
        };
        let raw_steal = (tgt_len * parity * ratio).round() as i32;
        let mut steal_mm = raw_steal.clamp(0, steal_cap.max(0));
        if steal_mm == 0 && steal_cap > 0 {
            steal_mm = 1;
        }

        let backlash_mm = if success {
            0
        } else {
            // Counter-bite chance grows with defensive/counter skills.
            let backlash_chance = (
                0.10
                + tgt.skill_scales as f64 * 0.015
                + tgt.skill_ironballs as f64 * 0.03
                + tgt.skill_ghost as f64 * 0.04
                + tgt.skill_eternal as f64 * 0.05
                - att.skill_phantom as f64 * 0.02
                - att.skill_cunning as f64 * 0.01
            )
            .clamp(0.05, 0.70);

            if rng.random_range(0.0_f64..1.0_f64) < backlash_chance {
                // Failed steal can backfire: attacker loses a tiny piece to target.
                let backlash_ratio = (
                    0.01
                    + tgt.skill_ironballs as f64 * 0.002
                    + tgt.skill_scales as f64 * 0.001
                    + if tgt.is_pussy() { 0.01 } else { 0.0 }
                )
                .clamp(0.01, 0.04);
                ((att_len * backlash_ratio) as i32).clamp(2, 12)
            } else {
                0
            }
        };

        (success, steal_mm, backlash_mm)
    };

    if success {
        sqlx::query("UPDATE huya SET length_mm = length_mm + $1 WHERE id = $2")
            .bind(steal_mm).bind(att.id).execute(pool).await?;
        sqlx::query("UPDATE huya SET length_mm = GREATEST(length_mm - $1, 0) WHERE id = $2")
            .bind(steal_mm).bind(tgt.id).execute(pool).await?;
    } else if backlash_mm > 0 {
        sqlx::query("UPDATE huya SET length_mm = GREATEST(length_mm - $1, 0) WHERE id = $2")
            .bind(backlash_mm).bind(att.id).execute(pool).await?;
        sqlx::query("UPDATE huya SET length_mm = length_mm + $1 WHERE id = $2")
            .bind(backlash_mm).bind(tgt.id).execute(pool).await?;
    }
    // One steal attempt consumes temporary steal booster.
    if att.steal_boost > 0 {
        let _ = sqlx::query("UPDATE huya SET steal_boost = 0 WHERE id = $1")
            .bind(att.id)
            .execute(pool)
            .await;
    }

    let xp_gain = if success {
        // Pickpocket grants bonus XP to the thief on successful steals.
        // No XP is removed from the victim.
        8 + att.skill_pickpocket.max(0) * 2
    } else {
        0
    };
    let att_updated = if xp_gain > 0 {
        apply_xp_gain(pool, att.id, xp_gain).await?
    } else {
        get_or_create(pool, chat_id, attacker_tg_id).await?.0
    };
    let (tgt_updated, _) = get_or_create(pool, chat_id, target_tg_id).await?;

    Ok(StealResult {
        success,
        steal_mm,
        backlash_mm,
        xp_gain,
        chance_pct,
        attacker: att_updated,
        target: tgt_updated,
    })
}

// ── Leaderboard ───────────────────────────────────────────────────────────────

pub async fn top(pool: &PgPool, _chat_id: i64, limit: i64) -> Result<Vec<(Huya, i64)>, AppError> {
    let rows = sqlx::query_as::<_, Huya>(
        &format!("SELECT {HUYA_SELECT} FROM huya ORDER BY length_mm DESC LIMIT $1"),
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|h| { let id = h.tg_id; (h, id) }).collect())
}

// ── Inventory & equipment ─────────────────────────────────────────────────────

fn ring_slots_for_length(length_mm: i32) -> i32 {
    let len = length_mm.max(0);
    match len {
        0..=49 => 0,
        50..=99 => 1,
        100..=149 => 2,
        150..=199 => 3,
        200..=299 => 4,
        300..=499 => 5,
        _ => 6,
    }
}

fn unlocked_piercing_slots(length_mm: i32) -> Vec<&'static str> {
    let len = length_mm.max(0);
    let mut slots = Vec::new();
    if len >= 50 {
        slots.push("piercing_tip_1");
        slots.push("piercing_shaft_1");
    }
    if len >= 120 {
        slots.push("piercing_base_1");
    }
    if len >= 200 {
        slots.push("piercing_tip_2");
        slots.push("piercing_shaft_2");
    }
    if len >= 300 {
        slots.push("piercing_base_2");
    }
    if len >= 450 {
        slots.push("piercing_tip_3");
        slots.push("piercing_shaft_3");
    }
    slots
}

pub fn slot_unlocked_for_length(slot: &str, length_mm: i32) -> bool {
    if slot.starts_with("ring_") {
        let requested = slot.trim_start_matches("ring_").parse::<i32>().unwrap_or(99);
        return requested <= ring_slots_for_length(length_mm);
    }
    if slot.starts_with("piercing_") {
        return unlocked_piercing_slots(length_mm).contains(&slot);
    }
    matches!(slot, "tip" | "base" | "balls")
}

pub async fn get_inventory(pool: &PgPool, chat_id: i64, tg_id: i64) -> Result<Vec<HuyaInventoryItem>, AppError> {
    let items = sqlx::query_as::<_, HuyaInventoryItem>(
        "SELECT id, chat_id, tg_id, item_id, rarity, item_kind, slot, trait AS trait_name, \
                roll, charges, sell_price_mm, booster_effect, booster_value, booster_scope, \
                socket_capacity, reforge_level, acquired_at \
         FROM huya_inventory WHERE tg_id = $1 \
         ORDER BY acquired_at DESC, id DESC",
    )
    .bind(tg_id)
    .fetch_all(pool)
    .await?;
    Ok(items)
}

pub async fn get_equipment(pool: &PgPool, chat_id: i64, tg_id: i64) -> Result<Vec<HuyaEquipmentSlot>, AppError> {
    let rows = sqlx::query_as::<_, HuyaEquipmentSlot>(
        "SELECT DISTINCT ON (slot) chat_id, tg_id, slot, inventory_id \
         FROM huya_equipment WHERE tg_id = $1 \
         ORDER BY slot, chat_id DESC",
    )
    .bind(tg_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Grant a loot item to a player. Returns the created inventory row.
pub async fn grant_loot(pool: &PgPool, chat_id: i64, tg_id: i64, item_id: &str) -> Result<HuyaInventoryItem, AppError> {
    let row = sqlx::query_as::<_, HuyaInventoryItem>(
        "INSERT INTO huya_inventory (chat_id, tg_id, item_id) \
         VALUES ($1, $2, $3) \
         RETURNING id, chat_id, tg_id, item_id, rarity, item_kind, slot, trait AS trait_name, \
                  roll, charges, sell_price_mm, booster_effect, booster_value, booster_scope, \
                  socket_capacity, reforge_level, acquired_at",
    )
    .bind(chat_id)
    .bind(tg_id)
    .bind(item_id)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

#[derive(Clone)]
pub struct ChestDef {
    pub id: &'static str,
    pub price_mm: i32,
    pub daily_free: bool,
}

#[derive(Clone)]
pub struct ItemTemplate {
    pub id: &'static str,
    pub rarity: &'static str,
    pub item_kind: &'static str,
    pub slot: Option<&'static str>,
    pub trait_name: Option<&'static str>,
    pub roll_min: i32,
    pub roll_max: i32,
    pub sell_price_mm: i32,
    pub booster_effect: Option<&'static str>,
    pub booster_value: i32,
    pub booster_scope: Option<&'static str>,
    pub socket_capacity_base: i32,
    pub weight: i32,
}

pub fn chest_defs() -> Vec<ChestDef> {
    vec![
        ChestDef { id: "cheap_crate", price_mm: 35, daily_free: false },
        ChestDef { id: "fighter_crate", price_mm: 90, daily_free: false },
        ChestDef { id: "royal_crate", price_mm: 220, daily_free: false },
        ChestDef { id: "daily_free_crate", price_mm: 0, daily_free: true },
    ]
}

pub fn item_templates() -> Vec<ItemTemplate> {
    vec![
        // Trash / common
        ItemTemplate { id: "ring_plastic", rarity: "trash", item_kind: "equipment", slot: Some("ring"), trait_name: Some("jittery"), roll_min: 1, roll_max: 3, sell_price_mm: 4, booster_effect: None, booster_value: 0, booster_scope: None, socket_capacity_base: 1, weight: 35 },
        ItemTemplate { id: "ring_tape", rarity: "trash", item_kind: "equipment", slot: Some("ring"), trait_name: Some("sticky"), roll_min: 1, roll_max: 4, sell_price_mm: 5, booster_effect: None, booster_value: 0, booster_scope: None, socket_capacity_base: 1, weight: 32 },
        ItemTemplate { id: "cage_cheap", rarity: "common", item_kind: "equipment", slot: Some("base"), trait_name: Some("cage_guard"), roll_min: 3, roll_max: 8, sell_price_mm: 10, booster_effect: None, booster_value: 0, booster_scope: None, socket_capacity_base: 1, weight: 24 },
        ItemTemplate { id: "tip_condom_plus", rarity: "common", item_kind: "equipment", slot: Some("tip"), trait_name: Some("safe_poke"), roll_min: 3, roll_max: 8, sell_price_mm: 11, booster_effect: None, booster_value: 0, booster_scope: None, socket_capacity_base: 1, weight: 24 },
        // Rare+
        ItemTemplate { id: "ring_spiked", rarity: "rare", item_kind: "equipment", slot: Some("ring"), trait_name: Some("spiked_ring_reflect"), roll_min: 6, roll_max: 14, sell_price_mm: 20, booster_effect: None, booster_value: 0, booster_scope: None, socket_capacity_base: 2, weight: 14 },
        ItemTemplate { id: "cage_iron", rarity: "rare", item_kind: "equipment", slot: Some("base"), trait_name: Some("anti_burst"), roll_min: 6, roll_max: 15, sell_price_mm: 22, booster_effect: None, booster_value: 0, booster_scope: None, socket_capacity_base: 2, weight: 12 },
        ItemTemplate { id: "tip_vamp", rarity: "epic", item_kind: "equipment", slot: Some("tip"), trait_name: Some("blood_taste"), roll_min: 10, roll_max: 20, sell_price_mm: 35, booster_effect: None, booster_value: 0, booster_scope: None, socket_capacity_base: 3, weight: 8 },
        ItemTemplate { id: "balls_dyn", rarity: "epic", item_kind: "equipment", slot: Some("balls"), trait_name: Some("raid_initiative"), roll_min: 10, roll_max: 22, sell_price_mm: 36, booster_effect: None, booster_value: 0, booster_scope: None, socket_capacity_base: 3, weight: 7 },
        ItemTemplate { id: "ring_legend_halo", rarity: "legendary", item_kind: "equipment", slot: Some("ring"), trait_name: Some("eternal_echo"), roll_min: 18, roll_max: 32, sell_price_mm: 70, booster_effect: None, booster_value: 0, booster_scope: None, socket_capacity_base: 4, weight: 2 },
        // Piercing gear
        ItemTemplate { id: "piercing_tip_silver", rarity: "common", item_kind: "equipment", slot: Some("piercing_tip"), trait_name: Some("tip_focus"), roll_min: 4, roll_max: 10, sell_price_mm: 13, booster_effect: None, booster_value: 0, booster_scope: None, socket_capacity_base: 1, weight: 14 },
        ItemTemplate { id: "piercing_shaft_chain", rarity: "rare", item_kind: "equipment", slot: Some("piercing_shaft"), trait_name: Some("shaft_grip"), roll_min: 8, roll_max: 16, sell_price_mm: 25, booster_effect: None, booster_value: 0, booster_scope: None, socket_capacity_base: 2, weight: 10 },
        ItemTemplate { id: "piercing_base_anchor", rarity: "epic", item_kind: "equipment", slot: Some("piercing_base"), trait_name: Some("base_anchor"), roll_min: 12, roll_max: 22, sell_price_mm: 38, booster_effect: None, booster_value: 0, booster_scope: None, socket_capacity_base: 2, weight: 6 },
        // Gems (consumed by socket/reforge)
        ItemTemplate { id: "gem_ruby_fury", rarity: "rare", item_kind: "gem", slot: None, trait_name: Some("gem_atk"), roll_min: 5, roll_max: 14, sell_price_mm: 18, booster_effect: None, booster_value: 0, booster_scope: None, socket_capacity_base: 0, weight: 11 },
        ItemTemplate { id: "gem_sapphire_wall", rarity: "rare", item_kind: "gem", slot: None, trait_name: Some("gem_def"), roll_min: 5, roll_max: 14, sell_price_mm: 18, booster_effect: None, booster_value: 0, booster_scope: None, socket_capacity_base: 0, weight: 11 },
        ItemTemplate { id: "gem_emerald_snatch", rarity: "epic", item_kind: "gem", slot: None, trait_name: Some("gem_steal"), roll_min: 8, roll_max: 18, sell_price_mm: 24, booster_effect: None, booster_value: 0, booster_scope: None, socket_capacity_base: 0, weight: 7 },
        ItemTemplate { id: "gem_topaz_haste", rarity: "epic", item_kind: "gem", slot: None, trait_name: Some("gem_initiative"), roll_min: 8, roll_max: 18, sell_price_mm: 25, booster_effect: None, booster_value: 0, booster_scope: None, socket_capacity_base: 0, weight: 7 },
        ItemTemplate { id: "gem_obsidian_thorns", rarity: "legendary", item_kind: "gem", slot: None, trait_name: Some("gem_reflect"), roll_min: 10, roll_max: 22, sell_price_mm: 42, booster_effect: None, booster_value: 0, booster_scope: None, socket_capacity_base: 0, weight: 3 },
        // Boosters
        ItemTemplate { id: "booster_rage_syrup", rarity: "common", item_kind: "booster", slot: None, trait_name: Some("rage"), roll_min: 0, roll_max: 0, sell_price_mm: 9, booster_effect: Some("atk_boost"), booster_value: 20, booster_scope: Some("next_fight"), socket_capacity_base: 0, weight: 12 },
        ItemTemplate { id: "booster_steal_grease", rarity: "rare", item_kind: "booster", slot: None, trait_name: Some("grease"), roll_min: 0, roll_max: 0, sell_price_mm: 16, booster_effect: Some("steal_boost"), booster_value: 2, booster_scope: Some("next_steal"), socket_capacity_base: 0, weight: 8 },
        ItemTemplate { id: "booster_growth_cream", rarity: "common", item_kind: "booster", slot: None, trait_name: Some("growth"), roll_min: 0, roll_max: 0, sell_price_mm: 12, booster_effect: Some("grow_boost"), booster_value: 1, booster_scope: Some("next_grow"), socket_capacity_base: 0, weight: 10 },
    ]
}

fn pick_weighted<'a>(items: &'a [ItemTemplate]) -> Option<&'a ItemTemplate> {
    if items.is_empty() {
        return None;
    }
    let total: i32 = items.iter().map(|x| x.weight.max(0)).sum();
    if total <= 0 {
        return items.first();
    }
    let mut rng = rand::rng();
    let mut roll = rng.random_range(1..=total);
    for item in items {
        roll -= item.weight.max(0);
        if roll <= 0 {
            return Some(item);
        }
    }
    items.first()
}

fn rarity_allowed(chest_id: &str, rarity: &str) -> bool {
    match chest_id {
        "cheap_crate" => matches!(rarity, "trash" | "common" | "rare"),
        "fighter_crate" => matches!(rarity, "common" | "rare" | "epic" | "legendary"),
        "royal_crate" => matches!(rarity, "rare" | "epic" | "legendary"),
        "daily_free_crate" => matches!(rarity, "trash" | "common" | "rare"),
        _ => false,
    }
}

pub async fn claim_daily_chest(
    pool: &PgPool,
    chat_id: i64,
    tg_id: i64,
    chest_id: &str,
) -> Result<(bool, Option<i64>), AppError> {
    let last_claim: Option<DateTime<Utc>> = sqlx::query_scalar::<_, Option<DateTime<Utc>>>(
        "SELECT MAX(claimed_at) FROM huya_daily_chest_claim
         WHERE tg_id = $1 AND chest_id = $2",
    )
    .bind(tg_id)
    .bind(chest_id)
    .fetch_one(pool)
    .await?;

    if let Some(ts) = last_claim {
        let diff = Utc::now() - ts;
        let secs = 24 * 3600 - diff.num_seconds();
        if secs > 0 {
            return Ok((false, Some(secs)));
        }
    }

    sqlx::query(
        "INSERT INTO huya_daily_chest_claim (chat_id, tg_id, chest_id, claimed_at)
         VALUES (0, $1, $2, NOW())
         ON CONFLICT (chat_id, tg_id, chest_id)
         DO UPDATE SET claimed_at = EXCLUDED.claimed_at",
    )
    .bind(tg_id)
    .bind(chest_id)
    .execute(pool)
    .await?;

    Ok((true, None))
}

pub async fn open_chest(
    pool: &PgPool,
    chat_id: i64,
    tg_id: i64,
    chest_id: &str,
    free_open: bool,
) -> Result<Option<HuyaInventoryItem>, AppError> {
    let chest = chest_defs().into_iter().find(|c| c.id == chest_id);
    let Some(chest) = chest else { return Ok(None) };
    let (h, _) = get_or_create(pool, chat_id, tg_id).await?;
    if !free_open && h.length_mm < chest.price_mm {
        return Ok(None);
    }

    let candidates: Vec<ItemTemplate> = item_templates()
        .into_iter()
        .filter(|t| rarity_allowed(chest_id, t.rarity))
        .collect();
    let Some(template) = pick_weighted(&candidates).cloned() else {
        return Ok(None);
    };

    let roll = if template.roll_max > template.roll_min {
        let mut rng = rand::rng();
        rng.random_range(template.roll_min..=template.roll_max)
    } else {
        template.roll_min
    };
    let resolved_slot = template.slot.map(|s| {
        let mut rng = rand::rng();
        match s {
            "ring" => format!("ring_{}", rng.random_range(1..=6)),
            "piercing_tip" => format!("piercing_tip_{}", rng.random_range(1..=3)),
            "piercing_shaft" => format!("piercing_shaft_{}", rng.random_range(1..=3)),
            "piercing_base" => format!("piercing_base_{}", rng.random_range(1..=2)),
            _ => s.to_string(),
        }
    });
    let socket_capacity = if template.item_kind == "equipment" {
        template.socket_capacity_base.max(0)
    } else {
        0
    };

    let mut tx = pool.begin().await?;
    if !free_open && chest.price_mm > 0 {
        sqlx::query("UPDATE huya SET length_mm = length_mm - $1 WHERE id = $2")
            .bind(chest.price_mm)
            .bind(h.id)
            .execute(&mut *tx)
            .await?;
    }
    let row = sqlx::query_as::<_, HuyaInventoryItem>(
        "INSERT INTO huya_inventory (
            chat_id, tg_id, item_id, rarity, item_kind, slot, trait, roll, charges,
            sell_price_mm, booster_effect, booster_value, booster_scope, socket_capacity
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
         RETURNING id, chat_id, tg_id, item_id, rarity, item_kind, slot, trait AS trait_name,
                  roll, charges, sell_price_mm, booster_effect, booster_value, booster_scope,
                  socket_capacity, reforge_level, acquired_at",
    )
    .bind(chat_id)
    .bind(tg_id)
    .bind(template.id)
    .bind(template.rarity)
    .bind(template.item_kind)
    .bind(resolved_slot)
    .bind(template.trait_name)
    .bind(roll)
    .bind(if template.item_kind == "booster" { 1 } else { 0 })
    .bind(template.sell_price_mm)
    .bind(template.booster_effect)
    .bind(template.booster_value)
    .bind(template.booster_scope)
    .bind(socket_capacity)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO huya_loot_log (chat_id, tg_id, chest_id, item_id, rarity, item_kind, rolled_trait, roll)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(chat_id)
    .bind(tg_id)
    .bind(chest_id)
    .bind(&row.item_id)
    .bind(&row.rarity)
    .bind(&row.item_kind)
    .bind(row.trait_name.clone())
    .bind(row.roll)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(Some(row))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EquipItemResult {
    Success,
    NotFound,
    NotEquipment,
    WrongSlot,
    SlotLocked,
    AlreadyEquipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnequipItemResult {
    Success,
    NotEquipped,
}

pub struct AutoEquipResult {
    pub changed_slots: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SellItemResult {
    Sold { item_id: String, refund_mm: i32 },
    NotFound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UseBoosterResult {
    Used { effect: String, value: i32 },
    NotFound,
    NotBooster,
    NoCharges,
    InvalidEffect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketGemResult {
    Success,
    ItemNotFound,
    ItemNotEquipment,
    NoSockets,
    GemNotFound,
    GemNotGem,
    SocketsFull,
}

#[derive(Debug, Clone)]
pub enum ReforgeItemResult {
    Completed(ReforgeResult),
    ItemNotFound,
    ItemNotEquipment,
    CatalystNotFound,
    CatalystNotGem,
}

/// Equip an inventory item into a slot.
pub async fn equip_item(
    pool: &PgPool,
    chat_id: i64,
    tg_id: i64,
    slot: &str,
    inventory_id: i32,
) -> Result<EquipItemResult, AppError> {
    let inv = sqlx::query_as::<_, HuyaInventoryItem>(
        "SELECT id, chat_id, tg_id, item_id, rarity, item_kind, slot, trait AS trait_name,
                roll, charges, sell_price_mm, booster_effect, booster_value, booster_scope,
                socket_capacity, reforge_level, acquired_at
         FROM huya_inventory WHERE id = $1 AND tg_id = $2",
    )
    .bind(inventory_id).bind(tg_id)
    .fetch_optional(pool).await?;
    let Some(inv) = inv else {
        return Ok(EquipItemResult::NotFound);
    };
    if inv.tg_id != tg_id || inv.item_kind != "equipment" {
        return Ok(EquipItemResult::NotEquipment);
    }
    if let Some(ref expected_slot) = inv.slot && expected_slot != slot {
        return Ok(EquipItemResult::WrongSlot);
    }
    if slot.starts_with("ring_") || slot.starts_with("piercing_") {
        let (h, _) = get_or_create(pool, chat_id, tg_id).await?;
        if !slot_unlocked_for_length(slot, h.length_mm) {
            return Ok(EquipItemResult::SlotLocked);
        }
    }

    // Ensure same inventory row isn't equipped in multiple slots.
    let already: Option<String> = sqlx::query_scalar(
        "SELECT slot FROM huya_equipment WHERE tg_id = $1 AND inventory_id = $2",
    )
    .bind(tg_id).bind(inventory_id)
    .fetch_optional(pool).await?;
    if already.is_some() {
        return Ok(EquipItemResult::AlreadyEquipped);
    }

    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM huya_equipment WHERE tg_id = $1 AND slot = $2")
        .bind(tg_id)
        .bind(slot)
        .execute(&mut *tx)
        .await?;

    sqlx::query(
        "INSERT INTO huya_equipment (chat_id, tg_id, slot, inventory_id) \
         VALUES ($1, $2, $3, $4)",
    )
    .bind(chat_id)
    .bind(tg_id)
    .bind(slot)
    .bind(inventory_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(EquipItemResult::Success)
}

fn rarity_rank(rarity: &str) -> i32 {
    match rarity {
        "trash" => 0,
        "common" => 1,
        "rare" => 2,
        "epic" => 3,
        "legendary" => 4,
        _ => 1,
    }
}

fn item_power_score(item: &HuyaInventoryItem) -> i32 {
    rarity_rank(&item.rarity) * 1_000
        + item.roll.max(0) * 10
        + item.reforge_level.max(0) * 25
        + item.socket_capacity.max(0) * 8
}

async fn get_equipment_candidates_for_autoequip(
    pool: &PgPool,
    tg_id: i64,
) -> Result<Vec<HuyaInventoryItem>, AppError> {
    let rows = sqlx::query_as::<_, HuyaInventoryItem>(
        "SELECT id, chat_id, tg_id, item_id, rarity, item_kind, slot, trait AS trait_name,
                roll, charges, sell_price_mm, booster_effect, booster_value, booster_scope,
                socket_capacity, reforge_level, acquired_at
         FROM huya_inventory
         WHERE tg_id = $1 AND item_kind = 'equipment'
         ORDER BY acquired_at DESC, id DESC",
    )
    .bind(tg_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn auto_equip_best(
    pool: &PgPool,
    chat_id: i64,
    tg_id: i64,
) -> Result<AutoEquipResult, AppError> {
    let started_at = std::time::Instant::now();
    let (h, _) = get_or_create(pool, chat_id, tg_id).await?;
    let items = get_equipment_candidates_for_autoequip(pool, tg_id).await?;
    let candidate_count = items.len();
    let equipped = get_equipment(pool, chat_id, tg_id).await?;

    let mut best_by_slot: std::collections::HashMap<String, HuyaInventoryItem> =
        std::collections::HashMap::new();
    for it in items.into_iter().filter(|i| i.item_kind == "equipment") {
        let Some(slot) = it.slot.clone() else {
            continue;
        };
        if !slot_unlocked_for_length(&slot, h.length_mm) {
            continue;
        }
        let replace = match best_by_slot.get(&slot) {
            None => true,
            Some(current) => item_power_score(&it) > item_power_score(current),
        };
        if replace {
            best_by_slot.insert(slot, it);
        }
    }

    let current_by_slot: std::collections::HashMap<String, i32> = equipped
        .into_iter()
        .map(|e| (e.slot, e.inventory_id))
        .collect();

    let mut tx = pool.begin().await?;
    let mut changed_slots = 0usize;
    for (slot, best_item) in best_by_slot {
        let already_equipped = current_by_slot
            .get(&slot)
            .map(|id| *id == best_item.id)
            .unwrap_or(false);
        if already_equipped {
            continue;
        }
        sqlx::query("DELETE FROM huya_equipment WHERE tg_id = $1 AND slot = $2")
            .bind(tg_id)
            .bind(&slot)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO huya_equipment (chat_id, tg_id, slot, inventory_id) VALUES ($1, $2, $3, $4)",
        )
        .bind(chat_id)
        .bind(tg_id)
        .bind(&slot)
        .bind(best_item.id)
        .execute(&mut *tx)
        .await?;
        changed_slots += 1;
    }
    tx.commit().await?;
    let elapsed_ms = started_at.elapsed().as_millis();
    if elapsed_ms >= 200 {
        tracing::debug!(
            "auto_equip_best slow path: tg_id={} candidates={} changed_slots={} elapsed_ms={}",
            tg_id,
            candidate_count,
            changed_slots,
            elapsed_ms
        );
    }

    Ok(AutoEquipResult { changed_slots })
}

pub async fn sell_inventory_item(
    pool: &PgPool,
    chat_id: i64,
    tg_id: i64,
    inventory_id: i32,
) -> Result<SellItemResult, AppError> {
    let mut tx = pool.begin().await?;
    let item = sqlx::query_as::<_, HuyaInventoryItem>(
        "SELECT id, chat_id, tg_id, item_id, rarity, item_kind, slot, trait AS trait_name,
                roll, charges, sell_price_mm, booster_effect, booster_value, booster_scope,
                socket_capacity, reforge_level, acquired_at
         FROM huya_inventory WHERE id = $1 AND tg_id = $2",
    )
    .bind(inventory_id).bind(tg_id)
    .fetch_optional(&mut *tx).await?;
    let Some(item) = item else {
        return Ok(SellItemResult::NotFound);
    };

    sqlx::query("DELETE FROM huya_equipment WHERE tg_id = $1 AND inventory_id = $2")
        .bind(tg_id).bind(inventory_id)
        .execute(&mut *tx).await?;
    sqlx::query("DELETE FROM huya_inventory WHERE id = $1 AND tg_id = $2")
        .bind(inventory_id).bind(tg_id)
        .execute(&mut *tx).await?;
    sqlx::query("UPDATE huya SET length_mm = length_mm + $1 WHERE tg_id = $2")
        .bind(item.sell_price_mm.max(0))
        .bind(tg_id)
        .execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(SellItemResult::Sold {
        item_id: item.item_id,
        refund_mm: item.sell_price_mm.max(0),
    })
}

pub async fn use_booster_item(
    pool: &PgPool,
    chat_id: i64,
    tg_id: i64,
    inventory_id: i32,
) -> Result<UseBoosterResult, AppError> {
    let mut tx = pool.begin().await?;
    let item = sqlx::query_as::<_, HuyaInventoryItem>(
        "SELECT id, chat_id, tg_id, item_id, rarity, item_kind, slot, trait AS trait_name,
                roll, charges, sell_price_mm, booster_effect, booster_value, booster_scope,
                socket_capacity, reforge_level, acquired_at
         FROM huya_inventory WHERE id = $1 AND tg_id = $2",
    )
    .bind(inventory_id).bind(tg_id)
    .fetch_optional(&mut *tx).await?;
    let Some(item) = item else {
        return Ok(UseBoosterResult::NotFound);
    };
    if item.item_kind != "booster" {
        return Ok(UseBoosterResult::NotBooster);
    }
    if item.charges <= 0 {
        return Ok(UseBoosterResult::NoCharges);
    }
    let effect = item.booster_effect.clone().unwrap_or_default();
    let value = item.booster_value;
    match effect.as_str() {
        "atk_boost" => {
            sqlx::query("UPDATE huya SET atk_boost = LEAST(atk_boost + $1, 90) WHERE tg_id = $2")
                .bind(value).bind(tg_id).execute(&mut *tx).await?;
        }
        "grow_boost" => {
            sqlx::query("UPDATE huya SET grow_boost = 1 WHERE tg_id = $1")
                .bind(tg_id).execute(&mut *tx).await?;
        }
        "steal_boost" => {
            sqlx::query("UPDATE huya SET steal_boost = LEAST(steal_boost + $1, 8) WHERE tg_id = $2")
                .bind(value).bind(tg_id).execute(&mut *tx).await?;
        }
        "energy_boost" => {
            sqlx::query("UPDATE huya SET actions_left = actions_left + $1 WHERE tg_id = $2")
                .bind(value.max(1)).bind(tg_id).execute(&mut *tx).await?;
        }
        _ => return Ok(UseBoosterResult::InvalidEffect),
    }
    sqlx::query("DELETE FROM huya_inventory WHERE id = $1 AND tg_id = $2")
        .bind(inventory_id).bind(tg_id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(UseBoosterResult::Used { effect, value })
}

pub async fn socketed_gems_for_item(
    pool: &PgPool,
    chat_id: i64,
    tg_id: i64,
    item_inventory_id: i32,
) -> Result<Vec<HuyaSocketedGem>, AppError> {
    let rows = sqlx::query_as::<_, HuyaSocketedGem>(
        "SELECT id, chat_id, tg_id, item_inventory_id, socket_index, gem_item_id, gem_trait, gem_roll, gem_rarity, created_at
         FROM huya_item_socket
         WHERE tg_id = $1 AND item_inventory_id = $2
         ORDER BY socket_index ASC",
    )
    .bind(tg_id)
    .bind(item_inventory_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn all_socketed_gems_for_player(
    pool: &PgPool,
    chat_id: i64,
    tg_id: i64,
) -> Result<Vec<HuyaSocketedGem>, AppError> {
    let rows = sqlx::query_as::<_, HuyaSocketedGem>(
        "SELECT id, chat_id, tg_id, item_inventory_id, socket_index, gem_item_id, gem_trait, gem_roll, gem_rarity, created_at
         FROM huya_item_socket
         WHERE tg_id = $1
         ORDER BY item_inventory_id ASC, socket_index ASC",
    )
    .bind(tg_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn available_gems(
    pool: &PgPool,
    chat_id: i64,
    tg_id: i64,
) -> Result<Vec<HuyaInventoryItem>, AppError> {
    let rows = sqlx::query_as::<_, HuyaInventoryItem>(
        "SELECT id, chat_id, tg_id, item_id, rarity, item_kind, slot, trait AS trait_name,
                roll, charges, sell_price_mm, booster_effect, booster_value, booster_scope,
                socket_capacity, reforge_level, acquired_at
         FROM huya_inventory
         WHERE tg_id = $1 AND item_kind = 'gem'
         ORDER BY acquired_at DESC, id DESC",
    )
    .bind(tg_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn socket_gem_into_item(
    pool: &PgPool,
    chat_id: i64,
    tg_id: i64,
    item_inventory_id: i32,
    gem_inventory_id: i32,
) -> Result<SocketGemResult, AppError> {
    let mut tx = pool.begin().await?;
    let item = sqlx::query_as::<_, HuyaInventoryItem>(
        "SELECT id, chat_id, tg_id, item_id, rarity, item_kind, slot, trait AS trait_name,
                roll, charges, sell_price_mm, booster_effect, booster_value, booster_scope,
                socket_capacity, reforge_level, acquired_at
         FROM huya_inventory
         WHERE id = $1 AND tg_id = $2",
    )
    .bind(item_inventory_id)
    .bind(tg_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(item) = item else {
        return Ok(SocketGemResult::ItemNotFound);
    };
    if item.item_kind != "equipment" {
        return Ok(SocketGemResult::ItemNotEquipment);
    }
    if item.socket_capacity <= 0 {
        return Ok(SocketGemResult::NoSockets);
    }
    let gem = sqlx::query_as::<_, HuyaInventoryItem>(
        "SELECT id, chat_id, tg_id, item_id, rarity, item_kind, slot, trait AS trait_name,
                roll, charges, sell_price_mm, booster_effect, booster_value, booster_scope,
                socket_capacity, reforge_level, acquired_at
         FROM huya_inventory
         WHERE id = $1 AND tg_id = $2",
    )
    .bind(gem_inventory_id)
    .bind(tg_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(gem) = gem else {
        return Ok(SocketGemResult::GemNotFound);
    };
    if gem.item_kind != "gem" {
        return Ok(SocketGemResult::GemNotGem);
    }
    let used: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM huya_item_socket WHERE item_inventory_id = $1",
    )
    .bind(item_inventory_id)
    .fetch_one(&mut *tx)
    .await?;
    if used >= item.socket_capacity as i64 {
        return Ok(SocketGemResult::SocketsFull);
    }
    let next_socket = (used as i32) + 1;

    sqlx::query(
        "INSERT INTO huya_item_socket (chat_id, tg_id, item_inventory_id, socket_index, gem_item_id, gem_trait, gem_roll, gem_rarity)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(chat_id)
    .bind(tg_id)
    .bind(item_inventory_id)
    .bind(next_socket)
    .bind(gem.item_id)
    .bind(gem.trait_name)
    .bind(gem.roll)
    .bind(gem.rarity)
    .execute(&mut *tx)
    .await?;

    sqlx::query("DELETE FROM huya_inventory WHERE id = $1 AND tg_id = $2")
        .bind(gem_inventory_id)
        .bind(tg_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(SocketGemResult::Success)
}

#[derive(Debug, Clone, Copy)]
pub enum ReforgeOutcome {
    Success,
    Fail,
    CritFail,
}

#[derive(Debug, Clone)]
pub struct ReforgeResult {
    pub outcome: ReforgeOutcome,
    pub old_roll: i32,
    pub new_roll: i32,
}

pub async fn reforge_item_with_gem(
    pool: &PgPool,
    chat_id: i64,
    tg_id: i64,
    item_inventory_id: i32,
    catalyst_gem_id: i32,
) -> Result<ReforgeItemResult, AppError> {
    let mut tx = pool.begin().await?;
    let item = sqlx::query_as::<_, HuyaInventoryItem>(
        "SELECT id, chat_id, tg_id, item_id, rarity, item_kind, slot, trait AS trait_name,
                roll, charges, sell_price_mm, booster_effect, booster_value, booster_scope,
                socket_capacity, reforge_level, acquired_at
         FROM huya_inventory
         WHERE id = $1 AND tg_id = $2",
    )
    .bind(item_inventory_id)
    .bind(tg_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(item) = item else {
        return Ok(ReforgeItemResult::ItemNotFound);
    };
    if item.item_kind != "equipment" {
        return Ok(ReforgeItemResult::ItemNotEquipment);
    }

    let catalyst = sqlx::query_as::<_, HuyaInventoryItem>(
        "SELECT id, chat_id, tg_id, item_id, rarity, item_kind, slot, trait AS trait_name,
                roll, charges, sell_price_mm, booster_effect, booster_value, booster_scope,
                socket_capacity, reforge_level, acquired_at
         FROM huya_inventory
         WHERE id = $1 AND tg_id = $2",
    )
    .bind(catalyst_gem_id)
    .bind(tg_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(catalyst) = catalyst else {
        return Ok(ReforgeItemResult::CatalystNotFound);
    };
    if catalyst.item_kind != "gem" {
        return Ok(ReforgeItemResult::CatalystNotGem);
    }

    let (mut succ, mut crit) = match item.rarity.as_str() {
        "epic" => (62_i32, 10_i32),
        "legendary" => (48_i32, 17_i32),
        _ => (75_i32, 5_i32),
    };
    let penalty = (item.reforge_level * 3).clamp(0, 25);
    succ = (succ - penalty).max(20);
    crit = (crit + penalty / 2).min(35);
    let fail = (100 - succ - crit).max(0);
    let roll_rng = {
        let mut rng = rand::rng();
        rng.random_range(1..=100)
    };
    let outcome = if roll_rng <= succ {
        ReforgeOutcome::Success
    } else if roll_rng <= succ + fail {
        ReforgeOutcome::Fail
    } else {
        ReforgeOutcome::CritFail
    };

    let old_roll = item.roll;
    let mut new_roll = old_roll;

    // Catalyst always consumed in medium profile.
    sqlx::query("DELETE FROM huya_inventory WHERE id = $1 AND tg_id = $2")
        .bind(catalyst_gem_id)
        .bind(tg_id)
        .execute(&mut *tx)
        .await?;

    match outcome {
        ReforgeOutcome::Success => {
            let add = rand::rng().random_range(2..=9);
            new_roll = old_roll + add;
            sqlx::query(
                "UPDATE huya_inventory
                 SET roll = $1, reforge_level = reforge_level + 1
                 WHERE id = $2 AND tg_id = $3",
            )
            .bind(new_roll)
            .bind(item_inventory_id)
            .bind(tg_id)
            .execute(&mut *tx)
            .await?;
        }
        ReforgeOutcome::Fail => {
            sqlx::query(
                "UPDATE huya_inventory
                 SET reforge_level = reforge_level + 1
                 WHERE id = $1 AND tg_id = $2",
            )
            .bind(item_inventory_id)
            .bind(tg_id)
            .execute(&mut *tx)
            .await?;
        }
        ReforgeOutcome::CritFail => {
            sqlx::query("DELETE FROM huya_inventory WHERE id = $1 AND tg_id = $2")
                .bind(item_inventory_id)
                .bind(tg_id)
                .execute(&mut *tx)
                .await?;
        }
    }

    let outcome_str = match outcome {
        ReforgeOutcome::Success => "success",
        ReforgeOutcome::Fail => "fail",
        ReforgeOutcome::CritFail => "crit_fail",
    };
    sqlx::query(
        "INSERT INTO huya_reforge_log
          (chat_id, tg_id, item_inventory_id, catalyst_item_id, old_roll, new_roll, old_reforge_level, new_reforge_level, outcome)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(chat_id)
    .bind(tg_id)
    .bind(item_inventory_id)
    .bind(catalyst.item_id)
    .bind(old_roll)
    .bind(new_roll)
    .bind(item.reforge_level)
    .bind(item.reforge_level + 1)
    .bind(outcome_str)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(ReforgeItemResult::Completed(ReforgeResult {
        outcome,
        old_roll,
        new_roll,
    }))
}

#[derive(Default, Clone, Copy)]
pub struct EquipmentEffects {
    pub atk_pct: f64,
    pub def_pct: f64,
    pub hp_flat: i32,
    pub hp_pct: f64,
    pub steal_chance_pct: f64,
    pub steal_resist_pct: f64,
    pub reflect_pct: f64,
    pub raid_initiative: f64,
    pub grow_bonus_pct: f64,
}

pub async fn equipment_effects_for_player(pool: &PgPool, chat_id: i64, tg_id: i64) -> Result<EquipmentEffects, AppError> {
    let rows = sqlx::query_as::<_, HuyaInventoryItem>(
        "WITH eq AS (
            SELECT DISTINCT ON (slot) tg_id, slot, inventory_id
            FROM huya_equipment
            WHERE tg_id = $1
            ORDER BY slot, chat_id DESC
         )
         SELECT i.id, i.chat_id, i.tg_id, i.item_id, i.rarity, i.item_kind, i.slot, i.trait AS trait_name,
                i.roll, i.charges, i.sell_price_mm, i.booster_effect, i.booster_value, i.booster_scope,
                i.socket_capacity, i.reforge_level, i.acquired_at
         FROM eq e
         JOIN huya_inventory i ON i.id = e.inventory_id",
    )
    .bind(tg_id)
    .fetch_all(pool).await?;

    let mut fx = EquipmentEffects::default();
    for it in rows {
        let r = it.roll.max(0) as f64;
        match it.trait_name.as_deref() {
            Some("spiked_ring_reflect") => fx.reflect_pct += 0.04 + r / 500.0,
            Some("cage_guard") | Some("anti_burst") => fx.def_pct += 0.03 + r / 400.0,
            Some("blood_taste") => fx.atk_pct += 0.03 + r / 500.0,
            Some("raid_initiative") => fx.raid_initiative += 0.04 + r / 500.0,
            Some("safe_poke") => fx.steal_resist_pct += 0.04 + r / 500.0,
            Some("eternal_echo") => {
                fx.atk_pct += 0.06 + r / 350.0;
                fx.def_pct += 0.04 + r / 450.0;
                fx.steal_chance_pct += 0.04;
            }
            Some("jittery") | Some("sticky") => fx.steal_chance_pct += 0.01 + r / 1000.0,
            _ => {}
        }
    }
    let gems = sqlx::query_as::<_, HuyaSocketedGem>(
        "WITH eq AS (
            SELECT DISTINCT ON (slot) tg_id, slot, inventory_id
            FROM huya_equipment
            WHERE tg_id = $1
            ORDER BY slot, chat_id DESC
         )
         SELECT s.id, s.chat_id, s.tg_id, s.item_inventory_id, s.socket_index, s.gem_item_id, s.gem_trait, s.gem_roll, s.gem_rarity, s.created_at
         FROM huya_item_socket s
         JOIN eq e
           ON e.tg_id = s.tg_id
          AND e.inventory_id = s.item_inventory_id
         WHERE s.tg_id = $1",
    )
    .bind(tg_id)
    .fetch_all(pool)
    .await?;
    for g in gems {
        let r = g.gem_roll.max(0) as f64;
        match g.gem_trait.as_deref() {
            Some("gem_atk") => fx.atk_pct += 0.015 + r / 700.0,
            Some("gem_def") => fx.def_pct += 0.015 + r / 700.0,
            Some("gem_steal") => fx.steal_chance_pct += 0.01 + r / 900.0,
            Some("gem_initiative") => fx.raid_initiative += 0.02 + r / 700.0,
            Some("gem_reflect") => fx.reflect_pct += 0.015 + r / 850.0,
            _ => {}
        }
    }
    // Safety caps.
    fx.atk_pct = fx.atk_pct.clamp(0.0, 0.35);
    fx.def_pct = fx.def_pct.clamp(0.0, 0.35);
    fx.reflect_pct = fx.reflect_pct.clamp(0.0, 0.20);
    fx.steal_chance_pct = fx.steal_chance_pct.clamp(0.0, 0.25);
    fx.steal_resist_pct = fx.steal_resist_pct.clamp(0.0, 0.25);
    fx.raid_initiative = fx.raid_initiative.clamp(0.0, 0.30);
    Ok(fx)
}

/// Unequip a slot.
pub async fn unequip_item(
    pool: &PgPool,
    chat_id: i64,
    tg_id: i64,
    slot: &str,
) -> Result<UnequipItemResult, AppError> {
    let res = sqlx::query(
        "DELETE FROM huya_equipment WHERE tg_id = $1 AND slot = $2",
    )
    .bind(tg_id)
    .bind(slot)
    .execute(pool)
    .await?;
    if res.rows_affected() > 0 {
        Ok(UnequipItemResult::Success)
    } else {
        Ok(UnequipItemResult::NotEquipped)
    }
}

/// When loser loses ring slots due to shrink, transfer one highest ring_X slot to winner.
async fn drop_rings_on_shrink(
    pool: &PgPool,
    chat_id: i64,
    winner_tg_id: i64,
    loser_tg_id: i64,
    loser_old_len: i32,
    loser_new_len: i32,
) -> Result<(), AppError> {
    let old_slots = ring_slots_for_length(loser_old_len);
    let new_slots = ring_slots_for_length(loser_new_len);
    if new_slots >= old_slots {
        return Ok(());
    }

    // Find highest-index ring_N that should fall.
    for idx in (new_slots + 1..=old_slots).rev() {
        let slot_name = format!("ring_{}", idx);
        if let Some(inv_id) = sqlx::query_scalar::<_, i32>(
            "SELECT inventory_id FROM huya_equipment \
             WHERE tg_id = $1 AND slot = $2",
        )
        .bind(loser_tg_id)
        .bind(&slot_name)
        .fetch_optional(pool)
        .await?
        {
            let mut tx = pool.begin().await?;
            sqlx::query("DELETE FROM huya_equipment WHERE tg_id = $1 AND slot = $2")
                .bind(loser_tg_id)
                .bind(&slot_name)
                .execute(&mut *tx)
                .await?;
            sqlx::query("UPDATE huya_inventory SET tg_id = $1 WHERE id = $2")
                .bind(winner_tg_id)
                .bind(inv_id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            break;
        }
    }
    Ok(())
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

/// Ordered list of all skill IDs (T1..T5) for UI.
const SKILL_ORDER: &[&str] = &[
    "shaft", "skin", "balls", "cunning", "stamina",
    "pierce", "scales", "spirit", "pickpocket", "dynamo",
    "eggtwist", "bloodsucker", "ironballs", "vortex", "phantom",
    "berserker", "vampire", "fortress", "speedrun", "ghost",
    "eternal", "absolute",
];

/// Returns skill IDs that can be upgraded right now (visible, prereqs met, under cap, enough SP).
pub fn skills_available_to_upgrade(huya: &Huya) -> Vec<&'static str> {
    SKILL_ORDER
        .iter()
        .filter(|s| skill_upgrade_info(huya, s).is_some())
        .copied()
        .collect()
}

// ── Pet energy (/huyapet) ─────────────────────────────────────────────────────

/// Reset pet energy once per day; returns up-to-date Huya row.
pub async fn reset_pet_energy_if_needed(pool: &PgPool, huya: &Huya) -> Result<Huya, AppError> {
    let today = Utc::now().date_naive();
    if huya.pet_energy_reset_at >= today {
        return Ok(huya.clone());
    }
    let updated = sqlx::query_as::<_, Huya>(
        &format!(
            "UPDATE huya SET pet_energy_left = 3, pet_energy_reset_at = $1 \
             WHERE id = $2 RETURNING {HUYA_SELECT}"
        ),
    )
    .bind(today)
    .bind(huya.id)
    .fetch_one(pool)
    .await?;
    Ok(updated)
}

/// Consume one pet energy; returns updated Huya or None if no energy left.
pub async fn consume_pet_energy(pool: &PgPool, huya: &Huya) -> Result<Option<Huya>, AppError> {
    let current = reset_pet_energy_if_needed(pool, huya).await?;
    if current.pet_energy_left <= 0 {
        return Ok(None);
    }
    let updated = sqlx::query_as::<_, Huya>(
        &format!(
            "UPDATE huya SET pet_energy_left = pet_energy_left - 1 \
             WHERE id = $1 RETURNING {HUYA_SELECT}"
        ),
    )
    .bind(current.id)
    .fetch_one(pool)
    .await?;
    Ok(Some(updated))
}

/// Check whether `from_tg_id` can pet `target_tg_id` today (no more than 3 distinct friends).
pub async fn can_pet_friend(
    pool: &PgPool,
    chat_id: i64,
    from_tg_id: i64,
    target_tg_id: i64,
) -> Result<bool, AppError> {
    let today = Utc::now().date_naive();
    // Already petted this target today – always allowed.
    let already_petted: bool = sqlx::query_scalar(
        "SELECT EXISTS(
             SELECT 1
             FROM huya_pet_daily
             WHERE chat_id = $1 AND from_tg_id = $2 AND target_tg_id = $3 AND day = $4
         )",
    )
    .bind(chat_id)
    .bind(from_tg_id)
    .bind(target_tg_id)
    .bind(today)
    .fetch_one(pool)
    .await?;
    if already_petted {
        return Ok(true);
    }
    // Count distinct friends already petted today.
    let (friends_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(DISTINCT target_tg_id) \
         FROM huya_pet_daily \
         WHERE chat_id = $1 AND from_tg_id = $2 AND day = $3",
    )
    .bind(chat_id)
    .bind(from_tg_id)
    .bind(today)
    .fetch_one(pool)
    .await?;
    Ok(friends_count < 3)
}

/// Register that `from_tg_id` has petted `target_tg_id` today.
pub async fn register_pet_friend(
    pool: &PgPool,
    chat_id: i64,
    from_tg_id: i64,
    target_tg_id: i64,
) -> Result<(), AppError> {
    let today = Utc::now().date_naive();
    sqlx::query(
        "INSERT INTO huya_pet_daily (chat_id, from_tg_id, target_tg_id, day) \
         VALUES ($1, $2, $3, $4) \
         ON CONFLICT (chat_id, from_tg_id, target_tg_id, day) DO NOTHING",
    )
    .bind(chat_id)
    .bind(from_tg_id)
    .bind(target_tg_id)
    .bind(today)
    .execute(pool)
    .await?;
    Ok(())
}

/// Outcome state for pet_friend.
pub enum PetFriendState {
    Ok,
    NoEnergy,
    TooManyFriends,
}

pub struct PetFriendResult {
    pub from: Huya,
    pub target: Huya,
    pub heal: i32,
    pub growth_mm: i32,
    pub xp_gain: i32,
    pub pussy_depth_reduce_mm: i32,
    pub state: PetFriendState,
}

/// Pet a friend's Huya using separate pet energy and daily friend limit.
pub async fn pet_friend(
    pool: &PgPool,
    chat_id: i64,
    from_tg_id: i64,
    target_tg_id: i64,
) -> Result<PetFriendResult, AppError> {
    let (from_huya, _) = get_or_create(pool, chat_id, from_tg_id).await?;
    let (target_huya, _) = get_or_create(pool, chat_id, target_tg_id).await?;

    if !can_pet_friend(pool, chat_id, from_tg_id, target_tg_id).await? {
        return Ok(PetFriendResult {
            from: from_huya,
            target: target_huya,
            heal: 0,
            growth_mm: 0,
            xp_gain: 0,
            pussy_depth_reduce_mm: 0,
            state: PetFriendState::TooManyFriends,
        });
    }

    let Some(updated_from) = consume_pet_energy(pool, &from_huya).await? else {
        return Ok(PetFriendResult {
            from: from_huya,
            target: target_huya,
            heal: 0,
            growth_mm: 0,
            xp_gain: 0,
            pussy_depth_reduce_mm: 0,
            state: PetFriendState::NoEnergy,
        });
    };

    let heal = {
        let base = if target_huya.is_pussy() {
            // Pizdyaka form is less explosive, but responds better to defensive touch.
            8 + target_huya.skill_skin * 3 + target_huya.skill_scales * 2
        } else {
            10 + target_huya.skill_stamina * 2
        };
        let max_hp = target_huya.max_hp();
        let new_hp = (target_huya.hp + base).min(max_hp);
        let applied = new_hp - target_huya.hp;
        sqlx::query("UPDATE huya SET hp = $1 WHERE id = $2")
            .bind(new_hp)
            .bind(target_huya.id)
            .execute(pool)
            .await?;
        applied
    };
    let growth_mm = if !target_huya.is_pussy() {
        let grow = {
            let mut rng = rand::rng();
            rng.random_range(2..=6)
        };
        sqlx::query("UPDATE huya SET length_mm = length_mm + $1 WHERE id = $2")
            .bind(grow)
            .bind(target_huya.id)
            .execute(pool)
            .await?;
        grow
    } else {
        0
    };
    let pussy_depth_reduce_mm = if target_huya.is_pussy() {
        let reduce_mm = {
            let mut rng = rand::rng();
            rng.random_range(4..=12)
        };
        let new_len = (target_huya.length_mm + reduce_mm).min(0);
        sqlx::query("UPDATE huya SET length_mm = $1 WHERE id = $2")
            .bind(new_len)
            .bind(target_huya.id)
            .execute(pool)
            .await?;
        (new_len - target_huya.length_mm).max(0)
    } else {
        0
    };

    let xp_gain: i32 = {
        let mut rng = rand::rng();
        rng.random_range(5..=15)
    };

    let updated_from_with_xp = apply_xp_gain(pool, updated_from.id, xp_gain).await?;

    register_pet_friend(pool, chat_id, from_tg_id, target_tg_id).await?;

    let (target_updated, _) = get_or_create(pool, chat_id, target_tg_id).await?;

    Ok(PetFriendResult {
        from: updated_from_with_xp,
        target: target_updated,
        heal,
        growth_mm,
        xp_gain,
        pussy_depth_reduce_mm,
        state: PetFriendState::Ok,
    })
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
        _ => None,
    }
}

pub fn energy_price_mm(huya: &Huya, item_id: &str) -> Option<i32> {
    let base = match item_id {
        "energy_small" => 65,
        "energy_big" => 170,
        _ => return None,
    };
    let buys = huya.energy_buys_today.max(0) as f64;
    let mult = 1.0 + buys * 0.35;
    Some(((base as f64) * mult).round() as i32)
}

pub async fn buy_energy_item(
    pool: &PgPool,
    huya: &Huya,
    item_id: &str,
    chat_id: i64,
) -> Result<Option<(Huya, i32)>, AppError> {
    let today = Utc::now().date_naive();
    let mut h = huya.clone();
    if h.energy_buys_reset_at < today {
        h = sqlx::query_as::<_, Huya>(
            &format!(
                "UPDATE huya
                 SET energy_buys_today = 0, energy_buys_reset_at = $1
                 WHERE id = $2
                 RETURNING {HUYA_SELECT}"
            ),
        )
        .bind(today)
        .bind(h.id)
        .fetch_one(pool)
        .await?;
    }
    let Some(cost) = energy_price_mm(&h, item_id) else {
        return Ok(None);
    };
    if h.length_mm < cost {
        return Ok(None);
    }
    let (booster_id, booster_value, sell_price) = match item_id {
        "energy_small" => ("booster_energy_small", 1, 18),
        "energy_big" => ("booster_energy_big", 3, 42),
        _ => return Ok(None),
    };
    let mut tx = pool.begin().await?;
    let updated = sqlx::query_as::<_, Huya>(
        &format!(
            "UPDATE huya
             SET length_mm = length_mm - $1,
                 energy_buys_today = energy_buys_today + 1
             WHERE id = $2 AND length_mm >= $1
             RETURNING {HUYA_SELECT}"
        ),
    )
    .bind(cost)
    .bind(h.id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(updated) = updated else {
        return Ok(None);
    };
    sqlx::query(
        "INSERT INTO huya_inventory (
            chat_id, tg_id, item_id, rarity, item_kind, slot, trait, roll, charges,
            sell_price_mm, booster_effect, booster_value, booster_scope, socket_capacity
         ) VALUES ($1,$2,$3,'common','booster',NULL,'energy',0,1,$4,'energy_boost',$5,'inventory',0)",
    )
    .bind(chat_id)
    .bind(updated.tg_id)
    .bind(booster_id)
    .bind(sell_price)
    .bind(booster_value)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Some((updated, cost)))
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

        _ => return Ok(None),
    };

    Ok(updated)
}

// ── Raid (party vs one target) ───────────────────────────────────────────────

const RAID_MAX_ADVANTAGE: f64 = 0.15;
const RAID_TARGET_COOLDOWN_MINUTES: i64 = 45;
const RAID_STEAL_CAP_PCT: f64 = 0.10;
const RAID_TURN_TIMEOUT_SECONDS: i64 = 45;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct HuyaRaid {
    pub id: i32,
    pub chat_id: i64,
    pub leader_tg_id: i64,
    pub target_tg_id: i64,
    pub status: String,
    pub target_accepted: bool,
    pub power_override_by_target: bool,
    pub message_id: i32,
    pub round: i32,
    pub turn_index: i32,
    pub focus_round: i32,
    pub initiative_order: String,
    pub max_rounds: i32,
    pub turn_deadline_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct HuyaRaidMember {
    pub id: i32,
    pub raid_id: i32,
    pub tg_id: i64,
    pub side: String,
    pub slot: i32,
    pub hp_snapshot: i32,
    pub is_alive: bool,
    pub acted_in_round: bool,
    pub guard_until_round: i32,
    pub damage_done: i32,
    pub created_at: DateTime<Utc>,
}

pub struct RaidPowerCheck {
    pub party_power: i64,
    pub target_power: i64,
    pub within_window: bool,
}

pub enum RaidTurnAction {
    Attack,
    Guard,
    Focus,
}

pub struct RaidTurnResult {
    pub raid: HuyaRaid,
    pub members: Vec<HuyaRaidMember>,
    pub actor_tg_id: i64,
    pub actor_side: String,
    pub log_line: String,
    pub finished: bool,
    pub winner_side: Option<String>,
}

const RAID_SELECT: &str =
    "id, chat_id, leader_tg_id, target_tg_id, status, target_accepted, power_override_by_target, message_id, \
     round, turn_index, focus_round, initiative_order, max_rounds, turn_deadline_at, created_at, expires_at";

const RAID_MEMBER_SELECT: &str =
    "id, raid_id, tg_id, side, slot, hp_snapshot, is_alive, acted_in_round, \
     guard_until_round, damage_done, created_at";

pub fn raid_player_power(h: &Huya) -> i64 {
    let mut skills_factor = 0_i64;
    skills_factor += (h.skill_shaft * 12) as i64;
    skills_factor += (h.skill_skin * 10) as i64;
    skills_factor += (h.skill_balls * 9) as i64;
    skills_factor += (h.skill_stamina * 8) as i64;
    skills_factor += (h.skill_spirit * 14) as i64;
    skills_factor += (h.skill_pierce * 14) as i64;
    skills_factor += (h.skill_scales * 10) as i64;
    skills_factor += (h.skill_berserker * 25) as i64;
    skills_factor += (h.skill_fortress * 22) as i64;
    skills_factor += (h.skill_ghost * 18) as i64;
    skills_factor += (h.skill_eternal * 45) as i64;

    h.length_mm.max(1) as i64
        + (h.hp.max(1) * 8) as i64
        + (h.level.max(1) * 40) as i64
        + skills_factor
}

fn parse_initiative_order(s: &str) -> Vec<i64> {
    s.split(',')
        .filter_map(|p| p.trim().parse::<i64>().ok())
        .collect()
}

fn serialize_initiative_order(order: &[i64]) -> String {
    order
        .iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn initiative_score(h: &Huya) -> i64 {
    let base = h.level as i64 * 2 + h.skill_speedrun as i64 * 12 + h.skill_dynamo as i64 * 4;
    base + (h.length_mm as i64 / 10).max(1)
}

fn next_alive_index(order: &[i64], members: &[HuyaRaidMember], from: usize) -> usize {
    if order.is_empty() {
        return 0;
    }
    for step in 0..order.len() {
        let idx = (from + step) % order.len();
        let actor = order[idx];
        if members.iter().any(|m| m.tg_id == actor && m.is_alive) {
            return idx;
        }
    }
    0
}

pub async fn get_pending_or_active_raid(pool: &PgPool, chat_id: i64) -> Result<Option<HuyaRaid>, AppError> {
    cancel_expired_raids_for_chat(pool, chat_id).await?;
    let row = sqlx::query_as::<_, HuyaRaid>(
        &format!(
            "SELECT {RAID_SELECT} FROM huya_raid
             WHERE chat_id = $1
               AND status IN ('pending', 'active')
               AND expires_at > NOW()
             ORDER BY created_at DESC LIMIT 1"
        ),
    )
    .bind(chat_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn cancel_expired_raids_for_chat(pool: &PgPool, chat_id: i64) -> Result<u64, AppError> {
    let res = sqlx::query(
        "UPDATE huya_raid
         SET status = 'cancelled'
         WHERE chat_id = $1
           AND status IN ('pending', 'active')
           AND expires_at <= NOW()",
    )
    .bind(chat_id)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

pub async fn cancel_expired_raids(pool: &PgPool) -> Result<Vec<(i32, i64, i32)>, AppError> {
    let rows = sqlx::query_as::<_, (i32, i64, i32)>(
        "UPDATE huya_raid
         SET status = 'cancelled'
         WHERE status IN ('pending', 'active')
           AND expires_at <= NOW()
         RETURNING id, chat_id, message_id",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get_raid(pool: &PgPool, raid_id: i32) -> Result<Option<HuyaRaid>, AppError> {
    let row = sqlx::query_as::<_, HuyaRaid>(
        &format!("SELECT {RAID_SELECT} FROM huya_raid WHERE id = $1"),
    )
    .bind(raid_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn get_raid_members(pool: &PgPool, raid_id: i32) -> Result<Vec<HuyaRaidMember>, AppError> {
    let rows = sqlx::query_as::<_, HuyaRaidMember>(
        &format!(
            "SELECT {RAID_MEMBER_SELECT} FROM huya_raid_member
             WHERE raid_id = $1 ORDER BY side DESC, slot ASC, created_at ASC"
        ),
    )
    .bind(raid_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn raid_target_on_cooldown(pool: &PgPool, chat_id: i64, target_tg_id: i64) -> Result<bool, AppError> {
    let recent: Option<i32> = sqlx::query_scalar(
        "SELECT id FROM huya_raid
         WHERE chat_id = $1 AND target_tg_id = $2
           AND created_at > NOW() - ($3 || ' minutes')::interval
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(chat_id)
    .bind(target_tg_id)
    .bind(RAID_TARGET_COOLDOWN_MINUTES)
    .fetch_optional(pool)
    .await?;
    Ok(recent.is_some())
}

pub async fn create_raid(
    pool: &PgPool,
    chat_id: i64,
    leader_tg_id: i64,
    target_tg_id: i64,
    leader_hp: i32,
    target_hp: i32,
) -> Result<HuyaRaid, AppError> {
    cancel_expired_raids_for_chat(pool, chat_id).await?;
    let raid = sqlx::query_as::<_, HuyaRaid>(
        &format!(
            "INSERT INTO huya_raid (chat_id, leader_tg_id, target_tg_id)
             VALUES ($1, $2, $3)
             RETURNING {RAID_SELECT}"
        ),
    )
    .bind(chat_id)
    .bind(leader_tg_id)
    .bind(target_tg_id)
    .fetch_one(pool)
    .await?;

    sqlx::query(
        "INSERT INTO huya_raid_member (raid_id, tg_id, side, slot, hp_snapshot)
         VALUES ($1, $2, 'party', 1, $3), ($1, $4, 'target', 1, $5)",
    )
    .bind(raid.id)
    .bind(leader_tg_id)
    .bind(leader_hp.max(1))
    .bind(target_tg_id)
    .bind(target_hp.max(1))
    .execute(pool)
    .await?;

    Ok(raid)
}

pub async fn set_raid_message_id(pool: &PgPool, raid_id: i32, message_id: i32) -> Result<(), AppError> {
    sqlx::query("UPDATE huya_raid SET message_id = $1 WHERE id = $2")
        .bind(message_id)
        .bind(raid_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn accept_raid(pool: &PgPool, raid_id: i32, target_tg_id: i64) -> Result<Option<HuyaRaid>, AppError> {
    let row = sqlx::query_as::<_, HuyaRaid>(
        &format!(
            "UPDATE huya_raid SET target_accepted = TRUE
             WHERE id = $1 AND target_tg_id = $2 AND status = 'pending'
             RETURNING {RAID_SELECT}"
        ),
    )
    .bind(raid_id)
    .bind(target_tg_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn approve_raid_power_override(
    pool: &PgPool,
    raid_id: i32,
    target_tg_id: i64,
) -> Result<Option<HuyaRaid>, AppError> {
    let row = sqlx::query_as::<_, HuyaRaid>(
        &format!(
            "UPDATE huya_raid SET power_override_by_target = TRUE
             WHERE id = $1 AND target_tg_id = $2 AND status = 'pending' AND target_accepted = TRUE
             RETURNING {RAID_SELECT}"
        ),
    )
    .bind(raid_id)
    .bind(target_tg_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn decline_raid(pool: &PgPool, raid_id: i32, target_tg_id: i64) -> Result<Option<HuyaRaid>, AppError> {
    let row = sqlx::query_as::<_, HuyaRaid>(
        &format!(
            "UPDATE huya_raid SET status = 'cancelled'
             WHERE id = $1 AND target_tg_id = $2 AND status = 'pending'
             RETURNING {RAID_SELECT}"
        ),
    )
    .bind(raid_id)
    .bind(target_tg_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn disband_raid_by_leader(pool: &PgPool, raid_id: i32, leader_tg_id: i64) -> Result<Option<HuyaRaid>, AppError> {
    let row = sqlx::query_as::<_, HuyaRaid>(
        &format!(
            "UPDATE huya_raid SET status = 'cancelled'
             WHERE id = $1 AND leader_tg_id = $2 AND status = 'pending'
             RETURNING {RAID_SELECT}"
        ),
    )
    .bind(raid_id)
    .bind(leader_tg_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn raid_join_party(
    pool: &PgPool,
    raid_id: i32,
    tg_id: i64,
    hp_snapshot: i32,
) -> Result<bool, AppError> {
    let raid = match get_raid(pool, raid_id).await? {
        Some(r) if r.status == "pending" && r.target_accepted => r,
        _ => return Ok(false),
    };
    if tg_id == raid.target_tg_id {
        return Ok(false);
    }
    let party_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM huya_raid_member WHERE raid_id = $1 AND side = 'party'",
    )
    .bind(raid_id)
    .fetch_one(pool)
    .await?;
    if party_count >= 5 {
        return Ok(false);
    }
    let slot = (party_count as i32) + 1;
    let inserted = sqlx::query(
        "INSERT INTO huya_raid_member (raid_id, tg_id, side, slot, hp_snapshot)
         VALUES ($1, $2, 'party', $3, $4)
         ON CONFLICT (raid_id, tg_id) DO NOTHING",
    )
    .bind(raid_id)
    .bind(tg_id)
    .bind(slot)
    .bind(hp_snapshot.max(1))
    .execute(pool)
    .await?;
    Ok(inserted.rows_affected() > 0)
}

pub async fn raid_kick_party_member(
    pool: &PgPool,
    raid_id: i32,
    leader_tg_id: i64,
    member_tg_id: i64,
) -> Result<bool, AppError> {
    let raid = match get_raid(pool, raid_id).await? {
        Some(r) if r.status == "pending" => r,
        _ => return Ok(false),
    };
    if raid.leader_tg_id != leader_tg_id || member_tg_id == leader_tg_id || member_tg_id == raid.target_tg_id {
        return Ok(false);
    }
    let res = sqlx::query(
        "DELETE FROM huya_raid_member
         WHERE raid_id = $1 AND tg_id = $2 AND side = 'party'",
    )
    .bind(raid_id)
    .bind(member_tg_id)
    .execute(pool)
    .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn raid_power_check(pool: &PgPool, raid_id: i32) -> Result<Option<RaidPowerCheck>, AppError> {
    let raid = match get_raid(pool, raid_id).await? {
        Some(r) => r,
        None => return Ok(None),
    };
    let members = get_raid_members(pool, raid_id).await?;
    let mut party_power = 0_i64;
    let mut target_power = 0_i64;
    for m in members {
        let (h, _) = get_or_create(pool, raid.chat_id, m.tg_id).await?;
        if m.side == "party" {
            party_power += raid_player_power(&h);
        } else {
            target_power = raid_player_power(&h);
        }
    }
    if target_power <= 0 {
        return Ok(None);
    }
    let max_allowed = (target_power as f64) * (1.0 + RAID_MAX_ADVANTAGE);
    Ok(Some(RaidPowerCheck {
        party_power,
        target_power,
        within_window: (party_power as f64) <= max_allowed || raid.power_override_by_target,
    }))
}

pub async fn raid_start(pool: &PgPool, raid_id: i32) -> Result<Option<HuyaRaid>, AppError> {
    let raid = match get_raid(pool, raid_id).await? {
        Some(r) if r.status == "pending" && r.target_accepted => r,
        _ => return Ok(None),
    };
    let members = get_raid_members(pool, raid_id).await?;
    let party_count = members.iter().filter(|m| m.side == "party").count();
    if !(2..=5).contains(&party_count) {
        return Ok(None);
    }
    let Some(power) = raid_power_check(pool, raid_id).await? else {
        return Ok(None);
    };
    if !power.within_window {
        return Ok(None);
    }

    let mut order: Vec<(i64, i64)> = Vec::new();
    for m in &members {
        if !m.is_alive {
            continue;
        }
        let (h, _) = get_or_create(pool, raid.chat_id, m.tg_id).await?;
        let fx = equipment_effects_for_player(pool, raid.chat_id, m.tg_id).await.unwrap_or_default();
        let ini = initiative_score(&h) + (fx.raid_initiative * 100.0) as i64;
        order.push((m.tg_id, ini));
    }
    order.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let order_ids: Vec<i64> = order.into_iter().map(|x| x.0).collect();
    if order_ids.is_empty() {
        return Ok(None);
    }

    let started = sqlx::query_as::<_, HuyaRaid>(
        &format!(
            "UPDATE huya_raid
             SET status = 'active', round = 1, turn_index = 0, focus_round = 0, initiative_order = $2,
                 turn_deadline_at = NOW() + ($3 || ' seconds')::interval
             WHERE id = $1 AND status = 'pending'
             RETURNING {RAID_SELECT}"
        ),
    )
    .bind(raid_id)
    .bind(serialize_initiative_order(&order_ids))
    .bind(RAID_TURN_TIMEOUT_SECONDS)
    .fetch_optional(pool)
    .await?;

    Ok(started)
}

pub async fn raid_current_actor(pool: &PgPool, raid_id: i32) -> Result<Option<i64>, AppError> {
    let mut raid = match get_raid(pool, raid_id).await? {
        Some(r) if r.status == "active" => r,
        _ => return Ok(None),
    };
    let members = get_raid_members(pool, raid_id).await?;
    let order = parse_initiative_order(&raid.initiative_order);
    if order.is_empty() {
        return Ok(None);
    }
    let mut idx = next_alive_index(&order, &members, raid.turn_index.max(0) as usize);
    if let Some(deadline) = raid.turn_deadline_at {
        if Utc::now() >= deadline {
            idx = next_alive_index(&order, &members, idx + 1);
            raid = sqlx::query_as::<_, HuyaRaid>(
                &format!(
                    "UPDATE huya_raid
                     SET turn_index = $2,
                         turn_deadline_at = NOW() + ($3 || ' seconds')::interval
                     WHERE id = $1
                     RETURNING {RAID_SELECT}"
                ),
            )
            .bind(raid.id)
            .bind(idx as i32)
            .bind(RAID_TURN_TIMEOUT_SECONDS)
            .fetch_one(pool)
            .await?;
        }
    }
    let idx = next_alive_index(&order, &members, raid.turn_index.max(0) as usize);
    Ok(order.get(idx).copied())
}

pub async fn raid_take_turn(
    pool: &PgPool,
    raid_id: i32,
    actor_tg_id: i64,
    action: RaidTurnAction,
) -> Result<Option<RaidTurnResult>, AppError> {
    let raid = match get_raid(pool, raid_id).await? {
        Some(r) if r.status == "active" => r,
        _ => return Ok(None),
    };
    let mut members = get_raid_members(pool, raid_id).await?;
    let order = parse_initiative_order(&raid.initiative_order);
    if order.is_empty() {
        return Ok(None);
    }

    let idx = next_alive_index(&order, &members, raid.turn_index.max(0) as usize);
    let current_actor = match order.get(idx) {
        Some(v) => *v,
        None => return Ok(None),
    };
    if current_actor != actor_tg_id {
        return Ok(None);
    }

    let actor = match members.iter().find(|m| m.tg_id == actor_tg_id && m.is_alive) {
        Some(v) => v.clone(),
        None => return Ok(None),
    };
    let actor_side = actor.side.clone();
    let (actor_huya, _) = get_or_create(pool, raid.chat_id, actor_tg_id).await?;

    let log_line = match action {
        RaidTurnAction::Guard => {
            sqlx::query(
                "UPDATE huya_raid_member SET guard_until_round = $1, acted_in_round = TRUE
                 WHERE raid_id = $2 AND tg_id = $3",
            )
            .bind(raid.round)
            .bind(raid.id)
            .bind(actor_tg_id)
            .execute(pool)
            .await?;
            "🛡️ Защита до следующего хода".to_string()
        }
        RaidTurnAction::Focus => {
            if actor_side != "party" {
                return Ok(None);
            }
            sqlx::query("UPDATE huya_raid SET focus_round = $1 WHERE id = $2")
                .bind(raid.round)
                .bind(raid.id)
                .execute(pool)
                .await?;
            sqlx::query(
                "UPDATE huya_raid_member SET acted_in_round = TRUE
                 WHERE raid_id = $1 AND tg_id = $2",
            )
            .bind(raid.id)
            .bind(actor_tg_id)
            .execute(pool)
            .await?;
            "🎯 Пати сфокусировала урон на эту цель".to_string()
        }
        RaidTurnAction::Attack => {
            let target_member = if actor_side == "party" {
                members
                    .iter()
                    .find(|m| m.side == "target" && m.is_alive)
                    .cloned()
            } else {
                let mut party_alive: Vec<HuyaRaidMember> = members
                    .iter()
                    .filter(|m| m.side == "party" && m.is_alive)
                    .cloned()
                    .collect();
                party_alive.sort_by(|a, b| a.hp_snapshot.cmp(&b.hp_snapshot));
                party_alive.into_iter().next()
            };
            let Some(target_member) = target_member else {
                return Ok(None);
            };
            let (target_huya, _) = get_or_create(pool, raid.chat_id, target_member.tg_id).await?;
            let actor_fx = equipment_effects_for_player(pool, raid.chat_id, actor_tg_id).await.unwrap_or_default();
            let target_fx = equipment_effects_for_player(pool, raid.chat_id, target_member.tg_id).await.unwrap_or_default();
            let rand_bonus = {
                let mut rng = rand::rng();
                rng.random_range(6.0_f64..18.0_f64)
            };
            let mut damage = (actor_huya.length_mm.max(1) as f64 * 0.035
                + actor_huya.level.max(1) as f64 * 3.0
                + rand_bonus) as i32;

            let atk_factor = 1.0
                + actor_huya.skill_shaft as f64 * 0.04
                + actor_huya.skill_pierce as f64 * 0.02
                + actor_huya.skill_eternal as f64 * 0.10
                + actor_fx.atk_pct;
            let mut def_factor =
                (1.0 - target_huya.skill_skin as f64 * 0.03 - target_huya.skill_scales as f64 * 0.01 - target_fx.def_pct).max(0.25);
            if target_member.guard_until_round == raid.round {
                def_factor *= 0.65;
            }
            if actor_side == "party" && raid.focus_round == raid.round {
                damage = (damage as f64 * 1.12) as i32;
            }
            let attacker_form_mult = if actor_huya.is_pussy() { 0.90 } else { 1.0 };
            let defender_form_mult = if target_huya.is_pussy() { 0.85 } else { 1.0 };
            damage = ((damage as f64) * atk_factor * def_factor * attacker_form_mult * defender_form_mult).round() as i32;
            damage = damage.max(3);

            sqlx::query(
                "UPDATE huya_raid_member
                 SET hp_snapshot = GREATEST(hp_snapshot - $1, 0),
                     is_alive = CASE WHEN hp_snapshot - $1 <= 0 THEN FALSE ELSE TRUE END
                 WHERE raid_id = $2 AND tg_id = $3",
            )
            .bind(damage)
            .bind(raid.id)
            .bind(target_member.tg_id)
            .execute(pool)
            .await?;
            sqlx::query(
                "UPDATE huya_raid_member
                 SET acted_in_round = TRUE, damage_done = damage_done + $1
                 WHERE raid_id = $2 AND tg_id = $3",
            )
            .bind(damage)
            .bind(raid.id)
            .bind(actor_tg_id)
            .execute(pool)
            .await?;
            if target_fx.reflect_pct > 0.0 {
                let reflected = ((damage as f64) * target_fx.reflect_pct).round() as i32;
                if reflected > 0 {
                    sqlx::query(
                        "UPDATE huya_raid_member
                         SET hp_snapshot = GREATEST(hp_snapshot - $1, 0),
                             is_alive = CASE WHEN hp_snapshot - $1 <= 0 THEN FALSE ELSE TRUE END
                         WHERE raid_id = $2 AND tg_id = $3",
                    )
                    .bind(reflected)
                    .bind(raid.id)
                    .bind(actor_tg_id)
                    .execute(pool)
                    .await?;
                }
            }
            format!("💥 Урон {damage} по цели")
        }
    };

    let mut raid_now = match get_raid(pool, raid_id).await? {
        Some(r) => r,
        None => return Ok(None),
    };
    members = get_raid_members(pool, raid_id).await?;

    let party_alive = members.iter().any(|m| m.side == "party" && m.is_alive);
    let target_alive = members.iter().any(|m| m.side == "target" && m.is_alive);
    if !party_alive || !target_alive || raid_now.round >= raid_now.max_rounds {
        let winner_side = if party_alive && !target_alive {
            Some("party".to_string())
        } else if !party_alive && target_alive {
            Some("target".to_string())
        } else {
            None
        };
        sqlx::query("UPDATE huya_raid SET status = 'done' WHERE id = $1")
            .bind(raid.id)
            .execute(pool)
            .await?;
        raid_now = match get_raid(pool, raid_id).await? {
            Some(r) => r,
            None => return Ok(None),
        };
        return Ok(Some(RaidTurnResult {
            raid: raid_now,
            members,
            actor_tg_id,
            actor_side,
            log_line,
            finished: true,
            winner_side,
        }));
    }

    let new_index = ((idx + 1) % order.len()) as i32;
    let wrapped = new_index == 0;
    if wrapped {
        sqlx::query("UPDATE huya_raid_member SET acted_in_round = FALSE WHERE raid_id = $1")
            .bind(raid.id)
            .execute(pool)
            .await?;
    }
    sqlx::query(
        "UPDATE huya_raid
         SET turn_index = $2,
             round = CASE WHEN $3 THEN round + 1 ELSE round END,
             turn_deadline_at = NOW() + ($4 || ' seconds')::interval
         WHERE id = $1",
    )
    .bind(raid.id)
    .bind(new_index)
    .bind(wrapped)
    .bind(RAID_TURN_TIMEOUT_SECONDS)
    .execute(pool)
    .await?;

    raid_now = match get_raid(pool, raid_id).await? {
        Some(r) => r,
        None => return Ok(None),
    };
    members = get_raid_members(pool, raid_id).await?;
    Ok(Some(RaidTurnResult {
        raid: raid_now,
        members,
        actor_tg_id,
        actor_side,
        log_line,
        finished: false,
        winner_side: None,
    }))
}

pub async fn raid_apply_rewards(pool: &PgPool, raid: &HuyaRaid, winner_side: Option<&str>) -> Result<i32, AppError> {
    let members = get_raid_members(pool, raid.id).await?;
    let target_member = match members.iter().find(|m| m.side == "target") {
        Some(v) => v,
        None => return Ok(0),
    };
    let (target_huya, _) = get_or_create(pool, raid.chat_id, target_member.tg_id).await?;

    match winner_side {
        Some("party") => {
            let raw_pool = ((target_huya.length_mm.max(0) as f64) * RAID_STEAL_CAP_PCT) as i32;
            let steal_pool = raw_pool.clamp(10, 200);
            let party: Vec<&HuyaRaidMember> = members.iter().filter(|m| m.side == "party").collect();
            if party.is_empty() {
                return Ok(0);
            }
            let total_damage: i32 = party.iter().map(|m| m.damage_done.max(0)).sum::<i32>().max(1);
            let mut distributed = 0_i32;
            for p in &party {
                let share = ((steal_pool as f64) * (p.damage_done.max(0) as f64 / total_damage as f64)).round() as i32;
                let gain = share.max(1);
                distributed += gain;
                sqlx::query("UPDATE huya SET length_mm = length_mm + $1 WHERE tg_id = $2")
                    .bind(gain)
                    .bind(p.tg_id)
                    .execute(pool)
                    .await?;
                let (party_huya, _) = get_or_create(pool, raid.chat_id, p.tg_id).await?;
                let _ = apply_xp_gain(pool, party_huya.id, 15).await?;
            }
            sqlx::query("UPDATE huya SET length_mm = GREATEST(length_mm - $1, 10) WHERE tg_id = $2")
                .bind(distributed.min(steal_pool))
                .bind(target_member.tg_id)
                .execute(pool)
                .await?;
            Ok(distributed.min(steal_pool))
        }
        Some("target") => {
            let (target_huya, _) = get_or_create(pool, raid.chat_id, target_member.tg_id).await?;
            let _ = apply_xp_gain(pool, target_huya.id, 40).await?;
            Ok(0)
        }
        _ => Ok(0),
    }
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
    let ch_fx = equipment_effects_for_player(pool, fight.chat_id, fight.challenger_tg_id).await.unwrap_or_default();
    let tg_fx = equipment_effects_for_player(pool, fight.chat_id, fight.target_tg_id).await.unwrap_or_default();

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
            let win_fx = if ch_wins_round { ch_fx } else { tg_fx };
            let lose_fx = if ch_wins_round { tg_fx } else { ch_fx };
            let atk_factor = (1.0 + winner.skill_shaft as f64 * 0.04 + winner.atk_boost as f64 / 100.0 + win_fx.atk_pct)
                * berserker_mult * eternal_mult * vortex_crit * eggtwist_crit;
            let def_factor = (1.0 - loser.skill_skin as f64 * 0.03 - loser.def_boost as f64 / 100.0 - lose_fx.def_pct)
                .max(0.15) * pierce_factor;
            let attacker_form_mult = if winner.is_pussy() { 0.90 } else { 1.0 };
            let defender_form_mult = if loser.is_pussy() { 0.85 } else { 1.0 };
            ((base * atk_factor * def_factor * attacker_form_mult * defender_form_mult) as i32).max(5)
        };
        if ch_wins_round {
            (0, damage, Some(fight.challenger_tg_id))
        } else {
            (damage, 0, Some(fight.target_tg_id))
        }
    };

    let mut new_ch_hp = (fight.ch_hp - ch_damage).max(0);
    let mut new_tg_hp = (fight.tg_hp - tg_damage).max(0);
    // Thorns-style reflect from equipment traits.
    if ch_damage > 0 && tg_fx.reflect_pct > 0.0 {
        new_ch_hp = (new_ch_hp - (ch_damage as f64 * tg_fx.reflect_pct).round() as i32).max(0);
    }
    if tg_damage > 0 && ch_fx.reflect_pct > 0.0 {
        new_tg_hp = (new_tg_hp - (tg_damage as f64 * ch_fx.reflect_pct).round() as i32).max(0);
    }

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

    let new_round = fight.round + 1;
    let fight_over = new_ch_hp == 0 || new_tg_hp == 0;

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

    let (winner, loser) = if winner_tg_id == fight.challenger_tg_id { (&ch, &tg) } else { (&tg, &ch) };

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

    let loser_old_len = loser.length_mm;
    let loser_len_after_raw = loser.length_mm - steal_mm;
    let steal_actual = if loser.skill_fortress > 0 && loser_len_after_raw < 10 {
        (loser.length_mm - 10).max(0)
    } else {
        steal_mm
    };
    let loser_new_len = (loser.length_mm - steal_actual).max(0);

    sqlx::query("UPDATE huya SET length_mm = length_mm + $1, fights_won = fights_won + 1 WHERE id = $2")
        .bind(steal_actual).bind(winner.id).execute(pool).await?;
    sqlx::query("UPDATE huya SET length_mm = GREATEST(length_mm - $1, 0), fights_lost = fights_lost + 1 WHERE id = $2")
        .bind(steal_actual).bind(loser.id).execute(pool).await?;

    // Fortress: clamp loser to 10mm min
    if loser.skill_fortress > 0 && loser_new_len < 10 {
        sqlx::query("UPDATE huya SET length_mm = 10 WHERE id = $1 AND length_mm < 10")
            .bind(loser.id).execute(pool).await?;
    }

    // Drop ring loot if loser lost available ring slots.
    drop_rings_on_shrink(
        pool,
        fight.chat_id,
        winner_tg_id,
        loser_tg_id,
        loser_old_len,
        loser_new_len,
    )
    .await?;

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

const DUTCH_HELM_EVENT_SELECT: &str =
    "id, event_date, start_at, join_deadline_at, status, seed, created_at, started_at, finished_at";

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DutchHelmChatSummary {
    pub chat_id: i64,
    pub participants: i64,
    pub reward_mm: i32,
}

pub async fn get_or_create_dutch_helm_event(
    pool: &PgPool,
    event_date: NaiveDate,
    start_at: DateTime<Utc>,
    join_deadline_at: DateTime<Utc>,
    seed: i32,
) -> Result<HuyaDutchHelmEvent, AppError> {
    let row = sqlx::query_as::<_, HuyaDutchHelmEvent>(&format!(
        "INSERT INTO huya_dutch_helm_event (event_date, start_at, join_deadline_at, status, seed)
         VALUES ($1, $2, $3, 'scheduled', $4)
         ON CONFLICT (event_date) DO UPDATE SET event_date = EXCLUDED.event_date
         RETURNING {DUTCH_HELM_EVENT_SELECT}"
    ))
    .bind(event_date)
    .bind(start_at)
    .bind(join_deadline_at)
    .bind(seed)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn get_dutch_helm_event_by_date(
    pool: &PgPool,
    event_date: NaiveDate,
) -> Result<Option<HuyaDutchHelmEvent>, AppError> {
    let row = sqlx::query_as::<_, HuyaDutchHelmEvent>(&format!(
        "SELECT {DUTCH_HELM_EVENT_SELECT}
         FROM huya_dutch_helm_event
         WHERE event_date = $1"
    ))
    .bind(event_date)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn activate_dutch_helm_event(
    pool: &PgPool,
    event_id: i32,
) -> Result<Option<HuyaDutchHelmEvent>, AppError> {
    let row = sqlx::query_as::<_, HuyaDutchHelmEvent>(&format!(
        "UPDATE huya_dutch_helm_event
         SET status = 'active', started_at = NOW()
         WHERE id = $1 AND status = 'scheduled'
         RETURNING {DUTCH_HELM_EVENT_SELECT}"
    ))
    .bind(event_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn get_active_dutch_helm_event(
    pool: &PgPool,
    now: DateTime<Utc>,
) -> Result<Option<HuyaDutchHelmEvent>, AppError> {
    let row = sqlx::query_as::<_, HuyaDutchHelmEvent>(&format!(
        "SELECT {DUTCH_HELM_EVENT_SELECT}
         FROM huya_dutch_helm_event
         WHERE status = 'active' AND start_at <= $1 AND join_deadline_at > $1
         ORDER BY id DESC LIMIT 1"
    ))
    .bind(now)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn join_dutch_helm_event(
    pool: &PgPool,
    event_id: i32,
    chat_id: i64,
    tg_id: i64,
    now: DateTime<Utc>,
) -> Result<bool, AppError> {
    let inserted = sqlx::query(
        "INSERT INTO huya_dutch_helm_participant (event_id, chat_id, tg_id)
         SELECT $1, $2, $3
         WHERE EXISTS (
             SELECT 1
             FROM huya_dutch_helm_event e
             WHERE e.id = $1 AND e.status = 'active' AND e.start_at <= $4 AND e.join_deadline_at > $4
         )
         ON CONFLICT (event_id, chat_id, tg_id) DO NOTHING",
    )
    .bind(event_id)
    .bind(chat_id)
    .bind(tg_id)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(inserted.rows_affected() > 0)
}

pub async fn get_dutch_helm_chat_participants_count(
    pool: &PgPool,
    event_id: i32,
    chat_id: i64,
) -> Result<i64, AppError> {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::bigint FROM huya_dutch_helm_participant WHERE event_id = $1 AND chat_id = $2",
    )
    .bind(event_id)
    .bind(chat_id)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

pub async fn list_dutch_helm_chat_counts(
    pool: &PgPool,
    event_id: i32,
) -> Result<Vec<(i64, i64)>, AppError> {
    let rows = sqlx::query_as::<_, (i64, i64)>(
        "SELECT chat_id, COUNT(*)::bigint AS cnt
         FROM huya_dutch_helm_participant
         WHERE event_id = $1
         GROUP BY chat_id",
    )
    .bind(event_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn finalize_dutch_helm_event(
    pool: &PgPool,
    event_id: i32,
) -> Result<Vec<DutchHelmChatSummary>, AppError> {
    let mut tx = pool.begin().await?;
    let can_finalize: Option<(i32,)> = sqlx::query_as(
        "SELECT id FROM huya_dutch_helm_event
         WHERE id = $1 AND status = 'active' AND join_deadline_at <= NOW()
         FOR UPDATE",
    )
    .bind(event_id)
    .fetch_optional(&mut *tx)
    .await?;
    if can_finalize.is_none() {
        tx.rollback().await?;
        return Ok(Vec::new());
    }

    let chat_rows = sqlx::query_as::<_, (i64, i64)>(
        "SELECT chat_id, COUNT(*)::bigint AS cnt
         FROM huya_dutch_helm_participant
         WHERE event_id = $1
         GROUP BY chat_id",
    )
    .bind(event_id)
    .fetch_all(&mut *tx)
    .await?;

    fn per_player_reward_mm(
        base: i32,
        max_extra: i32,
        event_id: i32,
        chat_id: i64,
        tg_id: i64,
    ) -> i32 {
        let mut x = (event_id as i64)
            .wrapping_mul(1103515245)
            .wrapping_add(chat_id.wrapping_mul(12345))
            ^ tg_id.wrapping_mul(1_000_003);
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let raw = (x & 0x7fff) as i32;
        let extra = if max_extra > 0 {
            raw % (max_extra + 1)
        } else {
            0
        };
        (base + extra).clamp(6, 80)
    }

    let mut out = Vec::with_capacity(chat_rows.len());
    for (chat_id, participants) in chat_rows {
        let participants_i32 = (participants as i32).max(1);
        let base = 4 + participants_i32;
        let max_extra = 2 + participants_i32;
        let mut total_reward_mm: i64 = 0;

        let player_rows = sqlx::query_as::<_, (i64,)>(
            "SELECT tg_id FROM huya_dutch_helm_participant
             WHERE event_id = $1 AND chat_id = $2",
        )
        .bind(event_id)
        .bind(chat_id)
        .fetch_all(&mut *tx)
        .await?;

        for (tg_id,) in player_rows {
            let reward_mm = per_player_reward_mm(base, max_extra, event_id, chat_id, tg_id);

            sqlx::query(
                "UPDATE huya
                 SET length_mm = CASE WHEN length_mm < 0 THEN length_mm - $1 ELSE length_mm + $1 END
                 WHERE chat_id = $2 AND tg_id = $3",
            )
            .bind(reward_mm)
            .bind(chat_id)
            .bind(tg_id)
            .execute(&mut *tx)
            .await?;

            sqlx::query(
                "UPDATE huya_dutch_helm_participant
                 SET reward_mm = $1
                 WHERE event_id = $2 AND chat_id = $3 AND tg_id = $4",
            )
            .bind(reward_mm)
            .bind(event_id)
            .bind(chat_id)
            .bind(tg_id)
            .execute(&mut *tx)
            .await?;

            total_reward_mm += reward_mm as i64;
        }

        let avg_reward_mm = if participants > 0 {
            (total_reward_mm / participants).max(0) as i32
        } else {
            0
        };

        out.push(DutchHelmChatSummary {
            chat_id,
            participants,
            reward_mm: avg_reward_mm,
        });
    }

    sqlx::query(
        "UPDATE huya_dutch_helm_event
         SET status = 'finished', finished_at = NOW()
         WHERE id = $1",
    )
    .bind(event_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(out)
}

pub async fn cleanup_old_dutch_helm_events(
    pool: &PgPool,
    keep_days: i64,
) -> Result<u64, AppError> {
    let deleted = sqlx::query(
        "DELETE FROM huya_dutch_helm_event
         WHERE event_date < (CURRENT_DATE - ($1::int * INTERVAL '1 day'))",
    )
    .bind(keep_days as i32)
    .execute(pool)
    .await?;
    Ok(deleted.rows_affected())
}

pub async fn cleanup_old_huya_fights(pool: &PgPool) -> Result<u64, AppError> {
    const BATCH_SIZE: i64 = 5_000;
    let mut total_deleted = 0_u64;
    loop {
        let deleted = sqlx::query(
            "WITH doomed AS (
                SELECT ctid
                FROM huya_fight
                WHERE status = 'done'
                  AND created_at < NOW() - INTERVAL '30 days'
                LIMIT $1
            )
            DELETE FROM huya_fight
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

pub async fn cleanup_old_huya_raids(pool: &PgPool) -> Result<u64, AppError> {
    const BATCH_SIZE: i64 = 5_000;
    let mut total_deleted = 0_u64;
    loop {
        let deleted = sqlx::query(
            "WITH doomed AS (
                SELECT ctid
                FROM huya_raid
                WHERE status IN ('done', 'cancelled')
                  AND created_at < NOW() - INTERVAL '30 days'
                LIMIT $1
            )
            DELETE FROM huya_raid
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

pub async fn cleanup_old_huya_loot_logs(pool: &PgPool) -> Result<u64, AppError> {
    const BATCH_SIZE: i64 = 5_000;
    let mut total_deleted = 0_u64;
    loop {
        let deleted = sqlx::query(
            "WITH doomed AS (
                SELECT ctid
                FROM huya_loot_log
                WHERE created_at < NOW() - INTERVAL '60 days'
                LIMIT $1
            )
            DELETE FROM huya_loot_log
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

pub async fn cleanup_old_huya_reforge_logs(pool: &PgPool) -> Result<u64, AppError> {
    const BATCH_SIZE: i64 = 5_000;
    let mut total_deleted = 0_u64;
    loop {
        let deleted = sqlx::query(
            "WITH doomed AS (
                SELECT ctid
                FROM huya_reforge_log
                WHERE created_at < NOW() - INTERVAL '60 days'
                LIMIT $1
            )
            DELETE FROM huya_reforge_log
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
