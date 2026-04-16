//! Entry: clap dispatch to run / config / migrate / commands.

use clap::Parser;
use sublime::alerts;
use sublime::db::kv;
use sublime::{cli::*, config::Config, error::AppError};
use std::io::{self, Write};
use teloxide::prelude::Requester;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<(), AppError> {
    // Important: if we panic before tracing/pool init, we still want Docker logs.
    std::panic::set_hook(Box::new(|info| {
        let bt = std::backtrace::Backtrace::capture();
        eprintln!("panic (pre-init): {info}\nbacktrace:\n{bt}");
    }));

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
    let _sentry_guard = cfg.sentry_dsn.as_ref().map(|dsn| {
        let guard = sentry::init((
            dsn.clone(),
            sentry::ClientOptions {
                release: sentry::release_name!(),
                ..Default::default()
            },
        ));
        tracing::info!("Sentry initialized");
        guard
    });

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect(&cfg.database_url)
        .await?;

    let bot = teloxide::Bot::new(&cfg.telegram_token);

    let panic_pool = pool.clone();
    std::panic::set_hook(Box::new(move |info| {
        let pool = panic_pool.clone();
        let bt = std::backtrace::Backtrace::capture();
        let msg = format!("panic in main bot: {info}\nbacktrace:\n{bt}");
        eprintln!("{msg}");
        tokio::spawn(async move {
            let _ = alerts::notify(&pool, &msg).await;
        });
    }));

    let full_schema = sublime::dispatcher::build_schema();

    // Deduplication for /pidorscan to avoid duplicate messages when the same update is processed twice.
    let pidorscan_dedup = std::sync::Arc::new(sublime::dedup::PidorscanDedup::new(2));

    let rate_limiter = std::sync::Arc::new(sublime::ratelimit::RateLimiter::new(2));

    let shutdown_token = CancellationToken::new();

    // Background autorun for daily Pidor game (Kyiv timezone, 3 times per day).
    {
        let bot_clone = bot.clone();
        let pool_clone = pool.clone();
        let scheduler_shutdown = shutdown_token.clone();
        tokio::spawn(async move {
            sublime::handlers::game::commands::run_pidor_autorun_scheduler(
                bot_clone,
                pool_clone,
                scheduler_shutdown,
            )
            .await;
        });
    }
    // Cancel expired duel challenges (1 min timeout).
    {
        let bot_clone = bot.clone();
        let pool_clone = pool.clone();
        let duel_shutdown = shutdown_token.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(45));
            loop {
                tokio::select! {
                    _ = duel_shutdown.cancelled() => {
                        break;
                    }
                    _ = interval.tick() => {
                        if let Err(e) =
                            sublime::handlers::game::duel::cancel_expired_duels(&bot_clone, &pool_clone).await
                        {
                            tracing::debug!("cancel_expired_duels: {:?}", e);
                        }
                    }
                }
            }
        });
    }
    // Auto-cancel expired huya raids.
    {
        let bot_clone = bot.clone();
        let pool_clone = pool.clone();
        let raid_shutdown = shutdown_token.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(45));
            loop {
                tokio::select! {
                    _ = raid_shutdown.cancelled() => {
                        break;
                    }
                    _ = interval.tick() => {
                        if let Err(e) =
                            sublime::handlers::huya::cancel_expired_raids(&bot_clone, &pool_clone).await
                        {
                            tracing::debug!("cancel_expired_raids: {:?}", e);
                        }
                    }
                }
            }
        });
    }

    let locale = std::sync::Arc::new(sublime::i18n::Locale::new());

    let error_pool = pool.clone();
    let mut disp = teloxide::dispatching::Dispatcher::builder(bot.clone(), full_schema)
        .dependencies(teloxide::dptree::deps![pool, cfg, pidorscan_dedup, rate_limiter, locale])
        .error_handler(std::sync::Arc::new(move |err: AppError| {
            let pool = error_pool.clone();
            async move {
                tracing::error!("Handler error (command or callback failed): {:?}", err);
                let text = sublime::alerts::inject_last_context(&format!("handler error: {:?}", err));
                let _ = alerts::notify(&pool, &text).await;
            }
        }))
        .enable_ctrlc_handler()
        .build();

    tracing::info!("Bot started");
    disp.dispatch().await;
    shutdown_token.cancel();
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
        BotCommand::new("pidorduel", "challenge to pidor duel (reply for tagged)"),
        BotCommand::new("duelstats", "duel Elo leaderboard"),
        BotCommand::new("pidorbet", "bet on who will be pidor of the day"),
        BotCommand::new("pidorset", "autorun settings (admins only)"),
        BotCommand::new("lang", "set chat language (admins only), e.g. /lang ru"),
        BotCommand::new("meme", "get some random meme"),
        BotCommand::new("achievements", "show your achievements"),
        BotCommand::new("pidorscan", "scan someone with pidor-detector"),
        BotCommand::new("huya", "dick tamagotchi: status"),
        BotCommand::new("huyareg", "register to the dick game"),
        BotCommand::new("huyagrow", "grow your dick"),
        BotCommand::new("huyafight", "fight another player (@user or reply)"),
        BotCommand::new("huyasteal", "steal from another player (@user or reply)"),
        BotCommand::new("huyaraid", "raid with up to 5 players (@user or reply)"),
        BotCommand::new("huyatop", "dick leaderboard"),
        BotCommand::new("huyaskills", "skill tree for your dick"),
        BotCommand::new("huyashop", "shop: items & boosters for length"),
        BotCommand::new("huyachest", "open chests with random loot"),
        BotCommand::new("huyainv", "inventory and equipment"),
        BotCommand::new("huyapet", "pet a friend's dick (@user or reply)"),
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
        BotCommand::new("huyaenergy", "toggle Huya energy limit (on/off/status)"),
        BotCommand::new("alerts", "alerts control (me/on/off/status/test)"),
    ];
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
    println!("Watchdog commands set for @{}", me.username.as_deref().unwrap_or("bot"));
    Ok(())
}

