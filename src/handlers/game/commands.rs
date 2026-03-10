use chrono::{DateTime, Datelike, Duration, Timelike, Utc};
use chrono_tz::Europe::Kyiv;
use rand::prelude::*;
use sqlx::PgPool;
use teloxide::prelude::*;
use teloxide::sugar::request::RequestLinkPreviewExt;
use teloxide::types::{CallbackQuery, ChatId, ChatMemberStatus, InlineKeyboardButton, InlineKeyboardMarkup, Message};
use teloxide::utils::html::escape as escape_html;

use crate::db::game;
use crate::db::models::TgUser;
use crate::db::user;
use crate::db::achievements;
use crate::error::AppError;
use crate::i18n::LOCALE;

use tokio_util::sync::CancellationToken;

const GAME_RESULT_TIME_DELAY_SECS: u64 = 2;

fn pidorules_html(lang: &str) -> &'static str {
    LOCALE.t(lang, "pidor.static.rules_html")
}

fn current_datetime_kyiv() -> DateTime<chrono_tz::Tz> {
    Utc::now().with_timezone(&Kyiv)
}

/// Menu/rules message auto-delete: 30 sec from last user interaction.
const RULES_AND_MENU_DELETE_AFTER_SECS: u64 = 30;

/// Schedules deletion of a message after RULES_AND_MENU_DELETE_AFTER_SECS. Fire-and-forget.
pub fn schedule_delete_message(bot: Bot, chat_id: ChatId, message_id: teloxide::types::MessageId) {
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(RULES_AND_MENU_DELETE_AFTER_SECS)).await;
        let _ = bot.delete_message(chat_id, message_id).await;
    });
}

pub async fn pidorules_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    tracing::info!("Game rules requested");
    if let Some(ref from) = msg.from {
        let tg_user = user::upsert_tg_user(&pool, from).await?;
        game::record_chat_member(&pool, msg.chat.id.0, tg_user.id).await?;
    }
    let invoker = msg.from.as_ref().map(|u| u.id.0 as i64);
    send_pidorules(&bot, msg.chat.id, invoker).await
}

/// Send rules. In groups, tries to send to invoker's PM so only they see it; falls back to chat.
/// Schedules delete of the rules message (and optional "sent to PM" notice) after 1 minute.
pub async fn send_pidorules(
    bot: &Bot,
    chat_id: ChatId,
    invoker_user_id: Option<i64>,
) -> Result<(), AppError> {
    let is_group = chat_id.0 < 0;
    let target = if is_group && invoker_user_id.is_some() {
        let pm = ChatId(invoker_user_id.unwrap());
        let r = bot
            .send_message(pm, pidorules_html("ru"))
            .parse_mode(teloxide::types::ParseMode::Html)
            .disable_link_preview(true)
            .await;
        match r {
            Ok(sent) => {
                schedule_delete_message(bot.clone(), pm, sent.id);
                if let Ok(notice) = bot
                    .send_message(chat_id, LOCALE.t("ru", "pidor.static.rules_sent_to_pm"))
                    .await
                {
                    schedule_delete_message(bot.clone(), chat_id, notice.id);
                }
                return Ok(());
            }
            Err(_) => chat_id,
        }
    } else {
        chat_id
    };
    let sent = bot
        .send_message(target, pidorules_html("ru"))
        .parse_mode(teloxide::types::ParseMode::Html)
        .disable_link_preview(true)
        .await?;
    schedule_delete_message(bot.clone(), target, sent.id);
    Ok(())
}

pub async fn send_pidorstats(bot: &Bot, pool: &PgPool, chat_id: ChatId) -> Result<(), AppError> {
    let cid = chat_id.0;
    let game = game::get_or_create_game(pool, cid).await?;
    let current_dt = current_datetime_kyiv();
    let cur_year = current_dt.year();
    let db_results = game::stats_current_year(pool, game.id, cur_year).await?;
    let players = game::get_players(pool, game.id).await?;
    let player_table = build_player_table(&db_results, &game.lang);
    let answer = LOCALE.t_fmt(&game.lang, "pidor.static.stats_current_year", &[
        ("player_stats", &player_table),
        ("player_count", &players.len().to_string()),
    ]);
    bot.send_message(chat_id, answer)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
    Ok(())
}

