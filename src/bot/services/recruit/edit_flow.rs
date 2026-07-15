use super::reply_target::{self, Resolution};
use crate::bot::data::{Data, Error};
use crate::bot::reply::{EmbedReply, MsgType};
use crate::engine::recruit::edit::{self, EditError};
use crate::engine::recruit::format;
use poise::serenity_prelude as serenity;

/// テキストを含むメッセージの処理。対象embedのタグをリプライ本文で編集し、再計算する。
/// Python の msgForOCRReply に対応。
pub async fn build(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    data: &Data,
) -> Result<Option<EmbedReply>, Error> {
    let engine_data = &data.state.recruit.data;

    let parsed = match reply_target::resolve(ctx, msg, engine_data).await? {
        Resolution::Ignore => return Ok(None),
        Resolution::Ready(parsed) => parsed,
    };

    let tags = match edit::apply_edit_commands(engine_data, &parsed.tags, &msg.content) {
        Ok(tags) => tags,
        Err(EditError::UnknownTags(tags)) => {
            return Ok(Some(EmbedReply::error(&format!(
                "{}のタグが分かりませんわ。どのタグを指してるのかしら？タグの正式名称を入力してくれると嬉しいわ。",
                tags.join(", ")
            ))))
        }
        Err(EditError::TooManyTags(_)) => {
            return Ok(Some(EmbedReply::error(
                "タグが多すぎるわ。8件ぐらいまでにしてちょうだい。",
            )))
        }
    };

    let results = engine_data.calculate(&tags, parsed.is_global, 4, None);
    let sorted_input = engine_data.normalize_names(&tags);
    let title = format::make_title(&sorted_input, parsed.is_global, true);
    let chunks = if results.is_empty() {
        vec!["★4以上になる組み合わせはありません".to_string()]
    } else {
        format::display_chunks(results)
    };
    Ok(Some(EmbedReply {
        title,
        chunks,
        msg_type: MsgType::Ok,
        reply_marker: None,
    }))
}
