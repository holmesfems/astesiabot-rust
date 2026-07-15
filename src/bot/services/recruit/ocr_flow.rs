use crate::bot::data::{Data, Error};
use crate::bot::reply::EmbedReply;
use poise::serenity_prelude as serenity;

/// 画像添付（テキスト無し）メッセージの処理。OCR→タグ抽出→計算。
/// 呼び出し側（mod.rs）がこの結果を trigger へのリプライとして送信する。
pub async fn build(msg: &serenity::Message, data: &Data) -> Result<Option<EmbedReply>, Error> {
    let text = match super::ocr::get_text(&msg.attachments[0].url).await? {
        Some(t) => t,
        None => return Ok(Some(EmbedReply::error("画像から文字を検出できませんでした"))),
    };

    let reply = super::embed::build_embed_reply(&data.state.recruit, &text);
    if reply.chunks.is_empty() {
        return Ok(None);
    }
    Ok(Some(reply))
}