pub async fn send_pidorall(bot: &Bot, pool: &PgPool, chat_id: ChatId) -> Result<(), AppError> {
    let cid = chat_id.0;
    let game = game::get_or_create_game(pool, cid).await?;
    let db_results = game::stats_all_time(pool, game.id).await?;
    let players = game::get_players(pool, game.id).await?;
    let player_table = build_player_table(&db_results, &game.lang);
    let answer = LOCALE.t_fmt(&game.lang, "pidor.static.stats_all_time", &[
        ("player_stats", &player_table),
        ("player_count", &players.len().to_string()),
    ]);
    bot.send_message(chat_id, answer)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
    Ok(())
}

pub async fn pidoreg_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    if !msg.chat.is_group() && !msg.chat.is_supergroup() {
        return Ok(());
    }

    let chat_id = msg.chat.id.0;
    let from_user = match msg.from.as_ref() {
        Some(u) => u,
        None => {
            let game = game::get_or_create_game(&pool, chat_id).await?;
            bot.send_message(
                msg.chat.id,
                LOCALE.t(&game.lang, "pidor.errors.anon_only"),
            )
            .await?;
            return Ok(());
        }
    };

    let tg_user = user::upsert_tg_user(&pool, from_user).await?;
    let game = game::get_or_create_game(&pool, chat_id).await?;

    // Check by Telegram id from message so we never miss "already registered" (roast).
    let players = game::get_players(&pool, game.id).await?;
    let tg_id_i64 = from_user.id.0 as i64;
    let already_registered = players.iter().any(|p| p.tg_id == tg_id_i64);
    if already_registered {
        let username = escape_html(&from_user.full_name());
        let text = LOCALE.t_rand_fmt(&game.lang, "pidor.already_registered", &[("username", &username)]);
        bot.send_message(msg.chat.id, text)
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;
        return Ok(());
    }

    game::record_chat_member(&pool, chat_id, tg_user.id).await?;
    game::add_player(&pool, game.id, tg_user.id).await?;
    let players = game::get_players(&pool, game.id).await?;
    if players.is_empty() {
        let username = from_user.full_name();
        let text = LOCALE.t_fmt(&game.lang, "pidor.errors.zero_players", &[("username", &escape_html(&username))]);
        bot.send_message(msg.chat.id, text)
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;
        return Ok(());
    }
    bot.send_message(msg.chat.id, LOCALE.t(&game.lang, "pidor.static.registration_success"))
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;

    // Achievement: first registration in game.
    if achievements::grant(&pool, tg_user.id, "first_pidoreg").await? {
        bot.send_message(
            msg.chat.id,
            LOCALE.t(&game.lang, "achievements.notifications.first_pidoreg"),
        )
        .await?;
    }
    Ok(())
}

pub async fn pidorunreg_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    if !msg.chat.is_group() && !msg.chat.is_supergroup() {
        return Ok(());
    }
    let chat_id = msg.chat.id.0;
    let from_user = msg
        .from
        .as_ref()
        .ok_or_else(|| AppError::GameLogic("No from user".into()))?;
    
    let tg_user = user::upsert_tg_user(&pool, from_user).await?;
    let game = game::get_or_create_game(&pool, chat_id).await?;
    
    if game::remove_player(&pool, game.id, tg_user.id).await? {
        bot.send_message(msg.chat.id, LOCALE.t(&game.lang, "pidor.static.remove_registration"))
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;
    } else {
        bot.send_message(msg.chat.id, LOCALE.t(&game.lang, "pidor.static.remove_not_registered"))
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;
    }
    Ok(())
}

/// Who triggered the run: manual = "Pidor of the Day" (1/day), autorun = morning/day/evening (3/day).
#[derive(Clone, Copy, Debug)]
enum PidorRunKind {
    Manual,
    Autorun(PidorAutorunSlot),
}

pub async fn pidor_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    if !msg.chat.is_group() && !msg.chat.is_supergroup() {
        return Ok(());
    }
    let chat_id = msg.chat.id;
    if let Some(ref from) = msg.from {
        let tg_user = user::upsert_tg_user(&pool, from).await?;
        game::record_chat_member(&pool, chat_id.0, tg_user.id).await?;
    }
    run_pidor_game(&bot, &pool, chat_id, PidorRunKind::Manual).await
}

