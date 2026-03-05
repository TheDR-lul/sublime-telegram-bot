//! Dispatcher schema (handler tree) for reuse in run_bot and tests.

use regex::Regex;
use sqlx::PgPool;
use std::sync::LazyLock;
use teloxide::dispatching::{HandlerExt, UpdateFilterExt};
use teloxide::dptree::case;
use teloxide::prelude::*;
use teloxide::types::{CallbackQuery, ChatMemberUpdated, InlineQuery, Update, UpdateKind};
use teloxide::utils::command::BotCommands;

use crate::config::Config;
use crate::error::AppError;
use crate::handlers::{
    about, achievements as achievements_handler, commands::Cmd, game::commands as game,
    game::duel as game_duel, huya as huya_handler, meme, misc, tiktok,
};

static PIDOR_YEAR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^/pidor(\d{4})(?:@.+)?$").expect("pidor year regex is valid")
});

async fn check_rate_limit(
    rl: &std::sync::Arc<crate::ratelimit::RateLimiter>,
    msg: &teloxide::types::Message,
) -> bool {
    let chat_id = msg.chat.id.0;
    let user_id = msg.from.as_ref().map(|u| u.id.0 as i64).unwrap_or(0);
    rl.check(chat_id, user_id).await
}

async fn callback_router(
    bot: Bot,
    query: CallbackQuery,
    pool: PgPool,
    config: Config,
) -> Result<(), AppError> {
    let data = query.data.as_deref().unwrap_or("");
    // RPG: development for future — disabled; reply instead of opening menu
    if data.starts_with("rpg:") {
        return misc::rpg_disabled_callback(bot, query).await;
    }
    if data.starts_with("menu:") {
        return misc::menu_callback(bot, query, pool, config).await;
    }
    if data.starts_with("settings:") {
        return game::pidorset_callback(bot, query, pool).await;
    }
    if data.starts_with("ach:") {
        return achievements_handler::achievements_callback(bot, query, pool).await;
    }
    if data.starts_with("duel_accept:") {
        return game_duel::duel_accept_callback(bot, query, pool).await;
    }
    if data.starts_with("duel:") {
        return game_duel::duel_move_callback(bot, query, pool).await;
    }
    if data.starts_with("duel_dice:") {
        return game_duel::duel_dice_callback(bot, query, pool).await;
    }
    if data.starts_with("duel_coin:") {
        return game_duel::duel_coin_callback(bot, query, pool).await;
    }
    if data.starts_with("duel_rps:") {
        return game_duel::duel_rps_callback(bot, query, pool).await;
    }
    if data.starts_with("huya_grow:") {
        return huya_handler::huya_grow_callback(bot, query, pool).await;
    }
    if data.starts_with("huya_fa:") {
        return huya_handler::huya_fight_accept_callback(bot, query, pool).await;
    }
    if data.starts_with("huya_fd:") {
        return huya_handler::huya_fight_decline_callback(bot, query, pool).await;
    }
    if data.starts_with("huya_fm:") {
        return huya_handler::huya_fight_move_callback(bot, query, pool).await;
    }
    if data.starts_with("huya_skill_page:") {
        return huya_handler::huya_skill_page_callback(bot, query, pool).await;
    }
    if data.starts_with("huya_skill:") {
        return huya_handler::huya_skill_callback(bot, query, pool).await;
    }
    if data.starts_with("huya_buy:") {
        return huya_handler::huya_buy_callback(bot, query, pool).await;
    }
    match data {
        "meme_en_refresh" => meme::meme_refresh_callback(bot, query).await,
        "meme_en_save" => meme::meme_save_callback(bot, query).await,
        "meme_ru_refresh" => meme::memeru_refresh_callback(bot, query, config).await,
        "meme_ru_save" => meme::memeru_save_callback(bot, query, config).await,
        _ => Ok(()),
    }
}

