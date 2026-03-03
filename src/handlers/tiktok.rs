use regex::Regex;
use sqlx::PgPool;
use std::process::Stdio;
use std::sync::LazyLock;
use std::time::Duration;
use teloxide::prelude::*;
use teloxide::types::{InlineQuery, InlineQueryResult, Message};
use tokio::process::Command;
use tokio::time::timeout;

use crate::config::Config;
use crate::db::tiktok;
use crate::error::AppError;

const PROCESSING_STARTED: &str = "Processing started.....";
const YT_DLP_TIMEOUT_SECS: u64 = 120;

static TIKTOK_URL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^https?://[vmtw.]{0,5}tiktok\.com/").expect("tiktok url regex")
});

fn is_valid_tiktok_url(s: &str) -> bool {
    TIKTOK_URL_RE.is_match(s.trim())
}

fn extract_tiktok_url(msg: &Message, arg: &str) -> Option<String> {
    if !arg.trim().is_empty() {
        return Some(arg.trim().to_string());
    }
    if let Some(reply) = msg.reply_to_message() {
        if let Some(text) = reply.text() {
            if text.len() > 10 {
                return Some(text.to_string());
            }
        }
    }
    None
}

async fn get_tt_video_info(url: &str, download: bool) -> Result<(String, Option<Vec<u8>>), AppError> {
    let mut cmd = Command::new("yt-dlp");
    cmd.arg("--quiet");
    cmd.arg("--no-playlist");
    if download {
        cmd.arg("--output").arg("%(title).30s-%(id)s.%(ext)s");
    } else {
        cmd.arg("--get-url");
    }
    cmd.arg(url);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.kill_on_drop(true);

    let output = timeout(
        Duration::from_secs(YT_DLP_TIMEOUT_SECS),
        cmd.output(),
    )
    .await??;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::YtDlp(format!("yt-dlp failed: {}", stderr)));
    }

    if download {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.lines().collect();
        let video_path = lines.last().ok_or_else(|| AppError::YtDlp("No output".into()))?;
        let video_bytes = tokio::fs::read(video_path).await?;
        let video_link = lines
            .iter()
            .find(|l| l.starts_with("http"))
            .ok_or_else(|| AppError::YtDlp("No URL in output".into()))?
            .to_string();
        let _ = tokio::fs::remove_file(video_path).await;
        Ok((video_link, Some(video_bytes)))
    } else {
        let video_link = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok((video_link, None))
    }
}

pub async fn tt_video_handler(
    bot: Bot,
    msg: Message,
    cmd: crate::handlers::commands::Cmd,
) -> Result<(), AppError> {
    let source_url = match &cmd {
        crate::handlers::commands::Cmd::Ttvideo(s) => extract_tiktok_url(&msg, s),
        _ => None,
    };

    let source_url = match source_url {
        Some(u) if is_valid_tiktok_url(&u) => u,
        _ => {
            bot.send_message(
                msg.chat.id,
                "Provide a valid TikTok link after the command or reply to the link",
            )
            .await?;
            return Ok(());
        }
    };

    let processing_msg = bot.send_message(msg.chat.id, PROCESSING_STARTED).await?;
    
    match get_tt_video_info(&source_url, true).await {
        Ok((video_link, Some(video_bytes))) => {
            use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};
            let video_url = url::Url::parse(&video_link)?;
            let keyboard = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::url(
                "🔗".to_string(),
                video_url,
            )]]);
            bot.send_video(msg.chat.id, teloxide::types::InputFile::memory(video_bytes))
                .reply_markup(keyboard)
                .await?;
            bot.delete_message(msg.chat.id, processing_msg.id).await.ok();
        }
        Ok((_, None)) => {
            bot.delete_message(msg.chat.id, processing_msg.id).await.ok();
            bot.send_message(msg.chat.id, "Failed to download video")
                .await?;
        }
        Err(e) => {
            tracing::warn!("TikTok download failed: {:?}", e);
            bot.delete_message(msg.chat.id, processing_msg.id).await.ok();
            bot.send_message(msg.chat.id, "Failed to process the link, please, try another one")
                .await?;
        }
    }
    Ok(())
}