pub async fn pidorbet_handler(
    bot: Bot,
    msg: Message,
    cmd: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    if !msg.chat.is_group() && !msg.chat.is_supergroup() {
        return Ok(());
    }
    let chat_id = msg.chat.id.0;
    let from_user = match msg.from.as_ref() {
        Some(u) => u,
        None => return Ok(()),
    };
    let bettor_tg_id = from_user.id.0 as i64;

    let tg_user = user::upsert_tg_user(&pool, from_user).await?;
    let g = game::get_or_create_game(&pool, chat_id).await?;
    let lang = g.lang.as_str();
    let players = game::get_players(&pool, g.id).await?;
    if !players.iter().any(|p| p.tg_id == bettor_tg_id) {
        bot.send_message(msg.chat.id, LOCALE.t(lang, "bet.not_registered"))
            .await?;
        return Ok(());
    }

    let current_dt = current_datetime_kyiv();
    let cur_year = current_dt.year();
    let cur_day = current_dt.ordinal() as i32;
    let slot = "manual";

    if game::get_today_result_for_slot(&pool, g.id, cur_year, cur_day, slot).await?.is_some() {
        bot.send_message(msg.chat.id, LOCALE.t(lang, "bet.already_played"))
            .await?;
        return Ok(());
    }

    let target_tg_id = extract_bet_target(&msg, &cmd, &pool).await;
    let target_tg_id = match target_tg_id {
        Some(id) if id == bettor_tg_id => id,
        Some(id) => {
            if !players.iter().any(|p| p.tg_id == id) {
                bot.send_message(msg.chat.id, LOCALE.t(lang, "bet.target_not_in_game"))
                    .await?;
                return Ok(());
            }
            id
        }
        None => {
            bot.send_message(msg.chat.id, LOCALE.t(lang, "bet.usage"))
                .await?;
            return Ok(());
        }
    };

    let _ = crate::db::bet::place_bet(&pool, chat_id, bettor_tg_id, target_tg_id, cur_year, cur_day, slot).await?;

    let bettor_name = escape_html(&from_user.full_name());
    let target_name = if target_tg_id == bettor_tg_id {
        "себя".to_string()
    } else {
        let target_user = user::get_by_tg_id(&pool, target_tg_id).await?;
        match target_user {
            Some(u) => escape_html(&u.full_username(true)),
            None => "???".to_string(),
        }
    };

    bot.send_message(
        msg.chat.id,
        LOCALE.t_fmt(lang, "bet.placed", &[("bettor", &bettor_name), ("target", &target_name)]),
    )
    .parse_mode(teloxide::types::ParseMode::Html)
    .await?;

    if achievements::grant(&pool, tg_user.id, "bet_first").await? {
        bot.send_message(msg.chat.id, LOCALE.t(lang, "achievements.notifications.bet_first"))
            .await?;
    }

    Ok(())
}

/// Extract bet target tg_id from the command.
/// Priority is delegated to the shared target resolver.
async fn extract_bet_target(msg: &Message, cmd: &crate::handlers::commands::Cmd, pool: &PgPool) -> Option<i64> {
    if let crate::handlers::commands::Cmd::Pidorbet(arg) = cmd {
        let arg_str = arg.as_str();
        let resolved = crate::telegram::target_resolver::resolve_target(pool, msg, arg_str).await;
        match resolved {
            crate::telegram::target_resolver::ResolvedTarget::User(id) => Some(id),
            crate::telegram::target_resolver::ResolvedTarget::IsBot => None,
            crate::telegram::target_resolver::ResolvedTarget::NotFound => None,
        }
    } else {
        None
    }
}

fn slot_str(slot: PidorAutorunSlot) -> &'static str {
    match slot {
        PidorAutorunSlot::Morning => "morning",
        PidorAutorunSlot::Day => "day",
        PidorAutorunSlot::Evening => "evening",
    }
}

