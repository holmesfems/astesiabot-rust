use crate::bot::data::Error;
use crate::bot::utils::channel_id_env;
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

/// 検知した処断の種別。3層構造（検知/ディスパッチ/実行）の検知層が返す。
/// 優先度: TrapChannel > MassMention > MultiChannel > SameChannel > None
/// （複数該当し得るのは稀だが、その場合も1つの理由に絞って処断する）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpamKind {
    None,
    TrapChannel,
    MassMention,
    MultiChannel,
    SameChannel,
}

/// 管理者が処断対象になった場合の扱い。
enum AdminPolicy {
    /// 正常系として完全に無視する（通報もしない）。
    Ignore,
    /// 処断は見送るが、乗っ取りの可能性があるため通報だけする。
    ReportOnly,
}

impl SpamKind {
    fn reason(self) -> Option<&'static str> {
        match self {
            SpamKind::None => None,
            SpamKind::TrapChannel => Some("罠チャンネルでスパム的メッセージを検出しました"),
            SpamKind::MassMention => Some("通常チャットで非管理者による全体通知を検出しました"),
            SpamKind::MultiChannel => Some("短時間に複数チャンネルへの投稿を検出しました"),
            SpamKind::SameChannel => Some("短時間に同一チャンネルへの連投を検出しました"),
        }
    }

    /// MassMention（配信宣伝など管理者による正常系）だけ完全無視。それ以外は
    /// 本来管理者が起こり得ない事象なので、乗っ取りの早期警報として通報だけ行う。
    fn admin_policy(self) -> AdminPolicy {
        match self {
            SpamKind::MassMention | SpamKind::None => AdminPolicy::Ignore,
            SpamKind::TrapChannel | SpamKind::MultiChannel | SpamKind::SameChannel => {
                AdminPolicy::ReportOnly
            }
        }
    }
}

/// 処断の対象。普段は発言者本人だが、スラッシュコマンドへの bot の返信では
/// コマンドを打った人（返信の author は bot）。連投検知・処断・遡及削除・通報は
/// すべてこの人を基準にする。
#[derive(Clone, Copy)]
struct Actor<'a> {
    user: &'a serenity::User,
    /// Some(bot) なら、user が打ったスラッシュコマンドへの bot の返信を検知した
    via_bot: Option<&'a serenity::User>,
}

impl<'a> Actor<'a> {
    fn author_of(msg: &'a serenity::Message) -> Self {
        Self { user: &msg.author, via_bot: None }
    }

    /// 遡及削除の対象か: 本人の発言、または本人が打ったスラッシュコマンドへの返信
    /// （返信した bot は問わない。複数の bot を使い分けて撒かれても拾えるように）。
    fn caused(&self, m: &serenity::Message) -> bool {
        match self.via_bot {
            None => m.author.id == self.user.id,
            Some(_) => command_invoker(m).is_some_and(|(_, invoker)| invoker.id == self.user.id),
        }
    }
}

/// スラッシュコマンド（コンテキストメニュー含む）への返信なら、その interaction の
/// ID とコマンドを打った人を返す。ボタン・モーダル由来の返信は数えない
/// （ゲーム系 bot のボタンを連打するのは正常な使い方なので）。
fn command_invoker(
    msg: &serenity::Message,
) -> Option<(serenity::InteractionId, &serenity::User)> {
    match msg.interaction_metadata.as_deref()? {
        serenity::MessageInteractionMetadata::Command(meta) => Some((meta.id, &meta.user)),
        _ => None,
    }
}

/// 検知層の結果。delete は kind と独立している（罠チャンネル在籍なら内容に
/// 関わらず常に削除するが、BAN対象コンテンツが無ければ kind は別の理由
/// （またはNone）になり得るため）。
struct Detection {
    delete: bool,
    kind: SpamKind,
}

/// bot 起動中ずっと保持するモデレーション用の可変状態。AppState から共有される。
pub struct ModerationState {
    autodel_channels: [serenity::ChannelId; 6],
    report_channel: serenity::ChannelId,
    url_re: Regex,
    multi_ch_history: Mutex<HashMap<serenity::UserId, VecDeque<(Instant, serenity::ChannelId)>>>,
    same_ch_history:
        Mutex<HashMap<(serenity::UserId, serenity::ChannelId), VecDeque<Instant>>>,
    /// 数え済みのスラッシュコマンド。1つのコマンドへの応答＋フォローアップを1回と数えるため
    seen_interactions: Mutex<HashMap<serenity::InteractionId, Instant>>,
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
            seen_interactions: Mutex::new(HashMap::new()),
        }
    }
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

/// モデレーションの入口。罠チャンネル削除・全体通知BAN・連投/爆撃検知を検知層(detect)で
/// 一括判定し、ディスパッチ層(dispatch)へ渡す。true を返したら、このメッセージは
/// 処断済み（呼び出し側は以降の処理を打ち切ってよい）。
pub async fn handle(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    state: &ModerationState,
    user_is_admin: bool,
) -> Result<bool, Error> {
    let detection = detect(state, msg);
    dispatch(ctx, msg, state, Actor::author_of(msg), detection, user_is_admin).await
}

