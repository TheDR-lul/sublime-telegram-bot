use chrono::{DateTime, Datelike, Duration, Timelike, Utc};
use chrono_tz::Europe::Kyiv;
use rand::prelude::*;
use sqlx::PgPool;
use teloxide::prelude::*;
use teloxide::sugar::request::RequestLinkPreviewExt;
use teloxide::types::{ChatId, Message};
use teloxide::utils::html::escape as escape_html;

use crate::db::game;
use crate::db::models::TgUser;
use crate::db::user;
use crate::db::achievements;
use crate::error::AppError;
use crate::handlers::game::phrases::{
    stage1, stage2, stage3, stage4, text_static,
};

const GAME_RESULT_TIME_DELAY_SECS: u64 = 2;

fn current_datetime_kyiv() -> DateTime<chrono_tz::Tz> {
    Utc::now().with_timezone(&Kyiv)
}

pub async fn pidorules_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
) -> Result<(), AppError> {
    tracing::info!("Game rules requested");
    let rules = "Правила игры <b>Пидор Дня</b> (только для групповых чатов):\n\
<b>1.</b> Зарегистрируйтесь в игру по команде /pidoreg\n\
<b>2.</b> Подождите пока зарегиструются все (или большинство :)\n\
<b>3.</b> Запустите розыгрыш по команде /pidor\n\
<b>4.</b> Просмотр статистики канала по команде /pidorstats, /pidorall\n\
<b>5.</b> Личная статистика по команде /pidorme\n\
<b>6.</b> Статистика за последний год по комнаде /pidor2020 (так же есть за 2016-2020)\n\
<b>7. (!!! Только для администраторов чатов)</b>: удалить из игры может только Админ канала, сначала выведя по команде список игроков: /pidormin list\n\
Удалить же игрока можно по команде (используйте идентификатор пользователя - цифры из списка пользователей): /pidormin del 123456\n\
\n\
<b>Важно</b>, розыгрыш проходит только <b>раз в день</b>, повторная команда выведет <b>результат</b> игры.\n\
\n\
Сброс розыгрыша происходит каждый день в 12 часов ночи по киевскому времени (UTC+2 / UTC+3 в зависимости от сезона).\n\n\
Поддержать бота можно по <a href=\"https://github.com/TheDR-lul/sublime\">ссылке</a> :)";
    bot.send_message(msg.chat.id, rules)
        .parse_mode(teloxide::types::ParseMode::Html)
        .disable_link_preview(true)
        .await?;
    Ok(())
}

pub async fn pidoreg_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    let chat_id = msg.chat.id.0;
    let from_user = msg.from.as_ref().ok_or_else(|| AppError::Config("No from user".into()))?;
    
    let tg_user = user::upsert_tg_user(&pool, from_user).await?;
    let game = game::get_or_create_game(&pool, chat_id).await?;
    
    let is_player = game::is_player_in_game(&pool, game.id, tg_user.id).await?;
    if is_player {
        bot.send_message(msg.chat.id, text_static::ERROR_ALREADY_REGISTERED)
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;
        return Ok(());
    }
    
    game::add_player(&pool, game.id, tg_user.id).await?;
    let players = game::get_players(&pool, game.id).await?;
    
    if players.is_empty() {
        let username = from_user.full_name();
        bot.send_message(msg.chat.id, text_static::ERROR_ZERO_PLAYERS.replace("{username}", &escape_html(&username)))
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;
        return Ok(());
    }
    bot.send_message(msg.chat.id, text_static::REGISTRATION_SUCCESS)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;

    // Achievement: first registration in game.
    if achievements::grant(&pool, tg_user.id, "first_pidoreg").await? {
        bot.send_message(
            msg.chat.id,
            "🏅 Новая ачивка: Я в деле (первая регистрация в игре).",
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
    let chat_id = msg.chat.id.0;
    let from_user = msg.from.as_ref().ok_or_else(|| AppError::Config("No from user".into()))?;
    
    let tg_user = user::upsert_tg_user(&pool, from_user).await?;
    let game = game::get_or_create_game(&pool, chat_id).await?;
    
    if game::remove_player(&pool, game.id, tg_user.id).await? {
        bot.send_message(msg.chat.id, text_static::REMOVE_REGISTRATION)
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;
    } else {
        bot.send_message(msg.chat.id, text_static::REMOVE_REGISTRATION_ERROR)
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;
    }
    Ok(())
}

pub async fn pidor_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    let chat_id = msg.chat.id;
    run_pidor_game(&bot, &pool, chat_id, false).await
}

