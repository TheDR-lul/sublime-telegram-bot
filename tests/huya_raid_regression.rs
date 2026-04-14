//! Regression tests for stale Huya raid lifecycle.
//! Requires DATABASE_URL and running Postgres.

use sublime::db::huya as huya_db;

fn database_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost/postgres".to_string())
}

async fn setup_pool() -> sqlx::PgPool {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect(&database_url())
        .await
        .expect("DATABASE_URL must point to a running Postgres");
    let migrations_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let migrator = sqlx::migrate::Migrator::new(migrations_dir)
        .await
        .expect("migrations dir");
    migrator.run(&pool).await.expect("run migrations");
    pool
}

#[tokio::test]
#[ignore = "requires DATABASE_URL and running Postgres"]
async fn expired_raid_is_not_returned_as_live() {
    let pool = setup_pool().await;
    let chat_id = 990_001_i64;
    let leader_tg_id = 880_001_i64;
    let target_tg_id = 880_002_i64;

    let _ = huya_db::get_or_create(&pool, chat_id, leader_tg_id)
        .await
        .expect("leader created");
    let _ = huya_db::get_or_create(&pool, chat_id, target_tg_id)
        .await
        .expect("target created");

    let raid = huya_db::create_raid(&pool, chat_id, leader_tg_id, target_tg_id, 100, 100)
        .await
        .expect("raid created");

    sqlx::query(
        "UPDATE huya_raid
         SET status = 'active', expires_at = NOW() - INTERVAL '14 days'
         WHERE id = $1",
    )
    .bind(raid.id)
    .execute(&pool)
    .await
    .expect("raid expired");

    let live = huya_db::get_pending_or_active_raid(&pool, chat_id)
        .await
        .expect("query live raid");
    assert!(live.is_none(), "expired raid should not be considered live");
}

#[tokio::test]
#[ignore = "requires DATABASE_URL and running Postgres"]
async fn can_create_new_raid_when_previous_one_is_expired() {
    let pool = setup_pool().await;
    let chat_id = 990_002_i64;
    let leader_tg_id = 880_003_i64;
    let target_tg_id = 880_004_i64;
    let next_target_tg_id = 880_005_i64;

    let _ = huya_db::get_or_create(&pool, chat_id, leader_tg_id)
        .await
        .expect("leader created");
    let _ = huya_db::get_or_create(&pool, chat_id, target_tg_id)
        .await
        .expect("target created");
    let _ = huya_db::get_or_create(&pool, chat_id, next_target_tg_id)
        .await
        .expect("next target created");

    let old_raid = huya_db::create_raid(&pool, chat_id, leader_tg_id, target_tg_id, 100, 100)
        .await
        .expect("old raid created");
    sqlx::query(
        "UPDATE huya_raid
         SET status = 'pending', expires_at = NOW() - INTERVAL '14 days'
         WHERE id = $1",
    )
    .bind(old_raid.id)
    .execute(&pool)
    .await
    .expect("old raid expired");

    let new_raid = huya_db::create_raid(&pool, chat_id, leader_tg_id, next_target_tg_id, 100, 100)
        .await
        .expect("new raid should be creatable");
    assert_ne!(new_raid.id, old_raid.id, "new raid must be a fresh row");
}

#[tokio::test]
#[ignore = "requires DATABASE_URL and running Postgres"]
async fn global_expired_raid_cleanup_marks_status_cancelled() {
    let pool = setup_pool().await;
    let chat_id = 990_003_i64;
    let leader_tg_id = 880_006_i64;
    let target_tg_id = 880_007_i64;

    let _ = huya_db::get_or_create(&pool, chat_id, leader_tg_id)
        .await
        .expect("leader created");
    let _ = huya_db::get_or_create(&pool, chat_id, target_tg_id)
        .await
        .expect("target created");

    let raid = huya_db::create_raid(&pool, chat_id, leader_tg_id, target_tg_id, 100, 100)
        .await
        .expect("raid created");

    sqlx::query(
        "UPDATE huya_raid
         SET status = 'active', expires_at = NOW() - INTERVAL '2 days'
         WHERE id = $1",
    )
    .bind(raid.id)
    .execute(&pool)
    .await
    .expect("raid expired");

    let cancelled = huya_db::cancel_expired_raids(&pool)
        .await
        .expect("cleanup executed");
    assert!(
        cancelled.iter().any(|(id, _, _)| *id == raid.id),
        "cleanup should return expired raid id"
    );

    let status: Option<String> = sqlx::query_scalar("SELECT status FROM huya_raid WHERE id = $1")
        .bind(raid.id)
        .fetch_optional(&pool)
        .await
        .expect("status fetched");
    assert_eq!(status.as_deref(), Some("cancelled"));
}
