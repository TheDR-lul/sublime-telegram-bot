use teloxide::prelude::*;
use teloxide::sugar::request::RequestLinkPreviewExt;
use teloxide::types::Message;

use crate::error::AppError;

pub async fn about_handler(
    bot: Bot,
    msg: Message,
    _: crate::handlers::commands::Cmd,
) -> Result<(), AppError> {
    bot.send_message(
        msg.chat.id,
        "The source code of the bot available via <a href=\"https://github.com/TheDR-lul/sublime\">GitHub repository</a>",
    )
    .parse_mode(teloxide::types::ParseMode::Html)
    .disable_link_preview(true)
    .await?;
    Ok(())
}