/// bot の発言のうち、スラッシュコマンドへの返信だけを「コマンドを打った人の操作」として
/// 連投/爆撃検知に数える（人間本人の発言と同じ履歴・同じ閾値）。罠チャンネル・全体通知は
/// 人間本人の発言だけが対象なので、ここでは見ない。
pub async fn handle_command_reply(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    state: &ModerationState,
) -> Result<(), Error> {
    let Some(guild_id) = msg.guild_id else {
        return Ok(());
    };
    let Some((interaction_id, invoker)) = command_invoker(msg) else {
        return Ok(());
    };
    if invoker.bot {
        return Ok(());
    }
    let now = Instant::now();
    if !first_sight_of_interaction(&state.seen_interactions, interaction_id, now) {
        return Ok(());
    }
    let kind = detect_rate_spam(state, invoker.id, msg.channel_id, now);
    if kind == SpamKind::None {
        return Ok(());
    }
    // メンバー取得はAPI呼び出しになり得るので、検知が出た時だけ行う
    let invoker_is_admin = member_is_admin(ctx, guild_id, invoker.id).await;
    let actor = Actor { user: invoker, via_bot: Some(&msg.author) };
    let detection = Detection { delete: false, kind };
    dispatch(ctx, msg, state, actor, detection, invoker_is_admin).await?;
    Ok(())
}

/// 初めて見るスラッシュコマンドなら true。古い記録は検知の時間窓より長くは持たない。
fn first_sight_of_interaction(
    seen: &Mutex<HashMap<serenity::InteractionId, Instant>>,
    interaction_id: serenity::InteractionId,
    now: Instant,
) -> bool {
    let keep = MULTI_CH_WINDOW.max(SAME_CH_WINDOW);
    let mut seen = seen.lock().unwrap();
    seen.retain(|_, at| now.duration_since(*at) <= keep);
    match seen.entry(interaction_id) {
        std::collections::hash_map::Entry::Occupied(_) => false,
        std::collections::hash_map::Entry::Vacant(slot) => {
            slot.insert(now);
            true
        }
    }
}

/// user_is_admin のメンバーID指定版（メッセージに PartialMember が無い、コマンドを打った人用）。
async fn member_is_admin(
    ctx: &serenity::Context,
    guild_id: serenity::GuildId,
    user_id: serenity::UserId,
) -> bool {
    let Ok(member) = guild_id.member(ctx, user_id).await else {
        return false;
    };
    ctx.cache
        .guild(guild_id)
        .is_some_and(|guild| guild.member_permissions(&member).administrator())
}

// ===== 検知層（純粋なロジック。detect_rate_spamの履歴更新以外は副作用なし） =====

fn has_banned_content(state: &ModerationState, msg: &serenity::Message) -> bool {
    let content_lower = msg.content.to_lowercase();
    contains_link(&state.url_re, &msg.content)
        || BANWORDS.iter().any(|word| content_lower.contains(word))
}

fn detect(state: &ModerationState, msg: &serenity::Message) -> Detection {
    let is_trap_channel = state.autodel_channels.contains(&msg.channel_id);
    let trap_banned = is_trap_channel && has_banned_content(state, msg);

    // detect_rate_spam は履歴更新の副作用を持つため、他の判定結果に関わらず必ず呼ぶ
    // （罠チャンネル投稿や全体通知も連投/爆撃検知のカウントに含める）。
    let rate_kind = detect_rate_spam(state, msg.author.id, msg.channel_id, Instant::now());

    let kind = if trap_banned {
        SpamKind::TrapChannel
    } else if msg.mention_everyone {
        SpamKind::MassMention
    } else {
        rate_kind
    };

    Detection {
        delete: is_trap_channel,
        kind,
    }
}

