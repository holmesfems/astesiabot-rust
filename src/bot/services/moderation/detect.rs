//! 検知層: スパムと禁止内容の判定（純粋なロジック。`detect_rate_spam` の履歴更新以外は副作用なし）。

use fancy_regex::Regex;
use poise::serenity_prelude as serenity;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use super::ModerationState;


pub(super) const MULTI_CH_WINDOW: Duration = Duration::from_millis(7_000);

const MULTI_CH_THRESHOLD: usize = 3;

pub(super) const SAME_CH_WINDOW: Duration = Duration::from_millis(10_000);

pub(super) const SAME_CH_THRESHOLD: usize = 6;

const BANWORDS: [&str; 3] = ["discord.gg", "everyone", "peach"];

// http/https、www、裸ドメイン(例: example.com/path)を検出。
// 否定先読み `(?<!...)` を使うため、標準regexではなくfancy-regexを使う
// （求人のmatcher.rsが `(?!上級)` にfancy-regexを使っているのと同じ理由）。
pub(super) const URL_PATTERN: &str = r#"(?ix)
(?<![A-Za-z0-9])(
    (?:https?://|ftp://)[^\s<>'"]+
  | (?:www\.)[^\s<>'"]+
  | (?:[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?\.)+
    (?:[A-Za-z]{2,})(?:/[^\s<>'"]*)?
)"#;

pub(super) fn contains_link(re: &Regex, text: &str) -> bool {
    matches!(re.is_match(text), Ok(true))
}

/// 検知した処断の種別。3層構造（検知/ディスパッチ/実行）の検知層が返す。
/// 優先度: TrapChannel > MassMention > MultiChannel > SameChannel > None
/// （複数該当し得るのは稀だが、その場合も1つの理由に絞って処断する）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SpamKind {
    None,
    TrapChannel,
    MassMention,
    MultiChannel,
    SameChannel,
}

/// 管理者が処断対象になった場合の扱い。
pub(super) enum AdminPolicy {
    /// 正常系として完全に無視する（通報もしない）。
    Ignore,
    /// 処断は見送るが、乗っ取りの可能性があるため通報だけする。
    ReportOnly,
}

impl SpamKind {
    pub(super) fn reason(self) -> Option<&'static str> {
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
    pub(super) fn admin_policy(self) -> AdminPolicy {
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
pub(super) struct Actor<'a> {
    pub(super) user: &'a serenity::User,
    /// Some(bot) なら、user が打ったスラッシュコマンドへの bot の返信を検知した
    pub(super) via_bot: Option<&'a serenity::User>,
}

impl<'a> Actor<'a> {
    pub(super) fn author_of(msg: &'a serenity::Message) -> Self {
        Self { user: &msg.author, via_bot: None }
    }

    /// 遡及削除の対象か: 本人の発言、または本人が打ったスラッシュコマンドへの返信
    /// （返信した bot は問わない。複数の bot を使い分けて撒かれても拾えるように）。
    pub(super) fn caused(&self, m: &serenity::Message) -> bool {
        match self.via_bot {
            None => m.author.id == self.user.id,
            Some(_) => command_invoker(m).is_some_and(|(_, invoker)| invoker.id == self.user.id),
        }
    }
}

/// スラッシュコマンド（コンテキストメニュー含む）への返信なら、その interaction の
/// ID とコマンドを打った人を返す。ボタン・モーダル由来の返信は数えない
/// （ゲーム系 bot のボタンを連打するのは正常な使い方なので）。
pub(super) fn command_invoker(
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
pub(super) struct Detection {
    pub(super) delete: bool,
    pub(super) kind: SpamKind,
}

/// 初めて見るスラッシュコマンドなら true。古い記録は検知の時間窓より長くは持たない。
pub(super) fn first_sight_of_interaction(
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

fn has_banned_content(state: &ModerationState, msg: &serenity::Message) -> bool {
    let content_lower = msg.content.to_lowercase();
    contains_link(&state.url_re, &msg.content)
        || BANWORDS.iter().any(|word| content_lower.contains(word))
}

/// ようこそメッセージの「手を振って挨拶しましょう！」ボタンで送られたスタンプか
/// （入室メッセージへの、本文なし・スタンプだけのリプライ）。新しい人が続けて来ると
/// 押す人は何度も押すので、連投には数えない。本文もリンクも無いので素通りさせても害は無い。
/// リプライ先が取れない（消された等）ときは挨拶とみなさず、今まで通り数える。
pub(super) fn is_welcome_wave(msg: &serenity::Message) -> bool {
    msg.content.is_empty()
        && !msg.sticker_items.is_empty()
        && msg
            .referenced_message
            .as_deref()
            .is_some_and(|r| r.kind == serenity::MessageType::MemberJoin)
}

pub(super) fn detect(state: &ModerationState, msg: &serenity::Message) -> Detection {
    let is_trap_channel = state.autodel_channels.contains(&msg.channel_id);
    let trap_banned = is_trap_channel && has_banned_content(state, msg);

    // detect_rate_spam は履歴更新の副作用を持つため、他の判定結果に関わらず必ず呼ぶ
    // （罠チャンネル投稿や全体通知も連投/爆撃検知のカウントに含める）。
    // 例外は挨拶ボタンのスタンプだけで、これは履歴にも残さない。
    let rate_kind = if is_welcome_wave(msg) {
        SpamKind::None
    } else {
        detect_rate_spam(state, msg.author.id, msg.channel_id, Instant::now())
    };

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
pub(super) fn detect_rate_spam(
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
