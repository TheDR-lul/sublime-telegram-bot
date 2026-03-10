//! Config load: TOML file + env (env overrides file). No .env required if using config file.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub telegram_token: String,
    pub database_url: String,
    #[serde(default)]
    pub sentry_dsn: Option<String>,
    // Keep space for future config fields; currently no extra options needed here.
}

impl Config {
    /// Load config: 1) --config path if given, 2) config.toml in CWD, 3) ~/.config/sublime-bot/config.toml, 4) env vars (TELEGRAM_BOT_API_SECRET or TELOXIDE_TOKEN, DATABASE_URL, etc.)
    pub fn load(config_path: Option<PathBuf>) -> Result<Self, crate::error::AppError> {
        dotenvy::dotenv().ok();

        let from_env = (
            std::env::var("TELEGRAM_BOT_API_SECRET").or_else(|_| std::env::var("TELOXIDE_TOKEN")),
            std::env::var("DATABASE_URL"),
            std::env::var("SENTRY_DSN").ok(),
            (),
        );

        let path = config_path
            .or_else(|| std::env::current_dir().ok().map(|p| p.join("config.toml")))
            .or_else(|| {
                dirs::config_dir().map(|d| d.join("sublime-bot").join("config.toml"))
            });

        let mut cfg: Option<Config> = path
            .as_ref()
            .filter(|p| p.exists())
            .and_then(|p| {
                let s = std::fs::read_to_string(p).ok()?;
                toml::from_str(&s).ok()
            });

        if cfg.is_none() {
            let (token, db, sentry, _dummy) = &from_env;
            if let (Ok(t), Ok(d)) = (token, db) {
                cfg = Some(Config {
                    telegram_token: t.clone(),
                    database_url: d.clone(),
                    sentry_dsn: sentry.clone(),
                });
            }
        }

        let mut c = cfg.ok_or_else(|| {
            crate::error::AppError::Config(
                "Missing config: set TELEGRAM_BOT_API_SECRET and DATABASE_URL, or use config.toml / sublime config init".into(),
            )
        })?;

        if let Ok(t) = std::env::var("TELEGRAM_BOT_API_SECRET").or_else(|_| std::env::var("TELOXIDE_TOKEN")) {
            c.telegram_token = t;
        }
        if let Ok(d) = std::env::var("DATABASE_URL") {
            c.database_url = d;
        }
        if let Ok(s) = std::env::var("SENTRY_DSN") {
            c.sentry_dsn = Some(s);
        }

        Ok(c)
    }

    /// Path to config file if one was found when loading.
    pub fn config_path() -> Option<PathBuf> {
        std::env::current_dir()
            .ok()
            .map(|p| p.join("config.toml"))
            .filter(|p| p.exists())
            .or_else(|| {
                dirs::config_dir()
                    .map(|d| d.join("sublime-bot").join("config.toml"))
                    .filter(|p| p.exists())
            })
    }
}
