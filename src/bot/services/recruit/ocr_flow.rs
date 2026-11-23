use crate::bot::data::{Data, Error};
use crate::bot::reply::EmbedReply;
use poise::serenity_prelude as serenity;
use std::collections::HashSet;
use super::local_ocr::{self, LocalOcrMode, LocalOutcome, LocalRead, LocalUnsure, MissVerdict};
use crate::engine::recruit::matcher::{MatchResult, TagLang};
use super::ocr::OcrText;

/// OCRフローの結果。
pub struct OcrOutcome {
    pub reply: EmbedReply,
    /// true の場合、計算結果に加えてリプライ誘導メッセージも送る
    pub needs_guidance: bool,
}

/// 画像添付（テキスト無し）メッセージの処理。添付されている画像1枚ごとに
/// OCR→タグ抽出→計算を行う。呼び出し側（mod.rs）が結果ごとに trigger への
/// リプライとして送信する。
///
/// ローカル OCR（LOCAL_OCR_MODE。local_ocr.rs）: on なら読み切れた画像は Google を呼ばない。
/// shadow ならローカルも読むが返信は常に Google の結果で、食い違いを記録する。
///
/// Vision APIが不調（全エンドポイント失敗。文字の無い画像は含まない）だった画像に当たった時点でループを
/// 打ち切る。全エンドポイントが揃って失敗するのは広域障害の可能性が高く、
/// 残りの画像でも同じ結果になる見込みが高いため。
pub async fn build(ctx: &serenity::Context, msg: &serenity::Message, data: &Data) -> Result<Vec<OcrOutcome>, Error> {
    let local_ocr = &data.state.local_ocr;
    let mut outcomes = Vec::new();
    for attachment in msg.attachments.iter().filter(|a| super::is_image_attachment(a)) {
        let read = match local_ocr.mode {
            LocalOcrMode::Off => None,
            _ => Some(local_ocr.read_attachment(&attachment.url, attachment.size).await),
        };
        let local = read.as_ref().map(|r| &r.outcome);
        if matches!(local, Some(LocalOutcome::Read { .. })) {
            local_ocr.record_read();
        }

        // on: ローカルが読み切れたら Google を呼ばない。読めなければそのまま Google へ
        if let (LocalOcrMode::On, Some(o)) = (local_ocr.mode, local) {
            println!("{}", local_ocr::on_log_line(o));
            if let LocalOutcome::Read { tags, .. } = o {
                let result = super::embed::build_embed_reply_from_tags(&data.state.recruit, tags);
                if !result.reply.chunks.is_empty() {
                    outcomes.push(OcrOutcome { reply: result.reply, needs_guidance: result.tag_count_illegal });
                }
                continue;
            }
        }

        let text = match super::ocr::get_text(&attachment.url).await? {
            OcrText::Text(t) => t,
            // 文字の無い画像は求人画像ではないだけ。他の画像の処理は続ける
            OcrText::NoText => {
                if let (LocalOcrMode::Shadow, Some(o)) = (local_ocr.mode, local) {
                    shadow_report(ctx, msg, data, o, &HashSet::new());
                }
                continue;
            }
            OcrText::Unavailable => {
                outcomes.push(OcrOutcome {
                    reply: EmbedReply::error("ごめんなさい。グーグル先生の調子が悪いみたい。また後で試してね"),
                    needs_guidance: false,
                });
                break;
            }
        };

        if let Some(read) = &read {
            let matched = data.state.recruit.matcher.match_tag(&text);
            if local_ocr.mode == LocalOcrMode::Shadow {
                shadow_report(ctx, msg, data, &read.outcome, &matched.matches);
            }
            jp_miss_report(ctx, msg, data, read, &matched);
        }

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

/// 日本語版の読めない画像の監視（shadow・on 共通）。ローカルが OCR の理由で読めず、Google のテキストが
/// 日本語で 5 タグ取れた画像を数え、連続したらオーナーに DM する。重複画像は数えない。
fn jp_miss_report(ctx: &serenity::Context, msg: &serenity::Message, data: &Data, read: &LocalRead, matched: &MatchResult) {
    let LocalOutcome::Unsure { reason: reason @ LocalUnsure::Ocr(_), .. } = &read.outcome else { return };
    if matched.lang != TagLang::Ja || matched.matches.len() != 5 {
        return;
    }
    let local_ocr = &data.state.local_ocr;
    let link = local_ocr::jump_link(msg.guild_id, msg.channel_id, msg.id);
    match local_ocr.record_jp_miss(read.digest, link) {
        MissVerdict::Duplicate => {}
        MissVerdict::Counted { streak, alert } => {
            println!("{}", local_ocr::jp_miss_log_line(streak, *reason));
            if let (Some(user), Some(links)) = (local_ocr.report_user, alert) {
                let text = local_ocr::miss_alert_message(streak, &links);
                let ctx = ctx.clone();
                tokio::spawn(async move { local_ocr::send_dm(&ctx.http, user, text).await });
            }
        }
    }
}

/// shadow: ローカルと Google のタグ集合を比べて、ログ・集計・（不一致なら）オーナーへの DM に残す。
/// DM は別タスクで送るので、失敗しても返信には影響しない。
fn shadow_report(ctx: &serenity::Context, msg: &serenity::Message, data: &Data, local: &LocalOutcome, google_tags: &HashSet<String>) {
    let local_ocr = &data.state.local_ocr;
    let comparison = local_ocr::compare(local, google_tags);
    let local_tags = match local {
        LocalOutcome::Read { tags, .. } => Some(tags.as_slice()),
        LocalOutcome::Unsure { .. } => None,
    };
    println!("{}", local_ocr::shadow_log_line(&comparison, local_tags, google_tags, local.min_best()));
    local_ocr.record(&comparison);

    if comparison == local_ocr::Comparison::Disagree {
        if let (Some(user), Some(tags)) = (local_ocr.report_user, local_tags) {
            let link = local_ocr::jump_link(msg.guild_id, msg.channel_id, msg.id);
            let text = local_ocr::disagreement_message(tags, google_tags, local.boxes(), &link);
            let ctx = ctx.clone();
            tokio::spawn(async move { local_ocr::send_dm(&ctx.http, user, text).await });
        }
    }
}
