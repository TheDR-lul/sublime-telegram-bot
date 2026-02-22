//! Entry: clap dispatch to run / config / migrate / commands.

use clap::Parser;
use sublime::{cli::*, config::Config, error::AppError};
use std::io::{self, Write};
use teloxide::prelude::Requester;

#[tokio::main]
async fn main() -> Result<(), AppError> {
    let cli = Cli::parse();

    match cli.subcommand {
        None => run_bot(cli.config).await,
        Some(Cmd::Run) => run_bot(cli.config).await,
        Some(Cmd::Config(c)) => run_config(c, cli.config),
        Some(Cmd::Migrate) => run_migrate(cli.config).await,
        Some(Cmd::Commands(CommandsCmd::Set)) => run_commands_set(cli.config).await,
        Some(Cmd::Watchdog(WatchdogCmd::Run)) => run_watchdog_bot().await,
        Some(Cmd::Watchdog(WatchdogCmd::Commands)) => run_watchdog_commands_set().await,
    }
}

async fn run_bot(config_path: Option<std::path::PathBuf>) -> Result<(), AppError> {
    let cfg = Config::load(config_path)?;
    
    #[cfg(feature = "sentry")]
    if let Some(ref dsn) = cfg.sentry_dsn {
        let _guard = sentry::init((
            dsn.clone(),
            sentry::ClientOptions {
                release: sentry::release_name!(),
                ..Default::default()
            },
        ));
        tracing::info!("Sentry initialized");
    }
    
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect(&cfg.database_url)
        .await?;

    let bot = teloxide::Bot::new(&cfg.telegram_token);

    let full_schema = sublime::dispatcher::build_schema();

    // Background autorun for daily Pidor game (Kyiv timezone, 3 times per day).
    {
        let bot_clone = bot.clone();
        let pool_clone = pool.clone();
        tokio::spawn(async move {
            sublime::handlers::game::commands::run_pidor_autorun_scheduler(bot_clone, pool_clone)
                .await;
        });
    }

    let mut disp = teloxide::dispatching::Dispatcher::builder(bot.clone(), full_schema)
        .dependencies(teloxide::dptree::deps![pool, cfg])
        .error_handler(teloxide::error_handlers::LoggingErrorHandler::with_custom_text(
            "Handler error (command or callback failed)",
        ))
        .enable_ctrlc_handler()
        .build();

    tracing::info!("Bot started");
    disp.dispatch().await;
    Ok(())
}

fn run_config(cmd: ConfigCmd, config_path: Option<std::path::PathBuf>) -> Result<(), AppError> {
    match cmd {
        ConfigCmd::Init => {
            let path = config_path
                .or_else(|| std::env::current_dir().ok().map(|p| p.join("config.toml")))
                .unwrap_or_else(|| std::path::PathBuf::from("config.toml"));
            let mut token = String::new();
            let mut database_url = String::new();
            print!("Telegram bot token: ");
            io::stdout().flush()?;
            io::stdin().read_line(&mut token)?;
            print!("DATABASE_URL (e.g. postgresql://user:pass@localhost/db): ");
            io::stdout().flush()?;
            io::stdin().read_line(&mut database_url)?;
            let cfg = Config {
                telegram_token: token.trim().to_string(),
                database_url: database_url.trim().to_string(),
                sentry_dsn: None,
                tiktok_cache_chat_id: None,
                meme_ru_channels: vec![],
            };
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let toml = toml::to_string_pretty(&cfg).map_err(|e| AppError::Config(e.to_string()))?;
            std::fs::write(&path, toml)?;
            println!("Wrote {}", path.display());
        }
        ConfigCmd::Set { key, value } => {
            let path = config_path
                .or_else(|| std::env::current_dir().ok().map(|p| p.join("config.toml")))
                .or_else(|| dirs::config_dir().map(|d| d.join("sublime-bot").join("config.toml")));
            let path = path.ok_or_else(|| AppError::Config("No config path".into()))?;
            let mut cfg: Config = if path.exists() {
                let s = std::fs::read_to_string(&path)?;
                toml::from_str(&s).map_err(|e| AppError::Config(e.to_string()))?
            } else {
                return Err(AppError::Config("Config file does not exist, use: sublime config init".into()));
            };
            match key.as_str() {
                "telegram_token" => cfg.telegram_token = value,
                "database_url" => cfg.database_url = value,
                "sentry_dsn" => cfg.sentry_dsn = if value.is_empty() { None } else { Some(value) },
                "tiktok_cache_chat_id" => cfg.tiktok_cache_chat_id = value.parse().ok(),
                _ => return Err(AppError::Config(format!("Unknown key: {}", key))),
            }
            let toml = toml::to_string_pretty(&cfg).map_err(|e| AppError::Config(e.to_string()))?;
            if let Some(p) = path.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::fs::write(&path, toml)?;
            println!("Updated {} in {}", key, path.display());
        }
        ConfigCmd::Show => {
            let cfg = Config::load(config_path)?;
            println!("telegram_token: {}...", mask(&cfg.telegram_token));
            println!("database_url: {}...", mask(&cfg.database_url));
            println!("sentry_dsn: {:?}", cfg.sentry_dsn.as_ref().map(|s| mask(s)));
            println!("tiktok_cache_chat_id: {:?}", cfg.tiktok_cache_chat_id);
        }
        ConfigCmd::Path => {
            let path = Config::config_path()
                .or_else(|| std::env::current_dir().ok().map(|p| p.join("config.toml")))
                .or_else(|| dirs::config_dir().map(|d| d.join("sublime-bot").join("config.toml")));
            match path {
                Some(p) => println!("{}", p.display()),
                None => println!("No config file found (use sublime config init)"),
            }
        }
    }
    Ok(())
}

