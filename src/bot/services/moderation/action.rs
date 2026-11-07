//! ディスパッチ層（管理者分岐）と実行層: 処断（タイムアウト・BAN・削除）と通報。

use crate::bot::data::Error;
use poise::serenity_prelude as serenity;
use std::collections::HashSet;
use super::detect::{SpamKind, AdminPolicy, Actor, Detection};
use super::ModerationState;

const TIMEOUT_MINUTES: i64 = 30;

const PURGE_LOOKBACK_SECS: i64 = 15;

const BULK_DELETE_LIMIT_DAYS: i64 = 14;

pub(super) async fn dispatch(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    state: &ModerationState,
    actor: Actor<'_>,
    detection: Detection,
    user_is_admin: bool,
) -> Result<bool, Error> {
    if detection.delete {
        // 罠チャンネルは管理者の発言も削除する（人間がしゃべる想定の場所ではない）
        msg.delete(ctx).await?;
    }

    let Some(reason) = detection.kind.reason() else {
        return Ok(detection.delete);
    };

    if user_is_admin {
        if let AdminPolicy::ReportOnly = detection.kind.admin_policy() {
            create_report(
                ctx,
                state,
                &format!(
                    "{reason}が、管理者ロール保持のため自動処断は見送りました。アカウント乗っ取りの可能性を確認してください。"
                ),
                Some((msg, actor)),
                &[msg.channel_id],
            )
            .await?;
        }
        return Ok(detection.delete);
    }

    execute(ctx, msg, state, actor, detection.kind, reason).await?;
    Ok(true)
}

// MultiChannel は運用で誤検知が無いと確認できたためミュートからBANへ引き上げた。
// SameChannel も同様に確認できたら、ここのmatch armを ban_and_report へ
// 差し替えるだけでよい（検知層・ディスパッチ層には触れない）。
async fn execute(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    state: &ModerationState,
    actor: Actor<'_>,
    kind: SpamKind,
    reason: &str,
) -> Result<(), Error> {
    match kind {
        SpamKind::TrapChannel | SpamKind::MassMention => {
            ban_and_report(ctx, msg, state, actor, reason, &[msg.channel_id]).await
        }
        SpamKind::MultiChannel => {
            let channels = if actor.via_bot.is_some() {
                // BANで消えるのは本人の発言だけで、本人が打ったコマンドへの bot の返信は
                // 残るので、撒かれた先から先に消しておく（履歴のクリアも purge_recent が行う）。
                purge_recent(ctx, msg, state, actor, kind).await.1
            } else {
                // BAN自体が過去7日分のメッセージを全チャンネルから消すので遡及削除は不要。
                // 通報に「どこに撒かれたか」を載せるため、履歴だけ取り出してクリアする。
                take_spread_channels(state, actor.user.id, msg.channel_id)
            };
            ban_and_report(ctx, msg, state, actor, reason, &channels).await
        }
        SpamKind::SameChannel => mute_and_report(ctx, msg, state, actor, kind, reason).await,
        SpamKind::None => Ok(()),
    }
}

/// multi_ch_history から該当ユーザーが撒いた先のチャンネル一覧を取り出し、
/// 連投/爆撃の履歴をクリアする（遅延メッセージでの二重処断を減らすため）。
fn take_spread_channels(
    state: &ModerationState,
    user_id: serenity::UserId,
    fallback: serenity::ChannelId,
) -> Vec<serenity::ChannelId> {
    let spread: HashSet<serenity::ChannelId> = state
        .multi_ch_history
        .lock()
        .unwrap()
        .remove(&user_id)
        .into_iter()
        .flatten()
        .map(|(_, ch)| ch)
        .collect();
    state
        .same_ch_history
        .lock()
        .unwrap()
        .retain(|(uid, _), _| *uid != user_id);
    if spread.is_empty() {
        vec![fallback]
    } else {
        spread.into_iter().collect()
    }
}

