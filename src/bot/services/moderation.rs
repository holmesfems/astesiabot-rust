use crate::bot::data::Error;
use fancy_regex::Regex;
use poise::serenity_prelude as serenity;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const MULTI_CH_WINDOW: Duration = Duration::from_millis(7_000);
const MULTI_CH_THRESHOLD: usize = 3;
const SAME_CH_WINDOW: Duration = Duration::from_millis(10_000);
const SAME_CH_THRESHOLD: usize = 6;
const TIMEOUT_MINUTES: i64 = 30;
const PURGE_LOOKBACK_SECS: i64 = 15;
const BULK_DELETE_LIMIT_DAYS: i64 = 14;

const BANWORDS: [&str; 3] = ["discord.gg", "everyone", "peach"];

// http/https、www、裸ドメイン(例: example.com/path)を検出。
// 否定先読み `(?<!...)` を使うため、標準regexではなくfancy-regexを使う
// （求人のmatcher.rsが `(?!上級)` にfancy-regexを使っているのと同じ理由）。
const URL_PATTERN: &str = r#"(?ix)
(?<![A-Za-z0-9])(
    (?:https?://|ftp://)[^\s<>'"]+
  | (?:www\.)[^\s<>'"]+
  | (?:[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?\.)+
    (?:[A-Za-z]{2,})(?:/[^\s<>'"]*)?
)"#;

fn contains_link(re: &Regex, text: &str) -> bool {
    matches!(re.is_match(text), Ok(true))
}

/// 検知した連投/爆撃の種別。3層構造（検知/ディスパッチ/実行）の検知層が返す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpamKind {
    None,
    MultiChannel,
    SameChannel,
}

/// bot 起動中ずっと保持するモデレーション用の可変状態。AppState から共有される。
pub struct ModerationState {
    autodel_channels: [serenity::ChannelId; 6],
    report_channel: serenity::ChannelId,
    url_re: Regex,
    multi_ch_history: Mutex<HashMap<serenity::UserId, VecDeque<(Instant, serenity::ChannelId)>>>,
    same_ch_history:
        Mutex<HashMap<(serenity::UserId, serenity::ChannelId), VecDeque<Instant>>>,
}

impl ModerationState {
    pub fn from_env() -> Self {
        let autodel_channels = [
            channel_id_env("AUTODEL_1"),
            channel_id_env("AUTODEL_2"),
            channel_id_env("AUTODEL_3"),
            channel_id_env("AUTODEL_4"),
            channel_id_env("AUTODEL_5"),
            channel_id_env("AUTODEL_6"),
        ];
        let report_channel = channel_id_env("REPORT_CHANNEL_ID");
        let url_re = Regex::new(URL_PATTERN).expect("URL_PATTERN must compile");
        Self {
            autodel_channels,
            report_channel,
            url_re,
            multi_ch_history: Mutex::new(HashMap::new()),
            same_ch_history: Mutex::new(HashMap::new()),
        }
    }
}

fn channel_id_env(key: &str) -> serenity::ChannelId {
    std::env::var(key)
        .unwrap_or_else(|_| panic!("{key} not set"))
        .parse()
        .unwrap_or_else(|_| panic!("{key} is not a valid channel id"))
}

/// 発言者が ADMINISTRATOR 権限を持つか判定する。DM やメンバー情報未取得なら false。
/// メッセージに同梱される PartialMember から計算するため、追加のAPI呼び出しは発生しない。
pub fn user_is_admin(ctx: &serenity::Context, msg: &serenity::Message) -> bool {
    let Some(guild_id) = msg.guild_id else {
        return false;
    };
    let Some(partial_member) = msg.member.as_deref() else {
        return false;
    };
    let Some(guild) = ctx.cache.guild(guild_id) else {
        return false;
    };
    guild
        .partial_member_permissions(msg.author.id, partial_member)
        .administrator()
}

/// モデレーションの入口。罠チャンネル削除・全体通知BAN・連投/爆撃検知を順に走らせる。
/// true を返したら、このメッセージは処断済み（呼び出し側は以降の処理を打ち切ってよい）。
pub async fn handle(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    state: &ModerationState,
    user_is_admin: bool,
) -> Result<bool, Error> {
    if auto_deletion(ctx, msg, state, user_is_admin).await? {
        return Ok(true);
    }

    let kind = detect_rate_spam(state, msg);
    handle_rate_spam(ctx, msg, state, kind, user_is_admin).await
}

// ===== 罠チャンネルの自動削除・BAN =====

async fn auto_deletion(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    state: &ModerationState,
    user_is_admin: bool,
) -> Result<bool, Error> {
    if state.autodel_channels.contains(&msg.channel_id) {
        // 罠チャンネルは管理者の発言も削除する（人間がしゃべる想定の場所ではない）
        msg.delete(ctx).await?;
        auto_ban_in_auto_deletion(ctx, msg, state, user_is_admin).await?;
        Ok(true)
    } else {
        auto_notice_and_ban(ctx, msg, state, user_is_admin).await
    }
}

async fn auto_ban_in_auto_deletion(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    state: &ModerationState,
    user_is_admin: bool,
) -> Result<bool, Error> {
    let content_lower = msg.content.to_lowercase();
    let has_link = contains_link(&state.url_re, &msg.content);
    let has_banword = BANWORDS.iter().any(|word| content_lower.contains(word));
    if !has_link && !has_banword {
        return Ok(false);
    }

    // 管理者は処断せず通報のみ。罠チャンネルに管理者の発言が来ること自体が異常で、
    // 乗っ取りの早期警報として意味がある。
    if user_is_admin {
        create_report(
            ctx,
            state,
            "罠チャンネルでスパム的メッセージを検出しましたが、管理者ロール保持のため自動BANは見送りました。アカウント乗っ取りの可能性を確認してください。",
            Some(msg),
        )
        .await?;
        return Ok(false);
    }

    if let Some(guild_id) = msg.guild_id {
        guild_id
            .ban_with_reason(
                &ctx.http,
                msg.author.id,
                7,
                "Auto-banned by the Astesia bot for sending spam messages",
            )
            .await?;
    }
    create_report(
        ctx,
        state,
        "スパムメッセージを検出したので、自動BANを実行しました。",
        Some(msg),
    )
    .await?;
    Ok(true)
}

// ===== 全体監視：非管理者の全体通知を検知 =====

async fn auto_notice_and_ban(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    state: &ModerationState,
    user_is_admin: bool,
) -> Result<bool, Error> {
    // 管理者の全体通知は配信宣伝などの正常系なので、検知対象から外す
    if user_is_admin {
        return Ok(false);
    }
    // 非管理者が mention_everyone=true にできる時点で異常（@everyone/@here どちらでもtrue）
    if !msg.mention_everyone {
        return Ok(false);
    }
    if let Some(guild_id) = msg.guild_id {
        guild_id
            .ban_with_reason(
                &ctx.http,
                msg.author.id,
                7,
                "Auto-banned by the Astesia bot for sending spam messages",
            )
            .await?;
    }
    create_report(
        ctx,
        state,
        "通常チャットで非管理者による全体通知を検出しました。自動BANを実行しました",
        Some(msg),
    )
    .await?;
    Ok(true)
}

// ===== レート制御：検知層（純粋にロジックのみ。処断はしない） =====

fn detect_rate_spam(state: &ModerationState, msg: &serenity::Message) -> SpamKind {
    let user_id = msg.author.id;
    let channel_id = msg.channel_id;
    // 経過時間の計測専用クロック。NTP同期などシステム時計の補正に影響されない
    let now = Instant::now();

    let same_hit = {
        let mut history = state.same_ch_history.lock().unwrap();
        let dq = history.entry((user_id, channel_id)).or_default();
        dq.push_back(now);
        while matches!(dq.front(), Some(&front) if now.duration_since(front) > SAME_CH_WINDOW) {
            dq.pop_front();
        }
        dq.len() >= SAME_CH_THRESHOLD
    };

    let multi_hit = {
        let mut history = state.multi_ch_history.lock().unwrap();
        let dq = history.entry(user_id).or_default();
        dq.push_back((now, channel_id));
        while matches!(dq.front(), Some(&(front, _)) if now.duration_since(front) > MULTI_CH_WINDOW)
        {
            dq.pop_front();
        }
        let distinct_channels: HashSet<_> = dq.iter().map(|(_, ch)| *ch).collect();
        distinct_channels.len() >= MULTI_CH_THRESHOLD
    };

    // 複数チャンネル型を優先（より悪質な爆撃とみなす）
    if multi_hit {
        SpamKind::MultiChannel
    } else if same_hit {
        SpamKind::SameChannel
    } else {
        SpamKind::None
    }
}

// ===== レート制御：ディスパッチ層（管理者分岐・種類分岐） =====

async fn handle_rate_spam(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    state: &ModerationState,
    kind: SpamKind,
    user_is_admin: bool,
) -> Result<bool, Error> {
    let reason = match kind {
        SpamKind::None => return Ok(false),
        SpamKind::MultiChannel => "短時間に複数チャンネルへの投稿を検出しました",
        SpamKind::SameChannel => "短時間に同一チャンネルへの連投を検出しました",
    };

    // 管理者は処断せず通報のみ（正規管理者が引っかかる想定はなく、乗っ取りの早期警報）。
    if user_is_admin {
        create_report(
            ctx,
            state,
            &format!(
                "{reason}が、管理者ロール保持のため処断は見送りました。アカウント乗っ取りの可能性を確認してください。"
            ),
            Some(msg),
        )
        .await?;
        return Ok(false);
    }

    // TODO: 誤検知が無いと確認できたら、種類ごとに ban_and_report へ差し替え可能。
    //       検知層（detect_rate_spam）には触れず、この分岐の呼び先を変えるだけでよい。
    mute_and_report(ctx, msg, state, kind, reason).await?;
    Ok(true)
}

// ===== レート制御：実行層（差し替え可能なアクション） =====

async fn mute_and_report(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    state: &ModerationState,
    kind: SpamKind,
    reason: &str,
) -> Result<(), Error> {
    let Some(guild_id) = msg.guild_id else {
        return Ok(());
    };

    // 遡及削除：トリガーとなった1件だけでなく、直近に撒かれた分をまとめて消す。
    // メッセージ削除はタイムアウト付与より先に行う（タイムアウト失敗で早期returnする前に
    // 削除を済ませておきたい）。
    let purged = purge_recent(ctx, msg, state, kind).await;

    let until =
        serenity::Timestamp::from(chrono::Utc::now() + chrono::Duration::minutes(TIMEOUT_MINUTES));
    let audit_reason = format!("Astesia bot: {reason}");
    let builder = serenity::EditMember::new()
        .disable_communication_until_datetime(until)
        .audit_log_reason(&audit_reason);
    if guild_id.edit_member(ctx, msg.author.id, builder).await.is_err() {
        // botの権限不足（Moderate Members権限が必要）や対象が上位ロールの場合
        create_report(
            ctx,
            state,
            &format!(
                "{reason}が、タイムアウト付与に失敗しました（権限を確認してください）。なお遡及削除は{purged}件を処理しました"
            ),
            Some(msg),
        )
        .await?;
        return Ok(());
    }

    create_report(
        ctx,
        state,
        &format!("{reason}。直近メッセージ{purged}件の削除と{TIMEOUT_MINUTES}分のタイムアウトを実行しました"),
        Some(msg),
    )
    .await?;
    Ok(())
}

/// 将来、誤検知ゼロを確認できた系統をBANに切り替えるための受け皿。
/// 検知層・ディスパッチ層は変えず、handle_rate_spam の呼び先をここに差し替えるだけでよい。
#[allow(dead_code)]
async fn ban_and_report(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    state: &ModerationState,
    reason: &str,
) -> Result<(), Error> {
    if let Some(guild_id) = msg.guild_id {
        guild_id
            .ban_with_reason(&ctx.http, msg.author.id, 7, &format!("Astesia bot: {reason}"))
            .await?;
    }
    create_report(ctx, state, &format!("{reason}。自動BANを実行しました"), Some(msg)).await?;
    Ok(())
}

/// 該当ユーザーが直近 PURGE_LOOKBACK_SECS 秒に撒いたメッセージを一括削除する。
///
/// 対象チャンネルは検知種別で絞る:
///   - SameChannel : メッセージが来たチャンネル1つ
///   - MultiChannel: multi_ch_history に記録された channel_id 群（撒かれた先のみ）
///
/// 検知は Instant（単調クロック）だが、purge の範囲指定はDiscord APIの都合上
/// 実時計でしか行えない。数秒の誤差はスパム判定が出ている以上割り切る。
async fn purge_recent(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    state: &ModerationState,
    kind: SpamKind,
) -> u64 {
    let user_id = msg.author.id;
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
    for channel_id in channel_ids {
        deleted_total += purge_channel(ctx, channel_id, user_id, cutoff).await;
    }

    // 消したらこのユーザーの履歴はクリアしておく（二重処理・無駄な保持を防ぐ）
    state.multi_ch_history.lock().unwrap().remove(&user_id);
    state
        .same_ch_history
        .lock()
        .unwrap()
        .retain(|(uid, _), _| *uid != user_id);

    deleted_total
}

async fn purge_channel(
    ctx: &serenity::Context,
    channel_id: serenity::ChannelId,
    user_id: serenity::UserId,
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
        .filter(|m| m.author.id == user_id && m.timestamp >= cutoff)
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

// ===== 通報 =====

async fn create_report(
    ctx: &serenity::Context,
    state: &ModerationState,
    report: &str,
    msg: Option<&serenity::Message>,
) -> Result<(), Error> {
    let mut content = format!("{report}\n");
    if let Some(msg) = msg {
        content += &format!("author:{}\n", msg.author.name);
        let sanitized = msg.content.replace('.', "_").replace("http", "ht tp");
        content += &format!("content:```{sanitized}```\n");
        content += &format!("channel:{}", msg.link());
    }
    state.report_channel.say(&ctx.http, content).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn re() -> Regex {
        Regex::new(URL_PATTERN).unwrap()
    }

    #[test]
    fn detects_scheme_urls() {
        assert!(contains_link(&re(), "見て https://example.com/foo"));
        assert!(contains_link(&re(), "http://example.com"));
    }

    #[test]
    fn detects_www_and_bare_domains() {
        assert!(contains_link(&re(), "www.example.com にアクセス"));
        assert!(contains_link(&re(), "example.com/path も検出できる"));
    }

    #[test]
    fn does_not_flag_plain_text() {
        assert!(!contains_link(&re(), "これは普通のテキストです"));
        assert!(!contains_link(&re(), "バージョン1.2.3をリリースしました"));
    }

    #[test]
    fn negative_lookbehind_avoids_scheme_glued_to_alnum() {
        // "https://" の直前が英数字（区切りなしで接続）だとスキームとしては
        // マッチしない。TLD付きドメインも含まれないため全体として非検出になる。
        assert!(!contains_link(&re(), "xhttps://evil"));
    }
}
