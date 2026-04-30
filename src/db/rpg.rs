//! RPG module DB helpers for Pidor-Royale.
//!
//! Contains helpers to work with rpg_player and rpg_ui_state tables.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::LazyLock;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};

use crate::error::AppError;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RpgPlayer {
    pub id: i32,
    pub user_id: i32,
    pub level: i32,
    pub xp: i64,
    pub xp_to_next: i64,

    pub hp_max: i32,
    pub hp_current: i32,

    pub stamina_max: i32,
    pub stamina_current: i32,
    pub stamina_updated_at: DateTime<Utc>,

    pub strength: i32,
    pub agility: i32,
    pub intellect: i32,
    pub vitality: i32,
    pub luck: i32,

    pub pos_x: i32,
    pub pos_y: i32,

    pub class_code: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RpgUiState {
    pub id: i32,
    pub user_id: i32,
    pub chat_id: i64,
    pub message_id: i64,
    pub mode: String,
    pub submode: Option<String>,
    pub payload_json: serde_json::Value,
}

#[derive(Debug, Clone)]
struct InMemoryUiState {
    state: RpgUiState,
    updated_at: DateTime<Utc>,
}

const RPG_UI_STATE_TTL_HOURS: i64 = 24;
static RPG_UI_STATE_STORE: LazyLock<RwLock<HashMap<(i32, i64), InMemoryUiState>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

fn ui_state_not_expired(updated_at: DateTime<Utc>) -> bool {
    updated_at > Utc::now() - chrono::Duration::hours(RPG_UI_STATE_TTL_HOURS)
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RpgMapTile {
    pub x: i32,
    pub y: i32,
    pub biome: String,
    pub object_code: Option<String>,
    pub min_level: Option<i32>,
    pub max_level: Option<i32>,
    pub flags: i32,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RpgBattle {
    pub id: i32,
    pub r#type: String,
    pub player1_id: i32,
    pub player2_id: Option<i32>,
    pub mob_group_code: Option<String>,
    pub turn_player_id: Option<i32>,
    pub status: String,
    pub state_json: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RpgItem {
    pub id: i32,
    pub code: String,
    pub name: String,
    pub description: String,
    pub item_type: String,
    pub slot: Option<String>,
    pub rarity: String,
    pub base_stats: serde_json::Value,
    pub effects: serde_json::Value,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RpgInventoryEntry {
    pub quantity: i32,
    pub name: String,
    pub rarity: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RpgEquipmentEntry {
    pub player_id: i32,
    pub slot: String,
    pub item_id: i32,
    pub equipped_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RpgMobTemplate {
    pub id: i32,
    pub code: String,
    pub name: String,
    pub description: String,
    pub base_stats: serde_json::Value,
    pub loot_table_code: Option<String>,
    pub recommended_level: i32,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RpgSkill {
    pub id: i32,
    pub code: String,
    pub name: String,
    pub description: String,
    pub cost_stamina: i32,
    pub cooldown_turns: i32,
    pub target_type: String,
    pub effect_type: String,
    pub scaling: serde_json::Value,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct GuildBossTemplate {
    pub id: i32,
    pub code: String,
    pub name: String,
    pub description: String,
    pub recommended_level: i32,
    pub max_hp: i64,
    pub attack_pattern: serde_json::Value,
    pub loot_table_code: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct GuildRaid {
    pub id: i32,
    pub guild_id: i32,
    pub boss_id: i32,
    pub status: String,
    pub hp_left: i64,
    pub phase_state: serde_json::Value,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct GuildRaidParticipation {
    pub raid_id: i32,
    pub player_id: i32,
    pub damage_done: i64,
    pub healing_done: i64,
    pub hits: i32,
    pub deaths: i32,
}

/// In-memory battle state for PvE MVP. Stored in rpg_battle.state_json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BattleParticipantState {
    pub hp: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpgBattleState {
    pub player_id: i32,
    pub enemy_code: String,
    pub player: BattleParticipantState,
    pub enemy: BattleParticipantState,
    pub turn_player_id: i32,
    pub log: Vec<String>,
}

/// Simple battle math helpers (MVP).
pub mod battle_math {
    use super::RpgPlayer;

    pub fn calc_attack_phys(player: &RpgPlayer) -> i32 {
        // Very approximate: base from strength and level.
        5 + player.strength * 2 + player.level
    }

    pub fn calc_defense_phys(player: &RpgPlayer) -> i32 {
        2 + player.vitality + (player.level / 2)
    }

    pub fn roll_hit_chance(acc: i32, eva: i32) -> bool {
        let base = 0.75_f64;
        let acc_term = (acc as f64) * 0.01;
        let eva_term = (eva as f64) * 0.01;
        let mut chance = base + acc_term - eva_term;
        if chance < 0.05 {
            chance = 0.05;
        }
        if chance > 0.95 {
            chance = 0.95;
        }
        // Deterministic approximation: treat chance >= 0.5 as hit.
        chance >= 0.5
    }

    pub fn roll_damage(
        attack: i32,
        defense: i32,
        crit_chance: f64,
        crit_multiplier: f64,
    ) -> (i32, bool) {
        let mut base = (attack as f64 - defense as f64 * 0.7).max(1.0);

        // Deterministic pseudo-variance: small variation based on attack/defense parity.
        let variance = if (attack + defense) % 2 == 0 { 1.1 } else { 0.9 };
        base *= variance;

        let is_crit = crit_chance > 0.0;
        if is_crit {
            base *= crit_multiplier;
        }

        (base.round() as i32, is_crit)
    }
}

/// Get or create RPG player for given tguser id.
pub async fn get_or_create_player(pool: &PgPool, user_id: i32) -> Result<RpgPlayer, AppError> {
    if let Some(player) = sqlx::query_as::<_, RpgPlayer>(
        r#"
        SELECT id, user_id, level, xp, xp_to_next, hp_max, hp_current,
               stamina_max, stamina_current, stamina_updated_at,
               strength, agility, intellect, vitality, luck,
               pos_x, pos_y,
               class_code
        FROM rpg_player
        WHERE user_id = $1
        "#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?
    {
        return Ok(player);
    }

    let player = sqlx::query_as::<_, RpgPlayer>(
        r#"
        INSERT INTO rpg_player (user_id)
        VALUES ($1)
        RETURNING id, user_id, level, xp, xp_to_next, hp_max, hp_current,
                  stamina_max, stamina_current, stamina_updated_at,
                  strength, agility, intellect, vitality, luck,
                  pos_x, pos_y,
                  class_code
        "#,
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    Ok(player)
}

/// Get latest UI state for user in given chat, if any.
pub async fn get_ui_state(
    _pool: &PgPool,
    user_id: i32,
    chat_id: i64,
) -> Result<Option<RpgUiState>, AppError> {
    let key = (user_id, chat_id);
    {
        let store = RPG_UI_STATE_STORE.read().await;
        if let Some(entry) = store.get(&key)
            && ui_state_not_expired(entry.updated_at)
        {
            return Ok(Some(entry.state.clone()));
        }
    }
    let mut store = RPG_UI_STATE_STORE.write().await;
    store.remove(&key);
    Ok(None)
}

/// Upsert UI state after we created/updated the RPG menu message.
pub async fn upsert_ui_state(
    pool: &PgPool,
    user_id: i32,
    chat_id: i64,
    message_id: i64,
    mode: &str,
    submode: Option<&str>,
) -> Result<(), AppError> {
    upsert_ui_state_with_payload(
        pool,
        user_id,
        chat_id,
        message_id,
        mode,
        submode,
        serde_json::json!({}),
    )
    .await
}

/// Upsert UI state with payload JSON.
/// Note: Always inserts new row; get_ui_state uses ORDER BY updated_at DESC LIMIT 1 to get latest.
pub async fn upsert_ui_state_with_payload(
    _pool: &PgPool,
    user_id: i32,
    chat_id: i64,
    message_id: i64,
    mode: &str,
    submode: Option<&str>,
    payload: serde_json::Value,
) -> Result<(), AppError> {
    let mut store = RPG_UI_STATE_STORE.write().await;
    store.retain(|_, v| ui_state_not_expired(v.updated_at));
    store.insert(
        (user_id, chat_id),
        InMemoryUiState {
            state: RpgUiState {
                id: 0,
                user_id,
                chat_id,
                message_id,
                mode: mode.to_string(),
                submode: submode.map(|s| s.to_string()),
                payload_json: payload,
            },
            updated_at: Utc::now(),
        },
    );
    Ok(())
}

/// Load square window of map tiles around (center_x, center_y) with the given radius.
pub async fn load_map_window(
    pool: &PgPool,
    center_x: i32,
    center_y: i32,
    radius: i32,
) -> Result<Vec<RpgMapTile>, AppError> {
    let min_x = center_x - radius;
    let max_x = center_x + radius;
    let min_y = center_y - radius;
    let max_y = center_y + radius;

    let tiles = sqlx::query_as::<_, RpgMapTile>(
        r#"
        SELECT x, y, biome, object_code, min_level, max_level, flags
        FROM rpg_map_tile
        WHERE x BETWEEN $1 AND $2
          AND y BETWEEN $3 AND $4
        "#,
    )
    .bind(min_x)
    .bind(max_x)
    .bind(min_y)
    .bind(max_y)
    .fetch_all(pool)
    .await?;

    Ok(tiles)
}

/// Helper for listing inventory entries for player.
pub async fn list_inventory(
    pool: &PgPool,
    player_id: i32,
) -> Result<Vec<RpgInventoryEntry>, AppError> {
    let rows = sqlx::query_as::<_, RpgInventoryEntry>(
        r#"
        SELECT
            i.quantity,
            it.name,
            it.rarity
        FROM rpg_inventory i
        JOIN rpg_item it ON it.id = i.item_id
        WHERE i.player_id = $1
        ORDER BY i.created_at ASC
        "#,
    )
    .bind(player_id)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

/// Add an item to player inventory (or increase quantity).
pub async fn add_item_to_inventory(
    pool: &PgPool,
    player_id: i32,
    item_code: &str,
    quantity: i32,
) -> Result<(), AppError> {
    let item: RpgItem = sqlx::query_as(
        r#"
        SELECT id, code, name, description, item_type, slot, rarity, base_stats, effects
        FROM rpg_item
        WHERE code = $1
        "#,
    )
    .bind(item_code)
    .fetch_one(pool)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO rpg_inventory (player_id, item_id, quantity)
        VALUES ($1, $2, $3)
        ON CONFLICT (player_id, item_id)
        DO UPDATE SET quantity = rpg_inventory.quantity + EXCLUDED.quantity
        "#,
    )
    .bind(player_id)
    .bind(item.id)
    .bind(quantity)
    .execute(pool)
    .await?;

    Ok(())
}

/// Recalculate stamina based on time passed since last update.
pub fn recalc_stamina(player: &RpgPlayer, now: DateTime<Utc>) -> (i32, DateTime<Utc>) {
    // Simple regen: +1 stamina every 60 seconds, up to stamina_max.
    let mut current = player.stamina_current;
    let mut updated_at = player.stamina_updated_at;
    if current >= player.stamina_max {
        return (current, updated_at);
    }
    let seconds = (now - updated_at).num_seconds();
    if seconds <= 0 {
        return (current, updated_at);
    }
    let regen_points = (seconds / 60) as i32;
    if regen_points <= 0 {
        return (current, updated_at);
    }
    current = (current + regen_points).min(player.stamina_max);
    updated_at = now;
    (current, updated_at)
}

/// Persist stamina values.
pub async fn update_stamina(
    pool: &PgPool,
    player_id: i32,
    stamina_current: i32,
    stamina_updated_at: DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE rpg_player
        SET stamina_current = $1,
            stamina_updated_at = $2
        WHERE id = $3
        "#,
    )
    .bind(stamina_current)
    .bind(stamina_updated_at)
    .bind(player_id)
    .execute(pool)
    .await?;

    Ok(())
}

/// Create a simple PvE battle against a basic enemy code using precomputed battle state.
pub async fn create_pve_battle(
    pool: &PgPool,
    player: &RpgPlayer,
    enemy_code: &str,
    enemy_hp: i32,
) -> Result<RpgBattle, AppError> {
    let state = RpgBattleState {
        player_id: player.id,
        enemy_code: enemy_code.to_string(),
        player: BattleParticipantState {
            hp: player.hp_current,
        },
        enemy: BattleParticipantState { hp: enemy_hp },
        turn_player_id: player.id,
        log: Vec::new(),
    };

    let state_json = serde_json::to_value(&state)?;

    let battle = sqlx::query_as::<_, RpgBattle>(
        r#"
        INSERT INTO rpg_battle (
            type,
            player1_id,
            player2_id,
            mob_group_code,
            turn_player_id,
            status,
            state_json
        )
        VALUES ('pve', $1, NULL, $2, $1, 'active', $3)
        RETURNING
            id,
            type,
            player1_id,
            player2_id,
            mob_group_code,
            turn_player_id,
            status,
            state_json,
            created_at,
            updated_at,
            finished_at
        "#,
    )
    .bind(player.id)
    .bind(enemy_code)
    .bind(state_json)
    .fetch_one(pool)
    .await?;

    Ok(battle)
}

/// Load active battle for given player, if any.
pub async fn get_active_battle_for_player(
    pool: &PgPool,
    player_id: i32,
) -> Result<Option<RpgBattle>, AppError> {
    let battle = sqlx::query_as::<_, RpgBattle>(
        r#"
        SELECT
            id,
            type,
            player1_id,
            player2_id,
            mob_group_code,
            turn_player_id,
            status,
            state_json,
            created_at,
            updated_at,
            finished_at
        FROM rpg_battle
        WHERE status = 'active'
          AND (player1_id = $1 OR player2_id = $1)
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .bind(player_id)
    .fetch_optional(pool)
    .await?;

    Ok(battle)
}

/// Update existing battle state JSON and status.
pub async fn update_battle_state(
    pool: &PgPool,
    battle_id: i32,
    state: &RpgBattleState,
    status: &str,
) -> Result<(), AppError> {
    let state_json = serde_json::to_value(state)?;

    sqlx::query(
        r#"
        UPDATE rpg_battle
        SET state_json = $1,
            status = $2,
            updated_at = NOW(),
            finished_at = CASE WHEN $2 = 'finished' THEN NOW() ELSE finished_at END
        WHERE id = $3
        "#,
    )
    .bind(state_json)
    .bind(status)
    .bind(battle_id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn cleanup_old_rpg_battles(pool: &PgPool) -> Result<u64, AppError> {
    const BATCH_SIZE: i64 = 5_000;
    let mut total_deleted = 0_u64;
    loop {
        let deleted = sqlx::query(
            "WITH doomed AS (
                SELECT ctid
                FROM rpg_battle
                WHERE status IN ('finished', 'timeout')
                  AND updated_at < NOW() - INTERVAL '14 days'
                LIMIT $1
            )
            DELETE FROM rpg_battle
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

/// Tests for RPG character (player) creation.
/// Require DATABASE_URL to a running Postgres (e.g. postgresql://user:pass@localhost/postgres).
#[cfg(test)]
mod tests {
    use super::{battle_math, get_or_create_player, recalc_stamina};
    use crate::error::AppError;
    use sqlx::PgPool;
    use chrono::{TimeZone, Utc};

    /// Insert a tguser and return its id (for FK from rpg_player).
    async fn insert_tguser(pool: &PgPool, tg_id: i64) -> Result<i32, AppError> {
        let row = sqlx::query_scalar::<_, i32>(
            r#"
            INSERT INTO tguser (tg_id, username, first_name, last_name, lang_code, is_blocked, created_at, updated_at, last_seen_at)
            VALUES ($1, 'testuser', 'Test', 'User', 'en', false, NOW(), NOW(), NOW())
            ON CONFLICT (tg_id) DO UPDATE SET updated_at = NOW()
            RETURNING id
            "#,
        )
        .bind(tg_id)
        .fetch_one(pool)
        .await?;
        Ok(row)
    }

    /// First RPG action from user: character (player) must be created.
    #[sqlx::test]
    #[ignore = "requires DATABASE_URL and running Postgres"]
    async fn get_or_create_player_first_time_creates_character(pool: PgPool) -> Result<(), AppError> {
        let user_id = insert_tguser(&pool, 99_001).await?;
        let player = get_or_create_player(&pool, user_id).await?;
        assert_eq!(player.user_id, user_id);
        assert_eq!(player.level, 1);
        assert_eq!(player.xp, 0);
        assert_eq!(player.hp_current, player.hp_max);
        assert_eq!(player.pos_x, 0);
        assert_eq!(player.pos_y, 0);
        assert!(player.class_code.is_none());
        Ok(())
    }

    /// Second call for same user must return same player (no duplicate character).
    #[sqlx::test]
    #[ignore = "requires DATABASE_URL and running Postgres"]
    async fn get_or_create_player_second_call_returns_same(pool: PgPool) -> Result<(), AppError> {
        let user_id = insert_tguser(&pool, 99_002).await?;
        let first = get_or_create_player(&pool, user_id).await?;
        let second = get_or_create_player(&pool, user_id).await?;
        assert_eq!(first.id, second.id);
        assert_eq!(first.user_id, second.user_id);
        Ok(())
    }

    #[test]
    fn stamina_regeneration_works() {
        let now = Utc
            .with_ymd_and_hms(2026, 2, 19, 12, 0, 0)
            .unwrap();
        let past = Utc
            .with_ymd_and_hms(2026, 2, 19, 11, 30, 0)
            .unwrap();
        let player = super::RpgPlayer {
            id: 1,
            user_id: 1,
            level: 1,
            xp: 0,
            xp_to_next: 100,
            hp_max: 100,
            hp_current: 100,
            stamina_max: 10,
            stamina_current: 5,
            stamina_updated_at: past,
            strength: 1,
            agility: 1,
            intellect: 1,
            vitality: 1,
            luck: 1,
            pos_x: 0,
            pos_y: 0,
            class_code: None,
        };
        let (stamina, _) = recalc_stamina(&player, now);
        // 30 minutes => +0.5 * 60-second ticks, but integer division → +30 ticks.
        // However we clamp to stamina_max, so expect full.
        assert_eq!(stamina, 10);
    }

    #[test]
    fn battle_math_damage_positive() {
        // Simple sanity check to ensure damage is >= 1.
        let player = super::RpgPlayer {
            id: 1,
            user_id: 1,
            level: 1,
            xp: 0,
            xp_to_next: 100,
            hp_max: 100,
            hp_current: 100,
            stamina_max: 10,
            stamina_current: 10,
            stamina_updated_at: Utc::now(),
            strength: 5,
            agility: 1,
            intellect: 1,
            vitality: 1,
            luck: 1,
            pos_x: 0,
            pos_y: 0,
            class_code: None,
        };
        let attack = battle_math::calc_attack_phys(&player);
        let defense = battle_math::calc_defense_phys(&player);
        let (damage, _) = battle_math::roll_damage(attack, defense, 0.1, 1.5);
        assert!(damage >= 1);
    }
}

