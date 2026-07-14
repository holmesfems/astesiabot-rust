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
}

impl EmbedReply {
    /// エラー表示用のコンストラクタ（Python の actionToDiscord のエラー系に対応）。
    pub fn error(msg: &str) -> EmbedReply {
        EmbedReply {
            title: "エラー".to_string(),
            chunks: vec![msg.to_string()],
            msg_type: MsgType::Err,
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

/// Python の actionToDiscord 相当。EmbedReply を受け取り、実際に Discord へ送信する。
/// 分割・整形ロジック（DESCRIPTION_CHUNK_LIMIT字詰め直し・同タイトル同色・
/// embed数/合計文字数上限を満たすバッチ送信）はここに集約する。
pub async fn send_embed_reply(
    cache_http: impl serenity::CacheHttp,
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
    // Python の getEmbbeds() 相当: arrangementChunks で DESCRIPTION_CHUNK_LIMIT に詰め直し、
    // 各チャンクを同タイトル・同色の embed にする。
    let packed = arrangement_chunks(&reply.chunks, DESCRIPTION_CHUNK_LIMIT);
    let colour = colour_for(reply.msg_type);
    let title_len = title.chars().count();

    // Discord の1メッセージには embed 数上限(10個)と、全 embed 合計文字数上限(6000字)の
    // 両方があるため、単純に10個ずつでは区切れない。両方を満たすようにバッチする。
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
        current_batch.push(
            serenity::CreateEmbed::new()
                .title(&title)
                .description(desc)
                .colour(colour),
        );
    }
    if !current_batch.is_empty() {
        batches.push(current_batch);
    }

    for batch in batches {
        let builder = serenity::CreateMessage::new().embeds(batch);
        channel_id.send_message(&cache_http, builder).await?;
    }
    Ok(())
}
