//! CLI: run, config (init/set/show/path), migrate, commands set.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "sublime")]
#[command(about = "Telegram bot")]
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
