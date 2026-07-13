use crate::bot::data::Error;
use crate::bot::reply::EmbedReply;
use poise::serenity_prelude as serenity;

/// 占い館チャンネル。今は骨組みのみ。
pub async fn handle(
    _ctx: &serenity::Context,
    msg: &serenity::Message,
) -> Result<Option<EmbedReply>, Error> {
    println!("[uranai] {} からの相談: {}", msg.author.name, msg.content);
    // TODO: ChatGPT API に投げて応答生成 → 返信
    Ok(None)
}