async fn mute_and_report(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    state: &ModerationState,
    actor: Actor<'_>,
    kind: SpamKind,
    reason: &str,
) -> Result<(), Error> {
    let Some(guild_id) = msg.guild_id else {
        return Ok(());
    };

    // 遡及削除：トリガーとなった1件だけでなく、直近に撒かれた分をまとめて消す。
    // メッセージ削除はタイムアウト付与より先に行う（タイムアウト失敗で早期returnする前に
    // 削除を済ませておきたい）。
    let (purged, channels) = purge_recent(ctx, msg, state, actor, kind).await;

    let until =
        serenity::Timestamp::from(chrono::Utc::now() + chrono::Duration::minutes(TIMEOUT_MINUTES));
    let audit_reason = format!("Astesia bot: {reason}");
    let builder = serenity::EditMember::new()
        .disable_communication_until_datetime(until)
        .audit_log_reason(&audit_reason);
    if guild_id.edit_member(ctx, actor.user.id, builder).await.is_err() {
        // botの権限不足（Moderate Members権限が必要）や対象が上位ロールの場合
        create_report(
            ctx,
            state,
            &format!(
                "{reason}が、タイムアウト付与に失敗しました（権限を確認してください）。なお遡及削除は{purged}件を処理しました"
            ),
            Some((msg, actor)),
            &channels,
        )
        .await?;
        return Ok(());
    }

    create_report(
        ctx,
        state,
        &format!("{reason}。直近メッセージ{purged}件の削除と{TIMEOUT_MINUTES}分のタイムアウトを実行しました"),
        Some((msg, actor)),
        &channels,
    )
    .await?;
    Ok(())
}

/// TrapChannel/MassMention/MultiChannel の実処断。SameChannel を将来BANに
/// 切り替える場合も、この関数をそのまま execute の呼び先に使う。
/// channels は通報文に載せる「投稿があったチャンネル」。
async fn ban_and_report(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    state: &ModerationState,
    actor: Actor<'_>,
    reason: &str,
    channels: &[serenity::ChannelId],
) -> Result<(), Error> {
    if let Some(guild_id) = msg.guild_id {
        guild_id
            .ban_with_reason(&ctx.http, actor.user.id, 7, &format!("Astesia bot: {reason}"))
            .await?;
    }
    create_report(
        ctx,
        state,
        &format!("{reason}。自動BANを実行しました"),
        Some((msg, actor)),
        channels,
    )
    .await?;
    Ok(())
}

/// 該当ユーザーが直近 PURGE_LOOKBACK_SECS 秒に撒いたメッセージを一括削除する。
/// コマンド経由（actor.via_bot）なら、本人が打ったスラッシュコマンドへの bot の返信を消す。
///
/// 対象チャンネルは検知種別で絞る:
///   - SameChannel : メッセージが来たチャンネル1つ
///   - MultiChannel: multi_ch_history に記録された channel_id 群（撒かれた先のみ）
///     ※人間本人の MultiChannel はBAN（メッセージ削除込み）で処断するため、ここには来ない。
///     来るのは、BANでは消えない bot の返信を消すコマンド経由の場合だけ
///
/// 検知は Instant（単調クロック）だが、purge の範囲指定はDiscord APIの都合上
/// 実時計でしか行えない。数秒の誤差はスパム判定が出ている以上割り切る。
///
/// 戻り値は (削除件数, 実際に対象にしたチャンネル一覧)。後者は通報文で
/// 「どこに撒かれたか」を表示するために使う。
async fn purge_recent(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    state: &ModerationState,
    actor: Actor<'_>,
    kind: SpamKind,
) -> (u64, Vec<serenity::ChannelId>) {
    let user_id = actor.user.id;
    let cutoff = serenity::Timestamp::from(
        chrono::Utc::now() - chrono::Duration::seconds(PURGE_LOOKBACK_SECS),
    );

    let channel_ids: Vec<serenity::ChannelId> = match kind {
        SpamKind::MultiChannel => {
            let history = state.multi_ch_history.lock().unwrap();
            let ids: HashSet<serenity::ChannelId> = history
                .get(&user_id)
                .into_iter()
                .flatten()
                .map(|(_, ch)| *ch)
                .collect();
            if ids.is_empty() {
                vec![msg.channel_id]
            } else {
                ids.into_iter().collect()
            }
        }
        _ => vec![msg.channel_id],
    };

    let mut deleted_total = 0u64;
    for &channel_id in &channel_ids {
        deleted_total += purge_channel(ctx, channel_id, actor, cutoff).await;
    }

    // 消したらこのユーザーの履歴はクリアしておく（二重処理・無駄な保持を防ぐ）
    state.multi_ch_history.lock().unwrap().remove(&user_id);
    state
        .same_ch_history
        .lock()
        .unwrap()
        .retain(|(uid, _), _| *uid != user_id);

    (deleted_total, channel_ids)
}

