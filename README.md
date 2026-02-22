# Sublime Telegram Bot (Rust)

Telegram bot written in Rust using [teloxide](https://docs.rs/teloxide/) framework.

## Features

- **About** - `/about` command with GitHub repository link
- **Misc commands** - `/hello`, `/echo`, `/slap`, `/me`, `/shrug`, `/google`, `/pin` + inline queries (echo/caps/shuffle)
- **Key-Value store** - `/get`, `/list`, `/set`, `/del` commands for chat-specific key-value storage
- **Pidor of the Day game** - `/pidor`, `/pidoreg`, `/pidorstats`, `/pidorall`, `/pidorme`, `/pidorYYYY` commands with daily draws (Moscow timezone)
- **Meme commands** - `/meme` (English memes from imgflip/Telegram) and `/memeru` (Russian memes from configurable Telegram channels)
- **TikTok integration** - `/ttvideo`, `/ttlink` commands and inline queries with video caching

## Prerequisites

- Rust (install via [rustup](https://rustup.rs/))
- PostgreSQL database (local or remote)
- `yt-dlp` binary in PATH (for TikTok commands, optional)

## Installation

1. Clone the repository:
   ```bash
   git clone https://github.com/TheDR-lul/sublime.git
   cd sublime
   ```

2. Configure the bot:
   ```bash
   # Option 1: Use CLI to create config interactively
   cargo run -- config init
   
   # Option 2: Create config.toml manually (see .env.example for reference)
   # Option 3: Use environment variables (see .env.example)
   ```

3. Apply database migrations:
   ```bash
   cargo run -- migrate
   ```

4. Set bot commands (optional):
   ```bash
   cargo run -- commands set
   ```

## Usage

### Development

```bash
# Run the bot
cargo run

# Or explicitly
cargo run -- run
```

### Production

1. Build release binary:
   ```bash
   cargo build --release
   ```

2. The binary will be at `target/release/sublime`

3. Run migrations:
   ```bash
   ./target/release/sublime migrate
   ```

4. Set bot commands:
   ```bash
   ./target/release/sublime commands set
   ```

5. Run the bot:
   ```bash
   ./target/release/sublime run
   ```

### Systemd Service

See `ansible/templates/sublime-bot.service.j2` for a systemd service file template.

## Configuration

Configuration can be provided via:
1. Command-line argument: `--config /path/to/config.toml`
2. Config file: `config.toml` in current directory or `~/.config/sublime-bot/config.toml`
3. Environment variables (see `.env.example`)

### Config File Format (TOML)

```toml
telegram_token = "YOUR_BOT_TOKEN"
database_url = "postgresql://user:password@localhost/dbname"
sentry_dsn = "https://..."  # Optional
tiktok_cache_chat_id = -1001234567890  # Optional, for TikTok inline caching

# Optional: Russian meme channels
[[meme_ru_channels]]
url = "https://t.me/channel"
start_id = 1
end_id = 1000
```

## CLI Commands

- `sublime run` (or just `sublime`) - Run the bot
- `sublime config init` - Interactive config creation
- `sublime config set <KEY> <value>` - Set a config value
- `sublime config show` - Show current config (secrets masked)
- `sublime config path` - Print config file path
- `sublime migrate` - Apply database migrations
- `sublime commands set` - Register bot commands in Telegram

## Development

### Project Structure

```
sublime/
├── Cargo.toml
├── migrations/          # SQLx migrations
│   └── *.sql
├── src/
│   ├── main.rs         # Entry point, CLI dispatch
│   ├── lib.rs          # Library exports
│   ├── config.rs       # Configuration loading
│   ├── cli.rs          # CLI argument parsing
│   ├── error.rs        # Error types
│   ├── handlers/       # Bot command handlers
│   │   ├── about.rs
│   │   ├── misc.rs
│   │   ├── kvstore.rs
│   │   ├── game/
│   │   ├── meme.rs
│   │   └── tiktok.rs
│   └── db/             # Database layer
│       ├── models.rs
│       ├── user.rs
│       ├── game.rs
│       ├── kv.rs
│       └── tiktok.rs
└── ansible/            # Deployment automation
```

### Running Tests

```bash
cargo test
```

### Code Formatting

```bash
cargo fmt
```

### Linting

```bash
cargo clippy
```

## Deployment

### Using Ansible

See `ansible/deploy-playbook.yml` for deployment automation.

### Manual Deployment

1. Build release binary on CI or locally
2. Copy binary to server
3. Create systemd service (see `ansible/templates/sublime-bot.service.j2`)
4. Configure and run migrations
5. Start the service

## Monitoring

To get **alerts when the bot crashes** on a remote server (e.g. Telegram message when the container is down), see [docs/MONITORING.md](docs/MONITORING.md): watchdog script + optional Sentry for errors.

## Troubleshooting

- **Messages show as "Group Anonymous Bot" instead of channel name**  
  This is a Telegram setting: the bot is added as an "Anonymous" admin. To show the channel name, add the bot as a normal (non-anonymous) admin, or link the group to the channel and give the bot permission to post as the channel in the group's settings.

## License

MIT

## Credits

Original Python implementation by [unicott](http://unicott.com/).  
Migrated to Rust by [TheDR-lul](https://github.com/TheDR-lul).