/// Parse "docker inspect -f '{{.State.Running}}'" stdout to bool. Used by status check and tests.
fn parse_docker_inspect_running(stdout: &[u8]) -> bool {
    let binding = String::from_utf8_lossy(stdout);
    let s = binding.trim();
    s.eq_ignore_ascii_case("true")
}

/// Docker socket when running inside container with mounted docker.sock
const DOCKER_SOCKET: &str = "unix:///var/run/docker.sock";

/// Check if the main bot container is running. Uses docker ps first (reliable in watchdog container),
/// then docker inspect. Passes -H unix:///var/run/docker.sock so the CLI uses the mounted socket.
fn check_container_running(container: &str) -> bool {
    let docker_binaries = ["/usr/bin/docker", "docker"];

    let run_ps = |args: &[&str]| {
        for bin in &docker_binaries {
            let mut full_args = vec!["-H", DOCKER_SOCKET];
            full_args.extend(args.iter().copied());
            let out = std::process::Command::new(*bin).args(&full_args).output();
            if let Ok(o) = out {
                if o.status.success() && !o.stdout.is_empty() {
                    return true;
                }
            }
        }
        false
    };

    if run_ps(&["ps", "-q", "--filter", &format!("name={}", container)]) {
        return true;
    }

    for bin in &docker_binaries {
        let out = std::process::Command::new(*bin)
            .args(["-H", DOCKER_SOCKET, "inspect", "-f", "{{.State.Running}}", container])
            .output();
        if let Ok(o) = out {
            if o.status.success() {
                return parse_docker_inspect_running(&o.stdout);
            }
        }
    }
    false
}