/// Message-only schema for tests. Uses a single endpoint that extracts Message from Update
/// so that MockBot's dependency injection does not require &Message (which comes from the filter).
pub fn build_message_schema() -> teloxide::dispatching::UpdateHandler<AppError> {
    dptree::entry().endpoint(
        |update: Update, bot: Bot, _pool: PgPool, _config: Config| async move {
            if let UpdateKind::Message(msg) = update.kind
                && let Some(text) = msg.text()
                && let Ok(cmd) = Cmd::parse(text, "")
                && matches!(cmd, Cmd::Rpg)
            {
                return misc::rpg_disabled_handler(bot, msg, cmd).await;
            }
            Ok(())
        },
    )
}

/// Full test schema: messages (all commands) and callback queries in one endpoint
/// so MockBot only needs Update, Bot, PgPool, Config, PidorscanDedup.
pub fn build_test_schema() -> teloxide::dispatching::UpdateHandler<AppError> {
    dptree::entry().endpoint(
        |update: Update, bot: Bot, pool: PgPool, config: Config, dedup: std::sync::Arc<crate::dedup::PidorscanDedup>| async move {
            match update.kind {
                UpdateKind::Message(msg) => {
                    let text = match msg.text() {
                        Some(t) => t,
                        None => return Ok(()),
                    };
                    let cmd = match Cmd::parse(text, "") {
                        Ok(c) => c,
                        Err(_) => return Ok(()),
                    };
                    match &cmd {
                        Cmd::Menu => misc::menu_handler(bot, msg, cmd).await,
                        Cmd::About => about::about_handler(bot, msg, cmd).await,
                        Cmd::Slap => misc::slap_handler(bot, msg, cmd).await,
                        Cmd::Shrug => misc::shrug_handler(bot, msg, cmd).await,
                        Cmd::Me(_) => misc::me_handler(bot, msg, cmd).await,
                        Cmd::Google(_) => misc::google_handler(bot, msg, cmd).await,
                        Cmd::Rpg => misc::rpg_disabled_handler(bot, msg, cmd).await,
                        Cmd::Pidorscan(_) => misc::pidorscan_handler(bot, msg, cmd, dedup).await,
                        Cmd::Pidor => game::pidor_handler(bot, msg, cmd, pool).await,
                        Cmd::Pidorules => game::pidorules_handler(bot, msg, cmd, pool).await,
                        Cmd::Pidoreg => game::pidoreg_handler(bot, msg, cmd, pool).await,
                        Cmd::Pidorunreg => game::pidorunreg_handler(bot, msg, cmd, pool).await,
                        Cmd::Pidorstats => game::pidorstats_handler(bot, msg, cmd, pool).await,
                        Cmd::Pidorall => game::pidorall_handler(bot, msg, cmd, pool).await,
                        Cmd::Pidorme => game::pidorme_handler(bot, msg, cmd, pool).await,
                        Cmd::Pidorset => game::pidorset_handler(bot, msg, pool).await,
                        Cmd::Achievements => achievements_handler::achievements_handler(bot, msg, cmd, pool).await,
                        Cmd::Meme => meme::meme_handler(bot, msg, cmd).await,
                        Cmd::Memeru => meme::memeru_handler(bot, msg, cmd, config).await,
                        Cmd::Ttvideo(_) => tiktok::tt_video_handler(bot, msg, cmd).await,
                        Cmd::Ttlink(_) => tiktok::tt_link_handler(bot, msg, cmd).await,
                        Cmd::Pidorduel => game_duel::pidorduel_handler(bot, msg, cmd, pool).await,
                        Cmd::Duelstats => game_duel::duelstats_handler(bot, msg, cmd, pool).await,
                        Cmd::Pidorbet(_) => game::pidorbet_handler(bot, msg, cmd, pool).await,
                        Cmd::Lang(_) => misc::lang_handler(bot, msg, cmd, pool).await,
                        Cmd::Huya(_) => huya_handler::huya_handler(bot, msg, cmd, pool).await,
                        Cmd::Huyareg => huya_handler::huyareg_handler(bot, msg, pool).await,
                        Cmd::Huyagrow => huya_handler::huya_handler(bot, msg, cmd, pool).await,
                        Cmd::Huyafight(_) => huya_handler::huya_handler(bot, msg, cmd, pool).await,
                        Cmd::Huyasteal(_) => huya_handler::huya_handler(bot, msg, cmd, pool).await,
                        Cmd::Huyatop => huya_handler::huyatop_handler(bot, msg, cmd, pool).await,
                        Cmd::Huyaskills => huya_handler::huyaskills_handler(bot, msg, cmd, pool).await,
                        Cmd::Huyashop => huya_handler::huyashop_handler(bot, msg, cmd, pool).await,
                    }
                }
                UpdateKind::CallbackQuery(query) => callback_router(bot, query, pool, config).await,
                _ => Ok(()),
            }
        },
    )
}