/// Run Pidor game. Manual = "Pidor of the Day" (1/day, slot=manual). Autorun = morning/day/evening (each 1/day, own slot).
async fn run_pidor_game(
    bot: &Bot,
    pool: &PgPool,
    chat_id: ChatId,
    run_kind: PidorRunKind,
) -> Result<(), AppError> {
    let (slot, is_manual) = match run_kind {
        PidorRunKind::Manual => ("manual", true),
        PidorRunKind::Autorun(s) => (slot_str(s), false),
    };
    let chat_id_raw = chat_id.0;
    let game = game::get_or_create_game(pool, chat_id_raw).await?;
    let lang = game.lang.as_str();
    
    tracing::info!("Game {} (chat_id={}, slot={})", game.id, chat_id_raw, slot);
    let players = game::get_players(pool, game.id).await?;
    
    if players.len() < 2 {
        if is_manual {
            bot.send_message(chat_id, LOCALE.t(lang, "pidor.errors.not_enough_players"))
                .await?;
        }
        return Ok(());
    }
    
    let current_dt = current_datetime_kyiv();
    let cur_year = current_dt.year();
    let cur_day = current_dt.ordinal() as i32;
    let last_day = current_dt.month() == 12 && current_dt.day() == 31;
    
    if let Some(result) = game::get_today_result_for_slot(pool, game.id, cur_year, cur_day, slot).await? {
        if is_manual {
            let winner = game::get_user_by_id(pool, result.winner_id)
                .await?
                .ok_or_else(|| AppError::NotFound("Winner not found".into()))?;
            let text = LOCALE.t_fmt(lang, "pidor.static.current_result", &[
                ("username", &escape_html(&winner.full_username(false))),
            ]);
            bot.send_message(chat_id, text)
                .parse_mode(teloxide::types::ParseMode::Html)
                .await?;
        }
        return Ok(());
    }
    
    let mut rng = rand::make_rng::<rand::rngs::StdRng>();
    let winner = players
        .choose(&mut rng)
        .expect("at least one player in game");
    
    game::insert_result_with_slot(pool, game.id, winner.id, cur_year, cur_day, slot).await?;

    // Award pidor_elo for winning the daily pidor game.
    let _ = crate::db::duel::add_pidor_elo(pool, chat_id_raw, winner.tg_id, 10).await;

    if last_day && is_manual {
        let announcement = LOCALE.t_fmt(lang, "pidor.static.year_announcement", &[("year", &cur_year.to_string())]);
        bot.send_message(chat_id, &announcement)
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;
    }
    
    if !is_manual {
        let key = match run_kind {
            PidorRunKind::Manual => unreachable!(),
            PidorRunKind::Autorun(PidorAutorunSlot::Morning) => "pidor.static.sudden_morning",
            PidorRunKind::Autorun(PidorAutorunSlot::Day) => "pidor.static.sudden_day",
            PidorRunKind::Autorun(PidorAutorunSlot::Evening) => "pidor.static.sudden_evening",
        };
        bot.send_message(chat_id, LOCALE.t(lang, key)).await?;
    }
    
    bot.send_message(chat_id, LOCALE.t_rand(lang, "pidor.stage1")).await?;
    tokio::time::sleep(tokio::time::Duration::from_secs(GAME_RESULT_TIME_DELAY_SECS)).await;
    
    bot.send_message(chat_id, LOCALE.t_rand(lang, "pidor.stage2")).await?;
    tokio::time::sleep(tokio::time::Duration::from_secs(GAME_RESULT_TIME_DELAY_SECS)).await;
    
    bot.send_message(chat_id, LOCALE.t_rand(lang, "pidor.stage3")).await?;
    tokio::time::sleep(tokio::time::Duration::from_secs(GAME_RESULT_TIME_DELAY_SECS)).await;
    
    let mut stage4_text = LOCALE.t_rand_fmt(lang, "pidor.stage4", &[
        ("username", &escape_html(&winner.full_username(true))),
    ]);
    // Replace "пидор дня/пидором дня" with correct slot for autorun.
    if !is_manual {
        let slot_name = match run_kind {
            PidorRunKind::Manual => unreachable!(),
            PidorRunKind::Autorun(PidorAutorunSlot::Morning) => "утра",
            PidorRunKind::Autorun(PidorAutorunSlot::Day) => "дня",
            PidorRunKind::Autorun(PidorAutorunSlot::Evening) => "вечера",
        };
        stage4_text = stage4_text.replace("пидором дня", &format!("пидором {}", slot_name));
        stage4_text = stage4_text.replace("пидор дня", &format!("пидор {}", slot_name));
    }
    bot.send_message(chat_id, stage4_text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;

    // Achievements only for "Pidor of the Day" (manual run).
    if is_manual {
        if let Some((_user, count)) = game::stats_personal(pool, game.id, winner.id).await? {
            if count == 1 && achievements::grant(pool, winner.id, "first_pidor_win").await? {
                bot.send_message(chat_id, LOCALE.t(lang, "achievements.notifications.first_pidor_win")).await?;
            }
            if count >= 3 && achievements::grant(pool, winner.id, "three_pidor_wins").await? {
                bot.send_message(chat_id, LOCALE.t(lang, "achievements.notifications.three_wins")).await?;
            }
        }
        let prev_dt = current_datetime_kyiv() - Duration::days(1);
        let prev_year = prev_dt.year();
        let prev_day = prev_dt.ordinal() as i32;
        if let Some(prev_res) =
            game::get_today_result_for_slot(pool, game.id, prev_year, prev_day, "manual").await?
            && prev_res.winner_id == winner.id
            && achievements::grant(pool, winner.id, "pidor_series_2").await?
        {
            bot.send_message(chat_id, LOCALE.t(lang, "achievements.notifications.series_2")).await?;
        }
        let hour = current_dt.hour();
        if (0..6).contains(&hour) && achievements::grant(pool, winner.id, "night_pidor").await? {
            bot.send_message(chat_id, LOCALE.t(lang, "achievements.notifications.night_pidor")).await?;
        }
    }

    // Resolve bets
    let winner_tg_id = winner.tg_id;
    let correct_bets = crate::db::bet::resolve_bets(pool, chat_id_raw, cur_year, cur_day, slot, winner_tg_id).await?;
    if !correct_bets.is_empty() {
        let mut names = Vec::new();
        for bet in &correct_bets {
            let u = user::get_by_tg_id(pool, bet.bettor_tg_id).await?;
            let name = u.map(|u| escape_html(&u.full_username(true))).unwrap_or_else(|| "???".to_string());
            names.push(name);
        }
        let joined = names.join(", ");
        let suffix = if correct_bets.len() > 1 {
            LOCALE.t(lang, "bet.winners_suffix_many")
        } else {
            LOCALE.t(lang, "bet.winners_suffix_one")
        };
        let text = LOCALE.t_fmt(lang, "bet.winners", &[("suffix", suffix), ("names", &joined)]);
        bot.send_message(chat_id, text)
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;

        for bet in &correct_bets {
            // Award pidor_elo for correct bet.
            let _ = crate::db::duel::add_pidor_elo(pool, chat_id_raw, bet.bettor_tg_id, 15).await;

            if let Some(uid) = crate::db::duel::user_id_by_tg_id(pool, bet.bettor_tg_id).await? {
                let total_correct = crate::db::bet::count_correct_bets(pool, chat_id_raw, bet.bettor_tg_id).await?;
                if total_correct >= 1 && achievements::grant(pool, uid, "bet_correct_1").await? {
                    bot.send_message(chat_id, LOCALE.t(lang, "achievements.notifications.bet_correct_1")).await?;
                }
                if total_correct >= 3 && achievements::grant(pool, uid, "bet_correct_3").await? {
                    bot.send_message(chat_id, LOCALE.t(lang, "achievements.notifications.bet_correct_3")).await?;
                }
                if bet.bettor_tg_id == bet.target_tg_id && achievements::grant(pool, uid, "bet_self_correct").await? {
                    bot.send_message(chat_id, LOCALE.t(lang, "achievements.notifications.bet_self_correct")).await?;
                }
                let streak = crate::db::bet::count_correct_streak(pool, chat_id_raw, bet.bettor_tg_id).await?;
                if streak >= 3 && achievements::grant(pool, uid, "bet_streak_3").await? {
                    bot.send_message(chat_id, LOCALE.t(lang, "achievements.notifications.bet_streak_3")).await?;
                }
            }
        }
    }

    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum PidorAutorunSlot {
    Morning,  // 8-10
    Day,      // 14-16
    Evening,  // 20-22
}

impl PidorAutorunSlot {
    fn window_hours(self) -> (i32, i32) {
        match self {
            PidorAutorunSlot::Morning => (8, 10),
            PidorAutorunSlot::Day => (14, 16),
            PidorAutorunSlot::Evening => (20, 22),
        }
    }
}

/// Background scheduler: runs Pidor game 3 times per day (morning/day/evening) in Kyiv timezone.
/// Each slot fires at a random time within a 2-hour window.
pub async fn run_pidor_autorun_scheduler(bot: Bot, pool: PgPool, shutdown: CancellationToken) {
    use std::collections::{HashMap, HashSet};
    use tokio::time::{sleep, Duration};

    let mut fired: HashSet<(i32, i32, PidorAutorunSlot)> = HashSet::new();
    let mut scheduled: HashMap<(i32, i32, PidorAutorunSlot), (i32, i32)> = HashMap::new();

    loop {
        let now = current_datetime_kyiv();
        let year = now.year();
        let day = now.ordinal() as i32;
        let hour = now.hour() as i32;
        let minute = now.minute() as i32;
        let now_minutes = hour * 60 + minute;

        fired.retain(|&(y, d, _)| y == year && d == day);
        scheduled.retain(|&(y, d, _), _| y == year && d == day);

        for slot in [PidorAutorunSlot::Morning, PidorAutorunSlot::Day, PidorAutorunSlot::Evening] {
            let key = (year, day, slot);
            if fired.contains(&key) {
                continue;
            }
            let (start_h, end_h) = slot.window_hours();
            let start_m = start_h * 60;
            let end_m = end_h * 60;
            let in_window = now_minutes >= start_m && now_minutes <= end_m;

            if !in_window {
                continue;
            }

            let fire_at = *scheduled.entry(key).or_insert_with(|| {
                let mut rng = rand::make_rng::<rand::rngs::StdRng>();
                let minutes_range: Vec<i32> = (start_m..=end_m).collect();
                let fire_m = *minutes_range
                .choose(&mut rng)
                .expect("minutes_range is non-empty");
                (fire_m / 60, fire_m % 60)
            });

            let fire_minutes = fire_at.0 * 60 + fire_at.1;
            if now_minutes >= fire_minutes {
                fired.insert(key);
                if let Err(err) = run_pidor_autorun_for_all_games(&bot, &pool, slot).await {
                    tracing::error!("Pidor autorun scheduler error: {:?}", err);
                }
            }
        }

        tokio::select! {
            _ = shutdown.cancelled() => {
                break;
            }
            _ = sleep(Duration::from_secs(30)) => {}
        }
    }
}

async fn run_pidor_autorun_for_all_games(bot: &Bot, pool: &PgPool, slot: PidorAutorunSlot) -> Result<(), AppError> {
    let slot_column = match slot {
        PidorAutorunSlot::Morning => "autorun_morning",
        PidorAutorunSlot::Day => "autorun_day",
        PidorAutorunSlot::Evening => "autorun_evening",
    };
    let games = game::list_games_for_autorun_slot(pool, slot_column).await?;
    for g in games {
        let chat_id = ChatId(g.chat_id);
        if let Err(err) = run_pidor_game(bot, pool, chat_id, PidorRunKind::Autorun(slot)).await {
            tracing::error!(
                "Failed to run autorun Pidor game for chat {}: {:?}",
                g.chat_id,
                err
            );
        }
    }
    Ok(())
}

fn build_player_table(player_list: &[(TgUser, i64)], lang: &str) -> String {
    let mut result = String::new();
    for (number, (tg_user, amount)) in player_list.iter().enumerate() {
        result.push_str(&LOCALE.t_fmt(lang, "pidor.static.stats_list_item", &[
            ("number", &(number + 1).to_string()),
            ("username", &escape_html(&tg_user.full_username(false))),
            ("amount", &amount.to_string()),
        ]));
    }
    result
}

pub async fn pidorstats_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    if !msg.chat.is_group() && !msg.chat.is_supergroup() {
        return Ok(());
    }
    let chat_id = msg.chat.id.0;
    let game = game::get_or_create_game(&pool, chat_id).await?;
    let current_dt = current_datetime_kyiv();
    let cur_year = current_dt.year();
    
    let db_results = game::stats_current_year(&pool, game.id, cur_year).await?;
    let players = game::get_players(&pool, game.id).await?;
    
    let player_table = build_player_table(&db_results, &game.lang);
    let answer = LOCALE.t_fmt(&game.lang, "pidor.static.stats_current_year", &[
        ("player_stats", &player_table),
        ("player_count", &players.len().to_string()),
    ]);
    bot.send_message(msg.chat.id, answer)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
    Ok(())
}

pub async fn pidorall_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    if !msg.chat.is_group() && !msg.chat.is_supergroup() {
        return Ok(());
    }
    let chat_id = msg.chat.id.0;
    let game = game::get_or_create_game(&pool, chat_id).await?;
    
    let db_results = game::stats_all_time(&pool, game.id).await?;
    let players = game::get_players(&pool, game.id).await?;
    
    let player_table = build_player_table(&db_results, &game.lang);
    let answer = LOCALE.t_fmt(&game.lang, "pidor.static.stats_all_time", &[
        ("player_stats", &player_table),
        ("player_count", &players.len().to_string()),
    ]);
    bot.send_message(msg.chat.id, answer)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
    Ok(())
}

