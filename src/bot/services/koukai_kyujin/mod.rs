pub mod ocr;

use crate::bot::data::{Data, Error};
use crate::recruit::EmbedReply;
use poise::serenity_prelude as serenity;

pub async fn handle(
    _ctx: &serenity::Context,
    msg: &serenity::Message,
    data: &Data,
) -> Result<Option<EmbedReply>, Error> {
    if msg.attachments.is_empty() {
        return Ok(None);
    }
    let text = match ocr::get_text(&msg.attachments[0].url).await? {
        Some(t) => t,
        None => return Ok(Some(EmbedReply::error("画像から文字を検出できませんでした"))),
    };

    let reply = data.state.recruit.process_for_embed(&text);
    if reply.chunks.is_empty() {
        return Ok(None);
    }
    Ok(Some(reply))
}
