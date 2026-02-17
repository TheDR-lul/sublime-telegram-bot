use teloxide::prelude::*;
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
    .disable_web_page_preview(true)
    .await?;
    Ok(())
}
