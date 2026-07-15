pub mod edit_flow;
pub mod embed;
pub mod ocr;
pub mod ocr_flow;
pub mod reply_target;

use crate::bot::data::{Data, Error};
use crate::bot::reply::reply_embed_reply;
use poise::serenity_prelude as serenity;

/// 実画像（width/height を持つ添付）があるか。Python の `not file.width or not file.height`
/// によるスキップと同じ判定。
fn has_image(msg: &serenity::Message) -> bool {
    msg.attachments
        .iter()
        .any(|a| a.width.is_some() && a.height.is_some())
}

fn has_text(msg: &serenity::Message) -> bool {
    !msg.content.trim().is_empty()
}

/// 公開求人チャンネルのメッセージ振り分け。
/// - 画像のみ → OCRフロー（trigger への正式リプライとして送信）
/// - テキストあり（画像の有無を問わない） → 編集フロー（画像は無視、trigger への正式リプライとして送信）
/// - どちらも無し → 何もしない
///
/// 編集フローの出力も正式リプライにするのは見た目の一貫性だけでなく、
/// フォールバック探索（reply_target.rs）が「bot発言の message_reference が
/// 送信者自身の発言を指しているか」で辿るための必須条件でもある。ここが
/// プレーン送信だと、2回目以降の編集がフォールバックから見えなくなり、
/// 常に一番最初のOCR結果まで遡ってしまう。
pub async fn handle(ctx: &serenity::Context, msg: &serenity::Message, data: &Data) -> Result<(), Error> {
    if has_text(msg) {
        if let Some(reply) = edit_flow::build(ctx, msg, data).await? {
            reply_embed_reply(ctx, msg, &reply).await?;
        }
        return Ok(());
    }
    if has_image(msg) {
        if let Some(reply) = ocr_flow::build(msg, data).await? {
            reply_embed_reply(ctx, msg, &reply).await?;
        }
    }
    Ok(())
}
