//! Application error type and global handler behavior.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),

    #[error("migrate error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),

    #[error("telegram error: {0}")]
    Telegram(#[from] teloxide::RequestError),

    #[error("http error: {0}")]
    Reqwest(#[from] reqwest::Error),

    #[error("config error: {0}")]
    Config(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("game logic error: {0}")]
    GameLogic(String),

    #[error("yt-dlp error: {0}")]
    YtDlp(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("url parse error: {0}")]
    UrlParse(#[from] url::ParseError),

    #[error("timeout error: {0}")]
    Timeout(#[from] tokio::time::error::Elapsed),

    #[error("serialization error: {0}")]
    SerdeJson(#[from] serde_json::Error),
}
