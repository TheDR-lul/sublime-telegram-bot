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
    use teloxide::types::BotCommand;
    let commands = [
        BotCommand::new("slap", "simulate /slap command from IRC"),
        BotCommand::new("me", "simulate /me command from IRC"),
        BotCommand::new("shrug", "shrug ¯\\_(ツ)_/¯"),
        BotCommand::new("google", "<query> let me google that for you"),
        BotCommand::new("get", "<key> get specific entry by key"),
        BotCommand::new("list", "list entries for current chat"),
        BotCommand::new("set", "<key> <value> set specific value for key"),
        BotCommand::new("del", "<key> remove specific key"),
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
        BotCommand::new("about", "some info about github repo"),
        BotCommand::new("achievements", "show your achievements"),
        BotCommand::new("pidorscan", "scan someone with pidor-detector"),
    ];
    let bot = teloxide::Bot::new(&cfg.telegram_token);
    bot.set_my_commands(commands).await?;
    let me = bot.get_me().await?;
    println!("Updated commands for @{}", me.username.as_deref().unwrap_or("bot"));
    Ok(())
}
