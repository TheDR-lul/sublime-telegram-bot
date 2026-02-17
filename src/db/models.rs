//! Row types for sqlx (no ORM).

use chrono::{DateTime, Utc};
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