/// Run minimal notification bot: /status (is main bot up), /stats (chats + users if DATABASE_URL set).
/// Requires NOTIFICATION_BOT_TOKEN; optional WATCHDOG_CONTAINER, DATABASE_URL for /stats.
async fn run_watchdog_bot() -> Result<(), AppError> {
    use sublime::db::huya as huya_db;
    use teloxide::payloads::{
        AnswerCallbackQuerySetters, EditMessageReplyMarkupSetters, SendMessageSetters,
    };
    use teloxide::types::{CallbackQuery, InlineKeyboardButton, InlineKeyboardMarkup, Message, ParseMode};

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
            .max_connections(5)
            .acquire_timeout(std::time::Duration::from_secs(5))
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
        let running = tokio::task::spawn_blocking(move || check_container_running(&container))
            .await
            .unwrap_or(false);
        let status_line = if running {
            "Контейнер sublime-bot: запущен."
        } else {
            "Контейнер sublime-bot: не запущен."
        };
        let note =
            "Это проверка только контейнера. Если бот не отвечает в чатах, проверяйте логи и /commands.";
        let text = format!("{}\n{}", status_line, note);
        bot.send_message(msg.chat.id, text).await?;
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

    async fn huyaenergy_handler(
        bot: teloxide::Bot,
        msg: Message,
        pool: Option<sqlx::PgPool>,
    ) -> Result<(), AppError> {
        let pool = if let Some(p) = pool {
            p
        } else {
            bot.send_message(msg.chat.id, "БД недоступна (не задан DATABASE_URL).").await?;
            return Ok(());
        };

        // Ограничим управление лимитом только алерт-чатом, если он задан.
        if let Ok(alert_chat) = std::env::var("ALERT_CHAT_ID") {
            if let Ok(alert_id) = alert_chat.parse::<i64>() {
                if msg.chat.id.0 != alert_id {
                    bot.send_message(msg.chat.id, "Эта команда доступна только в алерт-чате.").await?;
                    return Ok(());
                }
            }
        }

        let text = msg.text().unwrap_or("").trim();
        let mut parts = text.split_whitespace();
        let _cmd = parts.next();
        let arg = parts.next().unwrap_or("status");

        let current = kv::get(&pool, 0, "huya_energy_limit")
            .await?
            .map(|i| i.value)
            .unwrap_or_else(|| "1".to_string());

        match arg {
            "on" => {
                kv::set(&pool, 0, "huya_energy_limit", "1").await?;
                bot.send_message(
                    msg.chat.id,
                    "Лимит энергии для хуяки: ВКЛ (ограничение по действиям включено).",
                )
                .await?;
            }
            "off" => {
                kv::set(&pool, 0, "huya_energy_limit", "0").await?;
                bot.send_message(
                    msg.chat.id,
                    "Лимит энергии для хуяки: ВЫКЛ (действий бесконечно, растите хуяки сколько хотите).",
                )
                .await?;
            }
            _ => {
                let status = if current == "0" {
                    "сейчас: ВЫКЛ."
                } else {
                    "сейчас: ВКЛ."
                };
                let reply = format!("Лимит энергии для хуяки {}", status);
                bot.send_message(msg.chat.id, reply).await?;
            }
        }
        Ok(())
    }

    fn is_private_chat(msg: &Message) -> bool {
        msg.chat.is_private()
    }

    async fn alerts_handler(
        bot: teloxide::Bot,
        msg: Message,
        pool: Option<sqlx::PgPool>,
    ) -> Result<(), AppError> {
        let pool = if let Some(p) = pool {
            p
        } else {
            bot.send_message(msg.chat.id, "БД недоступна (не задан DATABASE_URL).").await?;
            return Ok(());
        };
        if !is_private_chat(&msg) {
            bot.send_message(msg.chat.id, "Команда доступна только в личке второго бота.").await?;
            return Ok(());
        }

        let from_id = match msg.from.as_ref() {
            Some(u) => u.id.0 as i64,
            None => {
                bot.send_message(msg.chat.id, "Не удалось определить пользователя.").await?;
                return Ok(());
            }
        };
        let text = msg.text().unwrap_or("").trim();
        let mut parts = text.split_whitespace();
        let _cmd = parts.next();
        let arg = parts.next().unwrap_or("status");

        match arg {
            "me" => {
                kv::set(&pool, 0, "watchdog_alerts_recipient_tg_id", &from_id.to_string()).await?;
                kv::set(&pool, 0, "watchdog_alerts_enabled", "1").await?;
                bot.send_message(msg.chat.id, "Алерты привязаны к тебе и включены.").await?;
            }
            "on" => {
                kv::set(&pool, 0, "watchdog_alerts_enabled", "1").await?;
                bot.send_message(msg.chat.id, "Алерты включены.").await?;
            }
            "off" => {
                kv::set(&pool, 0, "watchdog_alerts_enabled", "0").await?;
                bot.send_message(msg.chat.id, "Алерты выключены.").await?;
            }
            "test" => {
                let target = kv::get(&pool, 0, "watchdog_alerts_recipient_tg_id")
                    .await?
                    .and_then(|x| x.value.parse::<i64>().ok())
                    .unwrap_or(from_id);
                bot.send_message(
                    teloxide::types::ChatId(target),
                    "Тест алерта: доставка работает.",
                )
                .await?;
                bot.send_message(msg.chat.id, "Тест отправлен.").await?;
            }
            _ => {
                let enabled = kv::get(&pool, 0, "watchdog_alerts_enabled")
                    .await?
                    .map(|x| x.value == "1")
                    .unwrap_or(false);
                let target = kv::get(&pool, 0, "watchdog_alerts_recipient_tg_id")
                    .await?
                    .and_then(|x| x.value.parse::<i64>().ok())
                    .unwrap_or(0);
                let status = if enabled { "ВКЛ" } else { "ВЫКЛ" };
                bot.send_message(
                    msg.chat.id,
                    format!("Алерты: {status}\nПолучатель tg_id: {target}\nКоманды: /alerts me|on|off|status|test"),
                )
                .await?;
            }
        }
        Ok(())
    }

    async fn reward_error_handler(
        bot: teloxide::Bot,
        query: CallbackQuery,
        pool: Option<sqlx::PgPool>,
    ) -> Result<(), AppError> {
        let pool = if let Some(p) = pool {
            p
        } else {
            let _ = bot
                .answer_callback_query(query.id)
                .text("DATABASE_URL is not configured.")
                .await;
            return Ok(());
        };

        let Some(data) = query.data.as_deref() else {
            let _ = bot.answer_callback_query(query.id).await;
            return Ok(());
        };
        let parts: Vec<&str> = data.split(':').collect();
        if parts.len() != 3 || parts[0] != "reward_err" {
            let _ = bot.answer_callback_query(query.id).await;
            return Ok(());
        }
        let chat_id = match parts[1].parse::<i64>() {
            Ok(v) => v,
            Err(_) => {
                let _ = bot
                    .answer_callback_query(query.id)
                    .text("Invalid reward payload.")
                    .await;
                return Ok(());
            }
        };
        let target_tg_id = match parts[2].parse::<i64>() {
            Ok(v) => v,
            Err(_) => {
                let _ = bot
                    .answer_callback_query(query.id)
                    .text("Invalid reward payload.")
                    .await;
                return Ok(());
            }
        };

        let awarded = huya_db::open_chest(&pool, chat_id, target_tg_id, "daily_free_crate", true)
            .await?;
        let Some(item) = awarded else {
            let _ = bot
                .answer_callback_query(query.id)
                .text("Could not grant reward chest.")
                .await;
            return Ok(());
        };

        let text = format!(
            "<a href=\"tg://user?id={target_tg_id}\">This player</a>, the gnome dick-thieves found a dick bug and decided to reward the trigger with a chest. Reward: <b>{}</b> ({})",
            item.item_id,
            item.rarity
        );
        let announcer_bot = std::env::var("TELEGRAM_BOT_TOKEN")
            .or_else(|_| std::env::var("TELOXIDE_TOKEN"))
            .ok()
            .map(teloxide::Bot::new)
            .unwrap_or_else(|| bot.clone());

        if let Err(err) = announcer_bot
            .send_message(teloxide::types::ChatId(chat_id), text)
            .parse_mode(ParseMode::Html)
            .await
        {
            tracing::error!(
                "reward_error_handler failed to post reward message chat_id={} tg_id={} err={:?}",
                chat_id,
                target_tg_id,
                err
            );
            let _ = bot
                .answer_callback_query(query.id)
                .text("Reward granted to inventory, but message send failed in target chat.")
                .await;
            return Ok(());
        }

        if let Some(msg) = query.message.as_ref() {
            let _ = bot
                .edit_message_reply_markup(msg.chat().id, msg.id())
                .reply_markup(InlineKeyboardMarkup::new(Vec::<Vec<InlineKeyboardButton>>::new()))
                .await;
        }
        let _ = bot
            .answer_callback_query(query.id)
            .text("Reward granted.")
            .await;
        Ok(())
    }

    use teloxide::dispatching::UpdateFilterExt;
    use teloxide::types::Update;
    let container_clone = container.clone();
    let pool_for_messages = pool.clone();
    let pool_for_callbacks = pool.clone();
    let message_schema = Update::filter_message()
        .filter(|msg: Message| {
            msg.text()
                .map(|t| {
                    let t = t.trim();
                    t.starts_with("/status")
                        || t.eq_ignore_ascii_case("status")
                        || t.starts_with("/stats")
                        || t.eq_ignore_ascii_case("stats")
                        || t.starts_with("/huyaenergy")
                        || t.eq_ignore_ascii_case("huyaenergy")
                        || t.starts_with("/alerts")
                        || t.eq_ignore_ascii_case("alerts")
                })
                .unwrap_or(false)
        })
        .endpoint(move |bot: teloxide::Bot, msg: Message| {
            let text = msg.text().map(|s| s.to_string()).unwrap_or_default();
            let container = container_clone.clone();
            let pool = pool_for_messages.clone();
            async move {
                let trimmed = text.trim();
                if trimmed.starts_with("/status") || trimmed.eq_ignore_ascii_case("status") {
                    status_handler(bot, msg, container).await
                } else if trimmed.starts_with("/stats") || trimmed.eq_ignore_ascii_case("stats") {
                    stats_handler(bot, msg, pool).await
                } else if trimmed.starts_with("/alerts") || trimmed.eq_ignore_ascii_case("alerts") {
                    alerts_handler(bot, msg, pool).await
                } else {
                    huyaenergy_handler(bot, msg, pool).await
                }
            }
        });
    let callback_schema = Update::filter_callback_query().endpoint(
        move |bot: teloxide::Bot, query: CallbackQuery| {
            let pool = pool_for_callbacks.clone();
            async move { reward_error_handler(bot, query, pool).await }
        },
    );
    let schema = teloxide::dptree::entry()
        .branch(message_schema)
        .branch(callback_schema);

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
