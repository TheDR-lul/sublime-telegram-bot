//! Bot command enum for filter_command.

use teloxide::utils::command::BotCommands;

#[derive(BotCommands, Clone, Debug)]
#[command(rename_rule = "lowercase")]
pub enum Cmd {
    #[command(description = "menu with sections")]
    Menu,
    #[command(description = "about bot and repo")]
    About,
    #[command(description = "slap someone by replying to their message")]
    Slap,
    #[command(description = "shrug")]
    Shrug,
    #[command(description = "simulate /me command from IRC")]
    Me(String),
    #[command(description = "<query> let me google that for you")]
    Google(String),
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
    #[command(description = "show your achievements")]
    Achievements,
    #[command(description = "scan someone with pidor-detector")]
    Pidorscan(String),
    #[command(description = "autorun settings (admins only)")]
    Pidorset,
    #[command(
        description = "challenge to pidor duel (tic-tac-toe); reply to user for tagged challenge"
    )]
    Pidorduel,
    #[command(description = "duel Elo leaderboard")]
    Duelstats,
    #[command(description = "bet on who will be pidor of the day")]
    Pidorbet(String),
    #[command(description = "set chat language (admins only), e.g. /lang ru")]
    Lang(String),
    #[command(description = "register to the dick game")]
    Huyareg,
    #[command(description = "pet a friend's dick (@user or reply)")]
    Huyapet(String),
    #[command(description = "grow your dick")]
    Huyagrow,
    #[command(description = "fight another player's dick")]
    Huyafight(String),
    #[command(description = "steal from another player's dick")]
    Huyasteal(String),
    #[command(description = "raid a strong player with a party")]
    Huyaraid(String),
    #[command(description = "dick leaderboard")]
    Huyatop,
    #[command(description = "skill tree for your dick")]
    Huyaskills,
    #[command(description = "shop for dick items and boosters")]
    Huyashop,
    #[command(description = "open gacha chests")]
    Huyachest,
    #[command(description = "inventory and equipment")]
    Huyainv,
    #[command(description = "tamagotchi dick game status")]
    Huya(String),
    #[command(description = "enable bot in this topic (admins only, forums)")]
    Bothere,
    #[command(description = "disable bot in this topic (admins only, forums)")]
    Bothereoff,
    // RPG: development for future — disabled for now; /rpg shows stub message.
    #[command(description = "Pidor-Royale RPG (in development)")]
    Rpg,
}