pub async fn tt_link_handler(
    bot: Bot,
    msg: Message,
    cmd: crate::handlers::commands::Cmd,
) -> Result<(), AppError> {
    let source_url = match &cmd {
        crate::handlers::commands::Cmd::Ttlink(s) => extract_tiktok_url(&msg, s),
        _ => None,
    };

    let source_url = match source_url {
        Some(u) if is_valid_tiktok_url(&u) => u,
        _ => {
            bot.send_message(
                msg.chat.id,
                "Provide a valid TikTok link after the command or reply to the link",
            )
            .await?;
            return Ok(());
        }
    };

    let processing_msg = bot.send_message(msg.chat.id, PROCESSING_STARTED).await?;
    
    match get_tt_video_info(&source_url, false).await {
        Ok((video_link, _)) => {
            bot.send_message(msg.chat.id, video_link).await?;
            bot.delete_message(msg.chat.id, processing_msg.id).await.ok();
        }
        Err(e) => {
            tracing::warn!("TikTok link extraction failed: {:?}", e);
            bot.delete_message(msg.chat.id, processing_msg.id).await.ok();
            bot.send_message(msg.chat.id, "Failed to process the link, please, try another one")
                .await?;
        }
    }
    Ok(())
}

pub async fn tt_inline_handler(
    bot: Bot,
    query: InlineQuery,
    pool: PgPool,
    config: Config,
) -> Result<(), AppError> {
    let q = query.query.trim();
    if !TIKTOK_URL_RE.is_match(q) {
        return Ok(());
    }

    tracing::debug!("Processing inline query: {}", q);

    let cache = tiktok::find_by_link_or_share(&pool, q, q).await?;
    
    let (video_link, telegram_video_id) = if let Some(cached) = cache {
        (cached.link, cached.telegram_message_id)
    } else {
        let (link, video_bytes) = match get_tt_video_info(q, true).await {
            Ok((l, Some(bytes))) => (l, bytes),
            Ok((_l, None)) => {
                bot.answer_inline_query(query.id.clone(), vec![]).await?;
                return Ok(());
            }
            Err(e) => {
                tracing::warn!("TikTok inline download failed: {:?}", e);
                bot.answer_inline_query(query.id.clone(), vec![]).await?;
                return Ok(());
            }
        };

        let cache_chat_id = config
            .tiktok_cache_chat_id
            .ok_or_else(|| AppError::Config("tiktok_cache_chat_id not configured".into()))?;
        
        let video_msg = bot
            .send_video(
                teloxide::types::ChatId(cache_chat_id),
                teloxide::types::InputFile::memory(video_bytes),
            )
            .caption(&link)
            .await?;
        
        let file_id = video_msg
            .video()
            .ok_or_else(|| AppError::Config("No video in message".into()))?
            .file
            .id
            .clone();

        let file_id_str = file_id.to_string();
        let cached = tiktok::insert(&pool, &link, Some(q), &file_id_str).await?;
        (cached.link, cached.telegram_message_id)
    };

    use teloxide::types::{
        InlineKeyboardButton, InlineKeyboardMarkup, InlineQueryResultArticle,
        InlineQueryResultCachedVideo, InputMessageContent, InputMessageContentText,
    };

    let results = vec![
        InlineQueryResult::Article(InlineQueryResultArticle {
            id: uuid::Uuid::new_v4().to_string(),
            title: "Link".to_string(),
            description: Some("Depersonalized link to the TikTok video".to_string()),
            input_message_content: InputMessageContent::Text(InputMessageContentText {
                message_text: video_link.clone(),
                parse_mode: None,
                entities: None,
                link_preview_options: None,
            }),
            reply_markup: None,
            url: None,
            thumbnail_url: None,
            thumbnail_width: None,
            thumbnail_height: None,
        }),
        InlineQueryResult::CachedVideo(InlineQueryResultCachedVideo {
            id: uuid::Uuid::new_v4().to_string(),
            video_file_id: telegram_video_id.clone().into(),
            title: "Video".to_string(),
            description: None,
            caption: None,
            parse_mode: None,
            caption_entities: None,
            reply_markup: Some(InlineKeyboardMarkup::new(vec![vec![
                InlineKeyboardButton::url("🔗".to_string(), url::Url::parse(&video_link)?),
            ]])),
            input_message_content: None,
            show_caption_above_media: false,
        }),
    ];

    match bot.answer_inline_query(query.id.clone(), results).cache_time(86400).await {
        Ok(_) => {}
        Err(e) => {
            if e.to_string().contains("Document_invalid") {
                if let Some(cached) = tiktok::find_by_link_or_share(&pool, &video_link, q).await? {
                    tiktok::delete_by_id(&pool, cached.id).await?;
                    tracing::info!("Invalid video file, deleted from cache");
                }
            } else {
                tracing::error!("Error answering inline query: {:?}", e);
                return Err(e.into());
            }
        }
    }
    Ok(())
}
