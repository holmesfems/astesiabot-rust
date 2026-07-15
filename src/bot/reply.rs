use crate::bot::data::Error;
use crate::engine::recruit::format::arrangement_chunks;
use poise::serenity_prelude as serenity;

// Discord の embed description 上限は 4096 字。多少の余裕を持たせてこの値にする。
const DESCRIPTION_CHUNK_LIMIT: usize = 4000;
// Discord の1メッセージあたり embed 数上限。
const MAX_EMBEDS_PER_MESSAGE: usize = 10;
// Discord の1メッセージに含む全 embed の文字数合計（title+description+...）の上限。
const MAX_TOTAL_CHARS_PER_MESSAGE: usize = 6000;

/// bot の embed 出力用。Discord への送信表現なので送信側が持つ。
pub struct EmbedReply {
    pub title: String,
    pub chunks: Vec<String>,
    pub msg_type: MsgType,
    /// リプライ追跡用のマーカー（`"<固有名>:<状態>"`形式）。Some の場合、
    /// 全embedのfooterにそのまま出力する。footerはUIにも表示される
    /// （非表示ではない）が、bot/services/reply_dispatch.rs が
    /// リプライ解決時にここを読んで由来コマンドと状態を復元する。
    pub reply_marker: Option<String>,
}

impl EmbedReply {
    /// エラー表示用のコンストラクタ（Python の actionToDiscord のエラー系に対応）。
    pub fn error(msg: &str) -> EmbedReply {
        EmbedReply {
            title: "エラー".to_string(),
            chunks: vec![msg.to_string()],
            msg_type: MsgType::Err,
            reply_marker: None,
        }
    }
}

/// メッセージ種別（Python の RCMsgType）
#[derive(Clone, Copy)]
pub enum MsgType {
    Ok,
    Err,
}

/// Python の colour() に対応。0x8be02b(緑) / マゼンタ。
fn colour_for(msg_type: MsgType) -> u32 {
    match msg_type {
        MsgType::Ok => 0x8be02b,
        MsgType::Err => 0xff00ff, // magenta
    }
}

/// Python の getEmbbeds() 相当: EmbedReply を Discord の1メッセージ上限
/// （embed数10個・全embed合計6000字）を満たすバッチ列に変換する。
/// チャンネル送信(`send_embed_reply`)・スラッシュコマンド応答の両方から使う。
pub fn to_embed_batches(reply: &EmbedReply) -> Vec<Vec<serenity::CreateEmbed>> {
    if reply.chunks.is_empty() {
        return Vec::new();
    }
    let title = if reply.title.is_empty() {
        "Reply".to_string()
    } else {
        reply.title.clone()
    };
    let packed = arrangement_chunks(&reply.chunks, DESCRIPTION_CHUNK_LIMIT);
    let colour = colour_for(reply.msg_type);
    let title_len = title.chars().count();

    let mut batches: Vec<Vec<serenity::CreateEmbed>> = Vec::new();
    let mut current_batch: Vec<serenity::CreateEmbed> = Vec::new();
    let mut current_total = 0usize;

    for desc in &packed {
        let embed_len = title_len + desc.chars().count();
        let exceeds_count = current_batch.len() + 1 > MAX_EMBEDS_PER_MESSAGE;
        let exceeds_total = current_total + embed_len > MAX_TOTAL_CHARS_PER_MESSAGE;
        if !current_batch.is_empty() && (exceeds_count || exceeds_total) {
            batches.push(std::mem::take(&mut current_batch));
            current_total = 0;
        }
        current_total += embed_len;
        let mut embed = serenity::CreateEmbed::new()
            .title(&title)
            .description(desc)
            .colour(colour);
        if let Some(marker) = &reply.reply_marker {
            embed = embed.footer(serenity::CreateEmbedFooter::new(marker));
        }
        current_batch.push(embed);
    }
    if !current_batch.is_empty() {
        batches.push(current_batch);
    }
    batches
}

/// Python の actionToDiscord 相当。EmbedReply を受け取り、実際に Discord へ送信する。
pub async fn send_embed_reply(
    cache_http: impl serenity::CacheHttp,
    channel_id: serenity::ChannelId,
    reply: &EmbedReply,
) -> Result<(), Error> {
    for batch in to_embed_batches(reply) {
        let builder = serenity::CreateMessage::new().embeds(batch);
        channel_id.send_message(&cache_http, builder).await?;
    }
    Ok(())
}

/// `send_embed_reply` の Discord 正式リプライ版（`message_reference` 付き）。
/// 複数embedバッチに分かれる場合、リプライとして紐付けるのは先頭バッチのみ
/// （2バッチ目以降は同一チャンネルへの通常送信）。
pub async fn reply_embed_reply(
    cache_http: impl serenity::CacheHttp,
    trigger: &serenity::Message,
    reply: &EmbedReply,
) -> Result<(), Error> {
    let mut batches = to_embed_batches(reply).into_iter();
    if let Some(first) = batches.next() {
        let builder = serenity::CreateMessage::new()
            .embeds(first)
            .reference_message(trigger);
        trigger
            .channel_id
            .send_message(&cache_http, builder)
            .await?;
    }
    for batch in batches {
        let builder = serenity::CreateMessage::new().embeds(batch);
        trigger
            .channel_id
            .send_message(&cache_http, builder)
            .await?;
    }
    Ok(())
}

/// embed を伴わない、プレーンテキストのみの正式リプライ。
/// Python の `RCReply(plainText=...)` を `message.reply` で送るケースに対応
/// （例: OCRフローのタグ不足/過多に対するリプライ誘導メッセージ）。
pub async fn reply_plain_text(
    cache_http: impl serenity::CacheHttp,
    trigger: &serenity::Message,
    content: &str,
) -> Result<(), Error> {
    let builder = serenity::CreateMessage::new()
        .content(content)
        .reference_message(trigger);
    trigger
        .channel_id
        .send_message(&cache_http, builder)
        .await?;
    Ok(())
}
