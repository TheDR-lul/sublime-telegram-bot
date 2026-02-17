use chrono::{DateTime, Datelike, Utc};
use chrono_tz::Europe::Moscow;
use rand::seq::SliceRandom;
use rand::{rngs::StdRng, SeedableRng};
use sqlx::PgPool;
use teloxide::prelude::*;
use teloxide::types::Message;
use teloxide::utils::markdown::escape;

use crate::db::game;
use crate::db::models::TgUser;
use crate::db::user;
use crate::error::AppError;
use crate::handlers::game::phrases::{
    stage1, stage2, stage3, stage4, text_static,
};

const GAME_RESULT_TIME_DELAY_SECS: u64 = 2;

fn current_datetime_moscow() -> DateTime<chrono_tz::Tz> {
    Utc::now().with_timezone(&Moscow)
}

fn escape_md2(s: &str) -> String {
    escape(s)
}

pub async fn pidorules_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
) -> Result<(), AppError> {
    tracing::info!("Game rules requested");
    let rules = r"Правила игры *Пидор Дня* \(только для групповых чатов\):\n\
*1\.* Зарегистрируйтесь в игру по команде */pidoreg*\n\
*2\.* Подождите пока зарегиструются все \(или большинство :\)\n\
*3\.* Запустите розыгрыш по команде */pidor*\n\
*4\.* Просмотр статистики канала по команде */pidorstats*, */pidorall*\n\
*5\.* Личная статистика по команде */pidorme*\n\
*6\.* Статистика за последний год по комнаде */pidor2020* \(так же есть за 2016\-2020\)\n\
*7\. \(\!\!\! Только для администраторов чатов\)*: удалить из игры может только Админ канала, сначала выведя по команде список игроков: */pidormin* list\n\
Удалить же игрока можно по команде \(используйте идентификатор пользователя \- цифры из списка пользователей\): */pidormin* del 123456\n\
\n\
*Важно*, розыгрыш проходит только *раз в день*, повторная команда выведет *результат* игры\.\n\
\n\
Сброс розыгрыша происходит каждый день в 12 часов ночи по UTC\+2 \(примерно в два часа ночи по Москве\)\.\n\n\
Поддержать бота можно по [ссылке](https://github.com/TheDR-lul/sublime) :\)";
    bot.send_message(msg.chat.id, rules)
        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
        .disable_web_page_preview(true)
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
    let from_user = msg.from().ok_or_else(|| AppError::Config("No from user".into()))?;
    
    let tg_user = user::upsert_tg_user(&pool, from_user).await?;
    let game = game::get_or_create_game(&pool, chat_id).await?;
    
    let is_player = game::is_player_in_game(&pool, game.id, tg_user.id).await?;
    if is_player {
        bot.send_message(msg.chat.id, text_static::ERROR_ALREADY_REGISTERED)
            .parse_mode(teloxide::types::ParseMode::MarkdownV2)
            .await?;
        return Ok(());
    }
    
    game::add_player(&pool, game.id, tg_user.id).await?;
    let players = game::get_players(&pool, game.id).await?;
    
    if players.is_empty() {
        let username = from_user.full_name();
        bot.send_message(msg.chat.id, &text_static::ERROR_ZERO_PLAYERS.replace("{username}", &escape_md2(&username)))
            .parse_mode(teloxide::types::ParseMode::MarkdownV2)
            .await?;
        return Ok(());
    }
    
    bot.send_message(msg.chat.id, text_static::REGISTRATION_SUCCESS)
        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
        .await?;
    Ok(())
}

