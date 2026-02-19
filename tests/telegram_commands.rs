//! Integration tests: all Telegram commands and RPG callback chains (teloxide_tests).
//! Requires DATABASE_URL and running Postgres. Run: `cargo test --test telegram_commands -- --ignored`

use sublime::{config::Config, dispatcher};
use teloxide::dptree;
use teloxide_tests::{MockBot, MockCallbackQuery, MockMessageText, MockPrivateChat, MockUser};

fn database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgresql://postgres:postgres@localhost/postgres".to_string()
    })
}

async fn setup_pool_and_config() -> (sqlx::PgPool, Config) {
    let database_url = database_url();
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect(&database_url)
        .await
        .expect("DATABASE_URL must point to a running Postgres");
    let migrations_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let migrator = sqlx::migrate::Migrator::new(migrations_dir).await.expect("migrations dir");
    migrator.run(&pool).await.expect("run migrations");
    let config = Config {
        telegram_token: "test_token".to_string(),
        database_url: database_url.clone(),
        sentry_dsn: None,
        tiktok_cache_chat_id: None,
        meme_ru_channels: vec![],
    };
    (pool, config)
}

fn test_user() -> teloxide::types::User {
    MockUser::new()
        .id(90_001)
        .first_name("TestUser")
        .username("test_commands")
        .build()
}

fn test_chat() -> teloxide::types::Chat {
    MockPrivateChat::new().id(90_001).build()
}

#[tokio::test]
#[ignore = "requires DATABASE_URL and running Postgres"]
async fn command_about_sends_github_link() {
    let (pool, config) = setup_pool_and_config().await;
    let msg = MockMessageText::new()
        .text("/about")
        .from(test_user())
        .chat(test_chat());
    let mut bot = MockBot::new(msg, dispatcher::build_test_schema());
    bot.dependencies(dptree::deps![pool, config]);
    bot.dispatch().await;
    let r = bot.get_responses();
    let last = r.sent_messages.last().expect("one message");
    let text = last.text().expect("text");
    assert!(text.contains("GitHub") || text.contains("github"), "got: {}", text);
}

#[tokio::test]
#[ignore = "requires DATABASE_URL and running Postgres"]
async fn command_hello_sends_greeting() {
    let (pool, config) = setup_pool_and_config().await;
    let msg = MockMessageText::new()
        .text("/hello")
        .from(test_user())
        .chat(test_chat());
    let mut bot = MockBot::new(msg, dispatcher::build_test_schema());
    bot.dependencies(dptree::deps![pool, config]);
    bot.dispatch().await;
    let r = bot.get_responses();
    let last = r.sent_messages.last().expect("one message");
    let text = last.text().expect("text");
    assert!(text.contains("Hello"), "got: {}", text);
}

#[tokio::test]
#[ignore = "requires DATABASE_URL and running Postgres"]
async fn command_shrug_sends_shrug() {
    let (pool, config) = setup_pool_and_config().await;
    let msg = MockMessageText::new()
        .text("/shrug")
        .from(test_user())
        .chat(test_chat());
    let mut bot = MockBot::new(msg, dispatcher::build_test_schema());
    bot.dependencies(dptree::deps![pool, config]);
    bot.dispatch().await;
    let r = bot.get_responses();
    let last = r.sent_messages.last().expect("one message");
    let text = last.text().expect("text");
    assert!(text.contains("ツ"), "got: {}", text);
}

#[tokio::test]
#[ignore = "requires DATABASE_URL and running Postgres"]
async fn command_echo_echoes_text() {
    let (pool, config) = setup_pool_and_config().await;
    let msg = MockMessageText::new()
        .text("/echo hello world")
        .from(test_user())
        .chat(test_chat());
    let mut bot = MockBot::new(msg, dispatcher::build_test_schema());
    bot.dependencies(dptree::deps![pool, config]);
    bot.dispatch().await;
    let r = bot.get_responses();
    let last = r.sent_messages.last().expect("one message");
    let text = last.text().expect("text");
    assert!(text.contains("hello world"), "got: {}", text);
}

#[tokio::test]
#[ignore = "requires DATABASE_URL and running Postgres"]
async fn command_slap_sends_slap_text() {
    let (pool, config) = setup_pool_and_config().await;
    let msg = MockMessageText::new()
        .text("/slap victim")
        .from(test_user())
        .chat(test_chat());
    let mut bot = MockBot::new(msg, dispatcher::build_test_schema());
    bot.dependencies(dptree::deps![pool, config]);
    bot.dispatch().await;
    let r = bot.get_responses();
    let last = r.sent_messages.last().expect("one message");
    let text = last.text().expect("text");
    assert!(text.contains("slaps") && text.contains("victim"), "got: {}", text);
}

#[tokio::test]
#[ignore = "requires DATABASE_URL and running Postgres"]
async fn command_me_sends_action_text() {
    let (pool, config) = setup_pool_and_config().await;
    let msg = MockMessageText::new()
        .text("/me waves")
        .from(test_user())
        .chat(test_chat());
    let mut bot = MockBot::new(msg, dispatcher::build_test_schema());
    bot.dependencies(dptree::deps![pool, config]);
    bot.dispatch().await;
    let r = bot.get_responses();
    let last = r.sent_messages.last().expect("one message");
    let text = last.text().expect("text");
    assert!(text.contains("waves"), "got: {}", text);
}