pub async fn pidorme_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    if !msg.chat.is_group() && !msg.chat.is_supergroup() {
        return Ok(());
    }
    let chat_id = msg.chat.id.0;
    let from_user = msg.from.as_ref().ok_or_else(|| AppError::Config("No from user".into()))?;
    
    let tg_user = user::upsert_tg_user(&pool, from_user).await?;
    let game = game::get_or_create_game(&pool, chat_id).await?;
    
    if let Some((user, count)) = game::stats_personal(&pool, game.id, tg_user.id).await? {
        let text = LOCALE.t_fmt(&game.lang, "pidor.static.stats_personal", &[
            ("username", &escape_html(&user.full_username(false))),
            ("amount", &count.to_string()),
        ]);
        bot.send_message(msg.chat.id, text)
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;
    } else {
        let text = LOCALE.t_fmt(&game.lang, "pidor.static.stats_personal", &[
            ("username", &escape_html(&tg_user.full_username(false))),
            ("amount", "0"),
        ]);
        bot.send_message(msg.chat.id, text)
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;
    }
    Ok(())
}

pub async fn pidoryear_handler(
    bot: Bot,
    msg: Message,
    year: i32,
    pool: PgPool,
) -> Result<(), AppError> {
    let chat_id = msg.chat.id.0;
    let game = game::get_or_create_game(&pool, chat_id).await?;
    
    let db_results = game::stats_year(&pool, game.id, year).await?;
    if db_results.is_empty() {
        let from_user = msg.from.as_ref().map(|u| u.full_name()).unwrap_or_else(|| "user".to_string());
        let text = LOCALE.t_fmt(&game.lang, "pidor.errors.zero_players", &[("username", &escape_html(&from_user))]);
        bot.send_message(msg.chat.id, text)
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;
        return Ok(());
    }
    
    let player_table = build_player_table(&db_results, &game.lang);
    let answer = LOCALE.t_fmt(&game.lang, "pidor.static.year_results", &[
        ("year", &year.to_string()),
        ("username", &escape_html(&db_results[0].0.full_username(false))),
        ("player_list", &player_table),
    ]);
    bot.send_message(msg.chat.id, answer)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
    Ok(())
}

