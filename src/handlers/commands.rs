//! Bot command enum for filter_command.

use teloxide::utils::command::BotCommands;

#[derive(BotCommands, Clone, Debug)]
#[command(rename_rule = "lowercase")]
pub enum Cmd {
    #[command(description = "some info about github repo")]
    About,
    #[command(description = "say hello")]
    Hello,
    #[command(description = "simulate /slap command from IRC")]
    Slap(String),
    #[command(description = "shrug")]
    Shrug,
    #[command(description = "simulate /me command from IRC")]
    Me(String),
    #[command(description = "<query> let me google that for you")]
    Google(String),
    #[command(description = "pin replied message")]
    Pin,
    #[command(description = "echo message")]
    Echo(String),
    #[command(description = "<key> get specific entry by key")]
    Get(String),
    #[command(description = "list entries for current chat")]
    List,
    #[command(description = "<key> <value> set specific value for key")]
    Set(String),
    #[command(description = "<key> remove specific key")]
    Del(String),
    #[command(description = "play the game, see /pidorules first")]
    Pidor,
    #[command(description = "POTD game rules")]
    Pidorules,
    #[command(description = "register to the POTD game")]
    Pidoreg,
    #[command(description = "unregister from the POTD game")]
    Pidorunreg,
    #[command(description = "POTD game stats for this year")]
    Pidorstats,
    #[command(description = "POTD game stats for all time")]
    Pidorall,
    #[command(description = "POTD personal stats")]
    Pidorme,
    #[command(description = "get some random meme")]
    Meme,
    #[command(description = "get some random russian meme")]
    Memeru,
    #[command(description = "get video from tiktok")]
    Ttvideo(String),
    #[command(description = "get depersonalized tiktok link")]
    Ttlink(String),
    #[command(description = "show your achievements")]
    Achievements,
    #[command(description = "scan someone with pidor-detector")]
    Pidorscan(String),
}
