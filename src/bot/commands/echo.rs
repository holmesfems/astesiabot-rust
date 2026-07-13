use crate::bot::data::{Context, Error};

/// 引数つきの例：オウム返し
#[poise::command(slash_command)]
pub async fn echo(
    ctx: Context<'_>,
    #[description = "返したい文字列"] text: String,
) -> Result<(), Error> {
    ctx.say(text).await?;
    Ok(())
}