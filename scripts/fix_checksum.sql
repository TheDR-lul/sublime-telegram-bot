-- Fix VersionMismatch for 20240101000002_gameresult_slot.sql (SHA384 of current file).
-- Apply when migrate fails with VersionMismatch(20240101000002). Pipeline runs this automatically on migrate failure.
UPDATE _sqlx_migrations SET checksum = decode('a2a5341ebe64a34b34e832b064ecf9155a5d80a8833f21263f6eed895182aa7019abad40bc6cf403a88eacf52e71da37', 'hex') WHERE version = 20240101000002;