/// Run Pidor game for a chat. When result for today already exists: manual shows it, autorun skips (no spam).
async fn run_pidor_game(
    bot: &Bot,
    pool: &PgPool,
    chat_id: ChatId,
    is_autorun: bool,
) -> Result<(), AppError> {
    let chat_id_raw = chat_id.0;
    let game = game::get_or_create_game(pool, chat_id_raw).await?;
    
    tracing::info!("Game {} of the day started (chat_id={}, autorun={})", game.id, chat_id_raw, is_autorun);
    let players = game::get_players(pool, game.id).await?;
    
    if players.len() < 2 {
        bot.send_message(chat_id, text_static::ERROR_NOT_ENOUGH_PLAYERS)
            .await?;
        return Ok(());
    }
    
    let current_dt = current_datetime_kyiv();
    let cur_year = current_dt.year();
    let cur_day = current_dt.ordinal() as i32;
    let last_day = current_dt.month() == 12 && current_dt.day() == 31;
    
    if let Some(result) = game::get_today_result(pool, game.id, cur_year, cur_day).await? {
        if is_autorun {
            return Ok(());
        }
        let winner = game::get_user_by_id(pool, result.winner_id)
            .await?
            .ok_or_else(|| AppError::Config("Winner not found".into()))?;
        let text = text_static::CURRENT_DAY_GAME_RESULT.replace(
            "{username}",
            &escape_html(&winner.full_username(false)),
        );
        bot.send_message(chat_id, text)
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;
        return Ok(());
    }
    
    let mut rng = rand::make_rng::<rand::rngs::StdRng>();
    let winner = players.choose(&mut rng).unwrap();
    
    game::insert_result(pool, game.id, winner.id, cur_year, cur_day).await?;
    
    if last_day {
        let announcement = text_static::YEAR_RESULTS_ANNOUNCEMENT.replace("{year}", &cur_year.to_string());
        bot.send_message(chat_id, &announcement)
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;
    }
    
    if is_autorun {
        bot.send_message(chat_id, text_static::SUDDEN_PIDOR_ACTIVATED).await?;
    }
    
    let stage1_text = stage1::PHRASES.choose(&mut rng).unwrap();
    bot.send_message(chat_id, *stage1_text).await?;
    tokio::time::sleep(tokio::time::Duration::from_secs(GAME_RESULT_TIME_DELAY_SECS)).await;
    
    let stage2_text = stage2::PHRASES.choose(&mut rng).unwrap();
    bot.send_message(chat_id, *stage2_text).await?;
    tokio::time::sleep(tokio::time::Duration::from_secs(GAME_RESULT_TIME_DELAY_SECS)).await;
    
    let stage3_text = stage3::PHRASES.choose(&mut rng).unwrap();
    bot.send_message(chat_id, *stage3_text).await?;
    tokio::time::sleep(tokio::time::Duration::from_secs(GAME_RESULT_TIME_DELAY_SECS)).await;
    
    let phrase = stage4::PHRASES.choose(&mut rng).unwrap();
    let stage4_text = phrase.replace("{username}", &escape_html(&winner.full_username(true)));
    bot.send_message(chat_id, stage4_text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;

    // Achievements based on updated stats after today's game.
    if let Some((_user, count)) = game::stats_personal(pool, game.id, winner.id).await? {
        if count == 1 && achievements::grant(pool, winner.id, "first_pidor_win").await? {
            bot.send_message(
                chat_id,
                "🥇 Новая ачивка: Первый пошёл (первая победа в Пидор Дня).",
            )
            .await?;
        }
        if count >= 3 && achievements::grant(pool, winner.id, "three_pidor_wins").await? {
            bot.send_message(
                chat_id,
                "🏆 Новая ачивка: Почётный пидор чата (3 победы).",
            )
            .await?;
        }
    }

    // Achievement: two wins in a row (previous day winner is same user).
    let prev_dt = current_datetime_kyiv() - Duration::days(1);
    let prev_year = prev_dt.year();
    let prev_day = prev_dt.ordinal() as i32;
    if let Some(prev_res) =
        game::get_today_result(pool, game.id, prev_year, prev_day).await?
        && prev_res.winner_id == winner.id
        && achievements::grant(pool, winner.id, "pidor_series_2").await?
    {
        bot.send_message(
            chat_id,
            "🔥 Новая ачивка: Пидор‑серийник (2 победы подряд).",
        )
        .await?;
    }

    // Achievement: night win.
    let hour = current_dt.hour();
    if (0..6).contains(&hour)
        && achievements::grant(pool, winner.id, "night_pidor").await?
    {
        bot.send_message(
            chat_id,
            "🌙 Новая ачивка: Ночной пидор (победа ночью).",
        )
        .await?;
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
pub async fn run_pidor_autorun_scheduler(bot: Bot, pool: PgPool) {
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
                let fire_m = *minutes_range.choose(&mut rng).unwrap();
                (fire_m / 60, fire_m % 60)
            });

            let fire_minutes = fire_at.0 * 60 + fire_at.1;
            if now_minutes >= fire_minutes {
                fired.insert(key);
                if let Err(err) = run_pidor_autorun_for_all_games(&bot, &pool, true).await {
                    tracing::error!("Pidor autorun scheduler error: {:?}", err);
                }
            }
        }

        sleep(Duration::from_secs(30)).await;
    }
}