fn message_schema() -> teloxide::dispatching::UpdateHandler<AppError> {
    Update::filter_message()
        .filter_command::<Cmd>()
        .branch(case![Cmd::Menu].endpoint(misc::menu_handler))
        .branch(case![Cmd::About].endpoint(about::about_handler))
        .branch(case![Cmd::Slap].endpoint(misc::slap_handler))
        .branch(case![Cmd::Shrug].endpoint(misc::shrug_handler))
        .branch(case![Cmd::Me(_s)].endpoint(misc::me_handler))
        .branch(case![Cmd::Google(_s)].endpoint(misc::google_handler))
        // RPG: development for future — disabled; show stub message
        .branch(case![Cmd::Rpg].endpoint(misc::rpg_disabled_handler))
        .branch(case![Cmd::Pidorscan(_s)].endpoint(
            |bot: Bot,
             msg: Message,
             cmd: Cmd,
             dedup: std::sync::Arc<crate::dedup::PidorscanDedup>,
             rl: std::sync::Arc<crate::ratelimit::RateLimiter>| async move {
                if !check_rate_limit(&rl, &msg).await {
                    return Ok(());
                }
                misc::pidorscan_handler(bot, msg, cmd, dedup).await
            },
        ))
        .branch(case![Cmd::Pidor].endpoint(
            |bot: Bot,
             msg: Message,
             cmd: Cmd,
             pool: PgPool,
             rl: std::sync::Arc<crate::ratelimit::RateLimiter>| async move {
                if !check_rate_limit(&rl, &msg).await {
                    return Ok(());
                }
                game::pidor_handler(bot, msg, cmd, pool).await
            },
        ))
        .branch(case![Cmd::Pidorules].endpoint(|bot: Bot, msg: Message, cmd: Cmd, pool: PgPool| async move {
            game::pidorules_handler(bot, msg, cmd, pool).await
        }))
        .branch(case![Cmd::Pidorset].endpoint(|bot: Bot, msg: Message, pool: PgPool| async move {
            game::pidorset_handler(bot, msg, pool).await
        }))
        .branch(case![Cmd::Pidorduel].endpoint(|bot: Bot, msg: Message, cmd: Cmd, pool: PgPool| async move {
            game_duel::pidorduel_handler(bot, msg, cmd, pool).await
        }))
        .branch(case![Cmd::Duelstats].endpoint(|bot: Bot, msg: Message, cmd: Cmd, pool: PgPool| async move {
            game_duel::duelstats_handler(bot, msg, cmd, pool).await
        }))
        .branch(case![Cmd::Pidorbet(_s)].endpoint(|bot: Bot, msg: Message, cmd: Cmd, pool: PgPool| async move {
            game::pidorbet_handler(bot, msg, cmd, pool).await
        }))
        .branch(case![Cmd::Lang(_s)].endpoint(|bot: Bot, msg: Message, cmd: Cmd, pool: PgPool| async move {
            misc::lang_handler(bot, msg, cmd, pool).await
        }))
        .branch(case![Cmd::Huya(_s)].endpoint(|bot: Bot, msg: Message, cmd: Cmd, pool: PgPool| async move {
            huya_handler::huya_handler(bot, msg, cmd, pool).await
        }))
        .branch(case![Cmd::Huyareg].endpoint(|bot: Bot, msg: Message, pool: PgPool| async move {
            huya_handler::huyareg_handler(bot, msg, pool).await
        }))
        .branch(case![Cmd::Huyagrow].endpoint(|bot: Bot, msg: Message, cmd: Cmd, pool: PgPool| async move {
            huya_handler::huya_handler(bot, msg, cmd, pool).await
        }))
        .branch(case![Cmd::Huyafight(_s)].endpoint(|bot: Bot, msg: Message, cmd: Cmd, pool: PgPool| async move {
            huya_handler::huya_handler(bot, msg, cmd, pool).await
        }))
        .branch(case![Cmd::Huyasteal(_s)].endpoint(|bot: Bot, msg: Message, cmd: Cmd, pool: PgPool| async move {
            huya_handler::huya_handler(bot, msg, cmd, pool).await
        }))
        .branch(case![Cmd::Huyatop].endpoint(|bot: Bot, msg: Message, cmd: Cmd, pool: PgPool| async move {
            huya_handler::huyatop_handler(bot, msg, cmd, pool).await
        }))
        .branch(case![Cmd::Huyaskills].endpoint(|bot: Bot, msg: Message, cmd: Cmd, pool: PgPool| async move {
            huya_handler::huyaskills_handler(bot, msg, cmd, pool).await
        }))
        .branch(case![Cmd::Huyashop].endpoint(|bot: Bot, msg: Message, cmd: Cmd, pool: PgPool| async move {
            huya_handler::huyashop_handler(bot, msg, cmd, pool).await
        }))
        .branch(case![Cmd::Pidoreg].endpoint(|bot: Bot, msg: Message, cmd: Cmd, pool: PgPool| async move {
            game::pidoreg_handler(bot, msg, cmd, pool).await
        }))
        .branch(case![Cmd::Pidorunreg].endpoint(|bot: Bot, msg: Message, cmd: Cmd, pool: PgPool| async move {
            game::pidorunreg_handler(bot, msg, cmd, pool).await
        }))
        .branch(case![Cmd::Pidorstats].endpoint(|bot: Bot, msg: Message, cmd: Cmd, pool: PgPool| async move {
            game::pidorstats_handler(bot, msg, cmd, pool).await
        }))
        .branch(case![Cmd::Pidorall].endpoint(|bot: Bot, msg: Message, cmd: Cmd, pool: PgPool| async move {
            game::pidorall_handler(bot, msg, cmd, pool).await
        }))
        .branch(case![Cmd::Pidorme].endpoint(|bot: Bot, msg: Message, cmd: Cmd, pool: PgPool| async move {
            game::pidorme_handler(bot, msg, cmd, pool).await
        }))
        .branch(case![Cmd::Achievements].endpoint(|bot: Bot, msg: Message, cmd: Cmd, pool: PgPool| async move {
            achievements_handler::achievements_handler(bot, msg, cmd, pool).await
        }))
        .branch(case![Cmd::Meme].endpoint(
            |bot: Bot,
             msg: Message,
             cmd: Cmd,
             rl: std::sync::Arc<crate::ratelimit::RateLimiter>| async move {
                if !check_rate_limit(&rl, &msg).await {
                    return Ok(());
                }
                meme::meme_handler(bot, msg, cmd).await
            },
        ))
        .branch(case![Cmd::Memeru].endpoint(
            |bot: Bot,
             msg: Message,
             _cmd: Cmd,
             config: Config,
             rl: std::sync::Arc<crate::ratelimit::RateLimiter>| async move {
                if !check_rate_limit(&rl, &msg).await {
                    return Ok(());
                }
                meme::memeru_handler(bot, msg, _cmd, config).await
            },
        ))
        .branch(case![Cmd::Ttvideo(_s)].endpoint(
            |bot: Bot,
             msg: Message,
             cmd: Cmd,
             rl: std::sync::Arc<crate::ratelimit::RateLimiter>| async move {
                if !check_rate_limit(&rl, &msg).await {
                    return Ok(());
                }
                tiktok::tt_video_handler(bot, msg, cmd).await
            },
        ))
        .branch(case![Cmd::Ttlink(_s)].endpoint(
            |bot: Bot,
             msg: Message,
             cmd: Cmd,
             rl: std::sync::Arc<crate::ratelimit::RateLimiter>| async move {
                if !check_rate_limit(&rl, &msg).await {
                    return Ok(());
                }
                tiktok::tt_link_handler(bot, msg, cmd).await
            },
        ))
        .branch(dptree::endpoint(|bot: Bot, msg: Message, pool: PgPool| async move {
            use crate::handlers::game::commands;
            if let Some(text) = msg.text()
                && let Some(caps) = PIDOR_YEAR_RE.captures(text)
                && let Ok(year) = caps[1].parse::<i32>()
            {
                return commands::pidoryear_handler(bot, msg, year, pool).await;
            }
            Ok(())
        }))
}

