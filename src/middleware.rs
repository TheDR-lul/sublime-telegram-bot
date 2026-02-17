//! No global middleware: pool is in dispatcher deps; handlers that need TgUser call db::user::upsert_tg_user.
