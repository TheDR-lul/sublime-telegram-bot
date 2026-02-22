//! Fully autonomous integration tests: starts Postgres in Docker via testcontainers,
//! runs migrations, then runs all command and RPG tests. Requires Docker.
//! Run: `cargo test --test full_auto`

use sublime::{config::Config, dedup, dispatcher};
use teloxide::dptree;
use teloxide_tests::{MockBot, MockCallbackQuery, MockMessageText, MockPrivateChat, MockUser};
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

async fn start_postgres(
) -> Result<
    (
        testcontainers::core::ContainerAsync<Postgres>,
        sqlx::PgPool,
        Config,
    ),
    Box<dyn std::error::Error + Send + Sync>,
> {
    let postgres = Postgres::default().with_host_auth();
    let container = postgres.start().await.map_err(|e| format!("testcontainers: {:?}", e))?;
    let host = container.get_host().await.map_err(|e| format!("get_host: {:?}", e))?;
    let port = container
        .get_host_port_ipv4(5432u16)
        .await
        .map_err(|e| format!("get_port: {:?}", e))?;
    let database_url = format!("postgres://postgres:postgres@{}:{}/postgres", host, port);

    // Wait for Postgres to be ready
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // Retry connection with shorter timeout
    let pool: sqlx::PgPool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(std::time::Duration::from_secs(10))
        .connect(&database_url)
        .await
        .map_err(|e| format!("pool connect: {}", e))?;

    let migrations_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let migrator = sqlx::migrate::Migrator::new(migrations_dir).await.map_err(|e| format!("migrator: {}", e))?;
    migrator.run(&pool).await.map_err(|e| format!("migrate: {}", e))?;

    let config = Config {
        telegram_token: "test_token".to_string(),
        database_url: database_url.clone(),
        sentry_dsn: None,
        tiktok_cache_chat_id: None,
        meme_ru_channels: vec![],
    };
    Ok((container, pool, config))
}

fn test_user() -> teloxide::types::User {
    MockUser::new()
        .id(91_001)
        .first_name("Auto")
        .username("full_auto")
        .build()
}

fn test_chat() -> teloxide::types::Chat {
    MockPrivateChat::new().id(91_001).build()
}

/// Single comprehensive test: commands + RPG + DB verification
#[tokio::test]
async fn full_auto_all() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (container, pool, config) = start_postgres().await?;

    // /about
    let msg = MockMessageText::new().text("/about").from(test_user()).chat(test_chat());
    let mut bot = MockBot::new(msg, dispatcher::build_test_schema());
    bot.dependencies(dptree::deps![pool.clone(), config.clone(), std::sync::Arc::new(dedup::PidorscanDedup::new(2))]);
    bot.dispatch().await;
    let r = bot.get_responses();
    let text = r.sent_messages.last().and_then(|m| m.text()).unwrap_or_default();
    assert!(text.contains("GitHub") || text.contains("github"), "about: {}", text);

    // /hello
    let msg = MockMessageText::new().text("/hello").from(test_user()).chat(test_chat());
    bot.update(msg);
    bot.dispatch().await;
    let r = bot.get_responses();
    let text = r.sent_messages.last().and_then(|m| m.text()).unwrap_or_default();
    assert!(text.contains("Hello"), "hello: {}", text);

    // Small delay to ensure pool connections are released
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // /rpg (uses DB) - creates character
    let user = test_user();
    let msg = MockMessageText::new().text("/rpg").from(user.clone()).chat(test_chat());
    bot.update(msg);
    bot.dispatch().await;
    let r = bot.get_responses();
    let text = r.sent_messages.last().and_then(|m| m.text()).unwrap_or_default();
    assert!(text.contains("Pidor Royale"), "rpg: {}", text);

    // Verify character was created in DB
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM rpg_player r JOIN tguser u ON r.user_id = u.id WHERE u.tg_id = $1",
    )
    .bind(user.id.0 as i64)
    .fetch_one(&pool)
    .await?;
    assert_eq!(count.0, 1, "exactly one RPG character must exist");

    drop(pool);
    drop(container);
    Ok(())
}
