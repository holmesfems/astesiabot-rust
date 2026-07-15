use crate::bot::commands::fksearch;
use crate::bot::data::{Data, Error};
use crate::bot::reply::EmbedReply;
use poise::serenity_prelude as serenity;

/// スラッシュコマンド応答embedへの明示リプライを、由来コマンドの処理として
/// 再解釈し呼び出し直す。footerの`"<固有名>:<状態>"`マーカー（`EmbedReply::reply_marker`）
/// で由来コマンドと状態を判別する。フォールバック探索はせず、明示リプライのみ対応する
/// （recruitのreply_target.rsと異なり、履歴を遡らない）。
///
/// koukai_kyujin/uranaiチャンネルは既にそれぞれのハンドラが専有しているため、
/// ここは`handler.rs`のチャンネル別振り分けのelse節（どちらでもないチャンネル）
/// からのみ呼ばれる想定（1メッセージを複数ハンドラに分けない）。
pub async fn handle(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    data: &Data,
) -> Result<Option<EmbedReply>, Error> {
    let Some(msg_ref) = &msg.message_reference else {
        return Ok(None);
    };
    let Some(ref_id) = msg_ref.message_id else {
        return Ok(None);
    };
    let target = match &msg.referenced_message {
        Some(m) => (**m).clone(),
        None => msg.channel_id.message(&ctx.http, ref_id).await?,
    };
    if target.author.id != ctx.cache.current_user().id {
        return Ok(None);
    }
    let Some(footer_text) = target
        .embeds
        .first()
        .and_then(|e| e.footer.as_ref())
        .map(|f| f.text.as_str())
    else {
        return Ok(None);
    };
    let Some((key, payload)) = footer_text.split_once(':') else {
        return Ok(None);
    };

    match key {
        fksearch::REPLY_MARKER_KEY => {
            let skill_num = msg.content.trim();
            println!("[reply_dispatch] FKSEARCH operator_name={payload:?} skill_num={skill_num:?}");
            Ok(Some(
                fksearch::fk_search_reply(&data.state, payload, skill_num).await,
            ))
        }
        _ => Ok(None),
    }
}
