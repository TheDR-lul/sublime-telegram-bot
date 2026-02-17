# Quick Start Guide

## Database Setup Options

### Option 1: Docker (Fastest)
```bash
docker run --name sublime-postgres -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB=sublime_bot -p 5432:5432 -d postgres:16
```

### Option 2: Local PostgreSQL Installation
1. Download from https://www.postgresql.org/download/windows/
2. Install with default settings
3. Remember the password you set for `postgres` user
4. Update `config.toml` with your password if different

### Option 3: Cloud Database (Supabase)
1. Go to https://supabase.com
2. Create a free project
3. Copy the connection string
4. Update `config.toml` with the connection string

## After Database Setup

1. Apply migrations:
   ```bash
   cargo run -- migrate
   ```

2. Set bot commands (optional):
   ```bash
   cargo run -- commands set
   ```

3. Run the bot:
   ```bash
   cargo run
   ```

## Test Commands

Once the bot is running, test these commands in your Telegram group:
- `/about` - Bot information
- `/hello` - Greeting
- `/pidorules` - Game rules (doesn't require DB)
- `/meme` - Random English meme
- `/memeru` - Random Russian meme (requires channel config)
