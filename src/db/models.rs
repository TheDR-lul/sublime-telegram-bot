//! Row types for sqlx (no ORM).

use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct TgUser {
    pub id: i32,
    pub tg_id: i64,
    pub username: Option<String>,
    pub first_name: String,
    pub last_name: Option<String>,
    pub lang_code: String,
    pub is_blocked: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
}

impl TgUser {
    pub fn full_username(&self, mention: bool) -> String {
        if let Some(ref u) = self.username {
            if mention {
                format!("@{}", u)
            } else {
                u.clone()
            }
        } else {
            self.last_name
                .as_ref()
                .map(|l| format!("{} {}", self.first_name, l))
                .unwrap_or_else(|| self.first_name.clone())
        }
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Game {
    pub id: i32,
    pub chat_id: i64,
    pub autorun_enabled: bool,
    pub autorun_morning: bool,
    pub autorun_day: bool,
    pub autorun_evening: bool,
    pub lang: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct GamePlayer {
    pub game_id: i32,
    pub user_id: i32,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct GameResult {
    pub id: i32,
    pub game_id: i32,
    pub winner_id: i32,
    pub year: i32,
    pub day: i32,
    pub slot: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TiktokLink {
    pub id: i32,
    pub link: String,
    pub share_link: Option<String>,
    pub telegram_message_id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Row for stats: TgUser columns + win count. Use for stats_current_year, stats_all_time, stats_personal, stats_year.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UserWithCount {
    pub id: i32,
    pub tg_id: i64,
    pub username: Option<String>,
    pub first_name: String,
    pub last_name: Option<String>,
    pub lang_code: String,
    pub is_blocked: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub count: i64,
}

impl UserWithCount {
    pub fn to_tg_user(&self) -> TgUser {
        TgUser {
            id: self.id,
            tg_id: self.tg_id,
            username: self.username.clone(),
            first_name: self.first_name.clone(),
            last_name: self.last_name.clone(),
            lang_code: self.lang_code.clone(),
            is_blocked: self.is_blocked,
            created_at: self.created_at,
            updated_at: self.updated_at,
            last_seen_at: self.last_seen_at,
        }
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct KvItem {
    pub id: i32,
    pub chat_id: i64,
    pub key: String,
    pub value: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Achievement {
    pub id: i32,
    pub user_id: i32,
    pub code: String,
    pub earned_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DuelGame {
    pub id: i32,
    pub chat_id: i64,
    pub invite_message_id: Option<i64>,
    pub message_id: Option<i64>,
    pub challenger_tg_id: i64,
    pub invited_tg_id: Option<i64>,
    pub player1_tg_id: Option<i64>,
    pub player2_tg_id: Option<i64>,
    pub board: String,
    pub cell_filled_at: String,
    pub turn: i16,
    pub status: String,
    pub winner_tg_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub last_move_at: Option<DateTime<Utc>>,
    pub game_type: String,
    pub game_state: Option<serde_json::Value>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DuelElo {
    pub id: i32,
    pub chat_id: i64,
    pub tg_id: i64,
    pub elo: i32,
    pub wins: i32,
    pub losses: i32,
    pub pidor_elo: i32,
    pub huya_elo: i32,
}

impl DuelElo {
    pub fn total_elo(&self) -> i32 {
        self.elo + self.pidor_elo + self.huya_elo
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Huya {
    pub id: i32,
    pub chat_id: i64,
    pub tg_id: i64,
    pub length_mm: i32,
    pub level: i32,
    pub xp: i32,
    pub actions_left: i32,
    pub actions_reset_at: NaiveDate,
    pub created_at: DateTime<Utc>,
    // HP system
    pub hp: i32,
    // Skill points pool
    pub skill_points: i32,
    // Tier 1 — base skills (cap 20, cost 1 SP)
    pub skill_shaft:   i32, // ATK +4%/lv
    pub skill_skin:    i32, // DEF -3%/lv
    pub skill_balls:   i32, // maxHP +15/lv
    pub skill_cunning: i32, // steal chance +2.5%/lv
    pub skill_stamina: i32, // HP regen +3/action/lv
    // Tier 2 — specialisation (cap 15, cost 2 SP; unlock T1 >= 8)
    pub skill_pierce:     i32, // ignore 4% enemy DEF/lv  (shaft>=8)
    pub skill_scales:     i32, // -2.5% steal-vs-you/lv   (skin>=8)
    pub skill_spirit:     i32, // +12 HP on round win/lv  (balls>=8)
    pub skill_pickpocket: i32, // steal takes 3% XP/lv    (cunning>=8)
    pub skill_dynamo:     i32, // +1 max action per 5 lv  (stamina>=8)
    // Tier 3 — cross-branch combos (cap 10, cost 3 SP; require 2x T2 >= 5)
    pub skill_eggtwist:    i32, // round 3 deals x2 dmg     (pierce+spirit>=5)
    pub skill_bloodsucker: i32, // fight win = steal length  (pierce+pickpocket>=5)
    pub skill_ironballs:   i32, // counter on dodge          (scales+spirit>=5)
    pub skill_vortex:      i32, // round 1 always crits      (spirit+dynamo>=5)
    pub skill_phantom:     i32, // 1 steal even at 0 actions (pickpocket+scales>=5)
    // Tier 4 — hidden until T3 parent >= 7 (cap 5, cost 5 SP)
    pub skill_berserker: i32, // hp<30% → ATK x2           (eggtwist>=7)
    pub skill_vampire:   i32, // win heals from enemy HP    (bloodsucker>=7)
    pub skill_fortress:  i32, // can't go below 1cm         (ironballs>=7)
    pub skill_speedrun:  i32, // fights resolve in 1 round  (vortex>=7)
    pub skill_ghost:     i32, // 30% dodge steal flat       (phantom>=7)
    // Tier 5 — legendary, shown as ??? until prereqs (cap 3, cost 7 SP)
    pub skill_eternal:  i32, // +15% everything/lv         (berserker+fortress>=3)
    pub skill_absolute: i32, // +25% everything + title    (all T4>=1)
    // Fight stats
    pub fights_won:  i32,
    pub fights_lost: i32,
    // Temporary shop boosts (reset after use)
    pub atk_boost:  i32,
    pub def_boost:  i32,
    pub grow_boost: i32,
    pub steal_boost: i32,
    // Pet energy (for /huyapet friend petting)
    pub pet_energy_left: i32,
    pub pet_energy_reset_at: NaiveDate,
    pub energy_buys_today: i32,
    pub energy_buys_reset_at: NaiveDate,
}

impl Huya {
    /// Display length in cm with one decimal place.
    pub fn display_cm(&self) -> String {
        let abs = self.length_mm.unsigned_abs();
        format!("{}.{}", abs / 10, abs % 10)
    }

    pub fn is_pussy(&self) -> bool {
        self.length_mm < 0
    }

    pub fn is_pizdyaka(&self) -> bool {
        self.is_pussy()
    }

    pub fn is_ass(&self) -> bool {
        self.is_pussy()
    }

    /// Maximum HP scales with build size and defensive skills.
    /// This reduces one-shot risk in both normal fights and raids.
    pub fn max_hp(&self) -> i32 {
        if self.is_pussy() {
            // Pussy mode should not lose survivability while reducing depth.
            // HP here scales mostly from level/defensive build instead of raw depth.
            let base = 130
                + self.level.max(1) * 4
                + self.skill_balls * 15
                + self.skill_skin * 4
                + self.skill_scales * 4;
            let eternal_mult = 1.0 + self.skill_eternal as f64 * 0.15;
            return (base as f64 * eternal_mult) as i32;
        }
        let length_hp = self.length_mm.abs() / 5;
        let base = 100 + length_hp + self.skill_balls * 15;
        let eternal_mult = 1.0 + self.skill_eternal as f64 * 0.15;
        (base as f64 * eternal_mult) as i32
    }

    /// Maximum daily actions: base 4 + 1 per 5 levels of skill_dynamo.
    /// Dynamo slowly increases the cap for активные задроты.
    pub fn max_actions(&self) -> i32 {
        4 + self.skill_dynamo / 5
    }

    /// HP as a visual bar of 10 characters (█ filled, ░ empty).
    pub fn hp_bar(&self) -> String {
        let max = self.max_hp().max(1);
        let filled = ((self.hp.max(0) as f64 / max as f64) * 10.0).round() as usize;
        let filled = filled.min(10);
        format!("[{}{}]", "█".repeat(filled), "░".repeat(10 - filled))
    }

    /// Star display for small caps (e.g. toast): "★★★☆☆".
    pub fn skill_stars(level: i32, cap: i32) -> String {
        let cap_u = cap.max(1).min(10) as usize;
        let filled = level.max(0).min(cap) as usize;
        let filled = filled.min(cap_u);
        let empty = cap_u.saturating_sub(filled);
        format!("{}{}", "★".repeat(filled), "☆".repeat(empty))
    }

    /// Progress bar for a skill: `lv/cap  ████░░░░░░`.
    pub fn skill_bar(level: i32, cap: i32) -> String {
        let filled = if cap > 0 {
            ((level as f64 / cap as f64) * 10.0).round() as usize
        } else {
            0
        }
        .min(10);
        format!("{}/{}\t{}{}", level, cap, "█".repeat(filled), "░".repeat(10 - filled))
    }

    /// True when a Tier 4 skill is visible (T3 prereq reached).
    pub fn t4_visible(&self, skill: &str) -> bool {
        match skill {
            "berserker" => self.skill_eggtwist >= 7,
            "vampire"   => self.skill_bloodsucker >= 7,
            "fortress"  => self.skill_ironballs >= 7,
            "speedrun"  => self.skill_vortex >= 7,
            "ghost"     => self.skill_phantom >= 7,
            _ => false,
        }
    }

    /// True when skill_eternal is visible (berserker+fortress >= 3 each).
    pub fn eternal_visible(&self) -> bool {
        self.skill_berserker >= 3 && self.skill_fortress >= 3
    }

    /// True when skill_absolute is visible (all T4 skills >= 1).
    pub fn absolute_visible(&self) -> bool {
        self.skill_berserker >= 1
            && self.skill_vampire >= 1
            && self.skill_fortress >= 1
            && self.skill_speedrun >= 1
            && self.skill_ghost >= 1
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct HuyaInventoryItem {
    pub id: i32,
    pub chat_id: i64,
    pub tg_id: i64,
    pub item_id: String,
    pub rarity: String,
    pub item_kind: String,
    pub slot: Option<String>,
    pub trait_name: Option<String>,
    pub roll: i32,
    pub charges: i32,
    pub sell_price_mm: i32,
    pub booster_effect: Option<String>,
    pub booster_value: i32,
    pub booster_scope: Option<String>,
    pub socket_capacity: i32,
    pub reforge_level: i32,
    pub acquired_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct HuyaSocketedGem {
    pub id: i32,
    pub chat_id: i64,
    pub tg_id: i64,
    pub item_inventory_id: i32,
    pub socket_index: i32,
    pub gem_item_id: String,
    pub gem_trait: Option<String>,
    pub gem_roll: i32,
    pub gem_rarity: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct HuyaEquipmentSlot {
    pub chat_id: i64,
    pub tg_id: i64,
    pub slot: String,
    pub inventory_id: i32,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PidorBet {
    pub id: i32,
    pub chat_id: i64,
    pub bettor_tg_id: i64,
    pub target_tg_id: i64,
    pub year: i32,
    pub day: i32,
    pub slot: String,
    pub correct: Option<bool>,
    pub created_at: DateTime<Utc>,
}