fn autorun_settings_keyboard(g: &crate::db::models::Game) -> InlineKeyboardMarkup {
    let lang = g.lang.as_str();
    let on = "✅";
    let off = "❌";
    let autorun_label = LOCALE.t_fmt(lang, "pidor.settings.autorun_label", &[(
        "status",
        if g.autorun_enabled {
            LOCALE.t(lang, "pidor.settings.autorun_on")
        } else {
            LOCALE.t(lang, "pidor.settings.autorun_off")
        },
    )]);
    let row1 = vec![InlineKeyboardButton::callback(autorun_label, "settings:toggle:autorun")];
    let row2 = vec![
        InlineKeyboardButton::callback(
            format!("{} {}", LOCALE.t(lang, "pidor.settings.slot_morning"), if g.autorun_morning { on } else { off }),
            "settings:toggle:morning",
        ),
        InlineKeyboardButton::callback(
            format!("{} {}", LOCALE.t(lang, "pidor.settings.slot_day"), if g.autorun_day { on } else { off }),
            "settings:toggle:day",
        ),
        InlineKeyboardButton::callback(
            format!("{} {}", LOCALE.t(lang, "pidor.settings.slot_evening"), if g.autorun_evening { on } else { off }),
            "settings:toggle:evening",
        ),
    ];
    let row3 = vec![InlineKeyboardButton::callback(LOCALE.t(lang, "pidor.settings.back_btn"), "menu:admin")];
    InlineKeyboardMarkup::new(vec![row1, row2, row3])
}

