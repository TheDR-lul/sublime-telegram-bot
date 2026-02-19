//! Dispatcher schema (handler tree) for reuse in run_bot and tests.

use regex::Regex;
use sqlx::PgPool;
use teloxide::dispatching::{HandlerExt, UpdateFilterExt};
use teloxide::dptree::case;
use teloxide::prelude::*;
use teloxide::types::{CallbackQuery, InlineQuery, Update, UpdateKind};
use teloxide::utils::command::BotCommands;

use crate::config::Config;
use crate::error::AppError;
use crate::handlers::{
    about, achievements as achievements_handler, commands::Cmd, game::commands as game,
    kvstore, meme, misc, rpg, tiktok,
};

async fn callback_router(
    bot: Bot,
    query: CallbackQuery,
    pool: PgPool,
    config: Config,
) -> Result<(), AppError> {
    let data = query.data.as_deref().unwrap_or("");
    if data.starts_with("rpg:") {
        return rpg::rpg_callback_handler(bot, query, pool).await;
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
        |update: Update, bot: Bot, pool: PgPool, _config: Config| async move {
            if let UpdateKind::Message(msg) = update.kind
                && let Some(text) = msg.text()
                && let Ok(cmd) = Cmd::parse(text, "")
                && matches!(cmd, Cmd::Rpg)
            {
                return rpg::rpg_menu_handler(bot, msg, cmd, pool).await;
            }
            Ok(())
        },
    )
}

/// Full test schema: messages (all commands) and callback queries in one endpoint
/// so MockBot only needs Update, Bot, PgPool, Config.
pub fn build_test_schema() -> teloxide::dispatching::UpdateHandler<AppError> {
    dptree::entry().endpoint(
        |update: Update, bot: Bot, pool: PgPool, config: Config| async move {
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
                        Cmd::About => about::about_handler(bot, msg, cmd).await,
                        Cmd::Hello => misc::hello_handler(bot, msg, cmd).await,
                        Cmd::Slap(_) => misc::slap_handler(bot, msg, cmd).await,
                        Cmd::Shrug => misc::shrug_handler(bot, msg, cmd).await,
                        Cmd::Me(_) => misc::me_handler(bot, msg, cmd).await,
                        Cmd::Google(_) => misc::google_handler(bot, msg, cmd).await,
                        Cmd::Pin => misc::pin_handler(bot, msg, cmd).await,
                        Cmd::Echo(_) => misc::echo_handler(bot, msg, cmd).await,
                        Cmd::Rpg => rpg::rpg_menu_handler(bot, msg, cmd, pool).await,
                        Cmd::Pidorscan(_) => misc::pidorscan_handler(bot, msg, cmd).await,
                        Cmd::Get(_) => kvstore::get_handler(bot, msg, cmd, pool).await,
                        Cmd::List => kvstore::list_handler(bot, msg, cmd, pool).await,
                        Cmd::Set(_) => kvstore::set_handler(bot, msg, cmd, pool).await,
                        Cmd::Del(_) => kvstore::del_handler(bot, msg, cmd, pool).await,
                        Cmd::Pidor => game::pidor_handler(bot, msg, cmd, pool).await,
                        Cmd::Pidorules => game::pidorules_handler(bot, msg, cmd).await,
                        Cmd::Pidoreg => game::pidoreg_handler(bot, msg, cmd, pool).await,
                        Cmd::Pidorunreg => game::pidorunreg_handler(bot, msg, cmd, pool).await,
                        Cmd::Pidorstats => game::pidorstats_handler(bot, msg, cmd, pool).await,
                        Cmd::Pidorall => game::pidorall_handler(bot, msg, cmd, pool).await,
                        Cmd::Pidorme => game::pidorme_handler(bot, msg, cmd, pool).await,
                        Cmd::Achievements => achievements_handler::achievements_handler(bot, msg, cmd, pool).await,
                        Cmd::Meme => meme::meme_handler(bot, msg, cmd).await,
                        Cmd::Memeru => meme::memeru_handler(bot, msg, cmd, config).await,
                        Cmd::Ttvideo(_) => tiktok::tt_video_handler(bot, msg, cmd).await,
                        Cmd::Ttlink(_) => tiktok::tt_link_handler(bot, msg, cmd).await,
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
        .branch(case![Cmd::About].endpoint(about::about_handler))
        .branch(case![Cmd::Hello].endpoint(misc::hello_handler))
        .branch(case![Cmd::Slap(_s)].endpoint(misc::slap_handler))
        .branch(case![Cmd::Shrug].endpoint(misc::shrug_handler))
        .branch(case![Cmd::Me(_s)].endpoint(misc::me_handler))
        .branch(case![Cmd::Google(_s)].endpoint(misc::google_handler))
        .branch(case![Cmd::Pin].endpoint(misc::pin_handler))
        .branch(case![Cmd::Echo(_s)].endpoint(misc::echo_handler))
        .branch(case![Cmd::Rpg].endpoint(|bot: Bot, msg: Message, _cmd: Cmd, pool: PgPool| async move {
            rpg::rpg_menu_handler(bot, msg, _cmd, pool).await
        }))
        .branch(case![Cmd::Pidorscan(_s)].endpoint(misc::pidorscan_handler))
        .branch(case![Cmd::Get(_s)].endpoint(|bot: Bot, msg: Message, cmd: Cmd, pool: PgPool| async move {
            kvstore::get_handler(bot, msg, cmd, pool).await
        }))
        .branch(case![Cmd::List].endpoint(|bot: Bot, msg: Message, cmd: Cmd, pool: PgPool| async move {
            kvstore::list_handler(bot, msg, cmd, pool).await
        }))
        .branch(case![Cmd::Set(_s)].endpoint(|bot: Bot, msg: Message, cmd: Cmd, pool: PgPool| async move {
            kvstore::set_handler(bot, msg, cmd, pool).await
        }))
        .branch(case![Cmd::Del(_s)].endpoint(|bot: Bot, msg: Message, cmd: Cmd, pool: PgPool| async move {
            kvstore::del_handler(bot, msg, cmd, pool).await
        }))
        .branch(case![Cmd::Pidor].endpoint(|bot: Bot, msg: Message, cmd: Cmd, pool: PgPool| async move {
            game::pidor_handler(bot, msg, cmd, pool).await
        }))
        .branch(case![Cmd::Pidorules].endpoint(game::pidorules_handler))
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
        .branch(case![Cmd::Meme].endpoint(|bot: Bot, msg: Message, cmd: Cmd| async move {
            meme::meme_handler(bot, msg, cmd).await
        }))
        .branch(case![Cmd::Memeru].endpoint(|bot: Bot, msg: Message, _cmd: Cmd, config: Config| async move {
            meme::memeru_handler(bot, msg, _cmd, config).await
        }))
        .branch(case![Cmd::Ttvideo(_s)].endpoint(tiktok::tt_video_handler))
        .branch(case![Cmd::Ttlink(_s)].endpoint(tiktok::tt_link_handler))
        .branch(dptree::endpoint(|bot: Bot, msg: Message, pool: PgPool| async move {
            use crate::handlers::game::commands;
            let regex = Regex::new(r"^/pidor(\d{4})(?:@.+)?$").unwrap();
            if let Some(text) = msg.text()
                && let Some(caps) = regex.captures(text)
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

    dptree::entry()
        .branch(schema)
        .branch(callback_schema)
        .branch(inline_schema)
}

