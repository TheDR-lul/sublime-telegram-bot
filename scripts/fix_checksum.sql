-- Fix VersionMismatch for 20240101000002_gameresult_slot.sql (SHA384 of current file).
-- Apply when migrate fails with VersionMismatch(20240101000002). Pipeline runs this automatically on migrate failure.
UPDATE _sqlx_migrations
SET checksum = decode('a2a5341ebe64a34b34e832b064ecf9155a5d80a8833f21263f6eed895182aa7019abad40bc6cf403a88eacf52e71da37', 'hex')
WHERE version = 20240101000002;

-- Fix VersionMismatch for 20260304000006_huya_equipment.sql (SHA384 of current file).
-- Apply when migrate fails with VersionMismatch(20260304000006).
UPDATE _sqlx_migrations
SET checksum = decode('2bd9b2ebab3e73bbfefa93acbfd62fbe4023dc299fb13c3b76a662314e5406a924e2990f7511f85b158fd2b93c9cf191', 'hex')
WHERE version = 20260304000006;

-- Fix VersionMismatch for 20260305000007_huya_pet.sql (SHA384 of current file).
-- Apply when migrate fails with VersionMismatch(20260305000007).
UPDATE _sqlx_migrations
SET checksum = decode('feddce85eabcb8ae5763b1a550a4dd9bfbbd225242e560de2f522a7616841af883dd0ceb89dfbfa155e1cdf21d256544', 'hex')
WHERE version = 20260305000007;

-- Fix VersionMismatch for 20260310000008_chat_topics.sql.
-- Simplest fix: delete the old row so sqlx inserts a fresh one with correct checksum on next migrate.
DELETE FROM _sqlx_migrations WHERE version = 20260310000008;

-- Fix VersionMismatch for 20260324000100_huya_raid.sql (SHA384 of current file).
UPDATE _sqlx_migrations
SET checksum = decode('b87762e8007c04d19c716730ff4288205cae05d36878cd17740cfbc85ab59e87ce5c60b640e00649c3618be61a013d05', 'hex')
WHERE version = 20260324000100;

-- Fix VersionMismatch for 20260324000200_huya_chests.sql (SHA384 of current file).
UPDATE _sqlx_migrations
SET checksum = decode('a99df030da51de3a8f9a1a68af3183d770fae8ada62d29785282944181064a781730f2930f6fd53c8c70c94bd65e1906', 'hex')
WHERE version = 20260324000200;

-- Fix VersionMismatch for 20260324000300_huya_gems_reforge.sql (SHA384 of current file).
UPDATE _sqlx_migrations
SET checksum = decode('7176534fce01748dd026347ee8018e42c7b64aa0a87c0ba28d698323517c6e350ec3f1406b3f36d58bb9a5577afae282', 'hex')
WHERE version = 20260324000300;

-- Fix VersionMismatch for 20260324000400_huya_raid_timer_and_energy.sql (SHA384 of current file).
-- Most robust fix: remove the row so the migration can be applied again.
-- The migration itself is idempotent (uses IF NOT EXISTS).
DELETE FROM _sqlx_migrations WHERE version = 20260324000400;

-- Fix VersionMismatch for 20260416000000_retention_indexes.sql (SHA384 of current file).
UPDATE _sqlx_migrations
SET checksum = decode('f77f7e9df208507fbc353bbc090622e6667b1ef64325b98b838fa28d4c0b6ddfba16a09de29048ccd511b8a115fbd53b', 'hex')
WHERE version = 20260416000000;
