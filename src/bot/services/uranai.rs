use crate::bot::data::Error;
use poise::serenity_prelude as serenity;

pub async fn handle(_ctx: &serenity::Context, msg: &serenity::Message) -> Result<(), Error> {
    println!("[uranai] {} からの相談: {}", msg.author.name, msg.content);
    // TODO: ChatGPT APIに投げて応答生成 → ctx.say的に返信
    Ok(())
}