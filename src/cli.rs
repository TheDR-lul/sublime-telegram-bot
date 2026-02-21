//! CLI: run, config (init/set/show/path), migrate, commands set.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "sublime")]
#[command(about = "Telegram bot")]
#[command(version = env!("CARGO_PKG_VERSION"))]
pub struct Cli {
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub subcommand: Option<Cmd>,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// Run the bot (default)
    Run,

    #[command(subcommand)]
    Config(ConfigCmd),

    /// Apply database migrations and exit
    Migrate,

    /// Set bot menu commands (set_my_commands) and exit
    #[command(subcommand)]
    Commands(CommandsCmd),

    /// Run notification bot: /status, /stats. Use NOTIFICATION_BOT_TOKEN on server.
    #[command(subcommand)]
    Watchdog(WatchdogCmd),
}

#[derive(Subcommand)]
pub enum WatchdogCmd {
    /// Run the notification bot (status + stats)
    Run,
    /// Set notification bot menu commands only (status, stats) and exit
    Commands,
}

#[derive(Subcommand)]
pub enum ConfigCmd {
    /// Interactive config creation (config.toml)
    Init,

    /// Set one key (e.g. telegram_token, database_url)
    Set { key: String, value: String },

    /// Show current config (secrets masked)
    Show,

    /// Print config file path
    Path,
}

#[derive(Subcommand)]
pub enum CommandsCmd {
    /// Register bot commands in Telegram
    Set,
}