pub async fn is_chat_admin(bot: &Bot, chat_id: ChatId, user_id: u64) -> bool {
    match bot.get_chat_member(chat_id, teloxide::types::UserId(user_id)).await {
        Ok(m) => matches!(
            m.status(),
            ChatMemberStatus::Administrator | ChatMemberStatus::Owner
        ),
        Err(_) => false,
    }
}

pub async fn pidorset_handler(bot: Bot, msg: Message, pool: PgPool) -> Result<(), AppError> {
    if !msg.chat.is_group() && !msg.chat.is_supergroup() {
        return Ok(());
    }
    let from_user = match msg.from.as_ref() {
        Some(u) => u,
        None => return Ok(()),
    };
    let user_id = from_user.id.0;
    if !is_chat_admin(&bot, msg.chat.id, user_id as u64).await {
        let game = game::get_or_create_game(&pool, msg.chat.id.0).await?;
        bot.send_message(msg.chat.id, LOCALE.t(&game.lang, "pidor.settings.admin_only"))
            .await?;
        return Ok(());
    }
    send_pidorset_message(&bot, &pool, msg.chat.id).await
}

const PIDORSET_TEXT: &str = "Внезапный пидор: включить/выключить целиком или по слотам (утро 8–10, день 14–16, вечер 20–22 по Киеву).";

