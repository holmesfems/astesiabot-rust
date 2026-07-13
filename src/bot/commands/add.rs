use crate::bot::data::{Context, Error};

/// 引数つきの例：2つの数を足す
#[poise::command(slash_command)]
pub async fn add(
    ctx: Context<'_>,
    #[description = "1つ目の数"] a: i64,
    #[description = "2つ目の数"] b: i64,
) -> Result<(), Error> {
    ctx.say(format!("{a} + {b} = {}", a + b)).await?;
    Ok(())
}