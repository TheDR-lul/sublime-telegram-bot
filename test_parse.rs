
use teloxide::utils::command::BotCommands;
#[derive(BotCommands, Clone, Debug)]
#[command(rename_rule = "lowercase")]
pub enum Cmd {
    #[command(description = "pet a friend's dick (@user or reply)")]
    Huyapet(String),
}
fn main() {
    println!("{:?}", Cmd::parse("/huyapet@MainPidor_bot @alexambrosia", ""));
    println!("{:?}", Cmd::parse("/huyapet@MainPidor_bot", ""));
}