/// Send autorun settings keyboard. Call only after checking chat is group and user is admin.
pub async fn send_pidorset_message(
    bot: &Bot,
    pool: &PgPool,
    chat_id: ChatId,
) -> Result<(), AppError> {
    let cid = chat_id.0;
    let g = game::get_or_create_game(pool, cid).await?;
    bot.send_message(chat_id, PIDORSET_TEXT)
        .reply_markup(autorun_settings_keyboard(&g))
        .await?;
    Ok(())
}

/// Edit existing message to show autorun settings (keeps single menu message). Call when opening from menu.
pub async fn edit_message_to_pidorset(
    bot: &Bot,
    pool: &PgPool,
    chat_id: ChatId,
    message_id: teloxide::types::MessageId,
) -> Result<(), AppError> {
    let cid = chat_id.0;
    let g = game::get_or_create_game(pool, cid).await?;
    bot.edit_message_text(chat_id, message_id, PIDORSET_TEXT)
        .reply_markup(autorun_settings_keyboard(&g))
        .await?;
    Ok(())
}

/// Tag users seen in this chat who are not registered; tell them to run /pidoreg. Admin-only.
pub async fn send_pidorcall_message(
    bot: &Bot,
    pool: &PgPool,
    chat_id: ChatId,
) -> Result<(), AppError> {
    let cid = chat_id.0;
    let unreg = game::get_unregistered_in_chat(pool, cid).await?;
    let game = game::get_or_create_game(pool, cid).await?;
    let lang = game.lang.as_str();
    let text = if unreg.is_empty() {
        LOCALE.t(lang, "pidor.settings.call_all_registered").to_string()
    } else {
        let mentions: Vec<String> = unreg
            .iter()
            .map(|u| {
                let name = escape_html(&u.full_username(false));
                format!(r#"<a href="tg://user?id={}">{}</a>"#, u.tg_id, name)
            })
            .collect();
        LOCALE.t_fmt(lang, "pidor.settings.call_unregistered", &[("unregistered_list", &mentions.join(", "))])
    };
    bot.send_message(chat_id, text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
    Ok(())
}

pub async fn pidorset_callback(
    bot: Bot,
    query: CallbackQuery,
    pool: PgPool,
) -> Result<(), AppError> {
    let chat_id = match query.message.as_ref().map(|m| m.chat().id.0) {
        Some(id) => id,
        None => return Ok(()),
    };
    let user_id = query.from.id.0;
    if !is_chat_admin(&bot, teloxide::types::ChatId(chat_id), user_id as u64).await {
        bot.answer_callback_query(query.id)
            .text("Только для администраторов чата.")
            .await?;
        return Ok(());
    }
    let data = query.data.as_deref().unwrap_or("");
    let parts: Vec<&str> = data.splitn(3, ':').collect();
    let key = match parts[..] {
        [a, b, c] if a == "settings" && b == "toggle" => c,
        _ => {
            bot.answer_callback_query(query.id).await?;
            return Ok(());
        }
    };
    let g = game::get_or_create_game(&pool, chat_id).await?;
    match key {
        "autorun" => {
            game::update_autorun_settings(&pool, chat_id, Some(!g.autorun_enabled), None, None, None).await?;
        }
        "morning" => {
            game::update_autorun_settings(&pool, chat_id, None, Some(!g.autorun_morning), None, None).await?;
        }
        "day" => {
            game::update_autorun_settings(&pool, chat_id, None, None, Some(!g.autorun_day), None).await?;
        }
        "evening" => {
            game::update_autorun_settings(&pool, chat_id, None, None, None, Some(!g.autorun_evening)).await?;
        }
        _ => {
            bot.answer_callback_query(query.id).await?;
            return Ok(());
        }
    }
    bot.answer_callback_query(query.id).await?;
    let g2 = game::get_or_create_game(&pool, chat_id).await?;
    if let Some(ref msg) = query.message {
        bot.edit_message_reply_markup(msg.chat().id, msg.id())
            .reply_markup(autorun_settings_keyboard(&g2))
            .await?;
    }
    Ok(())
}
