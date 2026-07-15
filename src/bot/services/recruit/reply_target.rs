use crate::bot::data::Error;
use crate::engine::recruit::calc::RecruitData;
use crate::engine::recruit::edit::{self, ParsedTitle};
use poise::serenity_prelude as serenity;
use std::collections::HashSet;

/// フォールバック探索で遡る時間の窓（秒）。
const FALLBACK_WINDOW_SECS: i64 = 600;
/// フォールバック探索で取得する履歴件数。
const FALLBACK_HISTORY_LIMIT: u8 = 50;

/// 編集対象の解決結果。
pub enum Resolution {
    /// 対象embedのタイトルからタグを復元できた。
    Ready(ParsedTitle),
    /// 対象が見つからない、またはタイトルが解析不能。サイレント無視。
    Ignore,
}

/// 編集フローの対象embedを解決する。
/// `msg.message_reference` があれば明示リプライとして扱い、無ければ
/// フォールバック探索（送信者自身の直近発言を参照しているbot発言を遡って探す）を行う。
pub async fn resolve(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    data: &RecruitData,
) -> Result<Resolution, Error> {
    if msg.message_reference.is_some() {
        return resolve_explicit(ctx, msg, data).await;
    }
    Ok(match fallback_search(ctx, msg, data).await? {
        Some(parsed) => Resolution::Ready(parsed),
        None => Resolution::Ignore,
    })
}

/// 明示リプライの解決（Python の msgForOCRReply 冒頭に対応）。
/// 参照先がbot自身の発言でも、embedが無い（誘導メッセージ等のプレーンリプライ）、
/// またはタイトルが解析不能（エラーembed等）な場合は、明示参照が無かったものとして
/// フォールバック探索に委譲する。これにより「誘導メッセージやエラーメッセージに
/// うっかりリプしてしまった」場合でも直近の有効な計算結果を拾える。
async fn resolve_explicit(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    data: &RecruitData,
) -> Result<Resolution, Error> {
    let Some(msg_ref) = &msg.message_reference else {
        return Ok(Resolution::Ignore);
    };
    let Some(ref_id) = msg_ref.message_id else {
        return Ok(Resolution::Ignore);
    };
    let target = match &msg.referenced_message {
        Some(m) => (**m).clone(),
        None => msg.channel_id.message(&ctx.http, ref_id).await?,
    };

    if target.author.id != ctx.cache.current_user().id {
        // bot以外の発言へのリプライは対象外（Python の
        // `referenced_message.author != client.user` と同じ）。
        return Ok(Resolution::Ignore);
    }

    let parsed = target
        .embeds
        .first()
        .and_then(|e| e.title.as_deref())
        .and_then(|title| edit::parse_title(data, title));

    match parsed {
        Some(parsed) => Ok(Resolution::Ready(parsed)),
        None => Ok(match fallback_search(ctx, msg, data).await? {
            Some(parsed) => Resolution::Ready(parsed),
            None => Resolution::Ignore,
        }),
    }
}

/// リプライ参照が無い場合のフォールバック探索。
/// 履歴を時刻降順に走査し、「送信者自身が発言したメッセージ」を参照している
/// bot発言のうち、embedタイトルが解析できる最初の1件を採用する。
/// bot以外の発言・embed無し・解析失敗はスキップして続行し、
/// 10分より古くなった時点、または見つからずに履歴を使い切った時点で無視する。
async fn fallback_search(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    data: &RecruitData,
) -> Result<Option<ParsedTitle>, Error> {
    let bot_id = ctx.cache.current_user().id;
    let history = msg
        .channel_id
        .messages(
            &ctx.http,
            serenity::GetMessages::new().before(msg.id).limit(FALLBACK_HISTORY_LIMIT),
        )
        .await?;

    let own_ids: HashSet<serenity::MessageId> = history
        .iter()
        .filter(|m| m.author.id == msg.author.id)
        .map(|m| m.id)
        .collect();

    let now = msg.timestamp.unix_timestamp();
    for candidate in &history {
        if now - candidate.timestamp.unix_timestamp() > FALLBACK_WINDOW_SECS {
            break; // 履歴は新しい順なので、これ以降はさらに古い→打ち切り
        }
        if candidate.author.id != bot_id {
            continue;
        }
        let Some(ref_id) = candidate.message_reference.as_ref().and_then(|r| r.message_id) else {
            continue;
        };
        if !own_ids.contains(&ref_id) {
            continue;
        }
        let Some(title) = candidate.embeds.first().and_then(|e| e.title.as_deref()) else {
            continue;
        };
        if let Some(parsed) = edit::parse_title(data, title) {
            return Ok(Some(parsed));
        }
    }
    Ok(None)
}