// 注: タイムアウト/BAN反映前の遅延メッセージで再発火し、通報が重複することがある。
//     実害はない（タイムアウト・BANは冪等、purgeは空振り）ため許容している。
//     うるさくなったら処断済みユーザーのクールダウンHashMapを足す。
// now は Instant（経過時間の計測専用クロック。NTP同期などシステム時計の補正に影響されない）。
fn detect_rate_spam(
    state: &ModerationState,
    user_id: serenity::UserId,
    channel_id: serenity::ChannelId,
    now: Instant,
) -> SpamKind {
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

// ===== ディスパッチ層（管理者分岐。処断アクションの選択はexecuteへ委譲） =====

async fn dispatch(
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

// ===== 実行層（種類ごとの差し替え可能なアクション） =====

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
///       来るのは、BANでは消えない bot の返信を消すコマンド経由の場合だけ
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

// ===== 通報 =====

/// 通報文に埋め込むユーザー由来の文字列のバッククォートを、見た目の近い
/// U+02CB（ˋ）に置き換える。生のまま埋め込むと ``` でコードブロックを閉じられ、
/// 以降がMarkdownとして解釈される（偽の行やリンクを差し込まれる）ため。
fn neutralize_backticks(text: &str) -> String {
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

    fn test_state() -> ModerationState {
        ModerationState {
            autodel_channels: [serenity::ChannelId::new(1); 6],
            report_channel: serenity::ChannelId::new(2),
            url_re: re(),
            multi_ch_history: Mutex::new(HashMap::new()),
            same_ch_history: Mutex::new(HashMap::new()),
            seen_interactions: Mutex::new(HashMap::new()),
        }
    }

    fn user_json(id: u64, bot: bool) -> serde_json::Value {
        serde_json::json!({ "id": id.to_string(), "username": format!("u{id}"), "discriminator": "0", "bot": bot })
    }

    /// author が bot_id の bot で、interaction_metadata（kind="command"等）を持つメッセージ
    fn bot_reply(bot_id: u64, metadata: Option<serde_json::Value>) -> serenity::Message {
        let mut msg = serde_json::json!({
            "id": "100", "channel_id": "10", "author": user_json(bot_id, true),
            "content": "reply", "timestamp": "2026-10-16T00:00:00.000000+00:00",
            "edited_timestamp": null, "tts": false, "mention_everyone": false,
            "mentions": [], "mention_roles": [], "attachments": [], "embeds": [],
            "pinned": false, "type": 20,
        });
        if let Some(meta) = metadata {
            msg["interaction_metadata"] = meta;
        }
        serde_json::from_value(msg).expect("test message must deserialize")
    }

    fn metadata(kind: u8, interaction_id: u64, invoker_id: u64) -> serde_json::Value {
        let mut meta = serde_json::json!({
            "type": kind, "id": interaction_id.to_string(), "user": user_json(invoker_id, false),
            "authorizing_integration_owners": {},
        });
        if kind == 3 {
            meta["interacted_message_id"] = serde_json::json!("99");
        }
        meta
    }

    #[test]
    fn command_invoker_counts_only_slash_commands() {
        let slash = bot_reply(500, Some(metadata(2, 7, 42)));
        let (id, invoker) = command_invoker(&slash).expect("slash command reply");
        assert_eq!((id.get(), invoker.id.get()), (7, 42));

        // ボタン（type 3）由来の返信と、interaction ではない普通の bot 発言は数えない
        assert!(command_invoker(&bot_reply(500, Some(metadata(3, 8, 42)))).is_none());
        assert!(command_invoker(&bot_reply(500, None)).is_none());
    }

    #[test]
    fn actor_via_bot_matches_replies_to_its_own_commands_from_any_bot() {
        let reply = bot_reply(500, Some(metadata(2, 7, 42)));
        let other_bot_same_invoker = bot_reply(501, Some(metadata(2, 8, 42)));
        let other_invoker = bot_reply(500, Some(metadata(2, 9, 43)));
        let (_, invoker) = command_invoker(&reply).unwrap();
        let actor = Actor { user: invoker, via_bot: Some(&reply.author) };

        assert!(actor.caused(&reply));
        assert!(actor.caused(&other_bot_same_invoker));
        assert!(!actor.caused(&other_invoker));
        // 発言者本人として見た場合、bot の返信は本人の発言ではない
        assert!(!Actor { user: invoker, via_bot: None }.caused(&reply));
    }

    #[test]
    fn one_slash_command_is_counted_once() {
        let seen = Mutex::new(HashMap::new());
        let now = Instant::now();
        let id = serenity::InteractionId::new(7);
        assert!(first_sight_of_interaction(&seen, id, now));
        // 同じコマンドへのフォローアップ
        assert!(!first_sight_of_interaction(&seen, id, now + Duration::from_secs(1)));
        // 窓を過ぎた古い記録は捨てる
        let later = now + MULTI_CH_WINDOW.max(SAME_CH_WINDOW) + Duration::from_secs(1);
        assert!(first_sight_of_interaction(&seen, serenity::InteractionId::new(8), later));
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn commands_in_three_channels_trip_multi_channel_for_the_invoker() {
        let state = test_state();
        let invoker = serenity::UserId::new(42);
        let now = Instant::now();
        let kinds: Vec<_> = (0..3)
            .map(|i| detect_rate_spam(&state, invoker, serenity::ChannelId::new(10 + i), now + Duration::from_secs(i)))
            .collect();
        assert_eq!(kinds, [SpamKind::None, SpamKind::None, SpamKind::MultiChannel]);
    }

    #[test]
    fn neutralize_backticks_prevents_code_block_breakout() {
        let echoed = neutralize_backticks("x```\n@everyone <@&1> `y`");
        assert!(!echoed.contains('`'));
        assert_eq!(echoed, "x\u{02CB}\u{02CB}\u{02CB}\n@everyone <@&1> \u{02CB}y\u{02CB}");
    }
}