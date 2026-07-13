use crate::bot::data::Error;
use crate::recruit::format::arrangement_chunks;
use crate::recruit::EmbedReply;
use poise::serenity_prelude as serenity;

const MAX_LENGTH: usize = 1900; // Python と同じ

/// Python の actionToDiscord 相当。EmbedReply を受け取り、実際に Discord へ送信する。
/// 分割・整形ロジック（1900字詰め直し・同タイトル同色・10個ごと分割送信）はここに集約する。
pub async fn send_embed_reply(
    ctx: &serenity::Context,
    channel_id: serenity::ChannelId,
    reply: &EmbedReply,
) -> Result<(), Error> {
    if reply.chunks.is_empty() {
        return Ok(());
    }
    let title = if reply.title.is_empty() {
        "Reply".to_string()
    } else {
        reply.title.clone()
    };
    // Python の getEmbbeds(): arrangementChunks で 1900 に詰め直し、
    // 各チャンクを同タイトル・同色の embed にする。
    let packed = arrangement_chunks(&reply.chunks, MAX_LENGTH);
    let colour = reply.msg_type.colour();

    let embeds: Vec<serenity::CreateEmbed> = packed
        .iter()
        .map(|desc| {
            serenity::CreateEmbed::new()
                .title(&title)
                .description(desc)
                .colour(colour)
        })
        .collect();

    // Discord の1メッセージ embed 上限は10個。超える場合は分割送信。
    for batch in embeds.chunks(10) {
        let builder = serenity::CreateMessage::new().embeds(batch.to_vec());
        channel_id.send_message(&ctx.http, builder).await?;
    }
    Ok(())
}
