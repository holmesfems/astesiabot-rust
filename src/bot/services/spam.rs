use crate::bot::data::Error;
use poise::serenity_prelude as serenity;

/// スパムかどうかを判定する純粋関数。今は常にfalse（何も弾かない）。
pub fn is_spam(_msg: &serenity::Message) -> bool {
    // TODO: 連投・招待リンク・メンション爆撃などの判定を入れる
    false
}

/// スパムだった場合の処理。今はログのみ。
pub async fn handle(_ctx: &serenity::Context, msg: &serenity::Message) -> Result<(), Error> {
    println!("[spam] detected from {}: {}", msg.author.name, msg.content);
    // TODO: msg.delete(_ctx).await? などの対処
    Ok(())
}
