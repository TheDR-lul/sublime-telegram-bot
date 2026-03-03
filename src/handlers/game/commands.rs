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
use crate::handlers::game::phrases::{
    already_registered_roasts, stage1, stage2, stage3, stage4, text_static,
};

use tokio_util::sync::CancellationToken;

const GAME_RESULT_TIME_DELAY_SECS: u64 = 2;

pub const PIDORULES_HTML: &str = "Правила игры <b>Пидор Дня</b> (только для групповых чатов):\n\
<b>1.</b> Зарегистрируйтесь в игру по команде /pidoreg\n\
<b>2.</b> Подождите пока зарегистрируются все (или большинство :)\n\
<b>3.</b> Запустите розыгрыш по команде /pidor\n\
<b>4.</b> Просмотр статистики канала по команде /pidorstats, /pidorall\n\
<b>5.</b> Личная статистика по команде /pidorme\n\
<b>6.</b> Статистика за последний год по команде /pidor2024 (например /pidor2020, /pidor2019 и т.д.)\n\
<b>7. (!!! Только для администраторов чатов)</b>: удалить из игры может только Админ канала, сначала выведя по команде список игроков: /pidormin list\n\
Удалить же игрока можно по команде (используйте идентификатор пользователя - цифры из списка пользователей): /pidormin del 123456\n\
\n\
<b>Важно</b>, розыгрыш проходит только <b>раз в день</b>, повторная команда выведет <b>результат</b> игры.\n\
\n\
Сброс розыгрыша происходит каждый день в 12 часов ночи по киевскому времени (UTC+2 / UTC+3 в зависимости от сезона).\n\n\
Поддержать бота можно по <a href=\"https://github.com/TheDR-lul/sublime\">ссылке</a> :)";

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
            .send_message(pm, PIDORULES_HTML)
            .parse_mode(teloxide::types::ParseMode::Html)
            .disable_link_preview(true)
            .await;
        match r {
            Ok(sent) => {
                schedule_delete_message(bot.clone(), pm, sent.id);
                if let Ok(notice) = bot
                    .send_message(chat_id, "Правила отправил в личку.")
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
        .send_message(target, PIDORULES_HTML)
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
    let player_table = build_player_table(&db_results);
    let answer = text_static::STATS_CURRENT_YEAR
        .replace("{player_stats}", &player_table)
        .replace("{player_count}", &players.len().to_string());
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
    let player_table = build_player_table(&db_results);
    let answer = text_static::STATS_ALL_TIME
        .replace("{player_stats}", &player_table)
        .replace("{player_count}", &players.len().to_string());
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
            bot.send_message(
                msg.chat.id,
                "Регистрация возможна только от имени пользователя (не анонимно).",
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
        let phrase = already_registered_roasts::PHRASES
            .choose(&mut rand::rng())
            .expect("already_registered_roasts::PHRASES is non-empty");
        let text = phrase.replace("{username}", &username);
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
    let players = game::get_players(&pool, g.id).await?;
    if !players.iter().any(|p| p.tg_id == bettor_tg_id) {
        bot.send_message(msg.chat.id, "Сначала зарегистрируйся: /pidoreg")
            .await?;
        return Ok(());
    }

    let current_dt = current_datetime_kyiv();
    let cur_year = current_dt.year();
    let cur_day = current_dt.ordinal() as i32;
    let slot = "manual";

    if game::get_today_result_for_slot(&pool, g.id, cur_year, cur_day, slot).await?.is_some() {
        bot.send_message(msg.chat.id, "Розыгрыш уже прошёл. Ставки на завтра принимаются завтра.")
            .await?;
        return Ok(());
    }

    let target_tg_id = extract_bet_target(&msg, &cmd);
    let target_tg_id = match target_tg_id {
        Some(id) if id == bettor_tg_id => id,
        Some(id) => {
            if !players.iter().any(|p| p.tg_id == id) {
                bot.send_message(msg.chat.id, "Этот пользователь не зарегистрирован в игре.")
                    .await?;
                return Ok(());
            }
            id
        }
        None => {
            bot.send_message(
                msg.chat.id,
                "Укажи на кого ставишь: /pidorbet @user или ответом на сообщение.",
            )
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
        format!("🎲 <b>{}</b> ставит на {} как пидора дня!", bettor_name, target_name),
    )
    .parse_mode(teloxide::types::ParseMode::Html)
    .await?;

    if achievements::grant(&pool, tg_user.id, "bet_first").await? {
        bot.send_message(msg.chat.id, "🏅 Новая ачивка: Букмекер (первая ставка).")
            .await?;
    }

    Ok(())
}

fn extract_bet_target(msg: &Message, cmd: &crate::handlers::commands::Cmd) -> Option<i64> {
    if let crate::handlers::commands::Cmd::Pidorbet(arg) = cmd {
        let arg = arg.trim();
        if !arg.is_empty() {
            if let Some(entities) = msg.entities() {
                for e in entities {
                    if let teloxide::types::MessageEntityKind::TextMention { user } = &e.kind {
                        return Some(user.id.0 as i64);
                    }
                }
            }
        }
    }
    if let Some(reply) = msg.reply_to_message() {
        if let Some(ref from) = reply.from {
            return Some(from.id.0 as i64);
        }
    }
    None
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
    
    tracing::info!("Game {} (chat_id={}, slot={})", game.id, chat_id_raw, slot);
    let players = game::get_players(pool, game.id).await?;
    
    if players.len() < 2 {
        if is_manual {
            bot.send_message(chat_id, text_static::ERROR_NOT_ENOUGH_PLAYERS)
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
            let text = text_static::CURRENT_DAY_GAME_RESULT.replace(
                "{username}",
                &escape_html(&winner.full_username(false)),
            );
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
    
    if last_day && is_manual {
        let announcement = text_static::YEAR_RESULTS_ANNOUNCEMENT.replace("{year}", &cur_year.to_string());
        bot.send_message(chat_id, &announcement)
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;
    }
    
    if !is_manual {
        let msg = match run_kind {
            PidorRunKind::Manual => unreachable!(),
            PidorRunKind::Autorun(PidorAutorunSlot::Morning) => text_static::SUDDEN_PIDOR_MORNING,
            PidorRunKind::Autorun(PidorAutorunSlot::Day) => text_static::SUDDEN_PIDOR_DAY,
            PidorRunKind::Autorun(PidorAutorunSlot::Evening) => text_static::SUDDEN_PIDOR_EVENING,
        };
        bot.send_message(chat_id, msg).await?;
    }
    
    let stage1_text = stage1::PHRASES.choose(&mut rng).expect("stage1 phrases non-empty");
    bot.send_message(chat_id, *stage1_text).await?;
    tokio::time::sleep(tokio::time::Duration::from_secs(GAME_RESULT_TIME_DELAY_SECS)).await;
    
    let stage2_text = stage2::PHRASES.choose(&mut rng).expect("stage2 phrases non-empty");
    bot.send_message(chat_id, *stage2_text).await?;
    tokio::time::sleep(tokio::time::Duration::from_secs(GAME_RESULT_TIME_DELAY_SECS)).await;
    
    let stage3_text = stage3::PHRASES.choose(&mut rng).expect("stage3 phrases non-empty");
    bot.send_message(chat_id, *stage3_text).await?;
    tokio::time::sleep(tokio::time::Duration::from_secs(GAME_RESULT_TIME_DELAY_SECS)).await;
    
    let phrase = stage4::PHRASES.choose(&mut rng).expect("stage4 phrases non-empty");
    let mut stage4_text = phrase.replace("{username}", &escape_html(&winner.full_username(true)));
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
        let prev_dt = current_datetime_kyiv() - Duration::days(1);
        let prev_year = prev_dt.year();
        let prev_day = prev_dt.ordinal() as i32;
        if let Some(prev_res) =
            game::get_today_result_for_slot(pool, game.id, prev_year, prev_day, "manual").await?
            && prev_res.winner_id == winner.id
            && achievements::grant(pool, winner.id, "pidor_series_2").await?
        {
            bot.send_message(
                chat_id,
                "🔥 Новая ачивка: Пидор‑серийник (2 победы подряд).",
            )
            .await?;
        }
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
        bot.send_message(chat_id, format!("🎯 Угадал{}: {}!", if correct_bets.len() > 1 { "и" } else { "" }, joined))
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;

        for bet in &correct_bets {
            if let Some(uid) = crate::db::duel::user_id_by_tg_id(pool, bet.bettor_tg_id).await? {
                let total_correct = crate::db::bet::count_correct_bets(pool, chat_id_raw, bet.bettor_tg_id).await?;
                if total_correct >= 1 {
                    if achievements::grant(pool, uid, "bet_correct_1").await? {
                        bot.send_message(chat_id, "🏅 Новая ачивка: Пидор-аналитик (первое верное предсказание).").await?;
                    }
                }
                if total_correct >= 3 {
                    if achievements::grant(pool, uid, "bet_correct_3").await? {
                        bot.send_message(chat_id, "🏅 Новая ачивка: Ясновидящий (3 верных предсказания).").await?;
                    }
                }
                if bet.bettor_tg_id == bet.target_tg_id {
                    if achievements::grant(pool, uid, "bet_self_correct").await? {
                        bot.send_message(chat_id, "🏅 Новая ачивка: Самопидор-пророк (поставил на себя и угадал).").await?;
                    }
                }
                let streak = crate::db::bet::count_correct_streak(pool, chat_id_raw, bet.bettor_tg_id).await?;
                if streak >= 3 {
                    if achievements::grant(pool, uid, "bet_streak_3").await? {
                        bot.send_message(chat_id, "🏅 Новая ачивка: Нострадамус (3 верных предсказания подряд).").await?;
                    }
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
    if !msg.chat.is_group() && !msg.chat.is_supergroup() {
        return Ok(());
    }
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
    if !msg.chat.is_group() && !msg.chat.is_supergroup() {
        return Ok(());
    }
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
    if !msg.chat.is_group() && !msg.chat.is_supergroup() {
        return Ok(());
    }
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
        bot.send_message(
            msg.chat.id,
            text_static::ERROR_ZERO_PLAYERS.replace("{username}", &escape_html(&from_user)),
        )
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

fn autorun_settings_keyboard(g: &crate::db::models::Game) -> InlineKeyboardMarkup {
    let on = "✅";
    let off = "❌";
    let row1 = vec![InlineKeyboardButton::callback(
        format!(
            "Автопидор: {}",
            if g.autorun_enabled { "вкл" } else { "выкл" }
        ),
        "settings:toggle:autorun",
    )];
    let row2 = vec![
        InlineKeyboardButton::callback(
            format!("Утро {}", if g.autorun_morning { on } else { off }),
            "settings:toggle:morning",
        ),
        InlineKeyboardButton::callback(
            format!("День {}", if g.autorun_day { on } else { off }),
            "settings:toggle:day",
        ),
        InlineKeyboardButton::callback(
            format!("Вечер {}", if g.autorun_evening { on } else { off }),
            "settings:toggle:evening",
        ),
    ];
    let row3 = vec![InlineKeyboardButton::callback("← Назад", "menu:admin")];
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
        bot.send_message(msg.chat.id, "Только администраторы чата могут менять настройки.")
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
    let text = if unreg.is_empty() {
        "Все, кто писал в чат, уже зарегистрированы в игре.".to_string()
    } else {
        let mentions: Vec<String> = unreg
            .iter()
            .map(|u| {
                let name = escape_html(&u.full_username(false));
                format!(r#"<a href="tg://user?id={}">{}</a>"#, u.tg_id, name)
            })
            .collect();
        format!(
            "Ещё не зарегистрированы в игру: {}. Зарегистрируйтесь: /pidoreg",
            mentions.join(", ")
        )
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
