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
