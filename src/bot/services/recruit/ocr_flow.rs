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

/// 画像添付（テキスト無し）メッセージの処理。添付されている画像1枚ごとに
/// OCR→タグ抽出→計算を行う。呼び出し側（mod.rs）が結果ごとに trigger への
/// リプライとして送信する。
///
/// Vision APIが不調（全エンドポイント失敗）だった画像に当たった時点でループを
/// 打ち切る。全エンドポイントが揃って失敗するのは広域障害の可能性が高く、
/// 残りの画像でも同じ結果になる見込みが高いため。
pub async fn build(msg: &serenity::Message, data: &Data) -> Result<Vec<OcrOutcome>, Error> {
    let mut outcomes = Vec::new();
    for attachment in msg.attachments.iter().filter(|a| super::is_image_attachment(a)) {
        let text = match super::ocr::get_text(&attachment.url).await? {
            Some(t) => t,
            None => {
                outcomes.push(OcrOutcome {
                    reply: EmbedReply::error("ごめんなさい。グーグル先生の調子が悪いみたい。また後で試してね"),
                    needs_guidance: false,
                });
                break;
            }
        };

        let result = super::embed::build_embed_reply(&data.state.recruit, &text);
        if result.reply.chunks.is_empty() {
            continue;
        }
        outcomes.push(OcrOutcome {
            reply: result.reply,
            needs_guidance: result.tag_count_illegal,
        });
    }
    Ok(outcomes)
}
