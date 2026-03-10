use sqlx::PgPool;
use teloxide::prelude::*;
use teloxide::types::MessageEntityKind;

use crate::db::user;

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
        let start = e.offset as usize;
        let end = start.saturating_add(e.length as usize);
        if start < text.len() && end <= text.len() {
            let mention = &text[start..end];
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
/// 2) TextMention / TextLink / @mention in the replied message.
/// 3) Command argument: @username → DB lookup; plain text → display name lookup.
/// 4) Reply author.
pub async fn resolve_target(
    pool: &PgPool,
    msg: &Message,
    arg: &str,
) -> ResolvedTarget {
    // 1) Entities in the command message.
    if let Some(res) = resolve_from_entities(pool, msg).await {
        return res;
    }

    // 2) Entities in the replied message.
    if let Some(reply) = msg.reply_to_message() {
        if let Some(res) = resolve_from_entities(pool, &reply).await {
            return res;
        }
    }

    // 3) Command argument: try @username then display name.
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

    // 4) Fallback: reply author.
    if let Some(reply) = msg.reply_to_message() {
        if let Some(res) = resolve_from_reply(reply).await {
            return res;
        }
    }

    ResolvedTarget::NotFound
}

