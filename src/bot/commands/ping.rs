use crate::bot::data::{Context, Error};

/// 応答を返すだけのテストコマンド
#[poise::command(slash_command)]
pub async fn ping(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say("pong").await?;
    Ok(())
}