/// Build the full update handler tree (message + callback + inline).
/// Dependencies (pool, config) must be injected via Dispatcher::dependencies / MockBot::dependencies.
pub fn build_schema() -> teloxide::dispatching::UpdateHandler<AppError> {
    let schema = message_schema();
    let callback_schema = Update::filter_callback_query()
        .endpoint(|bot: Bot, query: CallbackQuery, pool: PgPool, config: Config| async move {
            let data = query.data.clone();
            if let Err(e) = callback_router(bot, query, pool, config).await {
                tracing::error!(
                    "Callback error (data: {:?}): {:?}",
                    data,
                    e
                );
            }
            Ok(())
        });

    let inline_schema = Update::filter_inline_query().branch(
        dptree::endpoint(|bot: Bot, query: InlineQuery, pool: PgPool, config: Config| async move {
            if query.query.trim().starts_with("http") {
                tiktok::tt_inline_handler(bot, query, pool, config).await
            } else {
                misc::inline_handler(bot, query).await
            }
        }),
    );

    let chat_member_schema = Update::filter_chat_member().branch(
        dptree::endpoint(
            |bot: Bot, upd: ChatMemberUpdated, pool: PgPool| async move {
                use teloxide::types::ChatMemberStatus;

                let chat_id = upd.chat.id.0;
                let new_user = &upd.new_chat_member.user;
                let status = upd.new_chat_member.status();

                // When the bot is added to a group, create a game so autorun runs in this chat too.
                let bot_me = bot.get_me().await.ok();
                if let Some(ref me) = bot_me {
                    if new_user.id == me.id {
                        let is_joined =
                            matches!(status, ChatMemberStatus::Member | ChatMemberStatus::Administrator);
                        if is_joined && upd.chat.is_group() {
                            if let Err(err) = crate::db::game::get_or_create_game(&pool, chat_id).await {
                                tracing::error!(
                                    "Failed to create game when bot added to chat {}: {:?}",
                                    chat_id,
                                    err
                                );
                            }
                        }
                        return Ok(());
                    }
                }

                // When a user joins (or is in chat), record them for "call unregistered" feature.
                let is_in_chat = matches!(status, ChatMemberStatus::Member | ChatMemberStatus::Administrator | ChatMemberStatus::Restricted);
                if is_in_chat && upd.chat.is_group() {
                    if let Ok(tg_user) = crate::db::user::upsert_tg_user(&pool, new_user).await {
                        let _ = crate::db::game::record_chat_member(&pool, chat_id, tg_user.id).await;
                    }
                }

                // When a user left the chat, unregister them from the pidor game.
                let user = upd.old_chat_member.user.clone();
                let is_gone = matches!(status, ChatMemberStatus::Left | ChatMemberStatus::Banned);

                if !is_gone {
                    return Ok(());
                }

                if let Some(tg_id) = user.id.0.try_into().ok() {
                    if let Err(err) =
                        crate::db::game::remove_player_by_chat_and_tg_id(&pool, chat_id, tg_id)
                            .await
                    {
                        tracing::error!(
                            "Failed to auto-unregister game player on leave (chat_id={}, tg_id={}): {:?}",
                            chat_id,
                            tg_id,
                            err
                        );
                    }
                    let _ = crate::db::game::remove_chat_member_by_tg_id(&pool, chat_id, tg_id).await;
                }

                Ok(())
            },
        ),
    );

    dptree::entry()
        .branch(schema)
        .branch(callback_schema)
        .branch(inline_schema)
        .branch(chat_member_schema)
}