async fn purge_channel(
    ctx: &serenity::Context,
    channel_id: serenity::ChannelId,
    actor: Actor<'_>,
    cutoff: serenity::Timestamp,
) -> u64 {
    let messages = match channel_id
        .messages(ctx, serenity::GetMessages::new().limit(100))
        .await
    {
        Ok(m) => m,
        // 権限エラー(Forbidden)やHTTPエラー。握りつぶして次のチャンネルへ（止血優先）
        Err(_) => return 0,
    };

    let target_ids: Vec<serenity::MessageId> = messages
        .into_iter()
        .filter(|m| actor.caused(m) && m.timestamp >= cutoff)
        .map(|m| m.id)
        .collect();
    if target_ids.is_empty() {
        return 0;
    }

    // 14日以内はバルク削除、それ超は個別deleteにフォールバック
    let bulk_cutoff = serenity::Timestamp::from(
        chrono::Utc::now() - chrono::Duration::days(BULK_DELETE_LIMIT_DAYS),
    );
    let (bulk, old): (Vec<_>, Vec<_>) = target_ids
        .into_iter()
        .partition(|id| id.created_at() >= bulk_cutoff);

    let mut deleted = 0u64;
    for chunk in bulk.chunks(100) {
        if channel_id
            .delete_messages(&ctx.http, chunk.to_vec())
            .await
            .is_ok()
        {
            deleted += chunk.len() as u64;
        }
    }
    for id in old {
        if channel_id.delete_message(&ctx.http, id).await.is_ok() {
            deleted += 1;
        }
    }
    deleted
}

/// 通報文に埋め込むユーザー由来の文字列のバッククォートを、見た目の近い
/// U+02CB（ˋ）に置き換える。生のまま埋め込むと ``` でコードブロックを閉じられ、
/// 以降がMarkdownとして解釈される（偽の行やリンクを差し込まれる）ため。
pub(super) fn neutralize_backticks(text: &str) -> String {
    text.replace('`', "\u{02CB}")
}

/// `channels` が空でなければ、末尾に `<#id>` のリストを列挙する。
/// Discord 側が自動でチャンネル名にリンク展開する。
async fn create_report(
    ctx: &serenity::Context,
    state: &ModerationState,
    report: &str,
    subject: Option<(&serenity::Message, Actor<'_>)>,
    channels: &[serenity::ChannelId],
) -> Result<(), Error> {
    let mut content = format!("{report}\n");
    if let Some((msg, actor)) = subject {
        content += &format!("author:{}", neutralize_backticks(&actor.user.name));
        if let Some(bot) = actor.via_bot {
            content += &format!("（{}のスラッシュコマンド経由）", neutralize_backticks(&bot.name));
        }
        content += "\n";
        let sanitized =
            neutralize_backticks(&msg.content.replace('.', "_").replace("http", "ht tp"));
        content += &format!("content:```{sanitized}```\n");
    }
    if !channels.is_empty() {
        let mentions = channels
            .iter()
            .map(|c| format!("<#{c}>"))
            .collect::<Vec<_>>()
            .join(" ");
        content += &format!("channel:{mentions}");
    }
    // 通報文にはスパム投稿者の文字列が入るので、@everyone/ロール/ユーザーへの
    // メンション通知を一切させない（<#id> のチャンネル表示は通知ではないので影響しない）。
    let builder = serenity::CreateMessage::new()
        .content(content)
        .allowed_mentions(serenity::CreateAllowedMentions::new());
    state.report_channel.send_message(&ctx.http, builder).await?;
    Ok(())
}