pub async fn pidorunreg_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
    pool: PgPool,
) -> Result<(), AppError> {
    let chat_id = msg.chat.id.0;
    let from_user = msg.from().ok_or_else(|| AppError::Config("No from user".into()))?;
    
    let tg_user = user::upsert_tg_user(&pool, from_user).await?;
    let game = game::get_or_create_game(&pool, chat_id).await?;
    
    if game::remove_player(&pool, game.id, tg_user.id).await? {
        bot.send_message(msg.chat.id, text_static::REMOVE_REGISTRATION)
            .parse_mode(teloxide::types::ParseMode::MarkdownV2)
            .await?;
    } else {
        bot.send_message(msg.chat.id, text_static::REMOVE_REGISTRATION_ERROR)
            .parse_mode(teloxide::types::ParseMode::MarkdownV2)
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
    let chat_id = msg.chat.id.0;
    let game = game::get_or_create_game(&pool, chat_id).await?;
    
    tracing::info!("Game {} of the day started", game.id);
    let players = game::get_players(&pool, game.id).await?;
    
    if players.len() < 2 {
        bot.send_message(msg.chat.id, text_static::ERROR_NOT_ENOUGH_PLAYERS)
            .await?;
        return Ok(());
    }
    
    let current_dt = current_datetime_moscow();
    let cur_year = current_dt.year();
    let cur_day = current_dt.ordinal() as i32;
    let last_day = current_dt.month() == 12 && current_dt.day() == 31;
    
    if let Some(result) = game::get_today_result(&pool, game.id, cur_year, cur_day).await? {
        let winner = game::get_user_by_id(&pool, result.winner_id)
            .await?
            .ok_or_else(|| AppError::Config("Winner not found".into()))?;
        let text = text_static::CURRENT_DAY_GAME_RESULT.replace(
            "{username}",
            &escape_md2(&winner.full_username(false)),
        );
        bot.send_message(msg.chat.id, text)
            .parse_mode(teloxide::types::ParseMode::MarkdownV2)
            .await?;
        return Ok(());
    }
    
    let mut rng = StdRng::from_entropy();
    let winner = players.choose(&mut rng).unwrap();
    
    game::insert_result(&pool, game.id, winner.id, cur_year, cur_day).await?;
    
    if last_day {
        let announcement = text_static::YEAR_RESULTS_ANNOUNCEMENT.replace("{year}", &cur_year.to_string());
        bot.send_message(msg.chat.id, &announcement)
            .parse_mode(teloxide::types::ParseMode::MarkdownV2)
            .await?;
    }
    
    let stage1_text = stage1::PHRASES.choose(&mut rng).unwrap();
    bot.send_message(msg.chat.id, *stage1_text).await?;
    tokio::time::sleep(tokio::time::Duration::from_secs(GAME_RESULT_TIME_DELAY_SECS)).await;
    
    let stage2_text = stage2::PHRASES.choose(&mut rng).unwrap();
    bot.send_message(msg.chat.id, *stage2_text).await?;
    tokio::time::sleep(tokio::time::Duration::from_secs(GAME_RESULT_TIME_DELAY_SECS)).await;
    
    let stage3_text = stage3::PHRASES.choose(&mut rng).unwrap();
    bot.send_message(msg.chat.id, *stage3_text).await?;
    tokio::time::sleep(tokio::time::Duration::from_secs(GAME_RESULT_TIME_DELAY_SECS)).await;
    
    let stage4_text = stage4::PHRASES.choose(&mut rng).unwrap().replace(
        "{username}",
        &winner.full_username(true),
    );
    bot.send_message(msg.chat.id, stage4_text)
        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
        .await?;
    
    Ok(())
}

fn build_player_table(player_list: &[(TgUser, i64)]) -> String {
    let mut result = String::new();
    for (number, (tg_user, amount)) in player_list.iter().enumerate() {
        result.push_str(&text_static::STATS_LIST_ITEM
            .replace("{number}", &(number + 1).to_string())
            .replace("{username}", &escape_md2(&tg_user.full_username(false)))
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
    let current_dt = current_datetime_moscow();
    let cur_year = current_dt.year();
    
    let db_results = game::stats_current_year(&pool, game.id, cur_year).await?;
    let players = game::get_players(&pool, game.id).await?;
    
    let player_table = build_player_table(&db_results);
    let answer = text_static::STATS_CURRENT_YEAR
        .replace("{player_stats}", &player_table)
        .replace("{player_count}", &players.len().to_string());
    bot.send_message(msg.chat.id, answer)
        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
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
        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
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
    let from_user = msg.from().ok_or_else(|| AppError::Config("No from user".into()))?;
    
    let tg_user = user::upsert_tg_user(&pool, from_user).await?;
    let game = game::get_or_create_game(&pool, chat_id).await?;
    
    if let Some((user, count)) = game::stats_personal(&pool, game.id, tg_user.id).await? {
        let text = text_static::STATS_PERSONAL
            .replace("{username}", &escape_md2(&user.full_username(false)))
            .replace("{amount}", &count.to_string());
        bot.send_message(msg.chat.id, text)
            .parse_mode(teloxide::types::ParseMode::MarkdownV2)
            .await?;
    } else {
        let text = text_static::STATS_PERSONAL
            .replace("{username}", &escape_md2(&tg_user.full_username(false)))
            .replace("{amount}", "0");
        bot.send_message(msg.chat.id, text)
            .parse_mode(teloxide::types::ParseMode::MarkdownV2)
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
        let from_user = msg.from().map(|u| u.full_name()).unwrap_or_else(|| "user".to_string());
        bot.send_message(msg.chat.id, &text_static::ERROR_ZERO_PLAYERS.replace("{username}", &escape_md2(&from_user)))
            .parse_mode(teloxide::types::ParseMode::MarkdownV2)
            .await?;
        return Ok(());
    }
    
    let player_table = build_player_table(&db_results);
    let answer = text_static::YEAR_RESULTS_MSG
        .replace("{year}", &year.to_string())
        .replace("{username}", &escape_md2(&db_results[0].0.full_username(false)))
        .replace("{player_list}", &player_table);
    bot.send_message(msg.chat.id, answer)
        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
        .await?;
    Ok(())
}
