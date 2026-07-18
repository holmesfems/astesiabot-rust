use crate::bot::data::{Data, Error};
use crate::bot::reply::EmbedReply;
use poise::serenity_prelude as serenity;

/// OCRフローの結果。
pub struct OcrOutcome {
    pub reply: EmbedReply,
    /// true の場合、計算結果に加えてリプライ誘導メッセージも送る
    /// （Python の `tagMatch.isIllegal()` 分岐に対応）。
    pub needs_guidance: bool,
}

/// 画像添付（テキスト無し）メッセージの処理。OCR→タグ抽出→計算。
/// 呼び出し側（mod.rs）がこの結果を trigger へのリプライとして送信する。
pub async fn build(msg: &serenity::Message, data: &Data) -> Result<Option<OcrOutcome>, Error> {
    let text = match super::ocr::get_text(&msg.attachments[0].url).await? {
        Some(t) => t,
        None => {
            return Ok(Some(OcrOutcome {
                reply: EmbedReply::error("ごめんなさい。グーグル先生の調子が悪いみたい。また後で試してね"),
                needs_guidance: false,
            }))
        }
    };

    let result = super::embed::build_embed_reply(&data.state.recruit, &text);
    if result.reply.chunks.is_empty() {
        return Ok(None);
    }
    Ok(Some(OcrOutcome {
        reply: result.reply,
        needs_guidance: result.tag_count_illegal,
    }))
}
