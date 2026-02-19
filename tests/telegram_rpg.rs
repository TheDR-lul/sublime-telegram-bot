//! Integration tests: bot behaviour via fake Telegram (teloxide_tests).
//! Requires DATABASE_URL and a running Postgres (e.g. postgresql://user:pass@localhost/postgres).
//! Run with: `cargo test --test telegram_rpg -- --ignored` (or `cargo test --test telegram_rpg` to run including ignored).

use sublime::{config::Config, dispatcher};
use teloxide::dptree;
use teloxide_tests::{MockBot, MockMessageText, MockPrivateChat, MockUser};

#[tokio::test]
#[ignore = "requires DATABASE_URL and running Postgres"]
async fn rpg_command_first_time_sends_menu_and_creates_character() {
    let msg = MockMessageText::new()
        .text("/rpg")
        .from(MockUser::new().id(88_001).first_name("Test").username("rpg_test").build())
        .chat(MockPrivateChat::new().id(88_001).build());

    let mut bot = MockBot::new(msg, dispatcher::build_message_schema());

    let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgresql://postgres:postgres@localhost/postgres".to_string()
    });
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect(&database_url)
        .await
        .expect("DATABASE_URL must point to a running Postgres");

    let migrations_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let migrator = sqlx::migrate::Migrator::new(migrations_dir)
        .await
        .expect("migrations dir");
    migrator.run(&pool).await.expect("run migrations");

    let config = Config {
        telegram_token: "test_token".to_string(),
        database_url: database_url.clone(),
        sentry_dsn: None,
        tiktok_cache_chat_id: None,
        meme_ru_channels: vec![],
    };

    bot.dependencies(dptree::deps![pool.clone(), config]);
    bot.dispatch().await;

    let responses = bot.get_responses();
    let last = responses
        .sent_messages
        .last()
        .expect("bot must send at least one message for /rpg");

    let text = last.text().expect("message must have text");
    assert!(
        text.contains("Pidor Royale"),
        "RPG menu must contain title; got: {}",
        text
    );
    assert!(
        text.contains("Level"),
        "RPG menu must show level; got: {}",
        text
    );

    // First time: character was created (one rpg_player for our tguser)
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM rpg_player r JOIN tguser u ON r.user_id = u.id WHERE u.tg_id = 88001",
    )
    .fetch_one(&pool)
    .await
    .expect("count query");
    assert_eq!(count.0, 1, "exactly one RPG character must exist for test user");
}