fn mask(s: &str) -> String {
    if s.len() <= 8 {
        "*".repeat(s.len())
    } else {
        format!("{}***{}", &s[..4], &s[s.len() - 4..])
    }
}

async fn run_migrate(config_path: Option<std::path::PathBuf>) -> Result<(), AppError> {
    let cfg = Config::load(config_path)?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect(&cfg.database_url)
        .await?;
    let migrations_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let migrator = sqlx::migrate::Migrator::new(migrations_dir).await?;
    migrator.run(&pool).await?;
    println!("Migrations applied.");
    Ok(())
}

async fn run_commands_set(config_path: Option<std::path::PathBuf>) -> Result<(), AppError> {
    let cfg = Config::load(config_path)?;
    use teloxide::payloads::SetMyCommandsSetters;
    use teloxide::types::{BotCommand, BotCommandScope};
    let commands = [
        BotCommand::new("menu", "menu with sections"),
        BotCommand::new("about", "about bot and repo"),
        BotCommand::new("slap", "slap someone by replying to their message"),
        BotCommand::new("me", "simulate /me command from IRC"),
        BotCommand::new("shrug", "shrug ¯\\_(ツ)_/¯"),
        BotCommand::new("google", "<query> let me google that for you"),
        BotCommand::new("pidor", "play the game, see /pidorules first"),
        BotCommand::new("pidorules", "POTD game rules"),
        BotCommand::new("pidoreg", "register to the POTD game"),
        BotCommand::new("pidorunreg", "unregister from the POTD game"),
        BotCommand::new("pidorstats", "POTD game stats for this year"),
        BotCommand::new("pidorall", "POTD game stats for all time"),
        BotCommand::new("pidorme", "POTD personal stats"),
        BotCommand::new("meme", "get some random meme"),
        BotCommand::new("memeru", "get some random russian meme"),
        BotCommand::new("ttvideo", "get video from tiktok"),
        BotCommand::new("ttlink", "get depersonalized tiktok link"),
        BotCommand::new("achievements", "show your achievements"),
        BotCommand::new("pidorscan", "scan someone with pidor-detector"),
    ];
    let bot = teloxide::Bot::new(&cfg.telegram_token);
    // Set same commands for default (fallback), all private chats, and all group/supergroup chats.
    bot.set_my_commands(commands.clone())
        .scope(BotCommandScope::Default)
        .await?;
    bot.set_my_commands(commands.clone())
        .scope(BotCommandScope::AllPrivateChats)
        .await?;
    bot.set_my_commands(commands.clone())
        .scope(BotCommandScope::AllGroupChats)
        .await?;
    let me = bot.get_me().await?;
    println!("Updated commands for @{} (default, private, group chats)", me.username.as_deref().unwrap_or("bot"));
    Ok(())
}

/// Set notification bot menu commands only (/status, /stats). Use NOTIFICATION_BOT_TOKEN.
async fn run_watchdog_commands_set() -> Result<(), AppError> {
    use teloxide::payloads::SetMyCommandsSetters;
    use teloxide::types::{BotCommand, BotCommandScope};

    let token = std::env::var("NOTIFICATION_BOT_TOKEN")
        .map_err(|_| AppError::Config("NOTIFICATION_BOT_TOKEN required".into()))?;
    let bot = teloxide::Bot::new(&token);
    let commands = [
        BotCommand::new("status", "is main bot running"),
        BotCommand::new("stats", "chats and users count"),
    ];
    bot.set_my_commands(commands.clone())
        .scope(BotCommandScope::Default)
        .await?;
    bot.set_my_commands(commands.clone())
        .scope(BotCommandScope::AllPrivateChats)
        .await?;
    let me = bot.get_me().await?;
    println!("Watchdog commands set for @{}", me.username.as_deref().unwrap_or("bot"));
    Ok(())
}

/// Parse "docker inspect -f '{{.State.Running}}'" stdout to bool. Used by status check and tests.
fn parse_docker_inspect_running(stdout: &[u8]) -> bool {
    let binding = String::from_utf8_lossy(stdout);
    let s = binding.trim();
    s.eq_ignore_ascii_case("true")
}

