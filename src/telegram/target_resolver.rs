use sqlx::PgPool;
use teloxide::prelude::*;
use teloxide::types::MessageEntityKind;

use crate::db::user;

/// Telegram MessageEntity offsets/length are defined in UTF-16 code units.
/// Rust string slices use UTF-8 byte indices, so we must translate boundaries.
fn utf16_to_byte_index(s: &str, target_cu: usize) -> Option<usize> {
    if target_cu == 0 {
        return Some(0);
    }

    let mut cu = 0usize;
    for (byte_idx, ch) in s.char_indices() {
        let ch_cu = ch.len_utf16();
        let next_cu = cu + ch_cu;

        // If the requested position lands inside this char (surrogate mismatch),
        // treat it as invalid to avoid slicing panics.
        if next_cu > target_cu {
            return None;
        }

        if next_cu == target_cu {
            return Some(byte_idx + ch.len_utf8());
        }

        cu = next_cu;
    }

    if cu == target_cu {
        Some(s.len())
    } else {
        None
    }
}

fn utf16_range_to_byte_range(s: &str, start_cu: usize, len_cu: usize) -> Option<(usize, usize)> {
    let end_cu = start_cu.checked_add(len_cu)?;
    let start_b = utf16_to_byte_index(s, start_cu)?;
    let end_b = utf16_to_byte_index(s, end_cu)?;
    if start_b <= end_b {
        Some((start_b, end_b))
    } else {
        None
    }
}

/// Resolution result for a target Telegram user.
pub enum ResolvedTarget {
    User(i64),
    IsBot,
    NotFound,
}

fn tg_id_from_textlink_url(url: &str) -> Option<i64> {
    if let Some(id_str) = url.strip_prefix("tg://user?id=") {
        return id_str.parse::<i64>().ok();
    }
    None
}

async fn resolve_from_entities(
    pool: &PgPool,
    msg: &Message,
) -> Option<ResolvedTarget> {
    let entities = msg.entities()?;
    let text = msg.text();

    // 1) TextMention / TextLink first.
    for e in entities {
        match &e.kind {
            MessageEntityKind::TextMention { user } => {
                return Some(if user.is_bot {
                    ResolvedTarget::IsBot
                } else {
                    ResolvedTarget::User(user.id.0 as i64)
                });
            }
            MessageEntityKind::TextLink { url } => {
                if let Some(id) = tg_id_from_textlink_url(url.as_str()) {
                    return Some(ResolvedTarget::User(id));
                }
            }
            _ => {}
        }
    }

    // 2) Plain @mention → look up in DB by username.
    if let (Some(text), Some(e)) = (text, entities.iter().find(|e| matches!(e.kind, MessageEntityKind::Mention))) {
        let start_cu = e.offset as usize;
        let len_cu = e.length as usize;
        if let Some((start_b, end_b)) = utf16_range_to_byte_range(text, start_cu, len_cu) {
            let mention = &text[start_b..end_b];
            let username = mention.trim_start_matches('@').trim();
            if !username.is_empty() {
                if let Ok(Some(u)) = user::get_by_username(pool, username).await {
                    return Some(ResolvedTarget::User(u.tg_id));
                }
            }
        }
    }

    None
}

async fn resolve_from_reply(reply: &Message) -> Option<ResolvedTarget> {
    if let Some(from) = reply.from.as_ref() {
        return Some(if from.is_bot {
            ResolvedTarget::IsBot
        } else {
            ResolvedTarget::User(from.id.0 as i64)
        });
    }
    None
}

/// Resolve target user from command argument, message entities and reply.
///
/// Priority:
/// 1) TextMention / TextLink / @mention in the command message.
/// 2) Command argument: @username → DB lookup; plain text → display name lookup.
/// 3) Reply author.
pub async fn resolve_target(
    pool: &PgPool,
    msg: &Message,
    arg: &str,
) -> ResolvedTarget {
    // 1) Entities in the command message.
    if let Some(res) = resolve_from_entities(pool, msg).await {
        return res;
    }

    // 2) Command argument: try @username then display name.
    let trimmed = arg.trim();
    if !trimmed.is_empty() {
        if trimmed.starts_with('@') {
            let username = trimmed.trim_start_matches('@').trim();
            if !username.is_empty() {
                if let Ok(Some(u)) = user::get_by_username(pool, username).await {
                    return ResolvedTarget::User(u.tg_id);
                }
            }
        } else if let Ok(Some(u)) = user::find_by_display_name(pool, trimmed).await {
            return ResolvedTarget::User(u.tg_id);
        }
    }

    // 3) Fallback: reply author.
    if let Some(reply) = msg.reply_to_message() {
        if let Some(res) = resolve_from_reply(reply).await {
            return res;
        }
    }

    ResolvedTarget::NotFound
}