async fn run_pidor_autorun_for_all_games(bot: &Bot, pool: &PgPool, is_autorun: bool) -> Result<(), AppError> {
    let games = game::list_games(pool).await?;
    for g in games {
        let chat_id = ChatId(g.chat_id);
        if let Err(err) = run_pidor_game(bot, pool, chat_id, is_autorun).await {
            tracing::error!(
                "Failed to run autorun Pidor game for chat {}: {:?}",
                g.chat_id,
                err
            );
        }
    }
    Ok(())
}

fn build_player_table(player_list: &[(TgUser, i64)]) -> String {
    let mut result = String::new();
    for (number, (tg_user, amount)) in player_list.iter().enumerate() {
        result.push_str(&text_static::STATS_LIST_ITEM
            .replace("{number}", &(number + 1).to_string())
            .replace("{username}", &escape_html(&tg_user.full_username(false)))
            .replace("{amount}", &amount.to_string()));
    }
    result
}

pub async fn pidorstats_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    let chat_id = msg.chat.id.0;
    let game = game::get_or_create_game(&pool, chat_id).await?;
    let current_dt = current_datetime_kyiv();
    let cur_year = current_dt.year();
    
    let db_results = game::stats_current_year(&pool, game.id, cur_year).await?;
    let players = game::get_players(&pool, game.id).await?;
    
    let player_table = build_player_table(&db_results);
    let answer = text_static::STATS_CURRENT_YEAR
        .replace("{player_stats}", &player_table)
        .replace("{player_count}", &players.len().to_string());
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
    let chat_id = msg.chat.id.0;
    let game = game::get_or_create_game(&pool, chat_id).await?;
    
    let db_results = game::stats_all_time(&pool, game.id).await?;
    let players = game::get_players(&pool, game.id).await?;
    
    let player_table = build_player_table(&db_results);
    let answer = text_static::STATS_ALL_TIME
        .replace("{player_stats}", &player_table)
        .replace("{player_count}", &players.len().to_string());
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
    let chat_id = msg.chat.id.0;
    let from_user = msg.from.as_ref().ok_or_else(|| AppError::Config("No from user".into()))?;
    
    let tg_user = user::upsert_tg_user(&pool, from_user).await?;
    let game = game::get_or_create_game(&pool, chat_id).await?;
    
    if let Some((user, count)) = game::stats_personal(&pool, game.id, tg_user.id).await? {
        let text = text_static::STATS_PERSONAL
            .replace("{username}", &escape_html(&user.full_username(false)))
            .replace("{amount}", &count.to_string());
        bot.send_message(msg.chat.id, text)
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;
    } else {
        let text = text_static::STATS_PERSONAL
            .replace("{username}", &escape_html(&tg_user.full_username(false)))
            .replace("{amount}", "0");
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
        bot.send_message(msg.chat.id, text_static::ERROR_ZERO_PLAYERS.replace("{username}", &escape_html(&from_user)))
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;
        return Ok(());
    }
    
    let player_table = build_player_table(&db_results);
    let answer = text_static::YEAR_RESULTS_MSG
        .replace("{year}", &year.to_string())
        .replace("{username}", &escape_html(&db_results[0].0.full_username(false)))
        .replace("{player_list}", &player_table);
    bot.send_message(msg.chat.id, answer)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
    Ok(())
}