/// Check if the main bot container is running. Tries docker inspect first, then docker ps as fallback.
/// Uses /usr/bin/docker when "docker" is not in PATH (e.g. in minimal container).
fn check_container_running(container: &str) -> bool {
    let docker_binaries = ["/usr/bin/docker", "docker"];
    for bin in &docker_binaries {
        let out = std::process::Command::new(*bin)
            .args(["inspect", "-f", "{{.State.Running}}", container])
            .output();
        match out {
            Ok(o) if o.status.success() => {
                if parse_docker_inspect_running(&o.stdout) {
                    return true;
                }
                return false;
            }
            Ok(o) => {
                tracing::debug!(
                    "docker inspect failed ({}): stderr={}",
                    o.status,
                    String::from_utf8_lossy(&o.stderr)
                );
            }
            Err(e) => {
                tracing::debug!("docker inspect command failed: {:?}", e);
            }
        }
    }
    // Fallback: docker ps -q --filter name=CONTAINER (match by name substring)
    for bin in &docker_binaries {
        let out = std::process::Command::new(*bin)
            .args(["ps", "-q", "--filter", &format!("name={}", container)])
            .output();
        if let Ok(o) = out {
            if o.status.success() && !o.stdout.is_empty() {
                return true;
            }
        }
    }
    false
}

/// Run minimal notification bot: /status (is main bot up), /stats (chats + users if DATABASE_URL set).
/// Requires NOTIFICATION_BOT_TOKEN; optional WATCHDOG_CONTAINER, DATABASE_URL for /stats.
async fn run_watchdog_bot() -> Result<(), AppError> {
    use teloxide::types::Message;

    let token = std::env::var("NOTIFICATION_BOT_TOKEN")
        .map_err(|_| AppError::Config("NOTIFICATION_BOT_TOKEN required for watchdog".into()))?;
    let container = std::env::var("WATCHDOG_CONTAINER").unwrap_or_else(|_| "sublime-bot".to_string());
    let database_url = std::env::var("DATABASE_URL").ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let bot = teloxide::Bot::new(&token);

    // Set menu commands for this bot
    if let Err(e) = run_watchdog_commands_set().await {
        tracing::warn!("Watchdog set_my_commands failed: {:?}", e);
    }

    let pool = if let Some(ref url) = database_url {
        match sqlx::postgres::PgPoolOptions::new()
            .connect(url)
            .await
        {
            Ok(p) => Some(p),
            Err(e) => {
                tracing::warn!("Watchdog DATABASE_URL connect failed: {:?}", e);
                None
            }
        }
    } else {
        None
    };

    async fn status_handler(bot: teloxide::Bot, msg: Message, container: String) -> Result<(), AppError> {
        let running = check_container_running(&container);
        let status = if running { "Бот работает." } else { "Бот не запущен." };
        bot.send_message(msg.chat.id, status).await?;
        Ok(())
    }

    async fn stats_handler(
        bot: teloxide::Bot,
        msg: Message,
        pool: Option<sqlx::PgPool>,
    ) -> Result<(), AppError> {
        let text = if let Some(ref pool) = pool {
            let chats: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM game")
                .fetch_one(pool)
                .await
                .unwrap_or((0,));
            let users: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM tguser")
                .fetch_one(pool)
                .await
                .unwrap_or((0,));
            format!(
                "Чатов с ботом: {}\nУникальных пользователей: {}",
                chats.0, users.0
            )
        } else {
            "Статистика недоступна (не задан DATABASE_URL).".to_string()
        };
        bot.send_message(msg.chat.id, text).await?;
        Ok(())
    }

    use teloxide::dispatching::UpdateFilterExt;
    use teloxide::types::Update;
    let container_clone = container.clone();
    let pool_clone = pool.clone();
    let schema = Update::filter_message()
        .filter(|msg: Message| {
            msg.text()
                .map(|t| {
                    let t = t.trim();
                    t.starts_with("/status") || t.eq_ignore_ascii_case("status")
                        || t.starts_with("/stats") || t.eq_ignore_ascii_case("stats")
                })
                .unwrap_or(false)
        })
        .endpoint(move |bot: teloxide::Bot, msg: Message| {
            let text = msg.text().map(|s| s.to_string()).unwrap_or_default();
            let container = container_clone.clone();
            let pool = pool_clone.clone();
            async move {
                if text.trim().starts_with("/status") || text.trim().eq_ignore_ascii_case("status") {
                    status_handler(bot, msg, container).await
                } else {
                    stats_handler(bot, msg, pool).await
                }
            }
        });

    let mut disp = teloxide::dispatching::Dispatcher::builder(bot, schema).build();
    tracing::info!("Watchdog bot started (/status, /stats)");
    disp.dispatch().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_docker_inspect_running;

    #[test]
    fn test_parse_docker_inspect_running() {
        assert!(parse_docker_inspect_running(b"true"));
        assert!(parse_docker_inspect_running(b"true\n"));
        assert!(parse_docker_inspect_running(b"  true  \n"));
        assert!(parse_docker_inspect_running(b"TRUE"));
        assert!(!parse_docker_inspect_running(b"false"));
        assert!(!parse_docker_inspect_running(b"false\n"));
        assert!(!parse_docker_inspect_running(b""));
        assert!(!parse_docker_inspect_running(b"something"));
    }
}