#[tokio::test]
#[ignore = "requires DATABASE_URL and running Postgres"]
async fn command_pidorules_sends_rules() {
    let (pool, config) = setup_pool_and_config().await;
    let msg = MockMessageText::new()
        .text("/pidorules")
        .from(test_user())
        .chat(test_chat());
    let mut bot = MockBot::new(msg, dispatcher::build_test_schema());
    bot.dependencies(dptree::deps![pool, config]);
    bot.dispatch().await;
    let r = bot.get_responses();
    let last = r.sent_messages.last().expect("one message");
    let text = last.text().expect("text");
    assert!(text.contains("Пидор") || text.contains("pidor"), "got: {}", text);
}

#[tokio::test]
#[ignore = "requires DATABASE_URL and running Postgres"]
async fn command_rpg_sends_menu() {
    let (pool, config) = setup_pool_and_config().await;
    let msg = MockMessageText::new()
        .text("/rpg")
        .from(test_user())
        .chat(test_chat());
    let mut bot = MockBot::new(msg, dispatcher::build_test_schema());
    bot.dependencies(dptree::deps![pool.clone(), config]);
    bot.dispatch().await;
    let r = bot.get_responses();
    let last = r.sent_messages.last().expect("one message");
    let text = last.text().expect("text");
    assert!(text.contains("Pidor Royale"), "got: {}", text);
    assert!(text.contains("Level"), "got: {}", text);
}

// RPG callback chain: open main menu then tap Profile / World / Inventory / Guild

#[tokio::test]
#[ignore = "requires DATABASE_URL and running Postgres"]
async fn rpg_callback_profile_open_edits_to_profile() {
    let (pool, config) = setup_pool_and_config().await;
    let cb = MockCallbackQuery::new()
        .data("rpg:profile:open")
        .from(test_user());
    let mut bot = MockBot::new(cb, dispatcher::build_test_schema());
    bot.dependencies(dptree::deps![pool, config]);
    bot.dispatch().await;
    let r = bot.get_responses();
    assert!(
        !r.edited_messages_text.is_empty() || !r.edited_messages_reply_markup.is_empty(),
        "expected edit for profile"
    );
    if let Some(msg) = r.edited_messages_text.last() {
        let text = msg.message.text().unwrap_or_default();
        assert!(text.contains("Profile") || text.contains("Level") || text.contains("STR"), "got: {}", text);
    }
}

#[tokio::test]
#[ignore = "requires DATABASE_URL and running Postgres"]
async fn rpg_callback_world_open_edits_to_world_map() {
    let (pool, config) = setup_pool_and_config().await;
    let cb = MockCallbackQuery::new()
        .data("rpg:world:open")
        .from(test_user());
    let mut bot = MockBot::new(cb, dispatcher::build_test_schema());
    bot.dependencies(dptree::deps![pool, config]);
    bot.dispatch().await;
    let r = bot.get_responses();
    assert!(
        !r.edited_messages_text.is_empty() || !r.edited_messages_reply_markup.is_empty(),
        "expected edit for world"
    );
}

#[tokio::test]
#[ignore = "requires DATABASE_URL and running Postgres"]
async fn rpg_callback_inventory_open_edits_to_inventory() {
    let (pool, config) = setup_pool_and_config().await;
    let cb = MockCallbackQuery::new()
        .data("rpg:inventory:open")
        .from(test_user());
    let mut bot = MockBot::new(cb, dispatcher::build_test_schema());
    bot.dependencies(dptree::deps![pool, config]);
    bot.dispatch().await;
    let r = bot.get_responses();
    assert!(
        !r.edited_messages_text.is_empty() || !r.edited_messages_reply_markup.is_empty(),
        "expected edit for inventory"
    );
}

#[tokio::test]
#[ignore = "requires DATABASE_URL and running Postgres"]
async fn rpg_callback_guild_open_edits_or_sends() {
    let (pool, config) = setup_pool_and_config().await;
    let cb = MockCallbackQuery::new()
        .data("rpg:guild:open")
        .from(test_user());
    let mut bot = MockBot::new(cb, dispatcher::build_test_schema());
    bot.dependencies(dptree::deps![pool, config]);
    bot.dispatch().await;
    let r = bot.get_responses();
    assert!(
        !r.edited_messages_text.is_empty()
            || !r.edited_messages_reply_markup.is_empty()
            || !r.sent_messages.is_empty(),
        "expected edit or send for guild"
    );
}

#[tokio::test]
#[ignore = "requires DATABASE_URL and running Postgres"]
async fn rpg_chain_main_then_profile_then_world() {
    let (pool, config) = setup_pool_and_config().await;
    let user = test_user();
    let chat = test_chat();

    let msg1 = MockMessageText::new()
        .text("/rpg")
        .from(user.clone())
        .chat(chat.clone());
    let mut bot = MockBot::new(msg1, dispatcher::build_test_schema());
    bot.dependencies(dptree::deps![pool.clone(), config.clone()]);
    bot.dispatch().await;
    let sent1 = bot.get_responses().sent_messages.last().expect("rpg sends menu").clone();

    let cb_profile = MockCallbackQuery::new()
        .data("rpg:profile:open")
        .from(user.clone())
        .message(sent1.clone());
    bot.update(cb_profile);
    bot.dispatch().await;
    let r2 = bot.get_responses();
    let msg_after_profile = r2.edited_messages_text.last().map(|e| e.message.clone()).unwrap_or_else(|| sent1.clone());

    let cb_world = MockCallbackQuery::new()
        .data("rpg:world:open")
        .from(user)
        .message(msg_after_profile);
    bot.update(cb_world);
    bot.dispatch().await;

    let r = bot.get_responses();
    assert!(
        !r.edited_messages_text.is_empty(),
        "chain: rpg -> profile -> world should produce edits"
    );
}
